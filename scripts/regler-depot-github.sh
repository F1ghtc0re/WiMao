#!/usr/bin/env bash
# Pose et vérifie les réglages de sécurité du dépôt GitHub public de WiMao.
#
# Pourquoi un script plutôt que des clics : les réglages sont ainsi relus (c'est du code versionné),
# reproductibles (si le dépôt devait être recréé) et vérifiables à tout moment. La liste complète, avec
# les valeurs exactes, est dans docs/superpowers/specs/2026-11-01-wimao-sous-projet-5-design.md § 4.
#
# Réglages posés (une fonction `regler_*` chacun, qui sait à la fois les POSER et les VÉRIFIER) :
#   - ruleset « main protégée »        : PR obligatoire, discussions résolues, CI verte, commits signés, pas
#                                         de suppression ni de réécriture d'historique, AUCUN contournement ;
#   - ruleset « étiquettes de version » : une étiquette `v*` ne peut être ni créée, ni déplacée, ni
#                                         supprimée, sauf par l'administrateur ;
#   - signalement privé des failles, alertes et correctifs Dependabot, détection des secrets ;
#   - jeton des workflows en lecture seule, actions limitées à une liste, CI des PR extérieures validée à
#     la main, releases immuables.
#
# Pourquoi 0 approbation sur « main protégée » : le propriétaire est seul ; GitHub interdit d'approuver
# sa propre PR, donc une approbation exigée bloquerait toute fusion. La protection vient de la PR
# obligatoire, des discussions résolues, de la CI verte et des commits signés. Personne ne peut contourner
# ce ruleset (`bypass_actors` vide), pas même l'administrateur : jamais d'envoi direct sur `main`. En cas
# d'urgence, l'administrateur désactive TEMPORAIREMENT le ruleset dans Settings > Rules ; `--verifier`
# affiche alors un ❌ tant qu'il n'est pas réappliqué (`--appliquer`).
#
# Pourquoi un seul ruleset d'étiquettes, contournable par l'administrateur : il protège contre les jetons
# de workflow (le job `brouillon` a `contents: write` et pourrait sinon déplacer une étiquette) et contre
# toute autre personne ; le propriétaire garde la possibilité de supprimer une étiquette ratée, comme le
# prévoit docs/publier-une-version.md. Un jeton administrateur volé pourrait de toute façon modifier les
# rulesets eux-mêmes : `signer-version.sh` reste le dernier contrôle (étiquette signée par la clé publiée).
#
# Les rulesets sont identifiés par leur NOM : s'il existe déjà, il est mis à jour (PUT), sinon créé
# (POST). Relancer le script ne crée donc jamais de doublon.
#
# Réglage indisponible : SEUL `immutable-releases` (réglage récent, pas proposé partout) tolère, à
# l'ÉCRITURE, un 404 ou un 422 : la ligne est alors un ⚠️ « non disponible sur ce dépôt » et le script
# n'échoue pas. Tout autre refus, sur n'importe quel autre réglage, est un ❌. À la LECTURE, un 404 sur
# `immutable-releases` signifie « désactivé » (❌), sauf si l'écriture du même lancement vient d'être
# classée « non disponible » (⚠️ répété).
#
# Usage     : scripts/regler-depot-github.sh [--appliquer | --verifier | --a-blanc] [--depot PROPRIÉTAIRE/NOM] [--aide]
#   --verifier   (défaut) relit chaque réglage avec `gh api` : une ligne ✅ / ⚠️ / ❌ par réglage
#   --appliquer  pose les réglages, puis enchaîne --verifier. Refusé si le dépôt n'est pas public (les
#                rulesets et les attestations l'exigent sur un compte gratuit)
#   --a-blanc    n'appelle PAS `gh` : imprime chaque requête sur une ligne `REQUÊTE <MÉTHODE> <chemin>`
#                suivie du corps JSON sur une ligne (ou `{}`). Sert au test
#   --depot      dépôt visé (défaut : F1ghtc0re/WiMao)
#   --aide       affiche cette aide
# Prérequis : `gh` connecté en administrateur du dépôt (`gh auth login`), et `jq`.
# Écrit     : rien en local ; --appliquer modifie les réglages du dépôt distant.
# Code      : 0 tout ✅ (⚠️ admis) ; 1 au moins un ❌, ou --appliquer sur un dépôt privé ; 2 erreur d'usage.
# Appelé par : à la main, juste après le passage du dépôt en public (docs/publier-une-version.md) ; la
#              CI (job `chaine`) lance seulement --a-blanc via scripts/tests/tester-regler-depot-github.sh.
set -euo pipefail

