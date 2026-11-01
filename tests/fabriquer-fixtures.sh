#!/usr/bin/env bash
# Fabrique les installeurs factices des tests d'intégration :
#   <sortie>/test-nsis.exe   NSIS,        outil : makensis (paquet nsis)
#   <sortie>/test.msi        Windows Installer, outil : wixl (paquets msitools et wixl)
#   <sortie>/faux-gestionnaire-setup.exe  archive 7z, outil : 7z (paquet 7zip)
#   <sortie>/test-inno.exe   Inno Setup,  outil : ISCC.exe lancé sous Wine
# Les trois installeurs test-* installent le bundle fixtures/plugin/WiMaoTest.vst3 dans
# C:\Program Files\Common Files\VST3. faux-gestionnaire-setup.exe imite l'installeur d'un
# gestionnaire d'éditeur (méthode extraire-7z) : il contient $PLUGINSDIR/app-64.7z, qui
# contient fixtures/gestionnaire/FauxGestionnaire.exe (binaire versionné, voir son .c).
# Voir CONTRIBUTING.md (« Tests d'intégration »).
#
# Usage : tests/fabriquer-fixtures.sh [--seulement-nsis] <dossier de sortie>
#   --seulement-nsis  ne fabrique que test-nsis.exe, puis s'arrête : ni wixl, ni 7z, ni Wine requis
#                     (makensis suffit ; utilisé par les tests d'installation des paquets,
#                     paquets/tester-installation.sh, qui n'ont ni msitools ni 7zip à installer).
#
# Variables : WIMAO_WINE_DIR (dossier de Wine, avec bin/wine ; défaut /opt/wine-mao-v2),
#             WIMAO_CACHE_INNO (où garder le téléchargement d'Inno Setup, pour ne le
#             faire qu'une fois ; défaut : le dossier de sortie).
# Rien n'est écrit hors de <sortie> et d'un préfixe Wine temporaire, supprimé à la fin :
# ni ~/.wine-mao ni le préfixe de WiMao ne sont touchés.
set -euo pipefail

# --- Inno Setup : version et empreinte --------------------------------------------------
# 6.4.3 : version fixée pour des fixtures reproductibles (empreinte ci-dessous). Les anciennes versions ne sont
# plus sur files.jrsoftware.org (404 constaté en 2026, remplacé par des « immutable
# releases » GitHub) : on prend la publication GitHub du projet jrsoftware/issrc.
INNO_VERSION=6.4.3
INNO_URL="https://github.com/jrsoftware/issrc/releases/download/is-6_4_3/innosetup-${INNO_VERSION}.exe"
INNO_SHA256=f3c42116542c4cc57263c5ba6c4feabfc49fe771f2f98a79d2f7628b8762723b

# Arguments : l'option facultative --seulement-nsis, puis le dossier de sortie (un seul).
SEULEMENT_NSIS=0
if [ "${1:-}" = "--seulement-nsis" ]; then
    SEULEMENT_NSIS=1
    shift
