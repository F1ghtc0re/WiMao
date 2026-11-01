#!/usr/bin/env bash
# Fabrique le paquet wimao-bridge_<version>_amd64.deb : yabridge 5.1.1 (binaires officiels) et
# les 8 DLL officielles de DXVK 3.1.1 (4 en 64 bits, 4 en 32 bits).
#
# Usage     : paquets/fabriquer-bridge.sh <dossier de sortie>
# Variables : WIMAO_CACHE_PAQUETS  dossier de cache des téléchargements
#                                  (défaut : <dossier de sortie>/cache)
#             SOURCE_DATE_EPOCH    date imposée aux fichiers (défaut : date du dernier commit)
# Écrit     : <sortie>/wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb, le cache, et un dossier
#             de travail <sortie>/travail-bridge (supprimé puis recréé à chaque fabrication).
# Contenu   : /opt/wimao/yabridge/ (archive yabridge, hôtes Wine réglés sur /opt/wimao/wine),
#             /opt/wimao/dxvk/{x64,x32}/*.dll (disposition attendue par
#             crates/wimao-core/src/correctifs/dxvk.rs), /usr/share/doc/wimao-bridge/{copyright,changelog.Debian.gz} et
#             /usr/share/lintian/overrides/wimao-bridge.
# Dépend de : curl, tar, sha256sum, dpkg-deb, dpkg-shlibdeps (paquet dpkg-dev).
# Les versions et empreintes ci-dessous doivent rester celles de scripts/install-yabridge.sh,
# scripts/install-dxvk.sh et crates/wimao-core/src/correctifs/dxvk.rs.
set -euo pipefail

# shellcheck source=paquets/commun.sh
source "$(dirname "${BASH_SOURCE[0]}")/commun.sh"

if [ "$#" -ne 1 ]; then
    echo "Usage : $0 <dossier de sortie>" >&2
    exit 2
fi
sortie="$(mkdir -p "$1" && cd "$1" && pwd)"
cache="${WIMAO_CACHE_PAQUETS:-$sortie/cache}"

# --- Versions, adresses officielles (Releases GitHub) et empreintes SHA-256 ---
VERSION_YABRIDGE=5.1.1
URL_YABRIDGE="https://github.com/robbert-vdh/yabridge/releases/download/${VERSION_YABRIDGE}/yabridge-${VERSION_YABRIDGE}.tar.gz"
SHA256_YABRIDGE=d11f0307412b5566b09a75ed4aa191266db56d22f4de4d20e4fbde6296cd1a47
VERSION_DXVK=3.1.1
URL_DXVK="https://github.com/doitsujin/dxvk/releases/download/v${VERSION_DXVK}/dxvk-${VERSION_DXVK}.tar.gz"
SHA256_DXVK=40565b4a724aadc4433fa4e010b4b23916d9b1f1baeee64e17186db94f54e608
# Le Wine du paquet wimao-wine : valeur par défaut de WINELOADER dans les hôtes yabridge.
WINE_PAQUET=/opt/wimao/wine/bin/wine
# Les seules DLL DXVK utilisées par WiMao.
DLLS_DXVK=(d3d11 dxgi d3d10core d3d9)
VERSION_PAQUET="${VERSION_YABRIDGE}+dxvk${VERSION_DXVK}-1"

date_source
telecharger_verifie "$URL_YABRIDGE" "$cache/yabridge-${VERSION_YABRIDGE}.tar.gz" "$SHA256_YABRIDGE"
telecharger_verifie "$URL_DXVK" "$cache/dxvk-${VERSION_DXVK}.tar.gz" "$SHA256_DXVK"

# --- Arborescence du paquet, reconstruite à neuf ---
travail="$sortie/travail-bridge"
racine_paquet="$travail/paquet"
rm -rf "$travail"
mkdir -p "$racine_paquet/opt/wimao" "$racine_paquet/usr/share/doc/wimao-bridge" "$racine_paquet/DEBIAN"

# yabridge : le contenu de l'archive (dossier « yabridge/ ») devient /opt/wimao/yabridge.
tar -xzf "$cache/yabridge-${VERSION_YABRIDGE}.tar.gz" -C "$travail"
mv "$travail/yabridge" "$racine_paquet/opt/wimao/yabridge"

