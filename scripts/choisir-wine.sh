#!/usr/bin/env bash
# Choisit le Wine utilisé par yabridge (et donc par Bitwig) : v1 = /opt/wine-mao (9.21 Staging),
# v2 = /opt/wine-mao-v2 (11.20 + annulation wm_state_serial). Bitwig doit être fermé.
# Usage : choisir-wine.sh v1|v2      (sans argument : affiche la version active)
set -euo pipefail

LIEN="$HOME/.local/share/yabridge"
export WINEPREFIX="$HOME/.wine-mao"

case "${1:-}" in
    v1) DEST=/opt/wine-mao ;;
    v2) DEST=/opt/wine-mao-v2 ;;
    "") echo "Actif : $(readlink "$LIEN")"; exit 0 ;;
    *)  echo "Usage : $0 v1|v2" >&2; exit 1 ;;
esac

if pgrep -x BitwigStudio > /dev/null; then
    echo "ERREUR : fermer Bitwig avant de changer de Wine" >&2; exit 1
fi
[ -L "$LIEN" ] || { echo "ERREUR : $LIEN n'est pas un lien" >&2; exit 1; }

# Arrêter le wineserver de l'autre version avant de toucher au préfixe
/opt/wine-mao/bin/wineserver -k 2>/dev/null || true
/opt/wine-mao-v2/bin/wineserver -k 2>/dev/null || true

ln -sfn "$DEST/yabridge" "$LIEN"

# Mise à jour du préfixe pour cette version (pas de fenêtre Mono/Gecko)
WINEDEBUG=-all WINEDLLOVERRIDES="mscoree,mshtml=" "$DEST/bin/wine" wineboot -u
"$DEST/bin/wineserver" -w

# Vérifications : isolation du dossier personnel et DXVK intacts
U="$WINEPREFIX/drive_c/users/$USER"
for l in Desktop Documents Downloads Music Pictures Videos; do
    if [ -L "$U/$l" ]; then
        echo "ATTENTION : $l est redevenu un lien, isolation rétablie"
        rm "$U/$l" && mkdir "$U/$l"
    fi
done
echo disable > "$WINEPREFIX/.update-timestamp"
head -c 128 "$WINEPREFIX/drive_c/windows/system32/d3d11.dll" | grep -q 'Wine builtin DLL' \
    && echo "ATTENTION : d3d11.dll n'est plus DXVK, relancer scripts/install-dxvk.sh"

echo "Wine actif : $("$DEST/bin/wine" --version)  ($DEST)"
