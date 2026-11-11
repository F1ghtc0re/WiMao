#!/usr/bin/env bash
# Fabrique la recette AUR (PKGBUILD et .SRCINFO) du paquet wimao-bin à partir du SHA256SUMS d'une Release.
#
# Usage     : paquets/fabriquer-pkgbuild.sh <SHA256SUMS> <dossier de sortie> [--local] [--cle <empreinte>]
#             --local  essai local : sources = noms de fichiers locaux, sans SHA256SUMS ni signature, et le
#                      PKGBUILD porte la ligne « # ESSAI LOCAL, NE PAS PUBLIER » (jamais à publier).
#             --cle    empreinte (40 chiffres hexadécimaux) écrite dans validpgpkeys à la place de celle de
#                      SECURITY.md ; réservé aux tests.
# Étapes    : 1. lit SHA256SUMS : exactement trois .deb (wimao, wimao-bridge, wimao-wine, versions bien formées,
#                les .rpm sont ignorés), la version de WiMao devant être celle de Cargo.toml ;
#             2. remplit le modèle paquets/aur/PKGBUILD.in avec paquets/aur/dependances ;
#             3. écrit PKGBUILD puis .SRCINFO (makepkg --printsrcinfo, utilisateur non privilégié).
# Lit       : SHA256SUMS, Cargo.toml, SECURITY.md (empreinte), paquets/aur/{PKGBUILD.in,prepare.in,dependances}.
# Écrit     : <sortie>/PKGBUILD et <sortie>/.SRCINFO ; rien d'autre (le travail se fait dans un dossier
#             temporaire supprimé par trap). Même entrée, même sortie, octet pour octet.
# Dépend de : awk, sha256sum, grep, podman (sauf sur Arch, qui fournit makepkg).
# Code      : 0 réussite, 1 refus (message « ERREUR : … »), 2 mauvais usage.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=paquets/aur/commun-aur.sh
source "$racine/paquets/aur/commun-aur.sh"
# shellcheck source=scripts/lib/publication.sh
source "$racine/scripts/lib/publication.sh"

usage() {
    echo "Usage : $0 <SHA256SUMS> <dossier de sortie> [--local] [--cle <empreinte>]" >&2
    exit 2
}
refus() {
    echo "ERREUR : $1" >&2
    exit 1
}

# --- Arguments ---
[ "$#" -ge 2 ] || usage
sommes="$1"
sortie_arg="$2"
shift 2
local_=0
cle=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --local) local_=1 ;;
        --cle)
            [ "$#" -ge 2 ] || usage
            cle="$2"
            shift
            ;;
        *) usage ;;
    esac
    shift
done
[ -f "$sommes" ] || refus "$sommes introuvable"

# --- Version de WiMao (workspace Cargo) ---
version_cargo="$(awk -F'"' '/^version = / { print $2; exit }' "$racine/Cargo.toml")"
[ -n "$version_cargo" ] || refus "version introuvable dans Cargo.toml"

# --- Les trois .deb de SHA256SUMS (les lignes .rpm sont ignorées) ---
# Chaque ligne : « <empreinte>  <nom> » (un « * » devant le nom est toléré).
nom_wimao="" nom_bridge="" nom_wine=""
somme_wimao="" somme_bridge="" somme_wine=""
version_wimao=""
while read -r somme nom; do
    nom="${nom#\*}"
    case "$nom" in *.deb) ;; *) continue ;; esac
    [[ "$somme" =~ ^[0-9a-f]{64}$ ]] || refus "empreinte mal formée dans SHA256SUMS pour $nom"
    if [[ "$nom" =~ ^wimao_([0-9]+\.[0-9]+\.[0-9]+)-1_amd64\.deb$ ]]; then
        [ -z "$nom_wimao" ] || refus "deux paquets wimao dans SHA256SUMS"
        nom_wimao="$nom" somme_wimao="$somme" version_wimao="${BASH_REMATCH[1]}"
    elif [[ "$nom" =~ ^wimao-bridge_[0-9]+(\.[0-9]+)+(\+[0-9A-Za-z.]+)?-1_amd64\.deb$ ]]; then
        [ -z "$nom_bridge" ] || refus "deux paquets wimao-bridge dans SHA256SUMS"
        nom_bridge="$nom" somme_bridge="$somme"
    elif [[ "$nom" =~ ^wimao-wine_[0-9]+(\.[0-9]+)+(\+[0-9A-Za-z.]+)?_amd64\.deb$ ]]; then
        [ -z "$nom_wine" ] || refus "deux paquets wimao-wine dans SHA256SUMS"
        nom_wine="$nom" somme_wine="$somme"
    else
        refus "paquet .deb inattendu ou version mal formée dans SHA256SUMS : $nom"
    fi
