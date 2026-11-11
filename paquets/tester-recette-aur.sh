#!/usr/bin/env bash
# Éprouve la VRAIE recette AUR de WiMao (paquets/aur/wimao-bin/) dans un Arch Linux vierge (conteneur) : elle
# télécharge les .deb de la Release GitHub, en vérifie les empreintes ET la signature, s'installe et marche ; puis
# elle prouve que trois sources trafiquées sont REFUSÉES. Lancé par .github/workflows/aur.yml sur les demandes de
# fusion qui touchent la recette ; lançable aussi en local (relance alors tout seul dans un conteneur Arch figé).
# Pendant « Release publiée » de paquets/tester-installation-aur.sh (qui, lui, essaie la recette en mode local
# avant l'étiquette) ; les fonctions communes sont dans paquets/aur/commun-aur.sh.
#
# ATTENTION : ce script installe des paquets, crée les utilisateurs « constructeur » et « essai » et écrit dans
# /opt, /usr et /home. Il s'exécute en root DANS UN CONTENEUR JETABLE (en CI : le conteneur du job ; en local :
# il lance lui-même podman si makepkg est absent, et ne touche alors à rien sur votre machine).
#
# Usage     : paquets/tester-recette-aur.sh <dossier de la recette> <dossier des journaux>
#               <dossier de la recette>  contient PKGBUILD et .SRCINFO (paquets/aur/wimao-bin/ en CI)
# Variables : DEXED_ZIP  chemin de Dexed-1.1.0-win.zip si on l'a déjà (sinon téléchargé ; empreinte vérifiée).
# Écrit     : <journaux>/{essai,makepkg,verifier,diagnostiquer,pipewire,refus-*}.log ; hors de là, ce que font pacman,
#             makepkg (/var/tmp) et l'utilisateur « essai » (/home/essai) DANS le conteneur.
# Code      : 0 si tout passe, ou si le dossier n'a pas de PKGBUILD (aucune recette publiée : note, rien à
#             éprouver) ; 1 au premier écart (message « ERREUR : … ») ; 2 mauvais usage.
#
# Étapes (chacune annoncée par « == … ») :
#   1. .SRCINFO : régénéré depuis le PKGBUILD, il doit être identique à celui du dossier ; pas de marque d'essai local.
#   2. Clé de signature : docs/cle-publique.asc importée dans le trousseau de « constructeur » (ce que doit faire
#      toute personne avant d'installer la recette).
#   3. makepkg RÉEL (téléchargement des sources sur la Release) + pacman -U.
#   4. Trois refus attendus, chacun dans une copie neuve de la recette et JUGÉ SUR LE MESSAGE de makepkg (un échec
#      pour une autre raison ne prouve rien) :
#        a. un .deb de wimao modifié d'un octet, posé à côté du PKGBUILD (makepkg le « trouve » et le réutilise
#           au lieu de le télécharger) : « did not pass the validity check » + .deb en « FAILED » ;
#        b. une empreinte sha256sums changée : même verdict, sur le .deb dont l'empreinte a été touchée ;
#        c. un SHA256SUMS.asc modifié d'un caractère : « PGP signatures could not be verified » + « bad signature » ; les
#           empreintes, elles, ont toutes passé (la signature est donc bien EXIGÉE).
#   5. Usage réel : version, Wine, init, Dexed, diagnostic, `verifier` (preuve de chargement).
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "Usage : $0 <dossier de la recette> <dossier des journaux>" >&2
    exit 2
fi
racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=paquets/aur/commun-aur.sh
source "$racine/paquets/aur/commun-aur.sh"

[ -d "$1" ] || { echo "ERREUR : dossier de la recette introuvable : $1" >&2; exit 2; }
RECETTE_SOURCE="$(cd "$1" && pwd)"
mkdir -p "$2"
JOURNAUX="$(cd "$2" && pwd)"

