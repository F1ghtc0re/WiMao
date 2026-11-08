#!/usr/bin/env bash
# Teste l'installation des trois RPM WiMao dans un Fedora vierge (conteneur Fedora 44 ou 45), leur
# utilisation réelle (Wine, yabridge, Carla, plugin Dexed), puis leur retrait. Lancé par le job
# `installation-rpm` de .github/workflows/paquets.yml. C'est le pendant Fedora de
# paquets/tester-installation.sh (qui teste les .deb) ; seules les vérifications propres aux RPM
# (rpm -V, conflit avec le Wine de Fedora, dnf remove) sont écrites ici.
#
# ATTENTION : ce script installe des paquets, crée un utilisateur « essai » et écrit dans /opt,
# /usr et /home. Il se lance en root DANS UN CONTENEUR JETABLE, jamais sur une machine de travail.
# Limite : un conteneur n'applique pas SELinux ; les étiquettes SELinux des fichiers installés ne sont
# donc PAS testées ici.
#
# Usage     : paquets/tester-installation-rpm.sh <dossier des .rpm et de SHA256SUMS> <dossier des journaux>
# Variables : DEXED_ZIP  chemin de Dexed-1.1.0-win.zip, si on l'a déjà (sinon le script le télécharge
#                        et vérifie son empreinte, comme le job de la CI pour les .deb) ;
#                        l'empreinte est vérifiée dans les deux cas.
# Écrit     : <journaux>/{installation,verifier,diagnostiquer,pipewire}.log ; hors de là, ce que font
#             dnf et l'utilisateur « essai » (/home/essai : données de WiMao, préfixe Wine, ~/.vst3/yabridge).
# Code      : 0 si tout passe ; 1 au premier écart (message « ERREUR : … » sur la sortie d'erreur).
#
# Étapes (chacune annoncée par « == n. … ») :
#   0. SHA256SUMS vérifié avant toute installation.
#   1. dnf install du paquet `wine` de Fedora, PUIS des trois RPM (dépôts de la distribution seulement),
#      puis des outils de test (installés APRÈS : ils ne doivent pas masquer une dépendance manquante).
#      Aucun conflit de fichiers avec le Wine de Fedora, qui garde /usr/bin/wine.
#   2. Contrôles : version de wimao-cli, `rpm -V` propre, Wine des paquets = wine-11.20.
#   3. Utilisateur « essai », HOME neuf : init (Wine et yabridge), installation de Dexed, pontage,
#      diagnostic, et PREUVE DE CHARGEMENT : `wimao-cli verifier` ouvre Dexed dans Carla avec le Wine
#      et le yabridge DES PAQUETS (/opt/wimao/*).
#   4. dnf remove : /opt/wimao et /usr/bin/wimao* disparus, Wine de Fedora intact, données de « essai » intactes.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage : $0 <dossier des .rpm> <dossier des journaux>" >&2
    exit 2
fi
RPMS="$(cd "$1" && pwd)"
mkdir -p "$2"
JOURNAUX="$(cd "$2" && pwd)"

PAQUETS=(wimao-wine wimao-bridge wimao)
# Dexed 1.1.0 (GPL-3, github.com/asb2m10/dexed) : mêmes adresse et empreinte que dans le workflow, pour
# les .deb. Sert uniquement à ce test : jamais livré dans un paquet.
DEXED_URL="https://github.com/asb2m10/dexed/releases/download/v1.1.0/Dexed-1.1.0-win.zip"
DEXED_SHA256="4773150a3dfa281d1caef583f493a41ed99ea95b054b5a3479780799ed6279cc"

# Utilisateur de test et son environnement. `env -i` (voir `essai`) : rien ne vient du root,
# en particulier ni WIMAO_WINE_DIR ni WIMAO_YABRIDGE_DIR, pour que WiMao choisisse /opt/wimao/*
# comme sur la machine d'une vraie personne (crates/wimao-core/src/chemins.rs).
HOME_ESSAI=/home/essai
ENV_ESSAI=(
    "HOME=$HOME_ESSAI"
    "XDG_DATA_HOME=$HOME_ESSAI/.local/share"
    "XDG_CONFIG_HOME=$HOME_ESSAI/.config"
    "XDG_CACHE_HOME=$HOME_ESSAI/.cache"
    "XDG_RUNTIME_DIR=/tmp/runtime-essai"
    "PATH=/usr/local/bin:/usr/bin:/bin"
    # Messages en anglais : les contrôles ci-dessous lisent les textes de WiMao.
    "LANG=C.UTF-8"
    "WINEDEBUG=-all"
)

