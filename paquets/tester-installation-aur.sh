#!/usr/bin/env bash
# Teste, dans un Arch Linux vierge (conteneur), que la recette AUR de wimao-bin se construit et installe un
# WiMao qui marche (Wine, yabridge, Carla, plugin Dexed), puis qu'elle se désinstalle proprement. Lancé par le
# job `installation-aur` de .github/workflows/paquets.yml, avant toute étiquette de version. Pendant Arch de
# paquets/tester-installation-rpm.sh ; la logique commune (construction, outils, usage) est dans
# paquets/aur/commun-aur.sh, que réutilise aussi le test de la Release publiée.
#
# ATTENTION : ce script installe des paquets, crée les utilisateurs « constructeur » et « essai » et écrit dans
# /opt, /usr et /home. Il se lance en root DANS UN CONTENEUR JETABLE, jamais sur une machine de travail.
#
# Usage     : paquets/tester-installation-aur.sh <dossier des .deb et de SHA256SUMS> <dossier des journaux>
# Variables : DEXED_ZIP  chemin de Dexed-1.1.0-win.zip, si on l'a déjà (sinon téléchargé) ; l'empreinte est
#                        vérifiée dans les deux cas.
# Écrit     : <journaux>/{installation,makepkg,verifier,diagnostiquer,pipewire}.log ; hors de là, ce que font
#             pacman, makepkg (/var/tmp) et l'utilisateur « essai » (/home/essai).
# Code      : 0 si tout passe ; 1 au premier écart (message « ERREUR : … » sur la sortie d'erreur) ; 2 mauvais usage.
#
# Étapes (chacune annoncée par « == … ») :
#   0. SHA256SUMS : les trois .deb sont vérifiés avant tout.
#   1. Recette en mode --local (sources = .deb voisins, pas de signature : celle-ci est éprouvée par aur.yml),
#      makepkg en « constructeur », pacman -U ; PUIS les outils de test (Carla, xwininfo, audio).
#   2. Version, utilisateur « essai », init, Dexed, diagnostic, `verifier` : preuve de chargement.
#   3. pacman -R : /opt/wimao et /usr/bin/wimao* disparus, données de « essai » intactes.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage : $0 <dossier des .deb> <dossier des journaux>" >&2
    exit 2
fi
racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=paquets/aur/commun-aur.sh
source "$racine/paquets/aur/commun-aur.sh"
DEBS="$(cd "$1" && pwd)"
mkdir -p "$2"
JOURNAUX="$(cd "$2" && pwd)"

# Tout ce que le script écrit va aussi dans le journal.
exec > >(tee "$JOURNAUX/installation.log") 2>&1

# --- 0. Empreintes -------------------------------------------------------------------------
etape "0. Empreintes SHA-256 des paquets"
[ -f "$DEBS/SHA256SUMS" ] || echec "$DEBS/SHA256SUMS absent"
# SHA256SUMS couvre aussi les .rpm en CI : on ne contrôle que les lignes des .deb, et on exige les trois.
(cd "$DEBS" && grep -E '\.deb$' SHA256SUMS | sha256sum -c -) || echec "une empreinte ne correspond pas : rien n'est construit"
[ "$(grep -cE '\.deb$' "$DEBS/SHA256SUMS")" -eq 3 ] || echec "SHA256SUMS doit lister exactement trois .deb"

# --- 1. Recette, construction, installation --------------------------------------------------
etape "1. Recette AUR, construction et installation"
# Les outils de base de la construction et de l'essai (la recette, elle, n'en a pas de dépendance de construction).
pacman -Syu --noconfirm --needed git
RECETTE=/var/tmp/recette-aur
rm -rf "$RECETTE"
mkdir -p "$RECETTE"
"$racine/paquets/fabriquer-pkgbuild.sh" "$DEBS/SHA256SUMS" "$RECETTE" --local
cp "$DEBS"/*.deb "$RECETTE/"
construire_installer_recette "$RECETTE" "$JOURNAUX"
outils_de_test_arch

# --- 2. Usage réel -----------------------------------------------------------------------------
essai_utilisation_wimao /var/tmp/wimao-fixtures "$JOURNAUX"

# --- 3. Retrait ----------------------------------------------------------------------------
etape "3. Retrait du paquet"
pacman -R --noconfirm wimao-bin
[ ! -e /opt/wimao ] || echec "/opt/wimao existe encore :
$(find /opt/wimao | head -n 20)"
restes="$(find /usr/bin -maxdepth 1 -name 'wimao*')"
[ -z "$restes" ] || echec "restes dans /usr/bin : $restes"
pacman -Q wimao-bin > /dev/null 2>&1 && echec "wimao-bin encore installé"
# Les données de l'utilisateur ne sont jamais touchées par le retrait.
[ -d "$HOME_ESSAI/.local/share/wimao" ] || echec "le dossier de données de WiMao (.local/share/wimao) a disparu"
{ [ -e "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" ] || [ -L "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" ]; } \
    || echec "le plugin ponté (.vst3/yabridge/Dexed.vst3) a disparu"
echo "ok : données de l'utilisateur intactes"

echo
echo "== Installation, usage et retrait : tout est passé =="
