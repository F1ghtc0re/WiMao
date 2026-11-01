#!/usr/bin/env bash
# Vérifie qu'aucune donnée personnelle ne figure dans les fichiers suivis par git.
#
# Le dépôt est public : on n'y veut ni le nom d'utilisateur Linux du propriétaire, ni le nom de sa
# machine, ni son adresse e-mail personnelle. Ce script cherche ces termes, sans tenir compte de la
# casse, dans les fichiers texte ET binaires SUIVIS par git (`git grep` : les fichiers non suivis, les
# dossiers ignorés et `target/` ne sont jamais examinés). Les binaires (images, archives…) sont fouillés
# dans une seconde passe, qui n'affiche que leur nom (pas de numéro de ligne : il n'a pas de sens).
#
# Termes cherchés (lus sur la machine à l'exécution : AUCUN n'est écrit dans le dépôt) :
#   - le nom d'utilisateur : $USER, à défaut `id -un` ;
#   - le nom de la machine : `hostname` ;
#   - l'e-mail de `git config user.email`, sauf une adresse `noreply.github.com` (publique par nature) ;
#   - les lignes du fichier NON SUIVI <dépôt>/.git/donnees-perso (un terme par ligne ; les lignes vides
#     et celles qui commencent par # sont ignorées) : pour ajouter d'autres termes sensibles ;
#   - ceux donnés par --terme.
#
# Règles pour éviter les faux positifs :
#   - un terme de moins de 3 caractères est ignoré (avec un message) ;
#   - root, runner, ubuntu, localhost, user, utilisateur sont ignorés (avec un message) : ce sont les
#     noms des machines de CI et des exemples, pas des données personnelles ;
#   - la recherche est LITTÉRALE (`git grep -F`) : un point n'est pas un joker.
#
# Sortie : une ligne `fichier:ligne` par occurrence dans un fichier texte, ou `fichier (binaire)` pour un
# fichier binaire qui contient le terme, SANS le terme trouvé ni le texte de la ligne (le journal de CI d'un dépôt public ne doit pas divulguer ce qu'on cherche). Pour voir la ligne, ouvrir le
# fichier à cet endroit. Le message final donne le nombre d'occurrences.
#
# Usage     : scripts/verifier-donnees-perso.sh [--aide] [--dossier <dépôt>] [--terme <texte>]... [--seulement]
#   --aide              affiche cette aide
#   --dossier <dépôt>   examine un autre dépôt git que le dossier du script (défaut : ce dépôt)
#   --terme <texte>     ajoute un terme à chercher (répétable)
#   --seulement         n'utilise QUE les termes de --terme (ni utilisateur, ni machine, ni e-mail, ni
#                       .git/donnees-perso) : sert aux tests, pour ne pas dépendre de la machine
# Variables : USER (nom d'utilisateur ; à défaut `id -un`).
# Écrit     : rien.
# Code      : 0 aucune occurrence ; 1 au moins une occurrence ; 2 erreur d'usage.
# Appelé par : la CI (job `chaine`), et à lancer à la main avant chaque version
#              (voir docs/publier-une-version.md). Testé par scripts/tests/tester-verifier-donnees-perso.sh.
set -euo pipefail

# Affiche l'en-tête de ce fichier (les lignes de commentaire du début) comme aide.
aide() {
    sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'
}

