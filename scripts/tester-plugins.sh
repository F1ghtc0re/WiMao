#!/usr/bin/env bash
# Vérifie que des plugins VST3 pontés par yabridge s'ouvrent : chargement dans carla-single,
# attente de la fenêtre d'interface, capture d'écran, fermeture.
# Usage : tester-plugins.sh "Nom" ...   (Nom = nom du fichier .vst3 dans ~/.vst3/yabridge, sous-dossiers compris)
# Captures et journaux : $SORTIE (par défaut /tmp/wimao-tests)
# Limites : sous XWayland, les plugins rendus en Direct3D 9 (iZotope) donnent des captures blanches ;
# certains plugins (AIR DrumSynth) font planter Carla lui-même.
SORTIE="${SORTIE:-/tmp/wimao-tests}"; mkdir -p "$SORTIE"
for N in "$@"; do
    P=$(find "$HOME/.vst3/yabridge" -maxdepth 2 -name "$N.vst3" | head -1)
    [ -n "$P" ] || { echo "$N : introuvable dans ~/.vst3/yabridge"; continue; }
    F="${N//[ !]/_}"; LOG="$SORTIE/$F.log"
    ( cd "$SORTIE" && exec setsid carla-single vst3 "$P" ) > "$LOG" 2>&1 &
    PG=$!
    R="ÉCHEC (délai)"; W=""
    for _ in $(seq 1 100); do
        W=$(xwininfo -root -tree | grep '(GUI)": ()' | awk '{print $1}' | head -1)
        if [ -n "$W" ]; then R=OK; break; fi
        if grep -qE 'aborting|stack overflow|Unhandled exception' "$LOG"; then
            R="PLANTAGE : $(grep -m1 -E 'aborting|overflow|Unhandled' "$LOG" | cut -c1-160)"; break
        fi
        kill -0 $PG 2>/dev/null || { R="ÉCHEC (Carla s'est arrêté)"; break; }
        sleep 3
    done
    if [ "$R" = OK ]; then sleep 10; xwd -id "$W" -silent | convert xwd:- -resize 50% "$SORTIE/$F.png"; fi
    echo "$N : $R"
    kill -- -$PG 2>/dev/null; wait $PG 2>/dev/null; sleep 6
done
