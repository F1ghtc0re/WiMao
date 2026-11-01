#!/usr/bin/env bash
# Vérifie que les workflows GitHub (.github/workflows/*.yml) sont « figés » :
#   1. chaque `uses:` désigne un commit complet (40 caractères hexadécimaux après `@`),
#      sauf les actions locales (`./...`) ;
#   2. chaque workflow a un bloc `permissions:` au niveau racine (colonne 0) ;
#   3. chaque image `container:` / `image:` est figée par `@sha256:<64 hexadécimaux>` ;
#   4. toute matrice est un bloc lisible (jamais `matrix: ${{ fromJSON(...) }}` ni autre valeur en ligne) ;
#   5. aucun droit d'écriture (`write`, `write-all`, valeur en ligne autre que `read-all`/`{}`) dans un bloc
#      `permissions:`, sauf `contents`, `id-token` et `attestations` du seul job `brouillon` de paquets.yml ;
#   6. aucun déclencheur `pull_request_target` ni `workflow_run` : ils exécutent le code d'une demande de
#      fusion (ou d'un autre workflow) avec un jeton en écriture, le schéma classique d'attaque des workflows ;
#   7. aucune expression `secrets.` (`secrets[...]`, `secrets: inherit`) : la chaîne n'utilise aucun secret.
#      Le jeton automatique `github.token` (`${{ github.token }}`) reste permis.
#
# Ce n'est PAS un analyseur YAML complet : il lit les workflows ligne par ligne et refuse volontairement
# ce qu'il ne sait pas lire (formes en ligne `{...}`, fins de ligne CRLF, expressions `${{ }}` sauf `image: ${{ matrix.image }}`, guillemets…).
# En cas de doute il échoue : mieux vaut un faux refus qu'une action non figée acceptée.
#
# Usage   : paquets/verifier-workflows.sh [dossier-de-workflows]
#           (par défaut : .github/workflows à la racine du dépôt)
# Variables : aucune.
# Écrit   : rien ; la liste des écarts va sur la sortie d'erreur. Code 0 = tout est figé, 1 = écart trouvé.
# Utilisé par le job `chaine` de la CI.
set -euo pipefail

racine="$(cd "$(dirname "$0")/.." && pwd)"
dossier="${1:-${racine}/.github/workflows}"
erreurs=0

# Signale un écart sur la sortie d'erreur et le compte.
echec() {
    echo "ERREUR : $1" >&2
    erreurs=$((erreurs + 1))
}

