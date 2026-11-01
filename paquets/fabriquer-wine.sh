#!/usr/bin/env bash
# Fabrique le paquet wimao-wine_<version>_amd64.deb : Wine 11.20 (archive officielle) avec le
# correctif patches-v2/ (annulation de la garde wm_state_serial de Wine 9.22), en mode WoW64,
# installé dans /opt/wimao/wine. Reprend scripts/build-wine-v2.sh (mêmes options `configure`,
# même contrôle du correctif) ; seuls le préfixe et la destination changent.
#
# Usage     : paquets/fabriquer-wine.sh <dossier de sortie>
#             paquets/fabriquer-wine.sh --paquets-compilation
#                 (affiche, un par ligne, les paquets Debian à installer pour compiler ; sert à la CI)
# Variables : WIMAO_CACHE_PAQUETS      dossier de cache des téléchargements
#                                      (défaut : <dossier de sortie>/cache)
#             SOURCE_DATE_EPOCH        date imposée aux fichiers (défaut : date du dernier commit)
#             WIMAO_WINE_DEJA_COMPILE  ESSAI D'EMPAQUETAGE SEULEMENT : dossier DESTDIR contenant déjà
#                                      un Wine installé sous opt/wimao/wine ; la compilation est
#                                      sautée et ce Wine est empaqueté tel quel (rien n'est téléchargé).
#                                      Le paquet obtenu n'est PAS celui de la version : jamais pour une livraison.
#             NPROC                    nombre de tâches de compilation (défaut : nproc)
# Écrit     : <sortie>/wimao-wine_11.20+wimao1_amd64.deb, le cache, et un dossier de travail
#             <sortie>/travail-wine (supprimé puis recréé à chaque fabrication ; la compilation
#             y occupe plusieurs Go).
# Contenu   : /opt/wimao/wine/ (Wine à l'exécution : sans en-têtes, bibliothèques .a ni outils de
#             développement winegcc, widl, winebuild…), le correctif dans
#             /usr/share/doc/wimao-wine/patches/, /usr/share/doc/wimao-wine/{copyright,changelog.gz}
#             et /usr/share/lintian/overrides/wimao-wine.
# Dépend de : les paquets de PAQUETS_COMPILATION, plus dpkg-deb et dpkg-shlibdeps (dpkg-dev).
# Les dépendances (Depends) sont CALCULÉES à la fabrication par dpkg-shlibdeps sur les
# binaires Linux, puis complétées par la liste fixe de control.in (voir paquets/wine/dependances.md).
set -euo pipefail

# shellcheck source=paquets/commun.sh
source "$(dirname "${BASH_SOURCE[0]}")/commun.sh"

# --- Paquets nécessaires à la compilation (Ubuntu 24.04 / Debian 13) ---
# Même ensemble que celui du Wine de développement (voir build/configure-v2.log) : un en-tête
# manquant fait silencieusement perdre une fonction à Wine (« configure: ... won't be supported »).
# Sont ajoutés par rapport à la machine de développement : libcups2-dev (impression) et
# libsdl2-dev (manettes), dont dépendent des Recommends du paquet.
PAQUETS_COMPILATION=(
    # Outils : compilateurs croisés PE (--enable-archs=i386,x86_64), analyseurs, extraction, correctif.
    build-essential ca-certificates gcc-mingw-w64 flex bison pkg-config curl xz-utils git
    # Affichage X11.
    libx11-dev libxext-dev libxrender-dev libxrandr-dev libxi-dev libxcursor-dev
    libxcomposite-dev libxfixes-dev libxinerama-dev libxxf86vm-dev
    # Polices, graphique, chiffrement.
    libfreetype-dev libfontconfig-dev libvulkan-dev libgl-dev libegl-dev libgnutls28-dev
    # Son, périphériques, bus système, impression, manettes, calcul, Kerberos.
    libpulse-dev libasound2-dev libudev-dev libusb-1.0-0-dev libdbus-1-dev libcups2-dev
    libsdl2-dev ocl-icd-opencl-dev libkrb5-dev
)

# Affichage de la liste pour la CI : `apt-get install -y $(paquets/fabriquer-wine.sh --paquets-compilation)`.
if [ "${1:-}" = "--paquets-compilation" ]; then
    printf '%s\n' "${PAQUETS_COMPILATION[@]}"
    exit 0
fi

if [ "$#" -ne 1 ]; then
    echo "Usage : $0 <dossier de sortie> | --paquets-compilation" >&2
    exit 2