# Affiche l'en-tête de ce fichier (les lignes de commentaire du début) comme aide.
aide() {
    sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

# --- Arguments ---------------------------------------------------------------------------------------
mode="verifier"
depot="F1ghtc0re/WiMao"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --aide | -h | --help)
            aide
            exit 0
            ;;
        --appliquer) mode="appliquer"; shift ;;
        --verifier) mode="verifier"; shift ;;
        --a-blanc) mode="a-blanc"; shift ;;
        --depot)
            if [[ $# -lt 2 ]]; then
                echo "Erreur : --depot attend PROPRIÉTAIRE/NOM." >&2
                exit 2
            fi
            depot="$2"
            shift 2
            ;;
        *)
            echo "Erreur : option inconnue « $1 » (voir --aide)." >&2
            exit 2
            ;;
    esac
done

if [[ ! "$depot" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ ]]; then
    echo "Erreur : dépôt « $depot » invalide (attendu : PROPRIÉTAIRE/NOM)." >&2
    exit 2
fi
if ! command -v jq > /dev/null; then
    echo "Erreur : jq est requis (sudo apt install jq)." >&2
    exit 2
fi
if [[ "$mode" != "a-blanc" ]] && ! command -v gh > /dev/null; then
    echo "Erreur : gh (GitHub CLI) est requis pour ce mode." >&2
    exit 2
fi

# --- Aides communes ----------------------------------------------------------------------------------
erreurs=0   # nombre de lignes ❌ ; décide du code de sortie
reponse=""  # corps de la dernière réponse de `appeler` (en cas d'échec : corps d'erreur + message de gh)
immuables_indisponible=0  # 1 si la POSE de immutable-releases a été classée « non disponible » (404/422)
etat_pose=0  # résultat du dernier `poser` (voir cette fonction)
fichier_erreur="$(mktemp)"  # sortie d'erreur de gh, lue seulement en cas d'échec
trap 'rm -f "$fichier_erreur"' EXIT

ok() { echo "✅ $1"; }
avertir() { echo "⚠️  $1"; }
ko() {
    echo "❌ $1"
    erreurs=$((erreurs + 1))
}

# appeler <MÉTHODE> <chemin> [corps JSON]
# Envoie la requête avec `gh api` (le corps passe par l'entrée standard) et range la réponse dans $reponse.
# En --a-blanc, imprime la requête au lieu de l'envoyer. Codes de retour :
#   0 succès ; 3 « non disponible » (HTTP 404 ou 422) ; 1 autre erreur (le message est dans $reponse).
# gh écrit « (HTTP 404) » sur sa sortie d'erreur : elle est gardée À PART dans $fichier_erreur et n'est
# ajoutée à $reponse qu'en cas d'échec (sur un succès, $reponse reste le JSON seul, sans pollution).
appeler() {
    local methode="$1" chemin="$2" corps="${3:-{\}}"
    if [[ "$mode" == "a-blanc" ]]; then
        printf 'REQUÊTE %s %s\n%s\n' "$methode" "$chemin" "$corps"
        reponse=""
        return 0
    fi
    local code=0
    if [[ "$methode" == "GET" ]]; then
        reponse="$(gh api "$chemin" 2> "$fichier_erreur")" || code=$?
    else
        reponse="$(printf '%s' "$corps" | gh api -X "$methode" "$chemin" --input - 2> "$fichier_erreur")" || code=$?
    fi
    if [[ "$code" -eq 0 ]]; then
        return 0
    fi
    # Échec : le corps d'erreur de GitHub (stdout) puis le message de gh (stderr, avec « HTTP nnn »).
    reponse="$reponse$(cat "$fichier_erreur")"
    if [[ "$reponse" == *"HTTP 404"* || "$reponse" == *"HTTP 422"* ]]; then
        return 3
    fi
    return 1
}

# poser <strict|tolerer> <libellé> <MÉTHODE> <chemin> [corps]
# Envoie une requête d'écriture et affiche ✅ / ⚠️ / ❌ selon le résultat. (En --a-blanc : rien d'autre
# que la requête imprimée.) Seul « tolerer » transforme un 404/422 en ⚠️ « non disponible » (réservé aux
# réglages récents que GitHub ne propose pas partout : immutable-releases). Avec « strict », TOUT refus est
# un ❌ qui cite la réponse de GitHub (par ex. le message de validation d'un 422 sur un ruleset).
# Résultat : $etat_pose = 0 posé, 3 non disponible toléré, 1 refus.
poser() {
    local tolerance="$1" libelle="$2" code=0
    shift 2
    appeler "$@" || code=$?
    if [[ "$code" -eq 3 && "$tolerance" != "tolerer" ]]; then
        code=1
    fi
    if [[ "$mode" == "a-blanc" ]]; then
        return 0
    fi
    case "$code" in
        0) ok "$libelle : posé" ;;
        3) avertir "$libelle : non disponible sur ce dépôt" ;;
        *) ko "$libelle : refusé par GitHub (${reponse:0:300})" ;;
    esac
    etat_pose="$code"   # pas de `return` : un code non nul arrêterait le script (set -e)
}

