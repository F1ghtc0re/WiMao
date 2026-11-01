#!/usr/bin/env bash
# Variante 2 : Wine récent (sans staging) avec uniquement l'annulation de la garde wm_state_serial
# de Wine 9.22 (patches-v2/), en mode WoW64, installé dans /opt/wine-mao-v2.
set -euo pipefail

VERSION=11.20
PREFIX=/opt/wine-mao-v2
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/src"
BUILD="$ROOT/build/wine-$VERSION-v2"
WINE_SHA256=eac8ca448f9c0087f963026b4dc276d1e2b7fbb64056edc093e59bbb7e56fe28

mkdir -p "$SRC" "$ROOT/build"
cd "$SRC"
[ -f wine-$VERSION.tar.xz ] || curl -fsSLO https://dl.winehq.org/wine/source/11.x/wine-$VERSION.tar.xz
echo "$WINE_SHA256  wine-$VERSION.tar.xz" | sha256sum -c -

rm -rf wine-$VERSION
tar xf wine-$VERSION.tar.xz
for p in "$ROOT"/patches-v2/*.patch; do
    (cd "$SRC/wine-$VERSION" && GIT_CEILING_DIRECTORIES="$SRC" git apply "$p")
done
# Contrôle : la garde de window_update_client_config doit être commentée
grep -q '^    //if (data->wm_state_serial) return 0;' "$SRC/wine-$VERSION/dlls/winex11.drv/window.c" \
    || { echo "ERREUR : patch wm_state_serial non appliqué" >&2; exit 1; }

rm -rf "$BUILD"
mkdir -p "$BUILD"
cd "$BUILD"
"$SRC/wine-$VERSION/configure" --prefix="$PREFIX" --enable-archs=i386,x86_64 \
    --disable-tests --without-ffmpeg --without-gstreamer \
    CFLAGS="-O2 -g0" CROSSCFLAGS="-O2 -g0" > "$ROOT/build/configure-v2.log" 2>&1
make -j"$(nproc)" > "$ROOT/build/make-v2.log" 2>&1
make install > "$ROOT/build/install-v2.log" 2>&1
"$PREFIX/bin/wine" --version
