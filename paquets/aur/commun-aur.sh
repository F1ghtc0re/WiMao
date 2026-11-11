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
#   MARQUE_ESSAI_LOCAL  texte qui signale une recette d'essai local (seule définition) ; tous les contrôles
#               cherchent ce texte comme sous-chaîne (grep -qF), jamais comme ligne exacte
# Fonctions :
#   utilisateur_constructeur            crée l'utilisateur « constructeur » s'il manque (en root, dans un conteneur)
#   srcinfo <dossier>                   écrit <dossier>/.SRCINFO à partir de <dossier>/PKGBUILD
#   srcinfo_ici <dossier>               idem, mais directement (makepkg doit être présent : conteneur Arch)
#   echec <message> / etape <titre>     message « ERREUR : … » puis exit 1 / titre d'étape « == … » (scripts d'essai)
#   importer_cle_constructeur <fichier>  importe la clé publique <fichier> dans le trousseau de « constructeur »
#   construire_installer_recette <dossier> <journaux>
#                                       makepkg en « constructeur » dans <dossier>, puis pacman -U (en root)
#   outils_de_test_arch                 installe APRÈS le paquet les outils du test (Carla, xwininfo, serveur audio)
#   essai_utilisation_wimao <fixtures> <journaux>
#                                       version, init, installation de Dexed, diagnostic, `verifier` (preuve de chargement)
# Dépend de : makepkg (ou podman pour srcinfo hors d'un système Arch), runuser, useradd, mktemp, cp ; pour les
# fonctions d'essai : un conteneur Arch jetable, en root.

IMAGE_ARCH='docker.io/library/archlinux:base-devel@sha256:97119fd9e21219b02b3c14ba256c9d3c0d031d07fc26c836c41ed793beeb5b74'

# Texte du marquage « essai local » (écrit par fabriquer-pkgbuild.sh --local, qui en garde sa propre copie
# littérale : le test de ce script vérifie que les deux concordent). Les refus le cherchent PARTOUT dans le
# PKGBUILD, même précédé ou suivi d'autres caractères (espace final, commentaire ajouté...).
# shellcheck disable=SC2034  # utilisée par les scripts qui sourcent cette bibliothèque
MARQUE_ESSAI_LOCAL='ESSAI LOCAL'

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

# --- Fonctions des scripts d'essai (conteneur Arch jetable, en root) ---------------------------------------

# Adresse et empreinte de Dexed 1.1.0 (GPL-3) : mêmes valeurs que dans le workflow et que
# paquets/tester-installation-rpm.sh. Sert uniquement aux essais : jamais livré dans un paquet.
DEXED_URL="https://github.com/asb2m10/dexed/releases/download/v1.1.0/Dexed-1.1.0-win.zip"
DEXED_SHA256="4773150a3dfa281d1caef583f493a41ed99ea95b054b5a3479780799ed6279cc"

# Utilisateur de test « essai » : `env -i` (voir `essai`) garantit que rien ne vient du root, en particulier ni
# WIMAO_WINE_DIR ni WIMAO_YABRIDGE_DIR, pour que WiMao choisisse /opt/wimao/* comme chez une vraie personne.
HOME_ESSAI=/home/essai
ENV_ESSAI=(
    "HOME=$HOME_ESSAI"
    "XDG_DATA_HOME=$HOME_ESSAI/.local/share"
    "XDG_CONFIG_HOME=$HOME_ESSAI/.config"
    "XDG_CACHE_HOME=$HOME_ESSAI/.cache"
    "XDG_RUNTIME_DIR=/tmp/runtime-essai"
    "PATH=/usr/local/bin:/usr/bin:/bin"
    # Messages en anglais : les contrôles lisent les textes de WiMao.
    "LANG=C.UTF-8"
    "WINEDEBUG=-all"
)

# echec : message d'erreur sur la sortie d'erreur et arrêt (code 1).
echec() {
    echo "ERREUR : $1" >&2
    exit 1
}

# etape : annonce une étape du script.
etape() {
    echo
    echo "== $1"
}

# essai : lance une commande en tant que « essai », avec l'environnement ENV_ESSAI et rien d'autre.
essai() {
    runuser -u essai -- env -i "${ENV_ESSAI[@]}" "$@"
}