# lire <chemin> : GET ; le code de retour (0 / 3 / 1, comme `appeler`) laisse l'appelant choisir la ligne
# à afficher.
lire() {
    appeler GET "$1"
}

# controler <libellé> <filtre jq> : vérifie que $reponse satisfait le filtre (jq -e) et affiche ✅ / ❌.
controler() {
    local libelle="$1" filtre="$2"
    if jq -e "$filtre" > /dev/null 2>&1 <<< "$reponse"; then
        ok "$libelle"
    else
        ko "$libelle : valeur inattendue"
    fi
}

# verifier_lecture <libellé> <chemin> <filtre jq>
# Lit <chemin> puis contrôle la réponse. Un 404/422 est un ❌ (réglage absent ou désactivé).
verifier_lecture() {
    local libelle="$1" chemin="$2" filtre="$3" code=0
    lire "$chemin" || code=$?
    case "$code" in
        0) controler "$libelle" "$filtre" ;;
        3) ko "$libelle : absent ou désactivé (HTTP 404/422)" ;;
        *) ko "$libelle : lecture impossible (${reponse:0:200})" ;;
    esac
}

# --- Rulesets ----------------------------------------------------------------------------------------
nom_ruleset_main="main protégée"
nom_ruleset_etiquettes="étiquettes de version"

# Corps JSON du ruleset de la branche par défaut. Les trois contrôles exigés sont les jobs de ci.yml.
# Aucun contournement : `bypass_actors` est vide, l'administrateur est soumis aux règles comme les autres.
corps_ruleset_main() {
    jq -n -c --arg nom "$nom_ruleset_main" '{
        name: $nom,
        target: "branch",
        enforcement: "active",
        conditions: {ref_name: {include: ["~DEFAULT_BRANCH"], exclude: []}},
        bypass_actors: [],
        rules: [
            {type: "deletion"},
            {type: "non_fast_forward"},
            {type: "required_signatures"},
            {type: "pull_request", parameters: {
                required_approving_review_count: 0,
                required_review_thread_resolution: true,
                dismiss_stale_reviews_on_push: false,
                require_code_owner_review: false,
                require_last_push_approval: false}},
            {type: "required_status_checks", parameters: {
                strict_required_status_checks_policy: false,
                required_status_checks: [{context: "unitaires"}, {context: "integration"}, {context: "chaine"}]}}
        ]}'
}

# Corps JSON du ruleset des étiquettes `v*` : création, déplacement (update) et suppression interdits,
# sauf pour l'administrateur (mode `always`).
corps_ruleset_etiquettes() {
    jq -n -c --arg nom "$nom_ruleset_etiquettes" '{
        name: $nom,
        target: "tag",
        enforcement: "active",
        conditions: {ref_name: {include: ["refs/tags/v*"], exclude: []}},
        bypass_actors: [{actor_id: 5, actor_type: "RepositoryRole", bypass_mode: "always"}],
        rules: [{type: "creation"}, {type: "update"}, {type: "deletion"}]}'
}