echec() {
    echo "ERREUR : $1" >&2
    exit 1
}

etape() {
    echo
    echo "== $1"
}

# Lance une commande en tant que « essai », avec l'environnement ENV_ESSAI et rien d'autre.
essai() {
    runuser -u essai -- env -i "${ENV_ESSAI[@]}" "$@"
}

# Tout ce que le script écrit va aussi dans le journal.
exec > >(tee "$JOURNAUX/installation.log") 2>&1

# --- 0. Empreintes -------------------------------------------------------------------------
etape "0. Empreintes SHA-256 des paquets"
[ -f "$RPMS/SHA256SUMS" ] || echec "$RPMS/SHA256SUMS absent"
# SHA256SUMS couvre aussi les .deb en CI : on ne contrôle que les lignes des RPM, et on exige les trois.
(cd "$RPMS" && grep -E '\.rpm$' SHA256SUMS | sha256sum -c -) || echec "une empreinte ne correspond pas : paquets non installés"
for paquet in "${PAQUETS[@]}"; do
    ls "$RPMS/$paquet"-[0-9]*.rpm > /dev/null 2>&1 || echec "RPM de $paquet absent de $RPMS"
done

# --- 1. Installation -----------------------------------------------------------------------
etape "1. Installation des paquets"
# Le Wine de Fedora d'abord (Review Focus 4) : wimao-wine doit s'installer à côté, sans conflit de fichiers
# (il vit sous /opt/wimao/wine et ne pose rien dans /usr).
dnf install -y wine
wine_systeme_avant="$(command -v wine)" || echec "le paquet wine de Fedora ne fournit pas la commande wine"
echo "Wine de Fedora : $wine_systeme_avant ($(wine --version))"