fi
sortie="$(mkdir -p "$1" && cd "$1" && pwd)"
cache="${WIMAO_CACHE_PAQUETS:-$sortie/cache}"

# --- Version, adresse officielle et empreinte SHA-256 (doivent rester celles de scripts/build-wine-v2.sh) ---
VERSION_WINE=11.20
URL_WINE="https://dl.winehq.org/wine/source/11.x/wine-${VERSION_WINE}.tar.xz"
SHA256_WINE=eac8ca448f9c0087f963026b4dc276d1e2b7fbb64056edc093e59bbb7e56fe28
# Révision du conditionnement : à incrémenter si le correctif ou les options changent.
VERSION_PAQUET="${VERSION_WINE}+wimao1"
PREFIXE=/opt/wimao/wine
dossier_wine="$(dirname "${BASH_SOURCE[0]}")/wine"

# Date des fichiers : celle du dernier commit qui touche les ENTRÉES DU CACHE de la CI, et non celle de
# HEAD. Le .deb de wimao-wine est mis en cache par paquets.yml avec une clé calculée sur ces mêmes
# chemins (patches-v2/**, paquets/fabriquer-wine.sh, paquets/wine/**, paquets/commun.sh) : tant que la
# clé ne change pas, la date ne doit pas changer non plus, sinon un paquet servi par le cache
# différerait d'une refabrication. Garder cette liste et la clé de cache de .github/workflows/paquets.yml
# identiques. Un clone superficiel (fetch-depth: 1) ne donne pas ce commit : on s'arrête.
if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
    SOURCE_DATE_EPOCH="$(git -C "$RACINE_DEPOT" log -1 --format=%ct -- patches-v2 paquets/fabriquer-wine.sh paquets/wine paquets/commun.sh)"
    if [ -z "$SOURCE_DATE_EPOCH" ]; then
        echo "ERREUR : date des sources de wimao-wine introuvable (clone superficiel ? utilisez fetch-depth: 0)" >&2
        exit 1
    fi
fi
export SOURCE_DATE_EPOCH
date_source

# --- Arborescence du paquet, reconstruite à neuf ---
travail="$sortie/travail-wine"
racine_paquet="$travail/paquet"
rm -rf "$travail"
mkdir -p "$racine_paquet/DEBIAN" "$racine_paquet/usr/share/doc/wimao-wine/patches"

if [ -n "${WIMAO_WINE_DEJA_COMPILE:-}" ]; then
    # Essai d'empaquetage : on copie (sans toucher à l'original) le Wine déjà installé.
    echo "ATTENTION : Wine déjà compilé (${WIMAO_WINE_DEJA_COMPILE}) ; compilation sautée, paquet d'essai." >&2
    [ -x "$WIMAO_WINE_DEJA_COMPILE$PREFIXE/bin/wine" ] || {
        echo "ERREUR : $WIMAO_WINE_DEJA_COMPILE$PREFIXE/bin/wine introuvable" >&2
        exit 1
    }
    mkdir -p "$racine_paquet/opt/wimao"
    cp -a "$WIMAO_WINE_DEJA_COMPILE$PREFIXE" "$racine_paquet$PREFIXE"
