#!/usr/bin/env bash
# Sauvegarde le préfixe ~/.wine-mao (plugins, licences, bibliothèques) dans une archive .tar.zst.
# Les fichiers temporaires, installeurs téléchargés et caches sont exclus (réinstallables).
# Usage : sauvegarde-prefixe.sh /chemin/vers/disque-externe   (crée le dossier WiMao-sauvegarde dessus)
# Restauration : voir la fin de ce fichier.
set -euo pipefail

DEST="${1:?Usage : $0 /chemin/vers/disque-externe}"
[ -d "$DEST" ] || { echo "ERREUR : $DEST n'existe pas" >&2; exit 1; }
if pgrep -x BitwigStudio > /dev/null; then
    echo "ERREUR : fermer Bitwig avant la sauvegarde" >&2; exit 1
fi

# Aucun programme Windows ne doit écrire dans le préfixe pendant la copie
/opt/wine-mao-v2/bin/wineserver -k 2>/dev/null || true
/opt/wine-mao/bin/wineserver -k 2>/dev/null || true

BESOIN=$(du -sb --exclude='drive_c/users/*/AppData/Local/Temp' --exclude='drive_c/users/*/Downloads' "$HOME/.wine-mao" | cut -f1)
LIBRE=$(df -B1 --output=avail "$DEST" | tail -1)
[ "$LIBRE" -gt "$BESOIN" ] || { echo "ERREUR : pas assez de place sur $DEST" >&2; exit 1; }

DEST="$DEST/WiMao-sauvegarde"; mkdir -p "$DEST"
ARCHIVE="$DEST/wine-mao-$(date +%F-%Hh%M).tar.zst"   # heure incluse : ne jamais écraser une sauvegarde précédente
[ -e "$ARCHIVE.sha256" ] && { echo "ERREUR : $ARCHIVE existe déjà" >&2; exit 1; }
# FAT32 (disques externes formatés Windows) : 4 Go maximum par fichier → archive découpée en morceaux
case "$(findmnt -no FSTYPE --target "$DEST")" in vfat|msdos|fat) DECOUPER=1 ;; *) DECOUPER=0 ;; esac
echo "Sauvegarde de ~/.wine-mao vers $ARCHIVE (environ $((BESOIN / 1024**3)) Go, plusieurs minutes)…"
tar -C "$HOME" \
    --exclude='.wine-mao/drive_c/users/*/AppData/Local/Temp' \
    --exclude='.wine-mao/drive_c/users/*/Downloads' \
    --exclude='.wine-mao/drive_c/users/*/AppData/Local/dxvk' \
    -cf - .wine-mao | zstd -T0 -3 -q -c | if [ $DECOUPER = 1 ]; then
        split -b 3900M -d -a 3 - "$ARCHIVE.part"
    else
        cat > "$ARCHIVE"
    fi
(cd "$DEST" && sha256sum "$(basename "$ARCHIVE")"* > "$(basename "$ARCHIVE").sha256")
echo "Terminé : $(du -ch "$ARCHIVE"* | tail -1 | cut -f1) dans $DEST (empreintes dans $(basename "$ARCHIVE").sha256)"

# Restauration (Bitwig fermé) :
#   cd /chemin/vers/disque-externe/WiMao-sauvegarde && sha256sum -c wine-mao-AAAA-MM-JJ-HHhMM.tar.zst.sha256
#   mv ~/.wine-mao ~/.wine-mao.ancien            # garder l'ancien tant que tout n'est pas vérifié
#   cat wine-mao-AAAA-MM-JJ-HHhMM.tar.zst.part* | zstd -dc | tar -C "$HOME" -xf -   # disque FAT32 (morceaux)
#   zstd -dc wine-mao-AAAA-MM-JJ-HHhMM.tar.zst | tar -C "$HOME" -xf -              # autre disque (fichier unique)
