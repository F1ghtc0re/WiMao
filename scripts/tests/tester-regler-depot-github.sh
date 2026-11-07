#!/usr/bin/env bash
# Teste scripts/regler-depot-github.sh SANS toucher à GitHub, avec un faux `gh`.
#
# Faux `gh` (écrit par ce test dans un dossier temporaire, placé en tête du PATH) :
#   - il consigne chaque appel dans $JOURNAL, une ligne `<MÉTHODE> <chemin> <corps>` (méthode GET sans -X,
#     corps lu sur l'entrée standard si `--input -`) ;
#   - sa réponse vient du dossier $REPONSES : fichier `<MÉTHODE>_<chemin avec / remplacé par _>.json` (sortie,
#     code 0) ou `.err` (texte sur la sortie d'erreur, code 1, par ex. « gh: Not Found (HTTP 404) »).
#     Sans fichier : `{}` pour une écriture ou une lecture, code 0 (`[]` pour la liste des rulesets).
# Seules ces formes sont émulées : `gh api [-X MÉTHODE] chemin [--input -]` (c'est tout ce que le script utilise).
# Les réponses « tout est conforme » sont fabriquées à partir de ce que le script lui-même envoie en --a-blanc.
#
# Cas couverts : voir les titres `# ---` ci-dessous (--a-blanc et valeurs du tableau de la spec § 4,
# --appliquer refusé sur dépôt privé, ruleset existant mis à jour sans doublon, 404 → ⚠️, erreur → ❌,
# --verifier tout conforme, Pages / environnement / politique de branches, option inconnue).
#
# Usage     : scripts/tests/tester-regler-depot-github.sh
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$racine/scripts/regler-depot-github.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT
depot="essai/depot"

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

# --- Faux gh -----------------------------------------------------------------------------------------
mkdir "$temporaire/bin"
cat > "$temporaire/bin/gh" <<'FAUX'
#!/usr/bin/env bash
# Faux gh : voir l'en-tête de tester-regler-depot-github.sh.
set -euo pipefail
[[ "$1" == "api" ]] || { echo "faux gh : commande inattendue $1" >&2; exit 99; }
shift
methode="GET"; chemin=""; corps=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        -X) methode="$2"; shift 2 ;;
        --input) corps="$(cat)"; shift 2 ;;
        *) chemin="$1"; shift ;;
    esac
done
echo "$methode $chemin $corps" >> "$JOURNAL"
fichier="$REPONSES/${methode}_${chemin//\//_}"
if [[ -f "$fichier.err" ]]; then
    cat "$fichier.err" >&2
    exit 1
fi
if [[ -f "$fichier.json" ]]; then
    cat "$fichier.json"
elif [[ "$chemin" == */rulesets ]]; then
    echo "[]"
else
    echo "{}"
fi
FAUX
chmod +x "$temporaire/bin/gh"
export JOURNAL="$temporaire/journal"
export PATH="$temporaire/bin:$PATH"

# Prépare un dossier de réponses vide et vide le journal.
neuf() {
    export REPONSES="$temporaire/reponses-$1"
    rm -rf "$REPONSES"
    mkdir -p "$REPONSES"
    : > "$JOURNAL"
}

# Lance le script et range code et sortie dans $code et $sortie.
lancer() {
    code=0
    sortie="$("$script" "$@" 2>&1)" || code=$?
}

attendre_code() {
    [[ "$code" -eq "$1" ]] || echec "$2 : code $code au lieu de $1 (sortie : $sortie)"
}

# --- --a-blanc : aucun appel à gh, corps JSON valides et valeurs clés ---------------------------------
neuf blanc
lancer --a-blanc --depot "$depot"
attendre_code 0 "--a-blanc"
[[ ! -s "$JOURNAL" ]] || echec "--a-blanc a appelé gh : $(cat "$JOURNAL")"

# Dictionnaire des requêtes : un fichier par ligne REQUÊTE, nommé <MÉTHODE>_<chemin>, contenant le corps.
mkdir "$temporaire/blanc"
awk -v dir="$temporaire/blanc" '
    /^REQUÊTE / { gsub("/", "_", $3); fichier = dir "/" $2 "_" $3; attend = 1; next }
    attend { print > fichier; close(fichier); attend = 0 }' <<< "$sortie"
