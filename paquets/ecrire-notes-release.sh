#!/usr/bin/env bash
# Écrit sur la sortie standard les notes (en français, Markdown) du brouillon de Release de WiMao :
# version, les paquets avec leur SHA-256, mention de la provenance et de la signature à venir.
# Partagé par les jobs `brouillon` et `brouillon-a-blanc` de .github/workflows/paquets.yml, pour que
# l'essai à blanc d'une demande de fusion exerce EXACTEMENT les mêmes notes que la vraie Release.
#
# Usage     : paquets/ecrire-notes-release.sh <version> <SHA256SUMS>
#             <version> : numéro sans « v » (ex. 0.4.0), ou « a-blanc » pour un essai.
#             <SHA256SUMS> : fichier au format de `sha256sum` (« somme  nom »), un paquet (.deb ou .rpm) par ligne.
# Écrit     : uniquement sur la sortie standard (le workflow la range dans notes.md).
# Code      : 0 ; 1 si le fichier de sommes manque ; 2 si l'usage est mauvais.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage : $0 <version> <SHA256SUMS>" >&2
    exit 2
fi
version="$1"
sommes="$2"
[ -f "$sommes" ] || { echo "ERREUR : $sommes introuvable" >&2; exit 1; }

echo "# WiMao ${version}"
echo
# Les six paquets : les .deb d'abord, puis les RPM (SHA256SUMS est trié par nom, les deux sortes y sont mêlées).
echo "Paquets Debian (Ubuntu 24.04+ et Debian 13+, amd64) :"
echo
while read -r somme nom; do
    case "$nom" in *.deb) echo "- \`${nom}\` : sha256 \`${somme}\`" ;; esac
done < "$sommes"
echo
echo "Paquets RPM (Fedora 44+, x86_64) :"
echo
while read -r somme nom; do
    case "$nom" in *.rpm) echo "- \`${nom}\` : sha256 \`${somme}\`" ;; esac
done < "$sommes"
echo
echo "Provenance : attestation GitHub (fichier \`.intoto.jsonl\`, vérifiable avec \`gh attestation verify\`)."
echo
echo "Signature SHA256SUMS.asc et clé publique à venir (scripts/signer-version.sh)."
