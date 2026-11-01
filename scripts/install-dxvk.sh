#!/usr/bin/env bash
# Installe DXVK (Direct3D 9/10/11 → Vulkan) dans ~/.wine-mao, uniquement d3d11, dxgi, d3d10core et d3d9.
# Corrige le scintillement des interfaces Direct2D/D3D11 (Serum 2) rendues par wined3d/OpenGL.
# d3d9 : interfaces iZotope (Ozone, Vocal Doubler) blanches avec wined3d.
# Usage : install-dxvk.sh            installe
#         install-dxvk.sh --retirer  remet les DLL de Wine (sauvegardées dans src/dxvk/sauvegarde)
set -euo pipefail

VERSION=3.1.1
SHA256=40565b4a724aadc4433fa4e010b4b23916d9b1f1baeee64e17186db94f54e608
DLLS="d3d11 dxgi d3d10core d3d9"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
D="$ROOT/src/dxvk"
export WINEPREFIX="$HOME/.wine-mao" WINEDEBUG=-all
WINE=/opt/wine-mao-v2/bin/wine
W="$WINEPREFIX/drive_c/windows"

if [ "${1:-}" = "--retirer" ]; then
    for f in $DLLS; do
        cp "$D/sauvegarde/system32/$f.dll" "$W/system32/"
        cp "$D/sauvegarde/syswow64/$f.dll" "$W/syswow64/"
        "$WINE" reg delete 'HKCU\Software\Wine\DllOverrides' /v "$f" /f
    done
    exit 0
fi

mkdir -p "$D" && cd "$D"
[ -f dxvk-$VERSION.tar.gz ] || gh release download v$VERSION -R doitsujin/dxvk -p "dxvk-$VERSION.tar.gz"
echo "$SHA256  dxvk-$VERSION.tar.gz" | sha256sum -c -
rm -rf dxvk-$VERSION && tar xzf dxvk-$VERSION.tar.gz

mkdir -p sauvegarde/system32 sauvegarde/syswow64
for f in $DLLS; do
    cp --update=none "$W/system32/$f.dll" sauvegarde/system32/
    cp --update=none "$W/syswow64/$f.dll" sauvegarde/syswow64/
    cp dxvk-$VERSION/x64/"$f".dll "$W/system32/"
    cp dxvk-$VERSION/x32/"$f".dll "$W/syswow64/"
    "$WINE" reg add 'HKCU\Software\Wine\DllOverrides' /v "$f" /t REG_SZ /d native,builtin /f
done