shopt -s nullglob
fichiers=("${dossier}"/*.yml "${dossier}"/*.yaml)
if [ "${#fichiers[@]}" -eq 0 ]; then
    echo "ERREUR : aucun workflow trouvé dans ${dossier}" >&2
    exit 1
fi

for f in "${fichiers[@]}"; do
    # 0. Fins de ligne CRLF : refusées (les contrôles ci-dessous supposent des lignes terminées par \n).
    if grep -q $'\r' "${f}"; then
        echec "${f}: fins de ligne CRLF non supportées"
    fi

    # 1. Actions figées par empreinte de commit. Échec par défaut : toute ligne (hors commentaire)
    # contenant `uses:` n'importe où (y compris `- {uses: ...}`, `[..., uses: ...]`) doit avoir
    # exactement la forme « uses: proprietaire/action@<40 hexadécimaux minuscules> [# commentaire] »
    # ou désigner une action locale `./...`. Tout le reste est refusé.
    while IFS= read -r ligne; do
        if ! [[ "${ligne}" =~ ^[[:space:]]*(-[[:space:]]+)?uses:[[:space:]]+(\./[^[:space:]]*|[A-Za-z0-9._/-]+@[0-9a-f]{40})([[:space:]]+#.*)?$ ]]; then
            echec "${f}: « uses: » absent de la forme figée acceptée : ${ligne}"
        fi
    done < <(grep -E '(^|[^A-Za-z0-9_-])uses:' "${f}" | grep -vE '^[[:space:]]*#' || true)

    # 2. Bloc permissions: à la racine du fichier.
    if ! grep -qE '^permissions:' "${f}"; then
        echec "${f}: pas de bloc « permissions: » au niveau racine"
    fi

    # 3. Images de conteneur figées par condensat. Même principe : toute ligne (hors commentaire)
    # contenant `container:` ou `image:` doit être « image: nom@sha256:<64 hexadécimaux> », ou un
    # `container:` seul (bloc dont la clé `image:` est contrôlée sur sa propre ligne). Les formes en
    # ligne (`container: {image: ...}`) sont donc refusées.
    # Seule expression admise : « image: ${{ matrix.image }} » (conteneur d'un job à matrice). Elle ne peut
    # venir que de la clé `image:` de la matrice, qui est une ligne `image:` comme les autres, donc figée.
    while IFS= read -r ligne; do
        if [[ "${ligne}" =~ ^[[:space:]]*(-[[:space:]]+)?container:[[:space:]]*$ ]]; then
            continue
        fi
        if [[ "${ligne}" =~ ^[[:space:]]*image:[[:space:]]+\$\{\{[[:space:]]*matrix\.image[[:space:]]*\}\}([[:space:]]+#.*)?$ ]]; then
            continue
        fi
        if ! [[ "${ligne}" =~ ^[[:space:]]*(-[[:space:]]+)?(container|image):[[:space:]]+[A-Za-z0-9._/:-]+@sha256:[0-9a-f]{64}([[:space:]]+#.*)?$ ]]; then
            echec "${f}: image non figée par @sha256: (ou forme non lisible) : ${ligne}"
        fi
    done < <(grep -E '(^|[^A-Za-z0-9_-])(container|image):' "${f}" | grep -vE '^[[:space:]]*#' || true)

    # 4. Matrice lisible : « matrix: » et « include: » doivent ouvrir un bloc (valeur vide). Une valeur
    # en ligne, notamment `matrix: ${{ fromJSON(...) }}`, fabriquerait des images que l'on ne peut pas
    # lire ici et contournerait l'exception « image: ${{ matrix.image }} » du contrôle 3.
    while IFS= read -r ligne; do
        echec "${f}: matrice non lisible (valeur en ligne ou expression) : ${ligne}"
    done < <(grep -E '^[[:space:]]*(-[[:space:]]+)?(matrix|include):[[:space:]]*[^[:space:]#]' "${f}" | grep -vE '^[[:space:]]*#' || true)

    # 5. Droits d'écriture. Lecture ligne à ligne : le job courant est la dernière clé à 2 espaces
    # d'indentation sous `jobs:` ; un bloc `permissions:` couvre les lignes plus indentées qui le suivent.
    # Un `permissions:` avec valeur en ligne n'est admis que s'il vaut `read-all` ou `{}` ; tout le reste
    # (write-all, {contents: write}…) est refusé. Seul le job `brouillon` de paquets.yml peut écrire, et
    # seulement ces trois droits : contents, id-token, attestations.
    while IFS= read -r ligne; do
        echec "${f}: ${ligne}"
    done < <(awk -v brouillon_permis="$([ "$(basename "${f}")" = "paquets.yml" ] && echo 1 || echo 0)" '
        function indent(l) { match(l, /^ */); return RLENGTH }
        /^[[:space:]]*#/ || /^[[:space:]]*$/ { next }
        {
            n = indent($0)
            if (n == 0) { enjobs = ($0 ~ /^jobs:/); job = "" }
            if (enjobs && n == 2 && $0 ~ /^  [A-Za-z0-9_-]+:/) { job = $0; sub(/^  /, "", job); sub(/:.*/, "", job) }
            if (bloc && n <= base) bloc = 0
            if ($0 ~ /^[[:space:]]*permissions:/) {
                valeur = $0; sub(/^[[:space:]]*permissions:[[:space:]]*/, "", valeur); sub(/[[:space:]]+#.*$/, "", valeur)
                base = n
                if (valeur == "") { bloc = 1 }
                else if (valeur != "read-all" && valeur != "{}") {
                    print "droits « permissions: " valeur " » en ligne refusés (job « " job " ») : " $0
                }
                next
            }
            if (bloc && $0 ~ /write/) {
                cle = $0; sub(/^[[:space:]]*/, "", cle); sub(/:.*/, "", cle)
                if (!(brouillon_permis == 1 && job == "brouillon" && (cle == "contents" || cle == "id-token" || cle == "attestations") && $0 ~ /:[[:space:]]*write[[:space:]]*(#.*)?$/))
                    print "droit d\047écriture interdit (job « " job " ») : " $0
            }
        }' "${f}")

    # 6. Déclencheurs dangereux (hors commentaires). `pull_request_target` et `workflow_run` donnent au code
    # d'une demande de fusion (ou d'un workflow déclenché par elle) les droits et le contexte du dépôt de
    # base : refusés quel que soit l'endroit où ils s'écrivent (liste, mapping, forme en ligne).
    while IFS= read -r ligne; do
        echec "${f}: déclencheur dangereux refusé (pull_request_target / workflow_run) : ${ligne}"
    done < <(grep -E '(^|[^A-Za-z0-9_])(pull_request_target|workflow_run)([^A-Za-z0-9_]|$)' "${f}" | grep -vE '^[[:space:]]*#' || true)

    # 7. Secrets. La contrainte de la chaîne est « aucun secret » : `secrets.X`, `secrets['X']`,
    # `secrets: inherit` sont refusés. `github.token` n'est pas un secret du dépôt (jeton temporaire de
    # l'exécution, déjà limité par `permissions:`) et ne correspond pas à ce motif.
    while IFS= read -r ligne; do
        echec "${f}: secret refusé (la chaîne n'en utilise aucun) : ${ligne}"
    done < <(grep -E '(^|[^A-Za-z0-9_-])secrets([.[]|:)' "${f}" | grep -vE '^[[:space:]]*#' || true)
    # Le contexte entier dans une expression (`${{ toJSON(secrets) }}`) ne passe pas non plus.
    while IFS= read -r ligne; do
        echec "${f}: secret refusé (contexte « secrets » dans une expression) : ${ligne}"
    done < <(grep -E '\$\{\{[^}]*(^|[^A-Za-z0-9_.-])secrets([^A-Za-z0-9_-]|$)' "${f}" | grep -vE '^[[:space:]]*#' || true)
done

if [ "${erreurs}" -gt 0 ]; then
    echo "${erreurs} écart(s) : les workflows ne sont pas figés." >&2
    exit 1
fi
echo "Workflows figés : ${#fichiers[@]} fichier(s) vérifié(s)."
