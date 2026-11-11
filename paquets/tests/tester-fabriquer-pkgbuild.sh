#!/usr/bin/env bash
# Teste paquets/fabriquer-pkgbuild.sh avec un faux SHA256SUMS (trois .deb et trois .rpm aux empreintes
# inventées) et une empreinte de clé d'essai (--cle). Il faut podman : le .SRCINFO est produit dans l'image
# Arch figée (IMAGE_ARCH, paquets/aur/commun-aur.sh), sur un poste qui n'est pas Arch.
#
# Cas : sortie réelle (pkgver = version de Cargo.toml, cinq adresses dans l'ordre, empreintes, validpgpkeys,
#   aucun .rpm) ; deux exécutions identiques (cmp) ; .SRCINFO (pkgbase, depends, optdepends) ; refus (code 1) :
#   .deb de wine absent, quatrième .deb, version de wimao différente de Cargo.toml, version mal formée ;
#   mode --local (marque, trois noms nus, pas de validpgpkeys) ; usage (code 2) : argument manquant, option
#   inconnue.
#
# Usage     : paquets/tests/tester-fabriquer-pkgbuild.sh
# Variables : aucune.
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fabriquer="$racine/paquets/fabriquer-pkgbuild.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

[ -f "$fabriquer" ] || echec "paquets/fabriquer-pkgbuild.sh est introuvable"
command -v podman > /dev/null || echec "podman est nécessaire à cet essai"

version="$(awk -F'"' '/^version = / { print $2; exit }' "$racine/Cargo.toml")"
cle="0123456789ABCDEF0123456789ABCDEF01234567"
deb_wimao="wimao_${version}-1_amd64.deb"
deb_bridge="wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb"
deb_wine="wimao-wine_11.20+wimao1_amd64.deb"
adresse="https://github.com/F1ghtc0re/WiMao/releases/download/v\${pkgver}"

# Écrit dans $1 un SHA256SUMS d'essai : les trois .deb (empreintes 11…, 22…, 33…) et trois .rpm.
faux_sommes() {
    local fichier="$1" c
    : > "$fichier"
    for c in "1 $deb_wimao" "2 $deb_bridge" "3 $deb_wine" "4 wimao-$version-1.x86_64.rpm" \
        "5 wimao-bridge-5.1.1+dxvk3.1.1-1.x86_64.rpm" "6 wimao-wine-11.20+wimao1-1.x86_64.rpm"; do
        printf '%s  %s\n' "$(printf '%064d' 0 | tr 0 "${c%% *}")" "${c#* }" >> "$fichier"
    done
}

# attend_code <code attendu> <description> <commande...> : la commande doit se terminer avec ce code.
attend_code() {
    local attendu="$1" description="$2" code=0
    shift 2
    "$@" > "$temporaire/sortie.txt" 2>&1 || code=$?
    [ "$code" -eq "$attendu" ] || {
        cat "$temporaire/sortie.txt" >&2
        echec "$description : code $code au lieu de $attendu"
    }
}

contient() { grep -qxF -- "$2" "$1" || echec "$3 : ligne absente « $2 »"; }

sommes="$temporaire/SHA256SUMS"
faux_sommes "$sommes"

# --- Sortie réelle ---
attend_code 0 "fabrication réelle" "$fabriquer" "$sommes" "$temporaire/a" --cle "$cle"
pkg="$temporaire/a/PKGBUILD"
contient "$pkg" "pkgver=$version" "pkgver"
contient "$pkg" "validpgpkeys=($cle)" "validpgpkeys"
contient "$pkg" "  $adresse/$deb_wimao" "source wimao"
# Les cinq adresses dans l'ordre : on compare le bloc source=( … ) en entier.
attendu="source=(
  $adresse/$deb_wimao
  $adresse/$deb_bridge
  $adresse/$deb_wine
  $adresse/SHA256SUMS
  $adresse/SHA256SUMS.asc
)"
[ "$(sed -n '/^source=(/,/^)/p' "$pkg")" = "$attendu" ] || echec "source= différent des cinq adresses attendues"
somme_sommes="$(sha256sum "$sommes" | awk '{ print $1 }')"
attendu="sha256sums=(
  $(printf '%064d' 0 | tr 0 1)
  $(printf '%064d' 0 | tr 0 2)
  $(printf '%064d' 0 | tr 0 3)
  $somme_sommes
  SKIP
)"
[ "$(sed -n '/^sha256sums=(/,/^)/p' "$pkg")" = "$attendu" ] || echec "sha256sums= incorrect"
! grep -q 'rpm' "$pkg" || echec "un .rpm est entré dans le PKGBUILD"
! grep -q 'ESSAI LOCAL' "$pkg" || echec "marque d'essai local en mode réel"
grep -q '^prepare()' "$pkg" || echec "prepare() absent en mode réel"
src="$temporaire/a/.SRCINFO"
contient "$src" "pkgbase = wimao-bin" ".SRCINFO"
grep -qP '^\tdepends = gtk4$' "$src" || echec ".SRCINFO sans « depends = gtk4 »"
grep -qP '^\toptdepends = carla: ' "$src" || echec ".SRCINFO sans « optdepends = carla: … »"
grep -qP '^\tprovides = wimao$' "$src" || echec ".SRCINFO sans provides = wimao"

