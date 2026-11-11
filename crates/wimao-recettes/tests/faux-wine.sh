#!/bin/sh
dir=$(dirname "$0")
printf "%s | %s\n" "$(pwd)" "$*" >> "$dir/appels.log"
[ "$1" = "reg" ] && exit 0
menu="$WINEPREFIX/drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs/BLEASS"
# Pose le raccourci autonome (préparé par le test dans $dir) et son exécutable cible.
lanceur() {
  mkdir -p "$menu" "$WINEPREFIX/drive_c/Program Files/BLEASS"
  cp "$dir/lanceur.lnk" "$menu/BLEASS Plugins Installer.lnk"
  echo MZ > "$WINEPREFIX/drive_c/Program Files/BLEASS/BLEASS Plugins Installer.exe"
}
case "$*" in
  *lanceur*)
    lanceur ;;
  *melange*)
    # Un plugin, un lanceur autonome et un désinstalleur : seul le plugin compte.
    mkdir -p "$WINEPREFIX/drive_c/Program Files/Common Files/VST3"
    echo plugin > "$WINEPREFIX/drive_c/Program Files/Common Files/VST3/Faux.vst3"
    lanceur
    cp "$dir/desinstalleur.lnk" "$menu/Uninstall BLEASS.lnk"
    echo MZ > "$WINEPREFIX/drive_c/Program Files/BLEASS/unins000.exe" ;;
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