done < "$sommes"
[ -n "$nom_wimao" ] || refus "SHA256SUMS ne contient pas le .deb de wimao"
[ -n "$nom_bridge" ] || refus "SHA256SUMS ne contient pas le .deb de wimao-bridge"
[ -n "$nom_wine" ] || refus "SHA256SUMS ne contient pas le .deb de wimao-wine"
[ "$version_wimao" = "$version_cargo" ] \
    || refus "le .deb de wimao est en version $version_wimao, Cargo.toml en $version_cargo"

# --- Empreinte de la clé de signature (mode réel seulement) ---
if [ "$local_" -eq 0 ]; then
    [ -n "$cle" ] || cle="$(empreinte_publiee "$racine")"
    [[ "$cle" =~ ^[0-9A-F]{40}$ ]] || refus "empreinte de clé mal formée : $cle"
fi

# --- Blocs à insérer dans le modèle ---
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT
dossier_aur="$racine/paquets/aur"

# Dépendances : « depends » pour les lignes simples, « optdepends » pour celles qui commencent par « opt: »
# (entre guillemets doubles : les raisons contiennent des apostrophes).
awk '/^[[:space:]]*(#|$)/ { next } /^opt:/ { next } { l = l "\n  " $0 } END { printf "depends=(%s\n)\n", l }' \
    "$dossier_aur/dependances" > "$temporaire/depends"
awk '/^opt:/ { sub(/^opt:/, ""); l = l "\n  \"" $0 "\"" } END { printf "optdepends=(%s\n)\n", l }' \
    "$dossier_aur/dependances" > "$temporaire/optdepends"

adresse="https://github.com/F1ghtc0re/WiMao/releases/download/v\${pkgver}"
somme_sommes="$(sha256sum "$sommes" | awk '{ print $1 }')"
if [ "$local_" -eq 1 ]; then
    printf 'source=(\n  %s\n  %s\n  %s\n)\n' "$nom_wimao" "$nom_bridge" "$nom_wine" > "$temporaire/sources"
    printf 'sha256sums=(\n  %s\n  %s\n  %s\n)\n' "$somme_wimao" "$somme_bridge" "$somme_wine" > "$temporaire/sommes"
    : > "$temporaire/cles"
    : > "$temporaire/prepare"
    echo "# ESSAI LOCAL, NE PAS PUBLIER" > "$temporaire/marque"
else
    printf 'source=(\n  %s/%s\n  %s/%s\n  %s/%s\n  %s/SHA256SUMS\n  %s/SHA256SUMS.asc\n)\n' \
        "$adresse" "$nom_wimao" "$adresse" "$nom_bridge" "$adresse" "$nom_wine" "$adresse" "$adresse" > "$temporaire/sources"
    printf 'sha256sums=(\n  %s\n  %s\n  %s\n  %s\n  SKIP\n)\n' \
        "$somme_wimao" "$somme_bridge" "$somme_wine" "$somme_sommes" > "$temporaire/sommes"
    printf 'validpgpkeys=(%s)\n' "$cle" > "$temporaire/cles"
    cp "$dossier_aur/prepare.in" "$temporaire/prepare"
    : > "$temporaire/marque"
fi
noms="$(printf "'%s' '%s' '%s'" "$nom_wimao" "$nom_bridge" "$nom_wine")"

# --- Remplissage du modèle ---
# Une ligne du modèle réduite à @NOM@ est remplacée par le contenu du fichier voisin. Dans les autres lignes,
# @PKGVER@ devient la version et @NOMS_DEB@ les noms des .deb. Un fichier vide fait disparaître la ligne
# (marque et prepare du mode local, validpgpkeys).
fabrique="$temporaire/PKGBUILD"
awk -v dossier="$temporaire" -v pkgver="$version_cargo" -v noms="$noms" '
    function bloc(fichier,   ligne, contenu) {
        contenu = ""
        while ((getline ligne < fichier) > 0) contenu = contenu ligne "\n"
        close(fichier)
        printf "%s", contenu
    }
    $0 == "@MARQUE_LOCAL@"  { bloc(dossier "/marque"); next }
    $0 == "@DEPENDS@"       { bloc(dossier "/depends"); next }
    $0 == "@OPTDEPENDS@"    { bloc(dossier "/optdepends"); next }
    $0 == "@SOURCES@"       { bloc(dossier "/sources"); next }
    $0 == "@SOMMES@"        { bloc(dossier "/sommes"); next }
    $0 == "@VALIDPGPKEYS@"  { bloc(dossier "/cles"); next }
    $0 == "@PREPARE@"       { bloc(dossier "/prepare"); next }
    { gsub(/@PKGVER@/, pkgver); gsub(/@NOMS_DEB@/, noms); print }
' "$dossier_aur/PKGBUILD.in" > "$fabrique"

mkdir -p -- "$sortie_arg"
sortie="$(cd -- "$sortie_arg" && pwd)"
cp "$fabrique" "$sortie/PKGBUILD"
srcinfo "$sortie"
echo "Recette écrite dans $sortie (PKGBUILD et .SRCINFO)."