# Les deux lanceurs de l'hôte appellent « wine » du PATH quand WINELOADER n'est pas exécutable.
# On remplace ce défaut par le Wine du paquet wimao-wine (même correctif que scripts/install-yabridge.sh,
# mais ici sur la ligne `if [ ! -x "$WINELOADER" ]; then WINELOADER="wine"; fi`).
for hote in yabridge-host.exe yabridge-host-32.exe; do
    fichier="$racine_paquet/opt/wimao/yabridge/$hote"
    sed -i "s|WINELOADER=\"wine\"|WINELOADER=\"${WINE_PAQUET}\"|" "$fichier"
    # Sans cette vérification, une archive qui changerait de forme donnerait un paquet qui lance le mauvais Wine.
    grep -qF "WINELOADER=\"${WINE_PAQUET}\"" "$fichier" || {
        echo "ERREUR : $hote n'a pas été modifié (WINELOADER)" >&2
        exit 1
    }
done

# DXVK : seulement les 4 DLL utiles, dans x64/ et x32/.
tar -xzf "$cache/dxvk-${VERSION_DXVK}.tar.gz" -C "$travail"
for arch in x64 x32; do
    mkdir -p "$racine_paquet/opt/wimao/dxvk/$arch"
    for dll in "${DLLS_DXVK[@]}"; do
        install -m 0644 "$travail/dxvk-${VERSION_DXVK}/$arch/$dll.dll" "$racine_paquet/opt/wimao/dxvk/$arch/$dll.dll"
    done
done

# Documentation légale et journal des changements (gzip -n : sans date ni nom, donc reproductible).
dossier_bridge="$(dirname "${BASH_SOURCE[0]}")/bridge"
install -m 0644 "$dossier_bridge/copyright" "$racine_paquet/usr/share/doc/wimao-bridge/copyright"
gzip -9n -c "$dossier_bridge/changelog" > "$racine_paquet/usr/share/doc/wimao-bridge/changelog.Debian.gz"
# Écarts volontaires à la charte Debian, avec leur justification (lisibles par lintian).
mkdir -p "$racine_paquet/usr/share/lintian/overrides"
install -m 0644 "$dossier_bridge/lintian-overrides" "$racine_paquet/usr/share/lintian/overrides/wimao-bridge"

# --- Dépendances sur les bibliothèques système, relevées sur les binaires Linux de yabridge ---
# dpkg-shlibdeps exige un fichier debian/control dans le dossier courant : on en fabrique un
# minimal dans un dossier jetable.
analyse="$travail/analyse"
mkdir -p "$analyse/debian"
printf 'Source: wimao-bridge\n\nPackage: wimao-bridge\nArchitecture: amd64\n' > "$analyse/debian/control"
options=()
for elf in "$racine_paquet"/opt/wimao/yabridge/*.so "$racine_paquet/opt/wimao/yabridge/yabridgectl"; do
    # Les « .so » des hôtes Wine (yabridge-host*.exe.so) sont des bibliothèques Wine, pas des ELF Linux ordinaires.
    case "$elf" in *.exe.so) continue ;; esac
    options+=("-e$elf")
done
# Les avertissements de dpkg-shlibdeps restent visibles (stderr non masqué) pour le diagnostic.
dependances="$(cd "$analyse" && dpkg-shlibdeps -O --ignore-missing-info "${options[@]}" | sed -n 's/^shlibs:Depends=//p')"
# Une liste vide donnerait un paquet installable sans ses bibliothèques système : on refuse.
if [ -z "$dependances" ]; then
    echo "ERREUR : dpkg-shlibdeps n'a relevé aucune dépendance de bibliothèque." >&2
    exit 1
fi
echo "Dépendances relevées : ${dependances}"

# --- Fichier control ---
cp "$dossier_bridge/control.in" "$racine_paquet/DEBIAN/control"
remplacer_marqueur "$racine_paquet/DEBIAN/control" "@VERSION@" "${VERSION_PAQUET}"
remplacer_marqueur "$racine_paquet/DEBIAN/control" "@DEPENDANCES@" "${dependances:+, ${dependances}}"
# Un marqueur @...@ resté dans le fichier produirait un paquet faux : on s'arrête.
if grep -q '@[A-Z]*@' "$racine_paquet/DEBIAN/control"; then
    echo "ERREUR : marqueur non remplacé dans DEBIAN/control" >&2
    exit 1
fi
chmod 0644 "$racine_paquet/DEBIAN/control"

normaliser_arborescence "$racine_paquet"
construire_deb "$racine_paquet" "$sortie/wimao-bridge_${VERSION_PAQUET}_amd64.deb"
rm -rf "$travail"
echo "Paquet : $sortie/wimao-bridge_${VERSION_PAQUET}_amd64.deb"
