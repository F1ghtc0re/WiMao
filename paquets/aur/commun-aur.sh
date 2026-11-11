#!/usr/bin/env bash
# Bibliothèque SOURCÉE (pas exécutée : sans bit d'exécution) des éléments communs aux scripts de l'AUR
# (paquets/fabriquer-pkgbuild.sh et les scripts d'essai, de publication et d'envoi qui suivent). Elle n'a pas
# de `set -e` propre : elle hérite des options de l'appelant.
#
# Principe : `makepkg` refuse de tourner en root ; on l'exécute donc toujours sous un utilisateur non
# privilégié (« constructeur »), y compris pour `makepkg --printsrcinfo`.
#
# Variable :
#   IMAGE_ARCH  image Arch Linux figée par empreinte (seule définition, pour les scripts comme pour les tests)
# Fonctions :
#   utilisateur_constructeur            crée l'utilisateur « constructeur » s'il manque (en root, dans un conteneur)
#   srcinfo <dossier>                   écrit <dossier>/.SRCINFO à partir de <dossier>/PKGBUILD
#   srcinfo_ici <dossier>               idem, mais directement (makepkg doit être présent : conteneur Arch)
# Dépend de : makepkg (ou podman pour srcinfo hors d'un système Arch), runuser, useradd, mktemp, cp.

IMAGE_ARCH='docker.io/library/archlinux:base-devel@sha256:97119fd9e21219b02b3c14ba256c9d3c0d031d07fc26c836c41ed793beeb5b74'

# Dossier de cette bibliothèque, retenu au moment où elle est sourcée (utile pour la monter dans un conteneur).
DOSSIER_LIB_AUR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# utilisateur_constructeur : crée « constructeur » (avec son dossier personnel) s'il n'existe pas encore.
utilisateur_constructeur() {
    id constructeur > /dev/null 2>&1 || useradd -m constructeur
}

# srcinfo_ici : écrit <dossier>/.SRCINFO. makepkg lit le PKGBUILD dans un dossier de travail temporaire lisible
# par « constructeur » (le dossier d'origine peut être privé : mktemp -d crée en 700), et le résultat est
# écrit par l'appelant, pas par l'utilisateur non privilégié. Le .SRCINFO n'est mis en place QUE si makepkg
# réussit (sinon rien n'est laissé, pas de fichier partiel) ; le dossier temporaire est supprimé dans tous
# les cas. Rend 1 avec un message si makepkg échoue.
srcinfo_ici() {
    local dossier="$1" travail statut=0
    travail="$(mktemp -d)"
    cp -- "$dossier/PKGBUILD" "$travail/PKGBUILD"
    if [ "$(id -u)" -eq 0 ]; then
        utilisateur_constructeur
        chmod 755 "$travail"
        chown -R constructeur "$travail"
        (cd "$travail" && runuser -u constructeur -- makepkg --printsrcinfo) > "$travail/.SRCINFO.nouveau" || statut=$?
    else
        (cd "$travail" && makepkg --printsrcinfo) > "$travail/.SRCINFO.nouveau" || statut=$?
    fi
    if [ "$statut" -eq 0 ]; then
        mv -- "$travail/.SRCINFO.nouveau" "$dossier/.SRCINFO" || statut=$?
    else
        echo "ERREUR : makepkg --printsrcinfo a échoué (code $statut)." >&2
    fi
    rm -rf "$travail"
    return "$statut"
}

# srcinfo : écrit <dossier>/.SRCINFO. Directement si makepkg est présent (conteneur Arch de la CI, machine
# Arch), sinon dans un conteneur IMAGE_ARCH (podman sans racine : l'utilisateur de l'hôte y est « root »).
srcinfo() {
    local dossier
    dossier="$(cd -- "$1" && pwd)"
    if command -v makepkg > /dev/null; then
        srcinfo_ici "$dossier"
    else
        podman run --rm -v "$DOSSIER_LIB_AUR:/lib-aur:ro,Z" -v "$dossier:/sortie:Z" "$IMAGE_ARCH" \
            bash -euo pipefail -c 'source /lib-aur/commun-aur.sh && srcinfo_ici /sortie'
    fi
}
