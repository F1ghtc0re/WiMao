#!/bin/sh
dir=$(dirname "$0")
printf "%s | %s\n" "$(pwd)" "$*" >> "$dir/appels.log"
[ "$1" = "reg" ] && exit 0
case "$*" in
  *VERYSILENT*)
    mkdir -p "$WINEPREFIX/drive_c/Program Files/Common Files/VST3"
    echo plugin > "$WINEPREFIX/drive_c/Program Files/Common Files/VST3/Faux.vst3" ;;
  *-quiet*)
    exit 67 ;;
  *"/s")
    echo $$ > "$dir/pid"
    exec sleep 30 ;;
esac
exit 0