fi
if [ $# -ne 1 ] || [[ "$1" == -* ]]; then
    echo "Usage : $0 [--seulement-nsis] <dossier de sortie>" >&2
    exit 2
fi

RACINE="$(cd "$(dirname "$0")" && pwd)"
FIXTURES="$RACINE/fixtures"
PLUGIN="$FIXTURES/plugin"
mkdir -p "$1"
SORTIE="$(cd "$1" && pwd)"
WINE_DIR="${WIMAO_WINE_DIR:-/opt/wine-mao-v2}"
WINE="$WINE_DIR/bin/wine"
WINESERVER="$WINE_DIR/bin/wineserver"
CACHE_INNO="${WIMAO_CACHE_INNO:-$SORTIE}"

echo "== NSIS =="
# SORTIE et PLUGIN sont lues par test.nsi (${SORTIE}, ${PLUGIN}).
makensis -V2 -DSORTIE="$SORTIE/test-nsis.exe" -DPLUGIN="$PLUGIN" "$FIXTURES/nsis/test.nsi"

# --seulement-nsis : l'installeur NSIS est prêt, on s'arrête avant tout ce qui demande wixl, 7z ou Wine.
if [ "$SEULEMENT_NSIS" = 1 ]; then
    echo "== Seulement NSIS (--seulement-nsis) =="
    ls -l "$SORTIE/test-nsis.exe"
    exit 0
fi

echo "== MSI =="
# -D PLUGIN : variable $(var.PLUGIN) de test.wxs. --arch x64 : paquet 64 bits (Platform="x64").
wixl --arch x64 -D PLUGIN="$PLUGIN" -o "$SORTIE/test.msi" "$FIXTURES/msi/test.wxs"

echo "== Faux gestionnaire (archive 7z) =="
# Même forme que l'installeur de Native Access ouvert par 7z (NOTES.md) : une archive
# externe qui contient $PLUGINSDIR/app-64.7z, l'archive de l'application. Fabriqué AVANT
# la sortie anticipée de WIMAO_FIXTURES_SANS_INNO : la CI en a besoin. Dossier de travail
# temporaire, supprimé aussitôt ; l'ancienne archive est retirée (7z a ajouterait dedans).
GESTIONNAIRE_TMP="$(mktemp -d)"
mkdir -p "$GESTIONNAIRE_TMP/externe/\$PLUGINSDIR"
7z a -bd -bso0 "$GESTIONNAIRE_TMP/externe/\$PLUGINSDIR/app-64.7z" \
    "$FIXTURES/gestionnaire/FauxGestionnaire.exe"
rm -f "$SORTIE/faux-gestionnaire-setup.exe"
# -t7z : le nom finit par .exe, 7z ne doit pas en déduire un autre format.
# '$PLUGINSDIR' est le nom LITTÉRAL du dossier NSIS ajouté à l'archive : il ne doit pas être développé.
# shellcheck disable=SC2016
(cd "$GESTIONNAIRE_TMP/externe" && 7z a -t7z -bd -bso0 "$SORTIE/faux-gestionnaire-setup.exe" '$PLUGINSDIR')
rm -rf "$GESTIONNAIRE_TMP"

# WIMAO_FIXTURES_SANS_INNO=1 : s'arrêter avant Inno Setup (utilisé par la CI, où
# l'installeur d'Inno Setup échoue ; voir .github/workflows/ci.yml).
if [ "${WIMAO_FIXTURES_SANS_INNO:-0}" = 1 ]; then
    echo "== Inno Setup ignoré (WIMAO_FIXTURES_SANS_INNO=1) =="
    ls -l "$SORTIE/test-nsis.exe" "$SORTIE/test.msi" "$SORTIE/faux-gestionnaire-setup.exe"
    exit 0
fi

echo "== Inno Setup =="
# Préfixe Wine jetable, dans un dossier temporaire créé ici et supprimé par `nettoyer`.
TEMPORAIRE="$(mktemp -d)"
export WINEPREFIX="$TEMPORAIRE/prefixe"
# Dossier personnel jetable, lui aussi. Wine écrit dans $HOME/.local/share/applications,
# ~/.config/menus et ~/.local/share/icons les raccourcis et les associations de fichiers
# des programmes installés (winemenubuilder). Avec le vrai HOME, l'installation d'Inno Setup
# a ajouté au menu du bureau de l'utilisateur des entrées pointant vers ce préfixe effacé
# (incident du 2026-10-31). Rien de ce script ne doit sortir de $TEMPORAIRE et $SORTIE.
mkdir -p "$TEMPORAIRE/home"
export HOME="$TEMPORAIRE/home"
export XDG_DATA_HOME="$HOME/.local/share" XDG_CONFIG_HOME="$HOME/.config"
# mscoree,mshtml= : sinon Wine propose d'installer Mono/Gecko dans une fenêtre sur le
# bureau de l'utilisateur (voir NOTES.md), inutile pour Inno Setup.
# winemenubuilder.exe=d : seconde protection, aucun raccourci ni association n'est créé.
export WINEDLLOVERRIDES="mscoree,mshtml=;winemenubuilder.exe=d"
export WINEDEBUG=-all

# Arrête le wineserver de CE préfixe (et seulement lui : WINEPREFIX est explicite), puis
# supprime le préfixe. Appelé à la sortie, même en cas d'erreur.
nettoyer() {
    "$WINESERVER" -k 2>/dev/null || true
    "$WINESERVER" -w 2>/dev/null || true
    rm -rf "$TEMPORAIRE"
}
trap nettoyer EXIT

# Télécharge l'installeur d'Inno Setup (une seule fois) et vérifie son empreinte avant
# de l'exécuter, comme `telecharger_verifie` côté Rust.
INSTALLEUR_INNO="$CACHE_INNO/innosetup-${INNO_VERSION}.exe"
if [ ! -f "$INSTALLEUR_INNO" ]; then
    curl -fsSL -o "$INSTALLEUR_INNO" "$INNO_URL"
fi
echo "$INNO_SHA256  $INSTALLEUR_INNO" | sha256sum -c -

# Xvfb : si l'installeur ouvre une fenêtre malgré /VERYSILENT, elle reste invisible.
# -a choisit un numéro d'écran libre ; l'écran fait 24 bits comme dans les tests de WiMao.
sous_xvfb() { xvfb-run -a -s "-screen 0 1280x1024x24" "$@"; }

"$WINE" wineboot -u
# /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- : options silencieuses d'Inno (documentation
# Inno Setup, « Setup Command Line Parameters »). /DIR : dossier d'installation de l'outil.
# /LOG : journal d'Inno, affiché en cas d'échec (sinon seul un code de sortie remonte).
code_inno=0
# Messages d'erreur de Wine gardés à part (WINEDEBUG=err+all) et affichés seulement en cas
# d'échec : ils disent quelle fonction de Windows a échoué (fenêtre, police, bibliothèque).
sous_xvfb env WINEDEBUG=err+all "$WINE" "$INSTALLEUR_INNO" \
    /VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- \
    '/DIR=C:\InnoSetup' '/LOG=C:\inno-installation.log' 2> "$TEMPORAIRE/wine-inno.err" \
    || code_inno=$?
"$WINESERVER" -w
if [ "$code_inno" -ne 0 ]; then
    echo "ERREUR : l'installeur d'Inno Setup a rendu le code $code_inno" >&2
    cat "$WINEPREFIX/drive_c/inno-installation.log" >&2 2>/dev/null \
        || echo "(aucun journal d'Inno : l'installeur n'a pas démarré)" >&2
    echo "--- Erreurs de Wine (fin) ---" >&2
    tail -n 60 "$TEMPORAIRE/wine-inno.err" >&2 || true
    exit 1
fi
# Un installeur Windows peut « réussir » sans rien installer : on le vérifie, avec un
# message clair (sinon la seule trace serait un code 1 muet à l'étape suivante).
if [ ! -f "$WINEPREFIX/drive_c/InnoSetup/ISCC.exe" ]; then
    echo "ERREUR : Inno Setup ne s'est pas installé (ISCC.exe absent de C:\\InnoSetup)" >&2
    ls -la "$WINEPREFIX/drive_c" >&2 || true
    exit 1
fi

# ISCC est un programme Windows : il lit le script et le plugin avec des chemins Windows.
# Chaque conversion est vérifiée : une conversion vide passerait sinon à ISCC un
# argument faux, et l'échec serait incompréhensible.
chemin_windows() {
    local w
    w="$("$WINE" winepath -w "$1" | tr -d '\r')"
    [ -n "$w" ] || { echo "ERREUR : winepath n'a pas converti $1" >&2; exit 1; }
    printf '%s' "$w"
}
PLUGIN_W="$(chemin_windows "$PLUGIN")"
SORTIE_W="$(chemin_windows "$SORTIE")"
SCRIPT_W="$(chemin_windows "$FIXTURES/inno/test.iss")"
echo "ISCC : plugin=$PLUGIN_W sortie=$SORTIE_W script=$SCRIPT_W"
sous_xvfb "$WINE" 'C:\InnoSetup\ISCC.exe' "/DPlugin=$PLUGIN_W" "/O$SORTIE_W" "$SCRIPT_W" \
    || { echo "ERREUR : ISCC a échoué (code $?)" >&2; exit 1; }
"$WINESERVER" -w
# Le nom de sortie vient de OutputBaseFilename=test-inno (extension .exe ajoutée par Inno).

echo "== Fixtures prêtes dans $SORTIE =="
ls -l "$SORTIE/test-nsis.exe" "$SORTIE/test.msi" "$SORTIE/faux-gestionnaire-setup.exe" \
    "$SORTIE/test-inno.exe"