else
    telecharger_verifie "$URL_WINE" "$cache/wine-${VERSION_WINE}.tar.xz" "$SHA256_WINE"

    # Sources : archive officielle + correctif (appliqué avec git apply, sans dépôt git autour).
    src="$travail/src"
    mkdir -p "$src"
    tar -xf "$cache/wine-${VERSION_WINE}.tar.xz" -C "$src"
    for p in "$RACINE_DEPOT"/patches-v2/*.patch; do
        (cd "$src/wine-${VERSION_WINE}" && GIT_CEILING_DIRECTORIES="$travail" git apply "$p")
    done
    # Contrôle : la garde de window_update_client_config doit être commentée.
    grep -q '^    //if (data->wm_state_serial) return 0;' "$src/wine-${VERSION_WINE}/dlls/winex11.drv/window.c" \
        || { echo "ERREUR : patch wm_state_serial non appliqué" >&2; exit 1; }

    # Compilation hors des sources. -ffile-prefix-map retire le chemin de travail des binaires
    # (reproductibilité) ; -g0 : pas d'informations de débogage (paquet plus léger).
    compil="$travail/build"
    mkdir -p "$compil"
    drapeaux="-O2 -g0 -ffile-prefix-map=${src}=."
    (cd "$compil" && "$src/wine-${VERSION_WINE}/configure" --prefix="$PREFIXE" \
        --enable-archs=i386,x86_64 --disable-tests --without-ffmpeg --without-gstreamer \
        CFLAGS="$drapeaux" CROSSCFLAGS="$drapeaux" > "$travail/configure.log" 2>&1)
    make -C "$compil" -j"${NPROC:-$(nproc)}" > "$travail/make.log" 2>&1
    make -C "$compil" install DESTDIR="$racine_paquet" > "$travail/install.log" 2>&1
    rm -rf "$src" "$compil"
fi

# Contrôle d'emplacement : tout Wine doit être sous /opt/wimao/wine.
[ -x "$racine_paquet$PREFIXE/bin/wine" ] || { echo "ERREUR : bin/wine absent du paquet" >&2; exit 1; }

# --- Fichiers de développement retirés ---
# WiMao n'en a jamais besoin à l'exécution : en-têtes, bibliothèques statiques (.a) et chaîne
# d'outils pour compiler des programmes Windows (winegcc & co.), avec leurs pages de manuel.
racine_wine="$racine_paquet$PREFIXE"
rm -rf "$racine_wine/include"
find "$racine_wine/lib/wine" -type f -name '*.a' -delete
for outil in winegcc wineg++ winecpp winebuild widl wmc wrc function_grep.pl winemaker winedump; do
    rm -f "$racine_wine/bin/$outil"
    # Pages de manuel de l'outil : share/man/man1 et les variantes par langue (*.UTF-8/man1).
    find "$racine_wine/share/man" -type f -name "$outil.1*" -delete 2>/dev/null || true
done
# Garde : un Wine qui garde ses fichiers de développement serait un paquet faux.
if [ -e "$racine_wine/bin/winegcc" ] || [ -e "$racine_wine/include" ]; then
    echo "ERREUR : fichiers de développement encore présents dans le paquet" >&2
    exit 1
fi

# Les pages de manuel restantes sont livrées compressées (charte Debian) ; gzip -n : sans date,
# donc reproductible. Un échec de gzip arrête le script ; seule l'absence du dossier est tolérée.
if [ -d "$racine_wine/share/man" ]; then
    find "$racine_wine/share/man" -type f -name '*.[0-9]*' ! -name '*.gz' -exec gzip -9n {} +
fi

# --- Documentation légale, correctif en source (obligation LGPL) et journal ---
install -m 0644 "$dossier_wine/copyright" "$racine_paquet/usr/share/doc/wimao-wine/copyright"
# Écarts volontaires à la charte Debian, avec leur justification (lisibles par lintian).
mkdir -p "$racine_paquet/usr/share/lintian/overrides"
install -m 0644 "$dossier_wine/lintian-overrides" "$racine_paquet/usr/share/lintian/overrides/wimao-wine"
for p in "$RACINE_DEPOT"/patches-v2/*.patch; do
    install -m 0644 "$p" "$racine_paquet/usr/share/doc/wimao-wine/patches/$(basename "$p")"
done
# changelog.gz et non changelog.Debian.gz : la version 11.20+wimao1 n'a pas de révision Debian
# (« -N »), le paquet est donc « natif » pour dpkg et lintian (wrong-name-for-changelog-of-native-package).
gzip -9n -c "$dossier_wine/changelog" > "$racine_paquet/usr/share/doc/wimao-wine/changelog.gz"

# --- Binaires Linux : allègement puis dépendances calculées ---
# Les ELF sont ceux de bin/ et de lib/wine/x86_64-unix/ (les DLL de x86_64-windows/ et
# i386-windows/ sont au format PE : ni allégées ici, ni sources de dépendances Linux).
elfs=()
while IFS= read -r -d '' fichier; do
    # Seuls les vrais ELF (les liens et scripts de bin/ sont ignorés).
    if [ "$(head -c4 "$fichier" | od -An -c | tr -d ' ')" = '177ELF' ]; then
        elfs+=("$fichier")
    fi
done < <(find "$racine_paquet$PREFIXE/bin" "$racine_paquet$PREFIXE/lib/wine/x86_64-unix" -type f -print0 | sort -z)
# Symboles inutiles retirés (exigence de lintian ; résultat déterministe).
strip --strip-unneeded "${elfs[@]}"

# dpkg-shlibdeps exige un fichier debian/control dans le dossier courant : on en fabrique un
# minimal dans un dossier jetable.
# -l indique où trouver les bibliothèques propres à Wine (ntdll.so, win32u.so…), qui ne sont
# pas des paquets : sans lui dpkg-shlibdeps avertit pour chacune.
analyse="$travail/analyse"
mkdir -p "$analyse/debian"
printf 'Source: wimao-wine\n\nPackage: wimao-wine\nArchitecture: amd64\n' > "$analyse/debian/control"
options=("-l$racine_paquet$PREFIXE/lib/wine/x86_64-unix")
for fichier in "${elfs[@]}"; do options+=("-e$fichier"); done
# Les avertissements de dpkg-shlibdeps restent visibles (stderr non masqué) pour le diagnostic.
dependances="$(cd "$analyse" && dpkg-shlibdeps -O --ignore-missing-info "${options[@]}" | sed -n 's/^shlibs:Depends=//p')"
# Une liste vide donnerait un paquet installable sans ses bibliothèques système : on refuse.
if [ -z "$dependances" ]; then
    echo "ERREUR : dpkg-shlibdeps n'a relevé aucune dépendance de bibliothèque." >&2
    exit 1
fi
echo "Dépendances relevées : ${dependances}"

# --- Fichier control ---
cp "$dossier_wine/control.in" "$racine_paquet/DEBIAN/control"
remplacer_marqueur "$racine_paquet/DEBIAN/control" "@VERSION@" "${VERSION_PAQUET}"
remplacer_marqueur "$racine_paquet/DEBIAN/control" "@DEPENDANCES@" "${dependances:+, ${dependances}}"
# Un marqueur @...@ resté dans le fichier produirait un paquet faux : on s'arrête.
if grep -q '@[A-Z]*@' "$racine_paquet/DEBIAN/control"; then
    echo "ERREUR : marqueur non remplacé dans DEBIAN/control" >&2
    exit 1
fi
chmod 0644 "$racine_paquet/DEBIAN/control"

# --- Aucune DLL Microsoft dans le paquet (contrainte globale) ---
# Contrôle de sécurité contre une erreur de manipulation (une vraie DLL de Windows glissée dans l'arbre
# installé), pas une preuve d'origine : celle-ci vient des sources officielles de Wine (empreinte SHA-256).
# Les DLL de Wine ELLES-MÊMES portent « Microsoft Corporation » comme CompanyName dans leurs ressources de
# version (Wine imite Windows : constaté sur ntdll.dll, kernel32.dll…) ; on ne peut donc pas refuser ce texte.
# Ce qui distingue une DLL de Wine : Winebuild écrit la marque « Wine builtin DLL » (ou « Wine placeholder
# DLL ») dans l'en-tête du fichier (constaté sur les ~1 000 DLL et .exe de Wine 11.20). Règle : tout
# fichier PE (commence par « MZ ») dont les ressources disent « Microsoft Corporation » (texte UTF-16,
# strings -el) doit porter cette marque dans ses 512 premiers octets ; sinon, arrêt.
refuser_pe_microsoft() {
    local dossier="$1" fichier faux=0
    while IFS= read -r -d '' fichier; do
        [ "$(head -c 2 "$fichier")" = "MZ" ] || continue
        # Substitution de processus et non tuyau : avec pipefail, `strings … | grep -q` rendrait « faux »
        # quand grep s'arrête au premier résultat et que strings reçoit SIGPIPE (gros fichiers) ; le
        # fichier serait alors sauté sans contrôle.
        grep -q "Microsoft Corporation" < <(strings -el "$fichier") || continue
        grep -aqE "Wine (builtin|placeholder) DLL" < <(head -c 512 "$fichier") && continue
        echo "ERREUR : fichier Windows qui n'est pas un fichier de Wine (Microsoft Corporation, sans marque « Wine builtin DLL ») : ${fichier#"$dossier"/}" >&2
        faux=1
    done < <(find "$dossier" -type f -print0)
    [ "$faux" -eq 0 ] || exit 1
}
refuser_pe_microsoft "$racine_paquet"
echo "ok : aucun fichier PE de Microsoft dans le paquet"

normaliser_arborescence "$racine_paquet"
construire_deb "$racine_paquet" "$sortie/wimao-wine_${VERSION_PAQUET}_amd64.deb"
rm -rf "$travail"
echo "Paquet : $sortie/wimao-wine_${VERSION_PAQUET}_amd64.deb"