# Aucune recette publiée (premier paquet pas encore préparé) : rien à éprouver, ce n'est pas une erreur.
if [ ! -f "$RECETTE_SOURCE/PKGBUILD" ]; then
    echo "Note : aucune recette publiée dans $RECETTE_SOURCE (pas de PKGBUILD) : rien à éprouver."
    exit 0
fi
[ -f "$RECETTE_SOURCE/.SRCINFO" ] || { echo "ERREUR : $RECETTE_SOURCE/.SRCINFO absent" >&2; exit 1; }

# Hors d'un Arch (pas de makepkg) : on se relance DANS le conteneur figé. Dépôt, recette et journaux y sont
# montés (le dépôt et la recette en lecture seule). Le conteneur a besoin du réseau (Release GitHub, Arch).
if ! command -v makepkg > /dev/null; then
    command -v podman > /dev/null || { echo "ERREUR : ni makepkg ni podman : lancez ce script dans un Arch Linux" >&2; exit 1; }
    exec podman run --rm -v "$racine:/depot:ro,Z" -v "$RECETTE_SOURCE:/recette-source:ro,Z" \
        -v "$JOURNAUX:/journaux:Z" -e DEXED_ZIP="${DEXED_ZIP:-}" "$IMAGE_ARCH" \
        /depot/paquets/tester-recette-aur.sh /recette-source /journaux
fi
[ "$(id -u)" -eq 0 ] || echec "ce script doit s'exécuter en root dans un conteneur jetable"
# Un DEXED_ZIP vide (variable transmise par podman sans valeur) équivaut à « non fourni ».
[ -n "${DEXED_ZIP:-}" ] || unset DEXED_ZIP

# Tout ce que le script écrit va aussi dans le journal.
exec > >(tee "$JOURNAUX/essai.log") 2>&1

# --- Outils du script ------------------------------------------------------------------------------

# Dossier de travail : copie de la recette (le dossier d'origine reste intact, souvent en lecture seule).
TRAVAIL=/var/tmp/recette-aur
# Dossier où construire_installer_recette fait tourner makepkg (voir commun-aur.sh) : les sources téléchargées
# y restent, ce qui évite de les retélécharger (87 Mo) pour chacun des refus.
CONSTRUCTION=/var/tmp/construction-aur

# refus <nom> <dossier> <motif> [motif…] : fait tourner makepkg dans <dossier> (copie neuve de la recette,
# modifiée) et exige (1) un échec, (2) AUCUN paquet produit, (3) que la sortie de makepkg contienne chacun des
# motifs (expressions régulières étendues) : l'échec doit venir de la vérification, pas d'autre chose.
refus() {
    local nom="$1" dossier="$2" journal motif code=0
    shift 2
    journal="$JOURNAUX/refus-$nom.log"
    chown -R constructeur "$dossier"
    chmod 755 "$dossier"
    (cd "$dossier" && LANG=C.UTF-8 runuser -u constructeur -- makepkg --nodeps --nocolor) > "$journal" 2>&1 || code=$?
    cat "$journal"
    [ "$code" -ne 0 ] || echec "refus $nom : makepkg a ACCEPTÉ des sources trafiquées (code 0)"
    for motif in "$@"; do
        grep -Eq -- "$motif" "$journal" || echec "refus $nom : makepkg a échoué (code $code) mais pas à la vérification attendue : « $motif » absent de $journal"
    done
    [ -z "$(find "$dossier" -maxdepth 1 -name '*.pkg.tar.*' -print -quit)" ] || echec "refus $nom : un paquet a été produit malgré l'échec"
    echo "ok : refus $nom (makepkg code $code, motifs trouvés : $*)"
}

