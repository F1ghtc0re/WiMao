#!/usr/bin/env bash
# Relais des liens native-access:// (retour de connexion NI dans Chrome) vers Native Access dans Wine.
# Déclaré par ~/.local/share/applications/wimao-native-access.desktop (x-scheme-handler/native-access).
# Les sorties sont redirigées vers un fichier : sans sortie valide, Electron plante (Error: open EBADF).
JOURNAL="${XDG_RUNTIME_DIR:-/tmp}/wimao-native-access.log"
echo "$(date '+%F %T') $1" >> "$JOURNAL"
export WINEPREFIX="$HOME/.wine-mao" WINEDEBUG=-all
exec /opt/wine-mao-v2/bin/wine 'C:\Program Files\Native Instruments\Native Access\Native Access.exe' \
    --no-sandbox --disable-gpu "$1" >> "$JOURNAL" 2>&1 < /dev/null