# importer_cle_constructeur <fichier> : importe la clé publique <fichier> (docs/cle-publique.asc) dans le trousseau
# de « constructeur » : makepkg y cherche la clé qui a signé SHA256SUMS (validpgpkeys), ce que toute personne
# doit faire avant d'installer la recette. Le fichier est d'abord copié lisible par « constructeur » (l'original
# peut être dans un dossier privé ou monté en lecture seule). À lancer en root, après utilisateur_constructeur.
importer_cle_constructeur() {
    install -m 644 -- "$1" /var/tmp/cle-publique.asc
    runuser -u constructeur -- gpg --batch --quiet --import /var/tmp/cle-publique.asc
}

# construire_installer_recette <dossier> <journaux> : construit le paquet de la recette <dossier>/PKGBUILD
# (les sources sont déjà à côté) puis l'installe. makepkg tourne sous « constructeur » (il refuse root) dans une
# copie du dossier ; sa sortie complète va dans <journaux>/makepkg.log. `pacman -U` résout les dépendances
# obligatoires dans les dépôts d'Arch ; makepkg, lui, tourne avec --nodeps (il vérifierait sinon les dépendances
# d'exécution AVANT la construction, alors que rien ne les exige pour extraire des .deb).
construire_installer_recette() {
    local dossier="$1" journaux="$2" travail paquet
    utilisateur_constructeur
    travail=/var/tmp/construction-aur
    rm -rf "$travail"
    mkdir -p "$travail"
    cp -- "$dossier"/* "$dossier"/.SRCINFO "$travail/"
    chown -R constructeur "$travail"
    chmod 755 "$travail"
    (cd "$travail" && runuser -u constructeur -- makepkg --nodeps) 2>&1 | tee "$journaux/makepkg.log" \
        && [ "${PIPESTATUS[0]}" -eq 0 ] || echec "makepkg a échoué (voir $journaux/makepkg.log)"
    paquet="$(find "$travail" -maxdepth 1 -name 'wimao-bin-*.pkg.tar.*' | head -n 1)"
    [ -n "$paquet" ] || echec "makepkg n'a produit aucun paquet"
    pacman -U --noconfirm "$paquet"
    pacman -Q wimao-bin || echec "wimao-bin non installé"
}

# outils_de_test_arch : installés APRÈS le paquet de WiMao, pour ne pas masquer une dépendance obligatoire
# manquante. Carla et xwininfo sont des dépendances facultatives de la recette (optdepends) ; le serveur audio
# est PipeWire (pipewire-jack fournit libjack à Carla, jack-example-tools donne jack_lsp) : un vrai jackd
# n'a pas de carte son dans un conteneur. vulkan-swrast (Vulkan logiciel) tient lieu de carte graphique : sans
# aucun pilote Vulkan, DXVK n'initialise pas (« Failed to create Vulkan instance »), l'hôte Wine de yabridge
# plante et Carla ne voit pas Dexed s'ouvrir (constaté : verdict « Loads correctly » au lieu de « The plugin
# opens »). Un poste réel a toujours le pilote Vulkan de sa carte graphique.
outils_de_test_arch() {
    pacman -S --noconfirm --needed carla xorg-xwininfo pipewire pipewire-jack jack-example-tools vulkan-swrast
    local outil
    for outil in curl Xvfb xvfb-run xwininfo pipewire jack_lsp runuser; do
        command -v "$outil" > /dev/null || echec "outil absent après installation : $outil"
    done
    command -v carla-single > /dev/null || command -v Carla > /dev/null || command -v carla > /dev/null \
        || echec "Carla absent après installation"
}

# essai_utilisation_wimao <dossier des fixtures> <journaux> : contrôles d'un WiMao installé, identiques à ceux
# de paquets/tester-installation-rpm.sh : version, Wine des paquets, utilisateur « essai », init, Dexed
# (téléchargé et vérifié, ou DEXED_ZIP), pontage, diagnostic et `verifier` (seul verdict admis :
# « Dexed | VST3 | ✅ | The plugin opens »).
essai_utilisation_wimao() {
    local fixtures="$1" journaux="$2" version_paquet version_cli version_wine lien code pid_audio attendu lignes
    etape "Fichiers installés"
    # Version du paquet Arch « 0.7.0-1 » -> 0.7.0. Sortie attendue : « wimao-cli X.Y.Z ».
    version_paquet="$(pacman -Q wimao-bin | awk '{ sub(/-[0-9]+$/, "", $2); print $2 }')"
    version_cli="$(env -i LANG=C.UTF-8 PATH=/usr/bin:/bin wimao-cli --version)"
    echo "wimao-cli --version : $version_cli (paquet : $version_paquet)"
    [ "$version_cli" = "wimao-cli $version_paquet" ] || echec "wimao-cli --version rend « $version_cli », attendu « wimao-cli $version_paquet »"
    version_wine="$(env -i PATH=/usr/bin:/bin WINEDEBUG=-all /opt/wimao/wine/bin/wine --version)"
    echo "/opt/wimao/wine/bin/wine --version : $version_wine"
    [ "$version_wine" = "wine-11.20" ] || echec "Wine des paquets : « $version_wine », attendu « wine-11.20 »"

    etape "Utilisateur « essai »"
    useradd --create-home --shell /bin/bash essai
    install -d -o essai -g essai -m 0700 /tmp/runtime-essai
    mkdir -p "$fixtures"
    if [ -n "${DEXED_ZIP:-}" ]; then
        cp "$DEXED_ZIP" "$fixtures/Dexed-1.1.0-win.zip"
    else
        curl -fsSL -o "$fixtures/Dexed-1.1.0-win.zip" "$DEXED_URL"
    fi
    echo "$DEXED_SHA256  $fixtures/Dexed-1.1.0-win.zip" | sha256sum -c - || echec "empreinte de Dexed incorrecte"
    chmod -R a+rX "$fixtures"

    echo "-- wimao-cli init"
    essai timeout 900 xvfb-run -a wimao-cli --langue en init
    echo "-- wimao-cli installer Dexed-1.1.0-win.zip"
    # --sans-test : le test d'ouverture (long) est fait plus bas par `verifier`, dont la sortie est jugée.
    essai timeout 900 xvfb-run -a wimao-cli --langue en installer "$fixtures/Dexed-1.1.0-win.zip" --oui --sans-test
    [ -e "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" ] || echec "Dexed.vst3 absent de ~/.vst3/yabridge : le plugin n'est pas ponté"
    echo "ok : ~/.vst3/yabridge/Dexed.vst3"
    # WiMao pose lui-même ce lien : les « chainloaders » de ~/.vst3/yabridge y cherchent la bibliothèque de yabridge.
    lien="$HOME_ESSAI/.local/share/yabridge"
    [ -L "$lien" ] || echec "$lien n'est pas un lien symbolique"
    [ "$(readlink "$lien")" = /opt/wimao/yabridge ] || echec "$lien vise $(readlink "$lien") et non /opt/wimao/yabridge"
    echo "ok : $lien -> /opt/wimao/yabridge"

    echo "-- wimao-cli diagnostiquer"
    essai wimao-cli --langue en diagnostiquer | tee "$journaux/diagnostiquer.log"
    grep -qx 'No problem found' "$journaux/diagnostiquer.log" || echec "diagnostiquer signale des problèmes (voir ci-dessus)"

    # Preuve de chargement avec le Wine et le yabridge DU PAQUET. Serveur audio PipeWire (pilote factice), lancé
    # par « essai » et arrêté par son PID exact (jamais par son nom).
    echo "-- wimao-cli verifier (plusieurs minutes)"
    essai timeout 900 pipewire > "$journaux/pipewire.log" 2>&1 &
    pid_audio=$!
    for _ in $(seq 1 20); do
        essai jack_lsp > /dev/null 2>&1 && break
        sleep 1
    done
    if ! essai jack_lsp > /dev/null 2>&1; then
        cat "$journaux/pipewire.log"
        kill "$pid_audio" || true
        echec "le serveur audio PipeWire ne répond pas à jack_lsp"
    fi
    echo "ok : serveur audio PipeWire démarré (jack_lsp répond)"
    code=0
    essai timeout 900 wimao-cli --langue en verifier > "$journaux/verifier.log" 2>&1 || code=$?
    # $pid_audio est celui de `runuser`, qui relaie SIGTERM à ses enfants (timeout puis pipewire).
    kill "$pid_audio" || true
    wait "$pid_audio" 2> /dev/null || true
    cat "$journaux/verifier.log"
    attendu="Dexed | VST3 | ✅ | The plugin opens"
    lignes="$(grep -F "Dexed | VST3 | " "$journaux/verifier.log" || true)"
    [ "$lignes" = "$attendu" ] || echec "verifier : attendu « $attendu », obtenu « $lignes »"
    echo "ok : $attendu"
    [ "$code" -eq 0 ] || echec "verifier a rendu le code $code (attendu : 0)"
}
