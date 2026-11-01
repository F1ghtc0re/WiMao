#!/usr/bin/env bash
# Installe yabridge 5.1.1 (binaires officiels) dans /opt/wine-mao/yabridge, forcé sur /opt/wine-mao,
# et crée le préfixe Wine ~/.wine-mao.
set -euo pipefail

VERSION=5.1.1
SHA256=d11f0307412b5566b09a75ed4aa191266db56d22f4de4d20e4fbde6296cd1a47
WINE=/opt/wine-mao/bin/wine
DEST=/opt/wine-mao/yabridge
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/src"
export WINEPREFIX="$HOME/.wine-mao"

# 1. Téléchargement et vérification
cd "$SRC"
[ -f yabridge-$VERSION.tar.gz ] || gh release download $VERSION -R robbert-vdh/yabridge -p "yabridge-$VERSION.tar.gz"
echo "$SHA256  yabridge-$VERSION.tar.gz" | sha256sum -c -
rm -rf yabridge-$VERSION && mkdir yabridge-$VERSION
tar xzf yabridge-$VERSION.tar.gz -C yabridge-$VERSION

# 2. Copie dans /opt/wine-mao/yabridge et forçage du Wine utilisé par l'hôte
rm -rf "$DEST"
cp -a yabridge-$VERSION/yabridge "$DEST"
for host in "$DEST"/yabridge-host.exe "$DEST"/yabridge-host-32.exe; do
    sed -i "s|WINELOADER=\"wine\"|WINELOADER=\"$WINE\"|" "$host"
    grep -q "WINELOADER=\"$WINE\"" "$host" || { echo "ERREUR : $host non modifié" >&2; exit 1; }
done

# 3. Lien standard exigé par les chainloaders de yabridge
mkdir -p "$HOME/.local/share"
if [ -e "$HOME/.local/share/yabridge" ] && [ ! -L "$HOME/.local/share/yabridge" ]; then
    echo "ERREUR : ~/.local/share/yabridge existe déjà et n'est pas un lien" >&2; exit 1
fi
ln -sfn "$DEST" "$HOME/.local/share/yabridge"

# 4. Préfixe Wine dédié (sans proposer Mono/Gecko, inutiles pour les plugins)
if [ ! -f "$WINEPREFIX/system.reg" ]; then
    WINEDLLOVERRIDES="mscoree,mshtml=" "$WINE" wineboot -i
    /opt/wine-mao/bin/wineserver -w
fi

# Isolation : Documents, Bureau, etc. deviennent de vrais dossiers du préfixe au lieu de liens vers ~,
# sinon les installeurs déposent leurs bibliothèques (parfois des dizaines de Go) dans ~/Documents.
# .update-timestamp=disable empêche Wine de recréer les liens (mise à jour manuelle : wineboot -u).
U="$WINEPREFIX/drive_c/users/$USER"
for l in Desktop Documents Downloads Music Pictures Videos; do
    if [ -L "$U/$l" ]; then rm "$U/$l" && mkdir "$U/$l"; fi
done
echo disable > "$WINEPREFIX/.update-timestamp"

# Option staging « Hide Wine version from applications » : évite le gel au clic des plugins JUCE
# (Serum 2, Arturia, Kick 2…), cf. README de yabridge.
"$WINE" reg add 'HKCU\Software\Wine' /v HideWineExports /t REG_SZ /d Y /f

# 5. Dossiers de plugins Windows standards, déclarés dans yabridgectl
PATH="/opt/wine-mao/bin:$PATH"
C="$WINEPREFIX/drive_c/Program Files"
mkdir -p "$C/Common Files/VST3" "$C/Common Files/CLAP" "$C/Common Files/VST2" "$C/VstPlugins" "$C/Steinberg/VstPlugins"
for d in "$C/Common Files/VST3" "$C/Common Files/CLAP" "$C/Common Files/VST2" "$C/VstPlugins" "$C/Steinberg/VstPlugins"; do
    "$DEST/yabridgectl" list 2>/dev/null | grep -qF "$d" || "$DEST/yabridgectl" add "$d"
done
"$DEST/yabridgectl" sync
"$DEST/yabridgectl" status
