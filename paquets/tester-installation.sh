#!/usr/bin/env bash
# Teste l'installation des trois paquets WiMao dans un système vierge (conteneur Ubuntu 24.04 ou
# Debian 13), puis leur retrait. Lancé par le job `installation` de .github/workflows/paquets.yml.
#
# ATTENTION : ce script installe des paquets, crée un utilisateur « essai » et écrit dans /opt,
# /usr et /home. Il se lance en root DANS UN CONTENEUR JETABLE, jamais sur une machine de travail.
#
# Usage     : paquets/tester-installation.sh <dossier des .deb et de SHA256SUMS> <dossier des fixtures>
#             Le dossier des fixtures contient test-nsis.exe (tests/fabriquer-fixtures.sh --seulement-nsis).
# Variables : JOURNAUX  dossier des journaux et de la capture d'écran (défaut /tmp/wimao-installation) ;
#                       la CI le publie en artefact (`installation-<distribution>`).
# Écrit     : $JOURNAUX/{installation,lintian,appstream,verifier,interface}.log, $JOURNAUX/interface.png ;
#             hors de là, ce que font l'installation des paquets (apt) et l'utilisateur « essai »
#             (/home/essai : données de WiMao, préfixe Wine, ~/.vst3/yabridge).
# Code      : 0 si tout passe ; 1 au premier écart (message « ERREUR : … » sur la sortie d'erreur).
#
# Étapes (chacune annoncée par « == n. … ») :
#   0. SHA256SUMS vérifié avant toute installation.
#   1. apt-get install des trois .deb (dépôts de la distribution seulement), puis des outils de test.
#      Les outils de test sont installés APRÈS les paquets : ils ne doivent pas masquer une
#      dépendance manquante des paquets.
#   2. lintian : aucune erreur, aucun avertissement hors de paquets/lintian.md ; puis validation des fichiers
#      INSTALLÉS : fiche AppStream (appstreamcli validate --no-net) et .desktop (desktop-file-validate).
#   3. Permissions : rien d'inscriptible par le groupe ou les autres, ni setuid/setgid, tout root:root ;
#      aucun script de mainteneur.
#   4. strings : aucun chemin de la machine de fabrication hormis les trois replis connus.
#   5. Utilisateur « essai », HOME neuf : init, installation d'un plugin factice, pontage dans
#      ~/.vst3/yabridge, diagnostic, et PREUVE DE CHARGEMENT : `wimao-cli verifier` ouvre le plugin
#      dans Carla avec le Wine et le yabridge DES PAQUETS (/opt/wimao/*) ; le verdict ne doit pas être ❌.
#   6. Interface : wimao sous Xvfb et dbus-run-session, processus vivant, fenêtre « WiMao », capture.
#   7. apt-get remove : /opt/wimao et /usr/bin/wimao* disparus, données de « essai » intactes.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage : $0 <dossier des .deb> <dossier des fixtures>" >&2
    exit 2
fi
DEBS="$(cd "$1" && pwd)"
FIXTURES="$(cd "$2" && pwd)"
RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
JOURNAUX="${JOURNAUX:-/tmp/wimao-installation}"
mkdir -p "$JOURNAUX"
JOURNAUX="$(cd "$JOURNAUX" && pwd)"
export DEBIAN_FRONTEND=noninteractive

PAQUETS=(wimao-wine wimao-bridge wimao)
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
# Des VARIABLE=valeur supplémentaires peuvent précéder la commande.
essai() {
    runuser -u essai -- env -i "${ENV_ESSAI[@]}" "$@"
}

# Tout ce que le script écrit va aussi dans le journal.
exec > >(tee "$JOURNAUX/installation.log") 2>&1

# --- 0. Empreintes -------------------------------------------------------------------------
etape "0. Empreintes SHA-256 des paquets"
[ -f "$DEBS/SHA256SUMS" ] || echec "$DEBS/SHA256SUMS absent"
# SHA256SUMS couvre aussi les RPM en CI : on ne contrôle que les lignes des .deb.
(cd "$DEBS" && grep -E '\.deb$' SHA256SUMS | sha256sum -c -) || echec "une empreinte ne correspond pas : paquets non installés"