# (Les valeurs `false` comptent : on garde des objets entiers plutôt que `valeur // null`, qui avalerait false.)
# Filtre jq qui ne garde d'un ruleset que ce qui compte pour nous. GitHub ajoute des champs (id, dates,
# paramètres par défaut) : comparer les JSON entiers échouerait toujours, on compare ce résumé.
# Attention : dans la construction d'un objet, jq 1.7 (celui d'Ubuntu 24.04, donc des runners de la CI)
# n'accepte comme valeur qu'un terme simple : `clé: (…)? // null` y est une erreur de syntaxe (jq 1.8
# l'accepte). Toute valeur qui utilise `?` ou `//` doit donc être entièrement entre parenthèses :
# `clé: ((…)? // null)`.
resume_ruleset='{
    target, enforcement,
    include: .conditions.ref_name.include,
    contournement: [(.bypass_actors // [])[] | {actor_id, actor_type, bypass_mode}] | sort_by(.actor_id),
    regles: [.rules[].type] | sort,
    pr: ((first(.rules[] | select(.type == "pull_request")).parameters
          | {required_approving_review_count, required_review_thread_resolution, dismiss_stale_reviews_on_push,
             require_code_owner_review, require_last_push_approval})? // null),
    strict: ((first(.rules[] | select(.type == "required_status_checks")).parameters
              | {strict_required_status_checks_policy})? // null),
    controles: ([.rules[] | select(.type == "required_status_checks") | .parameters.required_status_checks[].context] | sort)
}'

# Cherche le ruleset de ce nom et range son identifiant dans $ruleset_id (vide s'il n'existe pas). Retour
# 0 même sans ruleset ; 1 si la liste est illisible (le message est alors dans $reponse). Elle n'est pas
# appelée dans un `$(...)` : un sous-shell perdrait $reponse.
ruleset_id=""
id_ruleset() {
    local nom="$1" code=0
    ruleset_id=""
    lire "repos/$depot/rulesets" || code=$?
    if [[ "$code" -ne 0 ]]; then
        return 1
    fi
    ruleset_id="$(jq -r --arg n "$nom" '[.[] | select(.name == $n) | .id][0] // empty' <<< "$reponse")"
}

# regler_ruleset <poser|verifier> <nom> <fonction qui donne le corps>
regler_ruleset() {
    local action="$1" nom="$2" fabrique_corps="$3" id corps
    corps="$("$fabrique_corps")"
    if [[ "$action" == "poser" ]]; then
        if [[ "$mode" == "a-blanc" ]]; then
            appeler POST "repos/$depot/rulesets" "$corps"
            return 0
        fi
        if ! id_ruleset "$nom"; then
            ko "Ruleset « $nom » : liste illisible (${reponse:0:200})"
            return 0
        fi
        id="$ruleset_id"
        if [[ -n "$id" ]]; then
            poser strict "Ruleset « $nom » (mise à jour)" PUT "repos/$depot/rulesets/$id" "$corps"
        else
            poser strict "Ruleset « $nom » (création)" POST "repos/$depot/rulesets" "$corps"
        fi
        return 0
    fi
    # Vérification : retrouver le ruleset par son nom, le relire en entier, comparer les résumés.
    if ! id_ruleset "$nom"; then
        ko "Ruleset « $nom » : liste illisible (${reponse:0:200})"
        return 0
    fi
    id="$ruleset_id"
    if [[ -z "$id" ]]; then
        ko "Ruleset « $nom » : absent"
        return 0
    fi
    local code=0
    lire "repos/$depot/rulesets/$id" || code=$?
    if [[ "$code" -ne 0 ]]; then
        ko "Ruleset « $nom » : lecture impossible (${reponse:0:200})"
        return 0
    fi
    if jq -n -e --argjson reel "$reponse" --argjson voulu "$corps" \
        "def resume: $resume_ruleset; (\$reel | resume) == (\$voulu | resume)" > /dev/null 2>&1; then
        ok "Ruleset « $nom » conforme"
    else
        ko "Ruleset « $nom » : ne correspond pas aux valeurs attendues"
    fi
}

regler_ruleset_main() { regler_ruleset "$1" "$nom_ruleset_main" corps_ruleset_main; }
regler_ruleset_etiquettes() { regler_ruleset "$1" "$nom_ruleset_etiquettes" corps_ruleset_etiquettes; }

# --- Autres réglages ---------------------------------------------------------------------------------
regler_signalement_prive() {
    if [[ "$1" == "poser" ]]; then
        poser strict "Signalement privé des failles" PUT "repos/$depot/private-vulnerability-reporting"
    else
        verifier_lecture "Signalement privé des failles" "repos/$depot/private-vulnerability-reporting" '.enabled == true'
    fi
}

regler_alertes_dependabot() {
    if [[ "$1" == "poser" ]]; then
        poser strict "Alertes Dependabot" PUT "repos/$depot/vulnerability-alerts"
    else
        # GET répond 204 sans corps (activées) ou 404 (désactivées) : le code de retour suffit, pas de
        # contrôle du corps. Un 404 donne donc ❌ (« absent ou désactivé »).
        local code=0
        lire "repos/$depot/vulnerability-alerts" || code=$?
        if [[ "$code" -eq 0 ]]; then
            ok "Alertes Dependabot"
        else
            ko "Alertes Dependabot : désactivées ou illisibles (${reponse:0:200})"
        fi
    fi
}

regler_correctifs_dependabot() {
    if [[ "$1" == "poser" ]]; then
        poser strict "Correctifs Dependabot" PUT "repos/$depot/automated-security-fixes"
    else
        verifier_lecture "Correctifs Dependabot" "repos/$depot/automated-security-fixes" '.enabled == true'
    fi
}

regler_detection_secrets() {
    if [[ "$1" == "poser" ]]; then
        poser strict "Détection des secrets" PATCH "repos/$depot" "$(jq -n -c '{security_and_analysis: {
            secret_scanning: {status: "enabled"},
            secret_scanning_push_protection: {status: "enabled"}}}')"
    else
        verifier_lecture "Détection des secrets" "repos/$depot" \
            '.security_and_analysis.secret_scanning.status == "enabled" and .security_and_analysis.secret_scanning_push_protection.status == "enabled"'
    fi
}

regler_jeton_workflows() {
    if [[ "$1" == "poser" ]]; then
        poser strict "Jeton des workflows en lecture seule" PUT "repos/$depot/actions/permissions/workflow" \
            "$(jq -n -c '{default_workflow_permissions: "read", can_approve_pull_request_reviews: false}')"
    else
        verifier_lecture "Jeton des workflows en lecture seule" "repos/$depot/actions/permissions/workflow" \
            '.default_workflow_permissions == "read" and .can_approve_pull_request_reviews == false'
    fi
}

# Deux requêtes : d'abord « seules certaines actions sont autorisées », puis la liste. Les actions de
# GitHub sont permises ; celles de « créateurs vérifiés » non ; une seule action tierce : dtolnay/rust-toolchain.
regler_actions_autorisees() {
    if [[ "$1" == "poser" ]]; then
        poser strict "Actions autorisées (mode liste)" PUT "repos/$depot/actions/permissions" \
            "$(jq -n -c '{enabled: true, allowed_actions: "selected"}')"
        poser strict "Actions autorisées (liste)" PUT "repos/$depot/actions/permissions/selected-actions" \
            "$(jq -n -c '{github_owned_allowed: true, verified_allowed: false, patterns_allowed: ["dtolnay/rust-toolchain@*"]}')"
    else
        verifier_lecture "Actions autorisées (mode liste)" "repos/$depot/actions/permissions" \
            '.enabled == true and .allowed_actions == "selected"'
        verifier_lecture "Actions autorisées (liste)" "repos/$depot/actions/permissions/selected-actions" \
            '.github_owned_allowed == true and .verified_allowed == false and .patterns_allowed == ["dtolnay/rust-toolchain@*"]'
    fi
}

regler_ci_prs_exterieures() {
    if [[ "$1" == "poser" ]]; then
        poser strict "CI des PR extérieures validée à la main" PUT "repos/$depot/actions/permissions/fork-pr-contributor-approval" \
            "$(jq -n -c '{approval_policy: "all_external_contributors"}')"
    else
        verifier_lecture "CI des PR extérieures validée à la main" "repos/$depot/actions/permissions/fork-pr-contributor-approval" \
            '.approval_policy == "all_external_contributors"'
    fi
}

# Une Release publiée ne peut plus être modifiée ; le brouillon reste modifiable, donc `signer-version.sh`
# peut encore y joindre la signature. Réglage récent : seule sa POSE tolère un 404/422 (⚠️).
regler_releases_immuables() {
    if [[ "$1" == "poser" ]]; then
        poser tolerer "Releases immuables" PUT "repos/$depot/immutable-releases"
        if [[ "$etat_pose" -eq 3 ]]; then
            immuables_indisponible=1
        fi
        return 0
    fi
    # GET répond 404 quand le réglage est DÉSACTIVÉ : c'est un ❌. Seule exception : la pose, pendant ce
    # même lancement (--appliquer), vient d'être classée « non disponible » (404/422) ; la vérification
    # le répète alors en ⚠️ au lieu d'échouer.
    local code=0
    lire "repos/$depot/immutable-releases" || code=$?
    case "$code" in
        0) controler "Releases immuables" '.enabled == true' ;;
        3)
            if [[ "$immuables_indisponible" -eq 1 ]]; then
                avertir "Releases immuables : non disponible sur ce dépôt"
            else
                ko "Releases immuables : désactivées (HTTP 404/422)"
            fi
            ;;
        *) ko "Releases immuables : lecture impossible (${reponse:0:200})" ;;
    esac
}