# --- Reproductibilité ---
attend_code 0 "seconde fabrication" "$fabriquer" "$sommes" "$temporaire/b" --cle "$cle"
cmp "$temporaire/a/PKGBUILD" "$temporaire/b/PKGBUILD" || echec "PKGBUILD différent entre deux exécutions"
cmp "$temporaire/a/.SRCINFO" "$temporaire/b/.SRCINFO" || echec ".SRCINFO différent entre deux exécutions"

# --- Refus (code 1) ---
grep -v "$deb_wine" "$sommes" > "$temporaire/sans-wine"
attend_code 1 "SHA256SUMS sans le .deb de wine" "$fabriquer" "$temporaire/sans-wine" "$temporaire/r1" --cle "$cle"
{ cat "$sommes"; printf '%064d  autre_1.0_amd64.deb\n' 7; } > "$temporaire/quatre"
attend_code 1 "quatrième .deb" "$fabriquer" "$temporaire/quatre" "$temporaire/r2" --cle "$cle"
sed "s/$deb_wimao/wimao_9.9.9-1_amd64.deb/" "$sommes" > "$temporaire/autre-version"
attend_code 1 "version de wimao différente de Cargo.toml" "$fabriquer" "$temporaire/autre-version" "$temporaire/r3" --cle "$cle"
sed "s/$deb_wimao/wimao_0.7-1_amd64.deb/" "$sommes" > "$temporaire/mal-formee"
attend_code 1 "version mal formée" "$fabriquer" "$temporaire/mal-formee" "$temporaire/r4" --cle "$cle"
[ ! -e "$temporaire/r1/PKGBUILD" ] || echec "un refus a laissé un PKGBUILD"

# --- Mode local ---
attend_code 0 "mode local" "$fabriquer" "$sommes" "$temporaire/l" --local
pkg="$temporaire/l/PKGBUILD"
# La marque est celle que cherchent les refus (constante de commun-aur.sh, recherche par sous-chaîne).
( source "$racine/paquets/aur/commun-aur.sh"; head -n1 "$pkg" | grep -qF "$MARQUE_ESSAI_LOCAL" ) || echec "la marque du mode local ne contient pas MARQUE_ESSAI_LOCAL"
[ "$(head -n1 "$pkg")" = "# ESSAI LOCAL, NE PAS PUBLIER" ] || echec "première ligne du mode local"
attendu="source=(
  $deb_wimao
  $deb_bridge
  $deb_wine
)"
[ "$(sed -n '/^source=(/,/^)/p' "$pkg")" = "$attendu" ] || echec "source= du mode local"
! grep -q 'validpgpkeys' "$pkg" || echec "validpgpkeys en mode local"
! grep -q '^prepare()' "$pkg" || echec "prepare() en mode local"

# --- Échec de makepkg : faux makepkg en tête du PATH (chemin « makepkg présent »), qui écrit un début de
# sortie puis échoue. Le script doit échouer et ne laisser aucun .SRCINFO.
mkdir "$temporaire/faux-bin"
printf '#!/bin/sh\necho partiel\nexit 1\n' > "$temporaire/faux-bin/makepkg"
chmod +x "$temporaire/faux-bin/makepkg"
PATH="$temporaire/faux-bin:$PATH" attend_code 1 "makepkg en échec" "$fabriquer" "$sommes" "$temporaire/m" --cle "$cle"
[ ! -e "$temporaire/m/.SRCINFO" ] || echec "un makepkg en échec a laissé un .SRCINFO"

# --- Usage (code 2) ---
attend_code 2 "sans argument" "$fabriquer"
attend_code 2 "sortie manquante" "$fabriquer" "$sommes"
attend_code 2 "option inconnue" "$fabriquer" "$sommes" "$temporaire/u" --bizarre
attend_code 2 "--cle sans valeur" "$fabriquer" "$sommes" "$temporaire/u" --cle

echo "OK : paquets/fabriquer-pkgbuild.sh se comporte comme prévu."