for f in "$temporaire"/blanc/*; do
    jq -e . "$f" > /dev/null || echec "corps JSON invalide : $f"
done
# corps <MÉTHODE> [sous-chemin après repos/<dépôt>/] : fichier du corps de cette requête (voir l'awk ci-dessus).
corps() {
    local suite="${2:-}"
    echo "$temporaire/blanc/$1_repos_${depot//\//_}${suite:+_${suite//\//_}}"
}

# Vérifie qu'un filtre jq est vrai sur un corps.
verifier_corps() {
    jq -e "$3" "$(corps "$1" "$2")" > /dev/null || echec "$1 $2 : « $3 » faux"
}

# Les rulesets sont tous deux en POST sur .../rulesets : le second écrase le premier dans le dictionnaire.
# On relit donc la sortie complète, ligne par ligne.
main="$(grep -F '"name":"main protégée"' <<< "$sortie")"
etiq="$(grep -F '"name":"étiquettes de version"' <<< "$sortie")"
jq -e '.target == "branch" and .conditions.ref_name.include == ["~DEFAULT_BRANCH"]' <<< "$main" > /dev/null || echec "main : cible"
jq -e '.bypass_actors == []' <<< "$main" > /dev/null || echec "main : la liste de contournement doit être vide"
jq -e '[.rules[] | select(.type=="required_status_checks") | .parameters.required_status_checks[].context] | sort == ["chaine","integration","unitaires"]' <<< "$main" > /dev/null || echec "main : contrôles"
jq -e '[.rules[].type] | contains(["deletion","non_fast_forward","required_signatures","pull_request"])' <<< "$main" > /dev/null || echec "main : règles"
jq -e '.rules[] | select(.type=="pull_request") | .parameters | .required_approving_review_count == 0 and .required_review_thread_resolution == true' <<< "$main" > /dev/null || echec "main : pull_request"
jq -e '.target == "tag" and .conditions.ref_name.include == ["refs/tags/v*"] and ([.rules[].type] | sort) == ["creation","deletion","update"] and .bypass_actors[0].bypass_mode == "always"' <<< "$etiq" > /dev/null || echec "étiquettes"

verifier_corps PUT "actions/permissions/workflow" '.default_workflow_permissions == "read" and .can_approve_pull_request_reviews == false'
verifier_corps PUT "actions/permissions" '.allowed_actions == "selected"'
verifier_corps PUT "actions/permissions/selected-actions" '.patterns_allowed == ["dtolnay/rust-toolchain@*"]'
verifier_corps PUT "actions/permissions/fork-pr-contributor-approval" '.approval_policy == "all_external_contributors"'
verifier_corps PATCH "" '.security_and_analysis.secret_scanning_push_protection.status == "enabled"'
# GitHub Pages : en --a-blanc, on ignore si Pages existe, donc la création (POST) ET le réglage (PUT) sont imprimés.
verifier_corps POST "pages" '. == {build_type: "workflow"}'
verifier_corps PUT "pages" '. == {build_type: "workflow", https_enforced: true}'
verifier_corps PUT "environments/github-pages" '. == {deployment_branch_policy: {protected_branches: false, custom_branch_policies: true}}'
verifier_corps POST "environments/github-pages/deployment-branch-policies" '. == {name: "main", type: "branch"}'
for r in private-vulnerability-reporting vulnerability-alerts automated-security-fixes immutable-releases; do
    [[ -f "$(corps PUT "$r")" ]] || echec "requête PUT $r absente"
done

# --- Réponses « tout est conforme », fabriquées à partir des corps de --a-blanc ------------------------
# Remplit $REPONSES pour que --verifier affiche uniquement des ✅.
tout_conforme() {
    printf '[{"id":1,"name":"main protégée"},{"id":2,"name":"étiquettes de version"}]' \
        > "$REPONSES/GET_repos_${depot//\//_}_rulesets.json"
    printf '%s' "$main" > "$REPONSES/GET_repos_${depot//\//_}_rulesets_1.json"
    printf '%s' "$etiq" > "$REPONSES/GET_repos_${depot//\//_}_rulesets_2.json"
    jq '. + {visibility: "public"}' "$(corps PATCH "")" > "$REPONSES/GET_repos_${depot//\//_}.json"
    for r in private-vulnerability-reporting automated-security-fixes immutable-releases; do
        echo '{"enabled":true}' > "$REPONSES/GET_repos_${depot//\//_}_$r.json"
    done
    cp "$(corps PUT actions/permissions/workflow)" "$REPONSES/GET_repos_${depot//\//_}_actions_permissions_workflow.json"
    cp "$(corps PUT actions/permissions)" "$REPONSES/GET_repos_${depot//\//_}_actions_permissions.json"
    cp "$(corps PUT actions/permissions/selected-actions)" "$REPONSES/GET_repos_${depot//\//_}_actions_permissions_selected-actions.json"
    # Pages, environnement et politique de branches : ce que GitHub renverrait après nos écritures.
    cp "$(corps PUT pages)" "$REPONSES/GET_repos_${depot//\//_}_pages.json"
    cp "$(corps PUT environments/github-pages)" "$REPONSES/GET_repos_${depot//\//_}_environments_github-pages.json"
    echo '{"total_count":1,"branch_policies":[{"id":7,"name":"main","type":"branch"}]}' \
        > "$REPONSES/GET_repos_${depot//\//_}_environments_github-pages_deployment-branch-policies.json"
    cp "$(corps PUT actions/permissions/fork-pr-contributor-approval)" "$REPONSES/GET_repos_${depot//\//_}_actions_permissions_fork-pr-contributor-approval.json"
}

# --- --verifier : tout conforme → code 0, aucune écriture ---------------------------------------------
neuf conforme
tout_conforme
lancer --verifier --depot "$depot"
attendre_code 0 "--verifier conforme"
[[ "$sortie" != *"❌"* ]] || echec "--verifier conforme affiche un ❌ : $sortie"
! grep -Eq '^(POST|PUT|PATCH) ' "$JOURNAL" || echec "--verifier a écrit : $(cat "$JOURNAL")"

# --- --verifier seul : 404 sur immutable-releases = désactivé → ❌ et code 1 -------------------------------------------
echo "gh: Not Found (HTTP 404)" > "$REPONSES/GET_repos_${depot//\//_}_immutable-releases.err"
rm "$REPONSES/GET_repos_${depot//\//_}_immutable-releases.json"
lancer --verifier --depot "$depot"
attendre_code 1 "immutable-releases 404 en lecture seule (= désactivé)"
grep -Fq "❌ Releases immuables" <<< "$sortie" || echec "pas de ❌ pour immutable-releases : $sortie"

# --- --verifier : ruleset différent (une approbation exigée) → ❌ et code 1 ---------------------------
neuf different
tout_conforme
jq -c '(.rules[] | select(.type=="pull_request") | .parameters.required_approving_review_count) = 1' <<< "$main" \
    > "$REPONSES/GET_repos_${depot//\//_}_rulesets_1.json"
lancer --verifier --depot "$depot"
attendre_code 1 "ruleset différent"
grep -Fq "❌ Ruleset « main protégée »" <<< "$sortie" || echec "pas de ❌ pour le ruleset : $sortie"

# --- --verifier : revue obligatoire du propriétaire de code activée (doit être faux) → ❌ -------------------
neuf codeowner
tout_conforme
jq -c '(.rules[] | select(.type=="pull_request") | .parameters.require_code_owner_review) = true' <<< "$main" \
    > "$REPONSES/GET_repos_${depot//\//_}_rulesets_1.json"
lancer --verifier --depot "$depot"
attendre_code 1 "require_code_owner_review à true"

# --- --verifier : un contournement ajouté au ruleset de main (doit rester vide) → ❌ ----------------------
neuf contournement
tout_conforme
jq -c '.bypass_actors = [{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]' <<< "$main" \
    > "$REPONSES/GET_repos_${depot//\//_}_rulesets_1.json"
lancer --verifier --depot "$depot"
attendre_code 1 "contournement présent sur main"
grep -Fq "❌ Ruleset « main protégée »" <<< "$sortie" || echec "pas de ❌ pour le contournement : $sortie"

# --- --verifier : actions désactivées (enabled false) → ❌ ---------------------------------------------
neuf actions-off
tout_conforme
jq -c '.enabled = false' "$(corps PUT actions/permissions)" > "$REPONSES/GET_repos_${depot//\//_}_actions_permissions.json"
lancer --verifier --depot "$depot"
attendre_code 1 "actions désactivées"

# --- --verifier : dépôt privé et sans ruleset (cas réel de l'ancien dépôt) → ❌ -------------------------
neuf prive
echo '{"visibility":"private"}' > "$REPONSES/GET_repos_${depot//\//_}.json"
lancer --verifier --depot "$depot"
attendre_code 1 "dépôt privé en --verifier"

# --- --appliquer sur un dépôt privé : code 1, message « public », aucune écriture ----------------------
lancer --appliquer --depot "$depot"
attendre_code 1 "--appliquer sur dépôt privé"
[[ "$sortie" == *"public"* ]] || echec "message sans « public » : $sortie"
! grep -Eq '^(POST|PUT|PATCH) ' "$JOURNAL" || echec "--appliquer sur dépôt privé a écrit : $(cat "$JOURNAL")"

# --- --appliquer, ruleset déjà présent (id 42) : PUT sur 42, aucun POST pour ce nom --------------------
neuf existant
tout_conforme
echo '[{"id":42,"name":"main protégée"}]' > "$REPONSES/GET_repos_${depot//\//_}_rulesets.json"
lancer --appliquer --depot "$depot"
grep -Fq "PUT repos/$depot/rulesets/42 " "$JOURNAL" || echec "pas de PUT sur le ruleset 42 : $(cat "$JOURNAL")"
posts_rulesets="$(grep -E '^POST .*rulesets ' "$JOURNAL" || true)"
if grep -Fq '"name":"main protégée"' <<< "$posts_rulesets"; then
    echec "POST d'un ruleset « main protégée » déjà existant"
fi

# --- --appliquer : erreur (HTTP 500) sur vulnerability-alerts → ❌ et code 1 ---------------------------
neuf erreur
tout_conforme
echo "gh: Server Error (HTTP 500)" > "$REPONSES/PUT_repos_${depot//\//_}_vulnerability-alerts.err"
lancer --appliquer --depot "$depot"
attendre_code 1 "erreur sur vulnerability-alerts"
grep -Fq "❌ Alertes Dependabot" <<< "$sortie" || echec "pas de ❌ pour Dependabot : $sortie"

# --- --appliquer : 404 sur la pose d'immutable-releases → ⚠️, tout le reste conforme → code 0 ----------
neuf indisponible
tout_conforme
echo "gh: Not Found (HTTP 404)" > "$REPONSES/PUT_repos_${depot//\//_}_immutable-releases.err"
echo "gh: Not Found (HTTP 404)" > "$REPONSES/GET_repos_${depot//\//_}_immutable-releases.err"
rm "$REPONSES/GET_repos_${depot//\//_}_immutable-releases.json"
lancer --appliquer --depot "$depot"
attendre_code 0 "--appliquer avec immutable-releases indisponible"
grep -Fq "⚠️  Releases immuables : non disponible" <<< "$sortie" || echec "pas de ⚠️ à la pose : $sortie"

# --- --appliquer : 422 sur le POST du ruleset « main protégée » → ❌, code 1, message de GitHub cité -----
neuf refus422
tout_conforme
echo '[]' > "$REPONSES/GET_repos_${depot//\//_}_rulesets.json"   # aucun ruleset : le script fait un POST
echo '{"message":"Validation Failed: required_status_checks invalide"} gh: Unprocessable Entity (HTTP 422)' \
    > "$REPONSES/POST_repos_${depot//\//_}_rulesets.err"
lancer --appliquer --depot "$depot"
attendre_code 1 "422 sur le POST du ruleset"
grep -Fq "❌ Ruleset « main protégée » (création)" <<< "$sortie" || echec "pas de ❌ pour le 422 : $sortie"
grep -Fq "Validation Failed" <<< "$sortie" || echec "le message de GitHub n'est pas cité : $sortie"

# --- Pages, environnement et politique de branches : --verifier -------------------------------------------
# Chaque cas part de « tout conforme » puis dégrade UN réglage ; il doit donner un ❌ précis et le code 1.
neuf pages-absent
tout_conforme
echo "gh: Not Found (HTTP 404)" > "$REPONSES/GET_repos_${depot//\//_}_pages.err"
rm "$REPONSES/GET_repos_${depot//\//_}_pages.json"
lancer --verifier --depot "$depot"
attendre_code 1 "Pages non activé"
grep -Fq "❌ GitHub Pages" <<< "$sortie" || echec "pas de ❌ pour Pages absent : $sortie"

neuf pages-legacy
tout_conforme
echo '{"build_type":"legacy","https_enforced":true}' > "$REPONSES/GET_repos_${depot//\//_}_pages.json"
lancer --verifier --depot "$depot"
attendre_code 1 "Pages en mode legacy"
grep -Fq "❌ GitHub Pages : source" <<< "$sortie" || echec "pas de ❌ pour build_type legacy : $sortie"

neuf pages-http
tout_conforme
echo '{"build_type":"workflow","https_enforced":false}' > "$REPONSES/GET_repos_${depot//\//_}_pages.json"
lancer --verifier --depot "$depot"
attendre_code 1 "HTTPS non imposé"
grep -Fq "❌ GitHub Pages : HTTPS" <<< "$sortie" || echec "pas de ❌ pour HTTPS : $sortie"

neuf env-protegees
tout_conforme
echo '{"deployment_branch_policy":{"protected_branches":true,"custom_branch_policies":false}}' \
    > "$REPONSES/GET_repos_${depot//\//_}_environments_github-pages.json"
lancer --verifier --depot "$depot"
attendre_code 1 "environnement sans politique personnalisée"
grep -Fq "❌ Environnement github-pages" <<< "$sortie" || echec "pas de ❌ pour l'environnement : $sortie"

neuf politique-autre
tout_conforme
echo '{"total_count":2,"branch_policies":[{"id":7,"name":"main","type":"branch"},{"id":8,"name":"dev","type":"branch"}]}' \
    > "$REPONSES/GET_repos_${depot//\//_}_environments_github-pages_deployment-branch-policies.json"
lancer --verifier --depot "$depot"
attendre_code 1 "seconde politique de branche"
grep -Fq "❌ Branches autorisées" <<< "$sortie" || echec "pas de ❌ pour la seconde politique : $sortie"

neuf politique-etiquette
tout_conforme
echo '{"total_count":1,"branch_policies":[{"id":7,"name":"main","type":"tag"}]}' \
    > "$REPONSES/GET_repos_${depot//\//_}_environments_github-pages_deployment-branch-policies.json"
lancer --verifier --depot "$depot"
attendre_code 1 "politique « main » de type tag"

neuf politique-vide
tout_conforme
echo '{"total_count":0,"branch_policies":[]}' \
    > "$REPONSES/GET_repos_${depot//\//_}_environments_github-pages_deployment-branch-policies.json"
lancer --verifier --depot "$depot"
attendre_code 1 "aucune politique de branche"

# --- Pages, environnement et politique de branches : --appliquer ------------------------------------------
# Pages déjà activé (GET répond 200), politique « main » déjà là : PUT seulement, aucun POST.
neuf pages-existe
tout_conforme
lancer --appliquer --depot "$depot"
attendre_code 0 "--appliquer, Pages et politique déjà en place"
grep -Fq "PUT repos/$depot/pages " "$JOURNAL" || echec "pas de PUT pages : $(cat "$JOURNAL")"
grep -Fq "PUT repos/$depot/environments/github-pages " "$JOURNAL" || echec "pas de PUT environnement : $(cat "$JOURNAL")"
! grep -Eq '^POST [^ ]*(/pages|deployment-branch-policies) ' "$JOURNAL" || echec "POST inutile : $(cat "$JOURNAL")"

# Pages absent (404 sur GET pages) : POST de création PUIS PUT. Politique absente : POST de la politique.
neuf pages-cree
tout_conforme
echo "gh: Not Found (HTTP 404)" > "$REPONSES/GET_repos_${depot//\//_}_pages.err"
rm "$REPONSES/GET_repos_${depot//\//_}_pages.json"
echo '{"total_count":0,"branch_policies":[]}' \
    > "$REPONSES/GET_repos_${depot//\//_}_environments_github-pages_deployment-branch-policies.json"
lancer --appliquer --depot "$depot"
# Le 404 simulé persiste pour la vérification finale : le code 1 est donc attendu, l'ordre des appels compte ici.
ordre="$(grep -E '^(POST|PUT) [^ ]*/pages ' "$JOURNAL" | cut -d' ' -f1 | tr '\n' ' ')"
[[ "$ordre" == "POST PUT " ]] || echec "Pages absent : ordre « $ordre » au lieu de « POST PUT » : $(cat "$JOURNAL")"
grep -Fq "POST repos/$depot/pages {\"build_type\":\"workflow\"}" "$JOURNAL" || echec "corps du POST pages : $(cat "$JOURNAL")"
grep -Fq "POST repos/$depot/environments/github-pages/deployment-branch-policies {\"name\":\"main\",\"type\":\"branch\"}" "$JOURNAL" \
    || echec "pas de POST de la politique main : $(cat "$JOURNAL")"

# Refus de GitHub (HTTP 500) sur l'écriture de l'environnement → ❌ et code 1.
neuf env-refus
tout_conforme
echo "gh: Server Error (HTTP 500)" > "$REPONSES/PUT_repos_${depot//\//_}_environments_github-pages.err"
lancer --appliquer --depot "$depot"
attendre_code 1 "refus sur l'environnement"
grep -Fq "❌ Environnement github-pages" <<< "$sortie" || echec "pas de ❌ pour l'environnement : $sortie"

# --- Erreurs d'usage -----------------------------------------------------------------------------------
lancer --nimporte-quoi
attendre_code 2 "option inconnue"
lancer --depot
attendre_code 2 "--depot sans valeur"
lancer --depot "pas un depot"
attendre_code 2 "dépôt invalide"

echo "OK : tester-regler-depot-github.sh"