# copie_neuve <nom> : recrée /var/tmp/refus-<nom> avec la recette et les sources déjà téléchargées (.deb,
# SHA256SUMS, SHA256SUMS.asc), sans les dossiers src/ et pkg/ ni le paquet. Écrit le chemin sur la sortie.
copie_neuve() {
    local dossier="/var/tmp/refus-$1" f
    rm -rf "$dossier"
    mkdir -p "$dossier"
    cp -- "$RECETTE_SOURCE/PKGBUILD" "$RECETTE_SOURCE/.SRCINFO" "$dossier/"
    for f in "$CONSTRUCTION"/*.deb "$CONSTRUCTION/SHA256SUMS" "$CONSTRUCTION/SHA256SUMS.asc"; do
        [ -f "$f" ] || echec "source téléchargée introuvable : $f"
        cp -- "$f" "$dossier/"
    done
    echo "$dossier"
}

# --- 1. .SRCINFO -------------------------------------------------------------------------------------
etape "1. .SRCINFO à jour, pas de marque d'essai local"
! grep -q 'ESSAI LOCAL' "$RECETTE_SOURCE/PKGBUILD" || echec "le PKGBUILD porte la marque d'essai local : il ne doit pas être publié"
# Mise à jour d'Arch d'abord (Arch vivant : on éprouve la recette contre l'Arch d'aujourd'hui), puis outils.
pacman -Syu --noconfirm --needed git > "$JOURNAUX/pacman.log" 2>&1 || { cat "$JOURNAUX/pacman.log"; echec "pacman -Syu a échoué"; }
rm -rf "$TRAVAIL"
mkdir -p "$TRAVAIL"
cp -- "$RECETTE_SOURCE/PKGBUILD" "$TRAVAIL/"
srcinfo_ici "$TRAVAIL" || echec "makepkg --printsrcinfo a échoué"
cmp "$TRAVAIL/.SRCINFO" "$RECETTE_SOURCE/.SRCINFO" \
    || { diff -u "$RECETTE_SOURCE/.SRCINFO" "$TRAVAIL/.SRCINFO" || true; echec ".SRCINFO ne correspond pas au PKGBUILD : régénérez-le (paquets/fabriquer-pkgbuild.sh)"; }
echo "ok : .SRCINFO identique à celui que makepkg tire du PKGBUILD"
cp -- "$RECETTE_SOURCE/.SRCINFO" "$TRAVAIL/"

# --- 2. Clé de signature ---------------------------------------------------------------------------------
etape "2. Clé de signature de WiMao importée pour « constructeur »"
utilisateur_constructeur
importer_cle_constructeur "$racine/docs/cle-publique.asc"
runuser -u constructeur -- gpg --batch --list-keys --with-colons | grep -q '^fpr' || echec "clé non importée"
echo "ok : clé importée"

# --- 3. Construction réelle et installation ---------------------------------------------------------------
etape "3. makepkg RÉEL (téléchargement + empreintes + signature) et pacman -U"
construire_installer_recette "$TRAVAIL" "$JOURNAUX"
grep -qF 'Verifying source file signatures with gpg' "$JOURNAUX/makepkg.log" \
    || echec "makepkg n'a pas vérifié la signature des sources (voir $JOURNAUX/makepkg.log)"
echo "ok : makepkg a vérifié les empreintes et la signature"

# --- 4. Trois refus attendus -------------------------------------------------------------------------------
etape "4. Refus attendus de sources trafiquées"
# Nom (sans chemin) du .deb de wimao et son empreinte, lus dans la recette réelle.
deb_wimao="$(find "$CONSTRUCTION" -maxdepth 1 -name 'wimao_*_amd64.deb' -printf '%f\n')"
[ -n "$deb_wimao" ] || echec "le .deb de wimao n'a pas été téléchargé"

echo "-- a. .deb de wimao modifié d'un octet, posé à côté du PKGBUILD"
dossier="$(copie_neuve a)"
# On ajoute UN octet à la fin : le fichier présent est réutilisé tel quel par makepkg (pas de téléchargement).
printf 'x' >> "$dossier/$deb_wimao"
refus a "$dossier" 'did not pass the validity check' "${deb_wimao//./\\.} \.\.\. FAILED"
grep -qF -- "-> Found $deb_wimao" "$JOURNAUX/refus-a.log" \
    || echec "refus a : makepkg n'a pas signalé qu'il réutilisait le .deb posé à côté du PKGBUILD"
echo "ok : makepkg a réutilisé le .deb modifié au lieu de le télécharger"

echo "-- b. empreinte sha256sums changée dans le PKGBUILD"
dossier="$(copie_neuve b)"
# La première empreinte du tableau sha256sums (celle du .deb de wimao) : son premier caractère est remplacé.
premiere="$(awk '/^sha256sums=\(/ { dedans = 1; next } dedans && /^[[:space:]]*[0-9a-f]{64}[[:space:]]*$/ { gsub(/[[:space:]]/, ""); print; exit }' "$dossier/PKGBUILD")"
[ -n "$premiere" ] || echec "refus b : empreinte introuvable dans le PKGBUILD"
fausse="$([ "${premiere:0:1}" = 0 ] && echo 1 || echo 0)${premiere:1}"
sed -i "s/$premiere/$fausse/" "$dossier/PKGBUILD"
grep -q "$fausse" "$dossier/PKGBUILD" || echec "refus b : l'empreinte n'a pas été modifiée"
refus b "$dossier" 'did not pass the validity check' "${deb_wimao//./\\.} \.\.\. FAILED"

echo "-- c. SHA256SUMS.asc modifié d'un caractère"
dossier="$(copie_neuve c)"
# Un caractère du corps de la signature (en base64) est remplacé par un autre. La ligne de somme de contrôle de
# l'armure (« =XXXX ») est retirée : avec elle, gpg rejetterait l'armure avant même de lire la signature
# (« SIGNATURE NOT FOUND ») ; sans elle, il lit la signature altérée et la déclare fausse (« BAD signature »).
ligne="$(grep -n '^[A-Za-z0-9+/]\{20,\}$' "$dossier/SHA256SUMS.asc" | sed -n 2p | cut -d: -f1)"
[ -n "$ligne" ] || echec "refus c : corps de la signature introuvable"
texte="$(sed -n "${ligne}p" "$dossier/SHA256SUMS.asc")"
nouveau="$([ "${texte:10:1}" = A ] && echo B || echo A)"
# awk (et non sed) : la ligne contient des « / » possibles.
awk -v n="$ligne" -v nouvelle="${texte:0:10}${nouveau}${texte:11}" 'NR == n { print nouvelle; next } { print }' \
    "$dossier/SHA256SUMS.asc" > "$dossier/SHA256SUMS.asc.nouveau"
mv -- "$dossier/SHA256SUMS.asc.nouveau" "$dossier/SHA256SUMS.asc"
sed -i '/^=[A-Za-z0-9+\/]\{4\}$/d' "$dossier/SHA256SUMS.asc"
cmp -s "$dossier/SHA256SUMS.asc" "$CONSTRUCTION/SHA256SUMS.asc" && echec "refus c : la signature n'a pas été modifiée"
# Les empreintes passent (aucun « FAILED » de sha256) : seul le contrôle de la signature peut refuser.
refus c "$dossier" 'One or more PGP signatures could not be verified' 'SHA256SUMS \.\.\. FAILED \(bad signature'
empreintes="$(sed -n '/Validating source files with sha256sums/,/Verifying source file signatures/p' "$JOURNAUX/refus-c.log")"
[ -n "$empreintes" ] || echec "refus c : section « Validating source files with sha256sums » absente du journal"
! grep -Eq 'FAILED' <<< "$empreintes" || echec "refus c : une empreinte a échoué (le refus ne vient pas de la signature)"
echo "ok : les empreintes passent, la signature altérée est refusée"

# --- 5. Usage réel ----------------------------------------------------------------------------------------
etape "5. Usage réel"
outils_de_test_arch
essai_utilisation_wimao /var/tmp/wimao-fixtures "$JOURNAUX"

echo
echo "== Recette éprouvée : installation réelle, usage et trois refus constatés =="
