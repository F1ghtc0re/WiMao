#!/usr/bin/env bash
# Compile Wine 9.21 + wine-staging 9.21 en mode WoW64 et l'installe dans /opt/wine-mao.
# Sources dans src/, compilation et journaux dans build/ (ignorés par Git).
set -euo pipefail

VERSION=9.21
PREFIX=/opt/wine-mao
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="$ROOT/src"
BUILD="$ROOT/build/wine-$VERSION-wow64"
WINE_SHA256=4442b47ffd9b2ea457100e36ed5fd4e6f4d829d9db79a25e605175a988ca2fff
STAGING_SHA256=2e8c9cf9e378f99de133c7d86f9f0d5d506305869f4c802cb598f97b2f6bf8a6

mkdir -p "$SRC" "$ROOT/build"
cd "$SRC"

# 1. Téléchargement et vérification
[ -f wine-$VERSION.tar.xz ] || curl -fsSLO https://dl.winehq.org/wine/source/9.x/wine-$VERSION.tar.xz
[ -f wine-staging-$VERSION.tar.gz ] || curl -fsSL -o wine-staging-$VERSION.tar.gz \
    https://github.com/wine-staging/wine-staging/archive/refs/tags/v$VERSION.tar.gz
echo "$WINE_SHA256  wine-$VERSION.tar.xz" | sha256sum -c -
echo "$STAGING_SHA256  wine-staging-$VERSION.tar.gz" | sha256sum -c -

# 2. Extraction propre et application de tous les patchs staging
rm -rf wine-$VERSION wine-staging-$VERSION
tar xf wine-$VERSION.tar.xz
tar xf wine-staging-$VERSION.tar.gz
# GIT_CEILING_DIRECTORIES : sinon git apply (utilisé par gitapply.sh) voit le dépôt WiMao parent,
# lit les chemins depuis sa racine et ignore SILENCIEUSEMENT tous les fichiers de wine-$VERSION.
GIT_CEILING_DIRECTORIES="$SRC" \
python3 -I wine-staging-$VERSION/staging/patchinstall.py --destdir="$SRC/wine-$VERSION" --all \
    > "$ROOT/build/patchinstall.log" 2>&1
# Contrôle : le patch Staging/0002 ajoute « (Staging) » à la version dans configure.ac
grep -q '(Staging)' "$SRC/wine-$VERSION/configure.ac" \
    || { echo "ERREUR : les patchs staging ne sont pas appliqués" >&2; exit 1; }

# Correctifs WiMao (patches/) : rétroportages pour la toolchain récente d'Ubuntu 26.04
for p in "$ROOT"/patches/*.patch; do
    (cd "$SRC/wine-$VERSION" && GIT_CEILING_DIRECTORIES="$SRC" git apply "$p")
done

# 3. Configuration (WoW64, sans tests ni codecs vidéo ; C17 car GCC 15 passe en C23 par défaut)
rm -rf "$BUILD"
mkdir -p "$BUILD"
cd "$BUILD"
"$SRC/wine-$VERSION/configure" --prefix="$PREFIX" --enable-archs=i386,x86_64 \
    --disable-tests --without-ffmpeg --without-gstreamer \
    CFLAGS="-O2 -g0 -std=gnu17" CROSSCFLAGS="-O2 -g0" > "$ROOT/build/configure.log" 2>&1

# 4. Compilation et installation (/opt/wine-mao doit appartenir à l'utilisateur)
make -j"$(nproc)" > "$ROOT/build/make.log" 2>&1
make install > "$ROOT/build/install.log" 2>&1
"$PREFIX/bin/wine" --version