# Copie lisible par dnf (le dossier de la CI peut ne pas l'être) ; installation des trois RPM LOCAUX.
# dnf résout leurs Requires et Recommends (Carla, xwininfo) dans les dépôts de Fedora.
COPIE=/var/tmp/wimao-rpms
rm -rf "$COPIE"
mkdir -p "$COPIE"
cp "$RPMS"/*.rpm "$COPIE/"
chmod -R a+rX "$COPIE"
dnf install -y "$COPIE"/wimao-wine-[0-9]*.rpm "$COPIE"/wimao-bridge-[0-9]*.rpm "$COPIE"/wimao-[0-9]*.rpm
for paquet in "${PAQUETS[@]}"; do
    rpm -q "$paquet" || echec "$paquet non installé"
done

# Aucun conflit : le Wine de Fedora est toujours là, et c'est lui qui répond à `wine`.
[ "$(command -v wine)" = "$wine_systeme_avant" ] || echec "la commande wine a changé après l'installation de WiMao"
rpm -q wine > /dev/null || echec "le paquet wine de Fedora a disparu"
rpm -V wine || echec "rpm -V wine signale des écarts : WiMao a modifié des fichiers du Wine de Fedora"
echo "ok : wimao-wine cohabite avec le paquet wine de Fedora"

# Outils de test, après les paquets (voir l'en-tête). jack-tools (jack_lsp) : sert à savoir si le serveur audio
# répond. Le serveur lui-même n'est PAS JACK : dans Fedora, Carla dépend de PipeWire (paquet
# pipewire-jack-audio-connection-kit), dont la bibliothèque libjack remplace celle de JACK ; un vrai `jackd`
# n'y démarre d'ailleurs pas (« not implemented », constaté en Fedora 45). On lance donc `pipewire` comme sur
# un poste Fedora, ce qui donne à Carla un moteur audio dans le conteneur ; sans lui `verifier` ne teste rien.
# Polices et fontconfig : une image vierge n'en a presque pas et Wine en a besoin pour créer son préfixe.
# Carla n'est pas listé ici : c'est un Recommends de wimao, installé par dnf avec lui (et exigé plus bas).
dnf install -y --setopt=install_weak_deps=False \
    jack-tools \
    dejavu-sans-fonts liberation-sans-fonts fontconfig curl unzip procps-ng util-linux
for outil in carla-single Xvfb xvfb-run xwininfo pipewire jack_lsp runuser; do
    command -v "$outil" > /dev/null || echec "outil absent après installation : $outil (Carla et xwininfo viennent des Recommends de wimao)"
done

# --- 2. Contrôles des fichiers installés ---------------------------------------------------
etape "2. Fichiers installés"
# Version : celle du RPM wimao (0.5.0 pour wimao_0.5.0-1_amd64.deb). Sortie attendue : « wimao-cli X.Y.Z ».
version_rpm="$(rpm -q --qf '%{VERSION}' wimao)"
version_cli="$(env -i LANG=C.UTF-8 PATH=/usr/bin:/bin wimao-cli --version)"
echo "wimao-cli --version : $version_cli (RPM : $version_rpm)"
[ "$version_cli" = "wimao-cli $version_rpm" ] || echec "wimao-cli --version rend « $version_cli », attendu « wimao-cli $version_rpm »"

# rpm -V : aucun écart (taille, somme, droits, propriétaire, lien) sur les trois paquets, juste après l'installation.
for paquet in "${PAQUETS[@]}"; do
    ecarts="$(rpm -V "$paquet" || true)"
    [ -z "$ecarts" ] || echec "rpm -V $paquet signale des écarts :
$ecarts"
    echo "ok : rpm -V $paquet"
done

# Le Wine des paquets, appelé par son chemin complet (il ne remplace pas /usr/bin/wine).
version_wine="$(env -i PATH=/usr/bin:/bin WINEDEBUG=-all /opt/wimao/wine/bin/wine --version)"
echo "/opt/wimao/wine/bin/wine --version : $version_wine"
[ "$version_wine" = "wine-11.20" ] || echec "Wine des paquets : « $version_wine », attendu « wine-11.20 »"

# --- 3. Utilisateur « essai » : init, installation, pontage, vérification ------------------
etape "3. Utilisateur « essai »"
useradd --create-home --shell /bin/bash essai
install -d -o essai -g essai -m 0700 /tmp/runtime-essai

# Dexed : vrai plugin VST3 Windows, installé depuis son archive (méthode « extraire »).
FIXTURES_ESSAI=/var/tmp/wimao-fixtures
rm -rf "$FIXTURES_ESSAI"
mkdir -p "$FIXTURES_ESSAI"
if [ -n "${DEXED_ZIP:-}" ]; then
    cp "$DEXED_ZIP" "$FIXTURES_ESSAI/Dexed-1.1.0-win.zip"
else
    curl -fsSL -o "$FIXTURES_ESSAI/Dexed-1.1.0-win.zip" "$DEXED_URL"
fi
echo "$DEXED_SHA256  $FIXTURES_ESSAI/Dexed-1.1.0-win.zip" | sha256sum -c - || echec "empreinte de Dexed incorrecte"
chmod -R a+rX "$FIXTURES_ESSAI"

echo "-- wimao-cli init"
essai timeout 900 xvfb-run -a wimao-cli --langue en init
echo "-- wimao-cli installer Dexed-1.1.0-win.zip"
# --sans-test : le test d'ouverture (long) est fait plus bas par `verifier`, dont la sortie est jugée.
essai timeout 900 xvfb-run -a wimao-cli --langue en installer "$FIXTURES_ESSAI/Dexed-1.1.0-win.zip" --oui --sans-test
[ -e "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" ] \
    || echec "Dexed.vst3 absent de ~/.vst3/yabridge : le plugin n'est pas ponté"
echo "ok : ~/.vst3/yabridge/Dexed.vst3"

# WiMao pose lui-même ce lien (pontage::assurer_lien_yabridge) : les « chainloaders » de
# ~/.vst3/yabridge y cherchent la bibliothèque de yabridge.
lien="$HOME_ESSAI/.local/share/yabridge"
[ -L "$lien" ] || echec "$lien n'est pas un lien symbolique"
[ "$(readlink "$lien")" = /opt/wimao/yabridge ] \
    || echec "$lien vise $(readlink "$lien") et non /opt/wimao/yabridge"
echo "ok : $lien -> /opt/wimao/yabridge"

echo "-- wimao-cli diagnostiquer"
essai wimao-cli --langue en diagnostiquer | tee "$JOURNAUX/diagnostiquer.log"
grep -qx 'No problem found' "$JOURNAUX/diagnostiquer.log" \
    || echec "diagnostiquer signale des problèmes (voir ci-dessus)"

# Preuve de chargement : ni WIMAO_WINE_DIR ni WIMAO_YABRIDGE_DIR dans l'environnement d'« essai »,
# donc WiMao utilise /opt/wimao/wine et /opt/wimao/yabridge. `verifier` ouvre le plugin ponté dans Carla
# (écran Xvfb à lui) avec le serveur audio PipeWire lancé ci-dessous. Seul verdict admis, sans exception ni
# assouplissement : Dexed ✅ « The plugin opens » (sa fenêtre apparaît). Carla est obligatoire ici : il existe
# dans Fedora, un Carla absent est donc une erreur de paquet et non un cas prévu. Le faux plugin WiMaoTest
# n'est pas installé : sa fabrique VST3 est vide, ses verdicts sont ambigus (voir tester-installation.sh).
echo "-- wimao-cli verifier (plusieurs minutes)"
# Serveur audio PipeWire (sans carte son : pilote factice), lancé par « essai » ; arrêté plus bas par son PID exact (jamais par son nom).
essai timeout 900 pipewire > "$JOURNAUX/pipewire.log" 2>&1 &
pid_audio=$!
for _ in $(seq 1 20); do
    essai jack_lsp > /dev/null 2>&1 && break
    sleep 1
done
if ! essai jack_lsp > /dev/null 2>&1; then
    cat "$JOURNAUX/pipewire.log"
    kill "$pid_audio" || true
    echec "le serveur audio PipeWire ne répond pas à jack_lsp"
fi
echo "ok : serveur audio PipeWire démarré (jack_lsp répond)"
code=0
essai timeout 900 wimao-cli --langue en verifier > "$JOURNAUX/verifier.log" 2>&1 || code=$?
# $pid_audio est celui de `runuser`, qui relaie SIGTERM à ses enfants (timeout puis pipewire).
kill "$pid_audio" || true
wait "$pid_audio" 2> /dev/null || true
cat "$JOURNAUX/verifier.log"
attendu="Dexed | VST3 | ✅ | The plugin opens"
# -x -F : ligne entière, texte littéral (les « | » d'une expression régulière seraient des alternatives).
lignes="$(grep -F "Dexed | VST3 | " "$JOURNAUX/verifier.log" || true)"
[ "$lignes" = "$attendu" ] || echec "verifier : attendu « $attendu », obtenu « $lignes »"
echo "ok : $attendu"
[ "$code" -eq 0 ] || echec "verifier a rendu le code $code (attendu : 0)"

# --- 4. Retrait ----------------------------------------------------------------------------
etape "4. Retrait des paquets"
# Les Recommends (Carla, xwininfo) ne sont pas retirés avec wimao : ils ne sont pas ceux de WiMao.
dnf remove -y wimao wimao-bridge wimao-wine
[ ! -e /opt/wimao ] || echec "/opt/wimao existe encore :
$(find /opt/wimao | head -n 20)"
restes="$(find /usr/bin -maxdepth 1 -name 'wimao*')"
[ -z "$restes" ] || echec "restes dans /usr/bin : $restes"
installes="$(rpm -qa 'wimao*')"
[ -z "$installes" ] || echec "paquets encore installés : $installes"
# Le Wine de Fedora n'est pas touché par le retrait.
rpm -q wine > /dev/null || echec "le paquet wine de Fedora a disparu au retrait"
rpm -V wine || echec "rpm -V wine signale des écarts après le retrait"
# Les données de l'utilisateur ne sont jamais touchées par le retrait.
[ -d "$HOME_ESSAI/.local/share/wimao" ] || echec "le dossier de données de WiMao (.local/share/wimao) a disparu"
{ [ -e "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" ] || [ -L "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" ]; } \
    || echec "le plugin ponté (.vst3/yabridge/Dexed.vst3) a disparu"
[ -L "$lien" ] || echec "$lien a disparu"
echo "ok : données de l'utilisateur intactes"

echo
echo "== Installation, usage et retrait : tout est passé =="
