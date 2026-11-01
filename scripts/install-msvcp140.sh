#!/usr/bin/env bash
# Remplace msvcp140.dll de Wine par celle de Microsoft dans ~/.wine-mao (Maschine 3 appelle une fonction absente
# de la version Wine : basic_istream::operator=(&&)). Les DLL viennent du cache du VC++ Redistributable déjà
# installé dans le préfixe par Native Access (ProgramData/Package Cache, cab1.cab de vcRuntimeMinimum).
# Usage : install-msvcp140.sh            installe
#         install-msvcp140.sh --retirer  remet les DLL de Wine (sauvegardées dans src/msvcp140/sauvegarde)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
D="$ROOT/src/msvcp140"
export WINEPREFIX="$HOME/.wine-mao" WINEDEBUG=-all
WINE=/opt/wine-mao-v2/bin/wine
W="$WINEPREFIX/drive_c/windows"
CACHE="$WINEPREFIX/drive_c/ProgramData/Package Cache"

if [ "${1:-}" = "--retirer" ]; then
    cp "$D/sauvegarde/system32/msvcp140.dll" "$W/system32/"
    cp "$D/sauvegarde/syswow64/msvcp140.dll" "$W/syswow64/"
    "$WINE" reg delete 'HKCU\Software\Wine\DllOverrides' /v msvcp140 /f
    exit 0
fi

CAB64=$(find "$CACHE" -path '*vcRuntimeMinimum_amd64/cab1.cab' | sort -V | tail -1)
CAB32=$(find "$CACHE" -path '*vcRuntimeMinimum_x86/cab1.cab' | sort -V | tail -1)
if [ -z "$CAB64" ] || [ -z "$CAB32" ]; then
    echo "ERREUR : VC++ Redistributable absent du cache" >&2
    exit 1
fi

rm -rf "$D/x64" "$D/x86" && mkdir -p "$D/x64" "$D/x86" "$D/sauvegarde/system32" "$D/sauvegarde/syswow64"
cabextract -q -d "$D/x64" -F 'msvcp140.dll*' "$CAB64"
cabextract -q -d "$D/x86" -F 'msvcp140.dll*' "$CAB32"

cp --update=none "$W/system32/msvcp140.dll" "$D/sauvegarde/system32/"
cp --update=none "$W/syswow64/msvcp140.dll" "$D/sauvegarde/syswow64/"
cp "$D/x64/msvcp140.dll_amd64" "$W/system32/msvcp140.dll"
cp "$D/x86/msvcp140.dll" "$W/syswow64/msvcp140.dll"
"$WINE" reg add 'HKCU\Software\Wine\DllOverrides' /v msvcp140 /t REG_SZ /d native,builtin /f