# --- 1. Installation -----------------------------------------------------------------------
etape "1. Installation des paquets"
# L'image Docker d'Ubuntu est « minimisée » : /etc/dpkg/dpkg.cfg.d/excludes empêche dpkg de poser
# /usr/share/doc et d'autres fichiers, que `dpkg -L` liste pourtant (l'étape 3 les chercherait en
# vain). On retire cette exclusion pour installer et contrôler le paquet COMPLET, comme sur un poste.
if [ -f /etc/dpkg/dpkg.cfg.d/excludes ]; then
    echo "Image minimisée : exclusions de dpkg retirées (/etc/dpkg/dpkg.cfg.d/excludes)"
    rm -f /etc/dpkg/dpkg.cfg.d/excludes
fi
# Copie lisible par l'utilisateur « _apt » d'apt (sinon avertissement « sandbox » sur le dossier de la CI).
COPIE=/var/tmp/wimao-debs
rm -rf "$COPIE"
mkdir -p "$COPIE"
cp "$DEBS"/*.deb "$COPIE/"
chmod -R a+rX "$COPIE"
apt-get update
(cd "$COPIE" && apt-get install -y ./wimao-wine_*.deb ./wimao-bridge_*.deb ./wimao_*.deb)
for paquet in "${PAQUETS[@]}"; do
    dpkg -s "$paquet" > /dev/null || echec "$paquet non installé"
done

# Carla (Recommends de wimao) n'existe pas dans toutes les distributions (absent de Debian 13). La décision
# se prend sur les dépôts (apt-cache policy) et non sur la présence de carla-single : une installation
# cassée ne doit pas faire passer le test pour « Carla absent ». Variable lue aux étapes 1 et 5.
# a_un_candidat PAQUET : vrai si les dépôts proposent une version de PAQUET. La sortie est d'abord
# mise dans une variable : avec `apt-cache … | grep -q` et pipefail, grep -q qui s'arrête au premier
# résultat ferait recevoir SIGPIPE à apt-cache, et le tuyau entier serait « faux » (constaté en CI).
a_un_candidat() {
    local politique
    politique="$(apt-cache policy "$1")"
    grep -q 'Candidate: [^(]' <<< "$politique"
}
if a_un_candidat carla; then
    CARLA_DISPONIBLE=1
else
    CARLA_DISPONIBLE=0
    echo "ATTENTION : Carla absent des dépôts de cette distribution, test d'ouverture désactivé (prévu par la spec)"
fi

# Outils de test, après les paquets (voir l'en-tête). binutils : strings. xdotool, imagemagick (import)
# et dbus : étape 6. x11-utils (xwininfo) : le test d'ouverture de WiMao (wimao_core::test_plugin), aussi
# Recommends de wimao. Polices et fontconfig : comme le job `integration` de ci.yml, une image vierge n'en
# a presque pas et Wine en a besoin pour créer son préfixe.
#
# jackd2 (+ jack-example-tools quand la distribution le propose : jack_lsp est dans jackd2 sur Ubuntu 24.04,
# dans jack-example-tools sur Debian) : serveur audio JACK factice (pilote « dummy ») pour que Carla puisse
# démarrer son moteur audio dans le conteneur ; sans lui `verifier` ne teste rien. Inutile sans Carla.
#
# Pourquoi carla et x11-utils sont seulement des Recommends de wimao (installés par défaut par apt) et
# jackd un outil de TEST : la machine d'une vraie personne a déjà son serveur audio (PipeWire ou JACK) et
# choisit ses outils, et Carla manque à certaines distributions. Quand un outil manque, `wimao-cli verifier`
# répond par une remarque claire (« Install Carla / x11-utils to test plugins ») sans bloquer l'installation.
# appstream (appstreamcli) et desktop-file-utils (desktop-file-validate) : étape 2, validation de la fiche
# AppStream et du .desktop installés (sans elle, une fiche invalide ferait disparaître WiMao de GNOME
# Logiciels ou de Discover sans qu'aucun autre test n'échoue).
OUTILS_TEST=(lintian binutils xdotool imagemagick dbus x11-utils fonts-liberation fonts-dejavu-core fontconfig appstream desktop-file-utils)
OUTILS_REQUIS=(Xvfb xwininfo xvfb-run dbus-run-session xdotool import strings lintian appstreamcli desktop-file-validate)
if [ "$CARLA_DISPONIBLE" = 1 ]; then
    OUTILS_TEST+=(jackd2)
    if a_un_candidat jack-example-tools; then
        OUTILS_TEST+=(jack-example-tools)
    fi
    OUTILS_REQUIS+=(carla-single jackd jack_lsp)
fi
apt-get install -y --no-install-recommends "${OUTILS_TEST[@]}"
for outil in "${OUTILS_REQUIS[@]}"; do
    command -v "$outil" > /dev/null || echec "outil de test absent après installation : $outil"
done

# --- 2. lintian ----------------------------------------------------------------------------
etape "2. lintian"
# La liste des étiquettes acceptées est lue dans paquets/lintian.md (lignes « - `étiquette` : … »).
ETIQUETTES="$(mktemp)"
# shellcheck disable=SC2016  # les accents graves sont voulus : ils délimitent l'étiquette dans le fichier
sed -n 's/^- `\([a-z0-9-]*\)` : .*/\1/p' "$RACINE_DEPOT/paquets/lintian.md" > "$ETIQUETTES"
[ -s "$ETIQUETTES" ] || echec "aucune étiquette lue dans paquets/lintian.md"
echo "Étiquettes acceptées : $(paste -sd' ' "$ETIQUETTES")"
code=0
lintian --fail-on error,warning --tag-display-limit 0 --suppress-tags-from-file "$ETIQUETTES" \
    "$COPIE"/*.deb > "$JOURNAUX/lintian.log" 2>&1 || code=$?
cat "$JOURNAUX/lintian.log"
[ "$code" -eq 0 ] || echec "lintian a rendu le code $code (erreur ou avertissement hors de paquets/lintian.md)"

# Fiche AppStream installée. --no-net : aucun accès réseau (adresses de la fiche non testées). Sont
# refusés : un code de retour non nul (erreurs) ET toute ligne « E: » (erreur) ou « W: » (avertissement).
# Sont seulement affichés : « I: » (information) et « P: » (pédant), dont l'ensemble dépend de la version
# d'AppStream (constatées en 1.1 : developer-info-missing, recommandation du nouvel élément <developer> ;
# cid-contains-uppercase-letter, convention que l'identifiant fr.wimao.WiMao, déjà publié, ne suit pas).
code=0
appstreamcli validate --no-net /usr/share/metainfo/fr.wimao.WiMao.metainfo.xml > "$JOURNAUX/appstream.log" 2>&1 || code=$?
cat "$JOURNAUX/appstream.log"
[ "$code" -eq 0 ] || echec "appstreamcli validate a rendu le code $code (fiche AppStream invalide)"
! grep -qE '^[EW]:' "$JOURNAUX/appstream.log" || echec "appstreamcli validate signale une erreur ou un avertissement (voir ci-dessus)"
echo "ok : fiche AppStream valide"
# .desktop installé : desktop-file-validate rend un code non nul au moindre avertissement.
desktop-file-validate /usr/share/applications/fr.wimao.WiMao.desktop || echec "desktop-file-validate refuse le .desktop installé"
echo "ok : .desktop valide"

# --- 3. Permissions et scripts de mainteneur -----------------------------------------------
etape "3. Permissions"
for paquet in "${PAQUETS[@]}"; do
    # Chaque chemin installé par le paquet (dossiers compris), sans suivre les liens : sont
    # signalés les fichiers et dossiers inscriptibles par le groupe ou les autres, setuid/setgid,
    # ou n'appartenant pas à root:root.
    # find -files0-from - lit les chemins (séparés par NUL) sur l'entrée standard (findutils >= 4.9).
    # SC2185 : avec -files0-from, les chemins viennent de l'entrée ; ajouter « . » serait une erreur.
    # shellcheck disable=SC2185
    mauvais="$(dpkg -L "$paquet" | tr '\n' '\0' | find -files0-from - -maxdepth 0 ! -type l \
        \( -perm /022 -o -perm /6000 -o ! -user root -o ! -group root \) -print)"
    [ -z "$mauvais" ] || echec "$paquet : droits ou propriétaire anormaux :
$mauvais"
    echo "ok : $paquet"
done
trouves=""
for script in /var/lib/dpkg/info/wimao*.{preinst,postinst,prerm,postrm}; do
    # Sans correspondance, le motif reste tel quel (le fichier n'existe pas) : c'est le cas voulu.
    [ -e "$script" ] && trouves="$trouves $script"
done
[ -z "$trouves" ] || echec "scripts de mainteneur présents :$trouves"
echo "ok : aucun preinst/postinst/prerm/postrm"

# --- 4. Chemins de la machine de fabrication -----------------------------------------------
etape "4. Chemins de développement dans les binaires"
# Les binaires sont fabriqués avec WIMAO_PAQUET=1 (cfg `wimao_paquet`, voir
# paquets/fabriquer-wimao.sh) : aucun repli de développement n'y est compilé, donc AUCUN chemin
# de la machine de fabrication ne doit rester. Dans le conteneur de la CI, le dépôt est sous
# /__w/<nom>/<nom>. Toute correspondance avec /home, /__w ou /github/workspace est une fuite.
for binaire in /usr/bin/wimao /usr/bin/wimao-cli; do
    # -o : n'afficher que le chemin (les chaînes d'un binaire Rust sont collées les unes aux autres).
    restes="$(strings -a "$binaire" | grep -oE '(/home/|/__w/|/github/workspace)[A-Za-z0-9_./-]*' | sort -u || true)"
    [ -z "$restes" ] || echec "$binaire contient des chemins de la machine de fabrication :
$restes"
    echo "$binaire : aucun chemin de fabrication"
done

# --- 5. Utilisateur « essai » : init, installation, pontage, vérification ------------------
etape "5. Utilisateur « essai »"
useradd --create-home --shell /bin/bash essai
install -d -o essai -g essai -m 0700 /tmp/runtime-essai
# Fixtures lisibles par « essai » (le dossier de la CI peut ne pas l'être).
FIXTURES_ESSAI=/var/tmp/wimao-fixtures
rm -rf "$FIXTURES_ESSAI"
mkdir -p "$FIXTURES_ESSAI"
cp "$FIXTURES"/test-nsis.exe "$FIXTURES"/Dexed-1.1.0-win.zip "$FIXTURES_ESSAI/"
chmod -R a+rX "$FIXTURES_ESSAI"

echo "-- wimao-cli init"
essai timeout 900 xvfb-run -a wimao-cli --langue en init
echo "-- wimao-cli installer test-nsis.exe"
# --sans-test : le test d'ouverture (long) est fait plus bas par `verifier`, dont la sortie est jugée.
essai timeout 900 xvfb-run -a wimao-cli --langue en installer "$FIXTURES_ESSAI/test-nsis.exe" --oui --sans-test
[ -e "$HOME_ESSAI/.vst3/yabridge/WiMaoTest.vst3" ] \
    || echec "WiMaoTest.vst3 absent de ~/.vst3/yabridge : le plugin n'est pas ponté"
echo "ok : ~/.vst3/yabridge/WiMaoTest.vst3"

# WiMao pose lui-même ce lien (pontage::assurer_lien_yabridge) : les « chainloaders » de
# ~/.vst3/yabridge y cherchent la bibliothèque de yabridge.
lien="$HOME_ESSAI/.local/share/yabridge"
[ -L "$lien" ] || echec "$lien n'est pas un lien symbolique"
[ "$(readlink "$lien")" = /opt/wimao/yabridge ] \
    || echec "$lien vise $(readlink "$lien") et non /opt/wimao/yabridge"
echo "ok : $lien -> /opt/wimao/yabridge"

# Dexed : vrai plugin VST3 Windows (téléchargé et vérifié par le workflow), installé depuis son
# archive (méthode « extraire »), pour la preuve de chargement plus bas.
echo "-- wimao-cli installer Dexed-1.1.0-win.zip"
essai timeout 900 xvfb-run -a wimao-cli --langue en installer "$FIXTURES_ESSAI/Dexed-1.1.0-win.zip" --oui --sans-test
[ -e "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" ] \
    || echec "Dexed.vst3 absent de ~/.vst3/yabridge : le plugin n'est pas ponté"
echo "ok : ~/.vst3/yabridge/Dexed.vst3"

echo "-- wimao-cli diagnostiquer"
essai wimao-cli --langue en diagnostiquer | tee "$JOURNAUX/diagnostiquer.log"
grep -qx 'No problem found' "$JOURNAUX/diagnostiquer.log" \
    || echec "diagnostiquer signale des problèmes (voir ci-dessus)"

# Preuve de chargement : ni WIMAO_WINE_DIR ni WIMAO_YABRIDGE_DIR dans l'environnement d'« essai »,
# donc WiMao utilise /opt/wimao/wine et /opt/wimao/yabridge ; les scripts yabridge-host de
# wimao-bridge lancent Wine avec WINELOADER=/opt/wimao/wine/bin/wine. `verifier` ouvre chaque plugin
# ponté dans Carla (écran Xvfb à lui) avec un serveur JACK factice lancé ci-dessous. Verdicts exigés :
#   Dexed     ✅ « The plugin opens » : sa fenêtre apparaît, la chaîne Wine + yabridge des paquets marche ;
#   WiMaoTest ❌ « The plugin host stopped » (code 2) OU ⚠️ « The plugin did not finish loading within
#             5 minutes » (code 0) : sa fabrique VST3 est vide (tests/fixtures/plugin/WiMaoTest.c),
#             donc yabridge n'écrit jamais « Finished initializing » pour lui : il n'est JAMAIS « chargé ».
#             Selon le moment, Carla s'arrête en l'examinant (❌), ou reste vivant sans ouvrir de fenêtre
#             jusqu'au délai (⚠️). Les deux issues ont été constatées (3 fois sur 7 sur ubuntu-24.04
#             le 2026-11-01), donc les deux sont admises. En revanche ✅ « Loads correctly… » est REFUSÉ :
#             pour ce plugin-là, ce verdict serait un faux « chargé » (comme « The plugin opens »).
# `verifier` rend le code 2 si WiMaoTest est ❌, 0 s'il est ⚠️ ; aucun autre verdict n'est accepté.
# Exception : sur une distribution sans Carla (décidé par apt-cache policy), le seul verdict accepté pour
# les deux est ⚠️ « Install Carla to test plugins » (code 0) ; pontage et lien yabridge restent exigés.
if [ "$CARLA_DISPONIBLE" = 1 ]; then
    echo "-- wimao-cli verifier (plusieurs minutes : un passage de Carla par plugin)"
    # Serveur JACK factice, lancé par « essai » ; arrêté plus bas par son PID exact (jamais par son nom).
    essai timeout 900 jackd --no-realtime -d dummy -r 48000 -p 512 > "$JOURNAUX/jackd.log" 2>&1 &
    pid_jackd=$!
    for _ in $(seq 1 20); do
        essai jack_lsp > /dev/null 2>&1 && break
        sleep 1
    done
    if ! essai jack_lsp > /dev/null 2>&1; then
        cat "$JOURNAUX/jackd.log"
        kill "$pid_jackd" || true
        echec "le serveur JACK factice ne démarre pas"
    fi
    echo "ok : serveur JACK factice démarré"
else
    # Sans Carla, `verifier` n'ouvre rien : un serveur audio serait inutile, il n'est pas lancé.
    echo "ATTENTION : Carla absent de cette distribution, test d'ouverture désactivé (prévu par la spec) ;"
    echo "            seuls le pontage et le lien yabridge sont vérifiés."
    echo "-- wimao-cli verifier (sans Carla)"
fi
code=0
essai timeout 900 wimao-cli --langue en verifier > "$JOURNAUX/verifier.log" 2>&1 || code=$?
if [ "$CARLA_DISPONIBLE" = 1 ]; then
    # $pid_jackd est celui de `runuser`, qui relaie SIGTERM à ses enfants (timeout puis jackd).
    kill "$pid_jackd" || true
    wait "$pid_jackd" 2> /dev/null || true
fi
cat "$JOURNAUX/verifier.log"
# ⚠️ = U+26A0 U+FE0F, écrit en octets : indépendant de la locale du script.
AVERTISSEMENT=$'\xe2\x9a\xa0\xef\xb8\x8f'
if [ "$CARLA_DISPONIBLE" = 1 ]; then
    # Dexed : un seul verdict admis. WiMaoTest : deux issues admises (voir plus haut) ; le code attendu
    # de `verifier` dépend de celle obtenue, lue dans son journal avant la comparaison. Tout autre
    # verdict (dont ✅ « Loads correctly… ») ne correspond à aucune des deux : il est signalé comme écart.
    attendus=("Dexed | VST3 | ✅ | The plugin opens")
    wimaotest_echec="WiMaoTest | VST3 | ❌ | The plugin host stopped"
    wimaotest_delai="WiMaoTest | VST3 | $AVERTISSEMENT | The plugin did not finish loading within 5 minutes"
    if grep -q -x -F "$wimaotest_delai" "$JOURNAUX/verifier.log"; then
        attendus+=("$wimaotest_delai")
        code_attendu=0
    else
        attendus+=("$wimaotest_echec")
        code_attendu=2
    fi
else
    code_attendu=0
    attendus=("Dexed | VST3 | $AVERTISSEMENT | Install Carla to test plugins"
        "WiMaoTest | VST3 | $AVERTISSEMENT | Install Carla to test plugins")
fi
# verdict_de NOM : la ligne unique de `verifier` pour ce plugin (-F : texte littéral, les « | » d'une
# expression régulière seraient des alternatives).
verdict_de() {
    local lignes
    lignes="$(grep -F "$1 | VST3 | " "$JOURNAUX/verifier.log" || true)"
    [ -n "$lignes" ] || echec "verifier : aucune ligne pour $1"
    [ "$(printf '%s\n' "$lignes" | wc -l)" -eq 1 ] || echec "verifier : plusieurs lignes pour $1 :
$lignes"
    printf '%s\n' "$lignes"
}
ecarts=0
for attendu in "${attendus[@]}"; do
    nom="${attendu%% *}"
    ligne="$(verdict_de "$nom")"
    if [ "$ligne" = "$attendu" ]; then
        echo "ok : $attendu"
    else
        echo "ÉCART : attendu « $attendu », obtenu « $ligne »"
        ecarts=$((ecarts + 1))
    fi
done
if [ "$ecarts" -ne 0 ] && [ "$CARLA_DISPONIBLE" = 1 ]; then
    # Diagnostic : `verifier` efface le journal de Carla. On relance Carla à la main sur Dexed dans le
    # même environnement (celui de test_plugin.rs : Wine et yabridge du paquet en tête du PATH,
    # préfixe de WiMao), yabridge bavard, et on garde tout dans les artefacts.
    echo "-- diagnostic : carla-single sur Dexed (60 s au plus, journal : carla-diagnostic.log)"
    essai timeout 900 jackd --no-realtime -d dummy -r 48000 -p 512 >> "$JOURNAUX/jackd.log" 2>&1 &
    pid_jackd=$!
    sleep 3
    essai env WINEPREFIX="$HOME_ESSAI/.local/share/wimao/prefixe" \
        PATH="/opt/wimao/wine/bin:/opt/wimao/yabridge:/usr/local/bin:/usr/bin:/bin" \
        QT_QPA_PLATFORM=xcb YABRIDGE_DEBUG_LEVEL=2 \
        xvfb-run -a timeout 60 carla-single vst3 "$HOME_ESSAI/.vst3/yabridge/Dexed.vst3" \
        > "$JOURNAUX/carla-diagnostic.log" 2>&1 || true
    kill "$pid_jackd" || true
    wait "$pid_jackd" 2> /dev/null || true
    tail -n 80 "$JOURNAUX/carla-diagnostic.log"
fi
[ "$ecarts" -eq 0 ] || echec "verifier : $ecarts verdict(s) inattendu(s) (voir ci-dessus)"
[ "$code" -eq "$code_attendu" ] || echec "verifier a rendu le code $code (attendu : $code_attendu)"

# --- 6. Interface --------------------------------------------------------------------------
etape "6. Interface graphique"
# Script lancé par « essai » sous Xvfb et dbus-run-session (bus de session propre au test).
# GSK_RENDERER=cairo : pas de GPU dans le conteneur, rendu logiciel simple.
cat > /var/tmp/wimao-interface.sh << 'INTERNE'
#!/usr/bin/env bash
set -euo pipefail
wimao &
pid=$!
sleep 10
# Capture d'abord (même si le processus est mort, l'écran dit quelque chose), contrôles ensuite.
import -window root "$HOME/interface.png"
kill -0 "$pid" || { echo "ERREUR : wimao s'est arrêté" >&2; exit 1; }
# Au premier lancement la fenêtre peut être l'assistant : son titre contient aussi « WiMao ».
fenetre=""
for _ in $(seq 1 20); do
    fenetre="$(xdotool search --name WiMao | head -n 1 || true)"
    [ -z "$fenetre" ] || break
    sleep 1
done
if [ -z "$fenetre" ]; then
    echo "ERREUR : aucune fenêtre « WiMao »" >&2
    xwininfo -root -tree >&2 || true
    kill "$pid" || true
    exit 1
fi
echo "fenêtre WiMao : $fenetre ($(xdotool getwindowname "$fenetre"))"
# Arrêt par le PID exact (jamais par le nom), puis attente de la fin du processus.
kill "$pid"
wait "$pid" || true
! kill -0 "$pid" 2> /dev/null || { echo "ERREUR : wimao ne s'arrête pas" >&2; exit 1; }
echo "wimao arrêté"
INTERNE
chmod 0755 /var/tmp/wimao-interface.sh
# Le journal de WiMao (journal.log) est partagé avec les étapes précédentes (la CLI y a écrit, dont l'échec
# ATTENDU du plugin factice à l'étape 5) : on ne regarde que ce que le lancement de l'interface y AJOUTE.
JOURNAL_WIMAO="$HOME_ESSAI/.local/share/wimao/journal.log"
lignes_avant=0
if [ -f "$JOURNAL_WIMAO" ]; then lignes_avant="$(wc -l < "$JOURNAL_WIMAO")"; fi
code=0
essai GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1 timeout 300 \
    xvfb-run -a -s '-screen 0 1280x900x24' dbus-run-session -- /var/tmp/wimao-interface.sh \
    > "$JOURNAUX/interface.log" 2>&1 || code=$?
cat "$JOURNAUX/interface.log"
[ ! -f "$HOME_ESSAI/interface.png" ] || cp "$HOME_ESSAI/interface.png" "$JOURNAUX/interface.png"
[ "$code" -eq 0 ] || echec "l'interface n'a pas passé le contrôle (code $code, voir interface.log)"
[ -s "$JOURNAUX/interface.png" ] || echec "capture d'écran absente"

# Aucune erreur dans les journaux (spec § 6.4).
# 1. Sortie de l'interface : panique Rust ou message GLib/GTK de niveau CRITICAL. Les simples « WARNING »
#    de GTK/libadwaita/Mesa sont fréquents dans un conteneur sans GPU et ne sont pas des erreurs de WiMao.
! grep -nE 'panicked at|-CRITICAL' "$JOURNAUX/interface.log" || echec "l'interface a écrit une erreur (panique ou CRITICAL, voir ci-dessus)"
# 2. journal.log de WiMao (format : voir crates/wimao-core/src/journal.rs) : chaque entrée commence par
#    « [date] $ commande » et contient « code: N » (N ≠ 0 = commande en échec) ou « échec: cause »
#    (commande impossible à lancer, ou événement noté par `journal::noter`). Les « note: … » sont de simples
#    informations. On affiche « commande : ligne fautive » pour chaque entrée en erreur.
EXCEPTIONS_JOURNAL=(
    # Motifs (grep -E) de COMMANDES dont l'échec est toléré, chacun avec sa justification. Aucune pour
    # l'instant : ajouter ici (et expliquer) une commande qui échoue légitimement dans le conteneur.
)
erreurs_journal="$(tail -n +"$((lignes_avant + 1))" "$JOURNAL_WIMAO" 2> /dev/null | awk '
    /^\[[0-9?-]/ { commande = $0; next }
    /^  échec: / || /^  code: / && $2 != 0 { print commande " ==> " $0 }
' || true)"
for motif in "${EXCEPTIONS_JOURNAL[@]}"; do
    erreurs_journal="$(grep -vE -- "$motif" <<< "$erreurs_journal" || true)"
done
[ -z "$erreurs_journal" ] || echec "le journal de WiMao contient des erreurs écrites par le lancement de l'interface :
$erreurs_journal"
echo "ok : aucune erreur dans interface.log ni dans le journal de WiMao"

# --- 7. Retrait ----------------------------------------------------------------------------
etape "7. Retrait des paquets"
apt-get remove -y wimao wimao-bridge wimao-wine
[ ! -e /opt/wimao ] || echec "/opt/wimao existe encore"
restes="$(find /usr/bin -maxdepth 1 -name 'wimao*')"
[ -z "$restes" ] || echec "restes dans /usr/bin : $restes"
# Plus aucun paquet « ii » : « rc » (configuration restante) ou absent sont corrects.
installes="$(dpkg-query -W -f='${db:Status-Abbrev} ${Package}\n' 'wimao*' 2> /dev/null | grep '^ii' || true)"
[ -z "$installes" ] || echec "paquets encore installés : $installes"
dpkg -l 'wimao*' 2>&1 || true
# Les données de l'utilisateur ne sont jamais touchées par le retrait.
[ -d "$HOME_ESSAI/.local/share/wimao" ] || echec "le dossier de données de WiMao (.local/share/wimao) a disparu"
[ -e "$HOME_ESSAI/.vst3/yabridge/WiMaoTest.vst3" ] || [ -L "$HOME_ESSAI/.vst3/yabridge/WiMaoTest.vst3" ] \
    || echec "le plugin ponté (.vst3/yabridge/WiMaoTest.vst3) a disparu"
[ -L "$lien" ] || echec "$lien a disparu"
# Le lien yabridge est une donnée de l'utilisateur créée par WiMao : il reste, désormais pendant.
if [ -e "$lien" ]; then
    echo "note : $lien existe encore et vise $(readlink "$lien")"
else
    echo "note : $lien vise $(readlink "$lien"), désormais absent (lien pendant, sans conséquence)"
fi
echo "ok : données de l'utilisateur intactes"

echo
echo "== Installation, usage et retrait : tout est passé =="