depot=""
seulement=0
termes=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --aide | -h | --help)
            aide
            exit 0
            ;;
        --seulement)
            seulement=1
            shift
            ;;
        --dossier | --terme)
            if [[ $# -lt 2 ]]; then
                echo "Erreur : $1 attend une valeur." >&2
                exit 2
            fi
            if [[ "$1" == "--dossier" ]]; then
                depot="$2"
            else
                termes+=("$2")
            fi
            shift 2
            ;;
        *)
            echo "Erreur : option inconnue « $1 » (voir --aide)." >&2
            exit 2
            ;;
    esac
done

# Par défaut : le dépôt qui contient ce script.
if [[ -z "$depot" ]]; then
    depot="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fi
if ! git -C "$depot" rev-parse --git-dir > /dev/null 2>&1; then
    echo "Erreur : « $depot » n'est pas un dépôt git." >&2
    exit 2
fi

# --- Termes lus sur la machine (sauf --seulement) ---
if [[ "$seulement" -eq 0 ]]; then
    termes+=("${USER:-$(id -un)}")
    # hostname peut manquer sur une machine minimale : le terme est alors simplement absent.
    if machine="$(hostname 2> /dev/null)"; then
        termes+=("$machine")
    fi
    # user.email peut ne pas être défini (cas du runner de CI) : `git config` échoue alors.
    if courriel="$(git -C "$depot" config user.email 2> /dev/null)"; then
        if [[ "$courriel" != *noreply.github.com ]]; then
            termes+=("$courriel")
        fi
    fi
    # Termes supplémentaires du propriétaire, dans un fichier que git ne suit pas (il est dans .git/).
    fichier_termes="$(git -C "$depot" rev-parse --absolute-git-dir)/donnees-perso"
    if [[ -f "$fichier_termes" ]]; then
        while IFS= read -r ligne || [[ -n "$ligne" ]]; do
            # Retire le \r final (fichier CRLF) puis les espaces de début et de fin.
            ligne="${ligne%$'\r'}"
            ligne="${ligne#"${ligne%%[![:space:]]*}"}"
            ligne="${ligne%"${ligne##*[![:space:]]}"}"
            if [[ -n "$ligne" && "$ligne" != \#* ]]; then
                termes+=("$ligne")
            fi
        done < "$fichier_termes"
    fi
fi

# --- Recherche ---
# Les occurrences (fichier:ligne) de tous les termes sont accumulées dans $trouvees, une par ligne.
trouvees=""
for terme in "${termes[@]}"; do
    minuscule="${terme,,}"
    if [[ ${#terme} -lt 3 ]]; then
        echo "Terme trop court (moins de 3 caractères) : ignoré." >&2
        continue
    fi
    case "$minuscule" in
        root | runner | ubuntu | localhost | user | utilisateur)
            echo "Terme générique « $minuscule » : ignoré." >&2
            continue
            ;;
    esac
    # PASSE 1 (fichiers texte) : -n numéro de ligne ; -i sans casse ; -F littéral ; -I ignore les binaires ; -e protège un terme
    # qui commencerait par « - ». `git grep` renvoie 1 s'il ne trouve rien : ce n'est pas une erreur.
    # `cut -d: -f1,2` ne garde que « fichier:ligne » et jette le texte (donc le terme).
    # Codes de git grep : 0 = trouvé, 1 = absent, autre = ERREUR (jamais confondue avec « absent » : ce
    # serait un faux négatif silencieux). Le message d'erreur ne répète pas le terme.
    # NB : le chemin du fichier est affiché tel quel ; un nom de fichier contenant le terme le révélerait,
    # ce qui est acceptable puisque ce chemin figure déjà dans le dépôt.
    rc=0
    resultat="$(git -C "$depot" grep -n -i -F -I -e "$terme")" || rc=$?
    case "$rc" in
        0) trouvees+="$(cut -d: -f1,2 <<< "$resultat")"$'\n' ;;
        1) ;;
        *)
            echo "Erreur : git grep a échoué (code $rc) ; contrôle impossible." >&2
            exit 2
            ;;
    esac

    # PASSE 2 (fichiers binaires) : -a traite un binaire comme du texte, -l ne liste que les NOMS des
    # fichiers qui contiennent le terme (texte ou binaire). On retire ensuite ceux que la passe -I voit
    # (les fichiers texte, déjà signalés plus haut) : il reste les binaires. Même discipline de codes que
    # ci-dessus : 1 = rien, autre non nul = erreur (exit 2), et le terme n'est jamais affiché.
    rc=0
    tous="$(git -C "$depot" grep -l -a -i -F -e "$terme")" || rc=$?
    case "$rc" in
        0)
            rc=0
            textes="$(git -C "$depot" grep -l -i -F -I -e "$terme")" || rc=$?
            if [[ "$rc" -gt 1 ]]; then
                echo "Erreur : git grep a échoué (code $rc) ; contrôle impossible." >&2
                exit 2
            fi
            # comm -13 : lignes présentes dans $tous mais pas dans $textes (les deux listes triées).
            binaires="$(LC_ALL=C comm -13 <(LC_ALL=C sort <<< "$textes") <(LC_ALL=C sort <<< "$tous"))"
            if [[ -n "$binaires" ]]; then
                while IFS= read -r nom; do
                    trouvees+="$nom (binaire)"$'\n'
                done <<< "$binaires"
            fi
            ;;
        1) ;;
        *)
            echo "Erreur : git grep a échoué (code $rc) ; contrôle impossible." >&2
            exit 2
            ;;
    esac
done

# --- Rapport ---
if [[ -z "$trouvees" ]]; then
    echo "Aucune donnée personnelle trouvée."
    exit 0
fi
# Une même ligne trouvée par plusieurs termes n'est comptée et listée qu'une fois.
uniques="$(LC_ALL=C sort -u <<< "${trouvees%$'\n'}")"
echo "$uniques"
echo "$(wc -l <<< "$uniques") occurrence(s) : à retirer avant de publier."
exit 1
