#!/usr/bin/env bash
# Teste les options de tests/fabriquer-fixtures.sh : --seulement-nsis et les erreurs d'usage.
#
# Usage     : tests/tester-fixtures.sh
# Variables : aucune.
# Outils    : makensis (paquet nsis). Sans lui, seules les erreurs d'usage sont vérifiées
#             (message « SAUTÉ » pour le reste) ; la CI (job `installation`) l'installe.
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin.
# Code      : 0 si tout passe, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
script="$racine/fabriquer-fixtures.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

# 1. Erreurs d'usage : code 2 et message d'usage, sans rien fabriquer.
for arguments in "" "--seulement-nsis" "--inconnue $temporaire/a" "$temporaire/a $temporaire/b"; do
    code=0
    # shellcheck disable=SC2086  # découpage voulu : chaque mot est un argument
    sortie="$("$script" $arguments 2>&1)" || code=$?
    [ "$code" -eq 2 ] || echec "« $arguments » devrait rendre le code 2 (reçu $code)"
    [[ "$sortie" == *"Usage :"* ]] || echec "« $arguments » : pas de message d'usage"
done
echo "ok : erreurs d'usage"

# 2. --seulement-nsis : test-nsis.exe seul, aucun autre installeur.
if ! command -v makensis > /dev/null; then
    echo "SAUTÉ : makensis absent, --seulement-nsis non essayé (sudo apt install nsis)"
    exit 0
fi
"$script" --seulement-nsis "$temporaire/sortie" > "$temporaire/journal" 2>&1 \
    || { cat "$temporaire/journal" >&2; echec "--seulement-nsis a échoué"; }
[ -s "$temporaire/sortie/test-nsis.exe" ] || echec "test-nsis.exe absent ou vide"
autres="$(find "$temporaire/sortie" -mindepth 1 ! -name test-nsis.exe)"
[ -z "$autres" ] || echec "fichiers inattendus avec --seulement-nsis : $autres"
echo "ok : --seulement-nsis ne fabrique que test-nsis.exe"