# Tous les réglages, dans l'ordre d'affichage.
reglages=(
    regler_ruleset_main
    regler_ruleset_etiquettes
    regler_signalement_prive
    regler_alertes_dependabot
    regler_correctifs_dependabot
    regler_detection_secrets
    regler_jeton_workflows
    regler_actions_autorisees
    regler_ci_prs_exterieures
    regler_releases_immuables
)

# Ligne ✅ / ❌ sur la visibilité : les rulesets exigent un dépôt public (compte gratuit).
verifier_visibilite() {
    local code=0
    lire "repos/$depot" || code=$?
    if [[ "$code" -ne 0 ]]; then
        ko "Dépôt $depot : lecture impossible (${reponse:0:200})"
    elif jq -e '.visibility == "public"' > /dev/null 2>&1 <<< "$reponse"; then
        ok "Dépôt $depot public"
    else
        ko "Dépôt $depot : pas public (les rulesets l'exigent sur un compte gratuit)"
    fi
}

# --- Déroulement -------------------------------------------------------------------------------------
case "$mode" in
    a-blanc)
        for r in "${reglages[@]}"; do "$r" poser; done
        ;;
    appliquer)
        # Avant toute écriture : refuser un dépôt non public.
        code=0
        lire "repos/$depot" || code=$?
        if [[ "$code" -ne 0 ]]; then
            echo "Erreur : lecture de $depot impossible (${reponse:0:200})" >&2
            exit 1
        fi
        if ! jq -e '.visibility == "public"' > /dev/null 2>&1 <<< "$reponse"; then
            echo "Erreur : $depot n'est pas public. Les rulesets (et les attestations) exigent un dépôt public" >&2
            echo "sur un compte GitHub gratuit : passez d'abord le dépôt en public, puis relancez." >&2
            exit 1
        fi
        for r in "${reglages[@]}"; do "$r" poser; done
        echo "--- Vérification ---"
        for r in "${reglages[@]}"; do "$r" verifier; done
        ;;
    verifier)
        verifier_visibilite
        for r in "${reglages[@]}"; do "$r" verifier; done
        ;;
esac

if [[ "$erreurs" -gt 0 ]]; then
    if [[ "$mode" != "a-blanc" ]]; then
        echo "$erreurs réglage(s) en échec."
    fi
    exit 1
fi
