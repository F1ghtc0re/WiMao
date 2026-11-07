#!/usr/bin/env bash
# Teste scripts/publier-depot-apt.sh en mode --a-blanc, sans réseau : clé GnuPG jetable (avec sous-clé de
# signature), faux .deb fabriqués par `dpkg-deb --build` (nommés comme les vrais, en version 9.9.9 puis
# 9.9.10), et faux `gh` et `git` en tête du PATH qui se consignent et échouent : ils prouvent qu'aucun appel
# à gh ni à git n'a lieu à blanc.
#
# Cas couverts : premier passage (trois entrées pool/v9.9.9/ avec SHA256 et Size justes ; gpgv accepte
# InRelease et Release.gpg ; Release sans MD5Sum ni SHA1, avec les champs exacts et les empreintes SHA256 de
# Packages et Packages.gz, sans Valid-Until) ; même version relancée (Packages et Packages.gz identiques
# octet pour octet, aucun doublon) ; v9.9.10 ajoutée (six entrées triées par nom puis version) ;
# SHA256SUMS.asc d'une autre clé -> 1 ; .deb modifié -> 1 ; .deb en trop -> 1 ; un refus laisse --sortie
# intacte ; Packages identique sous une autre locale ; SHA256SUMS en mode binaire (*) accepté ; aucun appel à gh ni à git ; option inconnue -> 2 ; étiquette v1.2 -> 2 ; --a-blanc sans
# --dossier, --sortie ou --cle -> 2 ; --racine hors --a-blanc -> 2.
#
# Usage     : scripts/tests/tester-publier-depot-apt.sh
# Variables : aucune (GNUPGHOME et PATH sont réglés par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~/.gnupg.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
publier="$racine/scripts/publier-depot-apt.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

[ -f "$publier" ] || echec "scripts/publier-depot-apt.sh est introuvable"

# --- Trousseau GnuPG jetable : clé A (avec sous-clé de signature, celle qui signe) et clé B (autre) ---
export GNUPGHOME="$temporaire/gnupg"
mkdir -m 700 "$GNUPGHOME"
cat > "$temporaire/a.params" << 'PARAMS'
%no-protection
Key-Type: EDDSA
Key-Curve: ed25519
Key-Usage: sign
Subkey-Type: EDDSA
Subkey-Curve: ed25519
Subkey-Usage: sign
Name-Real: WiMao essai A
Name-Email: a@example.invalid
Expire-Date: 1d
%commit
PARAMS
sed -e 's/essai A/essai B/' -e 's/a@example/b@example/' -e '/^Subkey-/d' "$temporaire/a.params" > "$temporaire/b.params"
gpg --batch --quiet --gen-key "$temporaire/a.params" 2> /dev/null
gpg --batch --quiet --gen-key "$temporaire/b.params" 2> /dev/null
empreinte_a="$(gpg --with-colons --list-keys a@example.invalid 2> /dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"

# --- Faux dépôt : sa clé publique (celle de A) donne le trousseau de contrôle de gpgv ---
fausse_racine="$temporaire/racine"
mkdir -p "$fausse_racine/docs"
gpg --armor --export a@example.invalid > "$fausse_racine/docs/cle-publique.asc"

# --- Faux gh et faux git : consignent l'appel et échouent bruyamment ---
mkdir "$temporaire/bin"
journal="$temporaire/appels.log"
for outil in gh git; do
    cat > "$temporaire/bin/$outil" << FAUX
#!/usr/bin/env bash
echo "$outil \$*" >> "$journal"
echo "FAUX $outil APPELÉ : $outil \$*" >&2
exit 99
FAUX
    chmod +x "$temporaire/bin/$outil"
done
export PATH="$temporaire/bin:$PATH"

# fabriquer_deb <nom> <version> <dossier> : fabrique un vrai petit .deb (amd64) nommé <nom>_<version>_amd64.deb.
fabriquer_deb() {
    local nom="$1" version="$2" dest="$3" arbre="$temporaire/arbre-$1-$2"
    mkdir -p "$arbre/DEBIAN" "$arbre/usr/share/doc/$nom"
    printf 'Package: %s\nVersion: %s\nArchitecture: amd64\nMaintainer: Essai <essai@example.invalid>\nDescription: faux paquet d essai\n' \
        "$nom" "$version" > "$arbre/DEBIAN/control"
    echo "contenu de $nom $version" > "$arbre/usr/share/doc/$nom/note"
    dpkg-deb --root-owner-group --build "$arbre" "$dest/${nom}_${version}_amd64.deb" > /dev/null
}

# artefacts <vX.Y.Z-sans-v> <clé de signature> : dossier de faux artefacts (3 .deb, SHA256SUMS signé) ; rend son chemin.
artefacts() {
    local v="$1" cle="$2" d="$temporaire/artefacts-$1"
    rm -rf "$d"
    mkdir -p "$d"
    for nom in wimao wimao-bridge wimao-wine; do
        fabriquer_deb "$nom" "$v-1" "$d"
    done
    (cd "$d" && LC_ALL=C sha256sum -- *.deb > SHA256SUMS)
    gpg --batch --armor --detach-sign --local-user "$cle" --output "$d/SHA256SUMS.asc" "$d/SHA256SUMS" 2> /dev/null
    printf '%s' "$d"
}

# lancer <arguments> : exécute le script ; sortie dans $temporaire/sortie.log, code dans $code.
lancer() {
    code=0
    "$publier" "$@" > "$temporaire/sortie.log" 2>&1 || code=$?
}

# attendre_code <attendu> <description> : échec (avec la sortie du script) si $code diffère.
attendre_code() {
    [ "$code" -eq "$1" ] || { cat "$temporaire/sortie.log" >&2; echec "$2 : code $code au lieu de $1"; }
}

depot="$temporaire/depot-apt"
mkdir "$depot"
paquets_idx="$depot/dists/stable/main/binary-amd64/Packages"
a_blanc=(--a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$depot")

art_1="$(artefacts 9.9.9 a@example.invalid)"

# --- Premier passage ---
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_1"
attendre_code 0 "premier passage"
[ -f "$paquets_idx" ] || echec "Packages absent"
[ -f "$depot/dists/stable/main/binary-amd64/Packages.gz" ] || echec "Packages.gz absent"
[ "$(grep -c '^Package: ' "$paquets_idx")" -eq 3 ] || echec "Packages ne contient pas trois entrées"
[ "$(grep -c '^Filename: pool/v9.9.9/' "$paquets_idx")" -eq 3 ] || echec "les trois Filename ne sont pas en pool/v9.9.9/"
for deb in "$art_1"/*.deb; do
    nom="$(basename "$deb")"
    grep -qx "Filename: pool/v9.9.9/$nom" "$paquets_idx" || echec "Filename manquant pour $nom"
    grep -qx "SHA256: $(sha256sum "$deb" | cut -d' ' -f1)" "$paquets_idx" || echec "SHA256 faux pour $nom"
    grep -qx "Size: $(stat -c %s "$deb")" "$paquets_idx" || echec "Size faux pour $nom"
done
! grep -q '^MD5sum:\|^SHA1:\|^SHA512:' "$paquets_idx" || echec "Packages contient MD5sum, SHA1 ou SHA512"

release="$depot/dists/stable/Release"
for ligne in 'Origin: WiMao' 'Label: WiMao' 'Suite: stable' 'Codename: stable' 'Architectures: amd64' \
    'Components: main' 'Description: WiMao — Wine pour la MAO (paquets signés)'; do
    grep -qxF "$ligne" "$release" || echec "Release ne contient pas « $ligne »"
done
grep -q '^Date: ' "$release" || echec "Release sans Date"
! grep -q 'MD5Sum\|^SHA1\|^SHA512\|Valid-Until' "$release" || echec "Release contient MD5Sum, SHA1, SHA512 ou Valid-Until"
grep -q '^SHA256:' "$release" || echec "Release sans section SHA256"
for fichier in main/binary-amd64/Packages main/binary-amd64/Packages.gz; do
    empreinte_fichier="$(sha256sum "$depot/dists/stable/$fichier" | cut -d' ' -f1)"
    taille_fichier="$(stat -c %s "$depot/dists/stable/$fichier")"
    grep -qE "^ $empreinte_fichier +$taille_fichier $fichier$" "$release" || echec "Release : empreinte ou taille fausse pour $fichier"
done

# La section SHA256 ne doit lister que Packages et Packages.gz : un fichier Release qui se décrit lui-même
# (ligne « Release » ou « InRelease ») aurait une empreinte fausse.
autres="$(awk '/^[^ ]/ { section = ($0 == "SHA256:") } section && /^ / && $3 != "main/binary-amd64/Packages" && $3 != "main/binary-amd64/Packages.gz"' "$release")"
[ -z "$autres" ] || echec "Release liste d'autres fichiers que Packages et Packages.gz : $autres"

# gpgv (trousseau construit depuis la clé publique de A, jamais le trousseau personnel) accepte les deux signatures.
gpg --export a@example.invalid > "$temporaire/trousseau.gpg"
gpgv --keyring "$temporaire/trousseau.gpg" "$depot/dists/stable/InRelease" > /dev/null 2>&1 || echec "gpgv refuse InRelease"
gpgv --keyring "$temporaire/trousseau.gpg" "$depot/dists/stable/Release.gpg" "$release" > /dev/null 2>&1 || echec "gpgv refuse Release.gpg"

# --- Même version relancée : Packages identiques octet pour octet, aucun doublon ---
cp "$paquets_idx" "$temporaire/Packages.avant"
cp "$depot/dists/stable/main/binary-amd64/Packages.gz" "$temporaire/Packages.gz.avant"
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_1"
attendre_code 0 "même version relancée"
cmp -s "$paquets_idx" "$temporaire/Packages.avant" || echec "Packages a changé en relançant la même version"
cmp -s "$depot/dists/stable/main/binary-amd64/Packages.gz" "$temporaire/Packages.gz.avant" || echec "Packages.gz a changé en relançant la même version"
gpgv --keyring "$temporaire/trousseau.gpg" "$depot/dists/stable/InRelease" > /dev/null 2>&1 || echec "gpgv refuse InRelease après relance"

# --- Langue : une génération sous une autre locale donne le même Packages (ordre indépendant de la langue) ---
locale_autre=C.UTF-8
locales="$(locale -a 2> /dev/null || true)"
if grep -qix 'fr_FR.utf-\?8' <<< "$locales"; then locale_autre=fr_FR.UTF-8; fi
depot_loc="$temporaire/depot-locale"
mkdir "$depot_loc"
art_loc="$(artefacts 9.9.9 a@example.invalid)"
# Les .deb sont refabriqués, donc leurs sommes diffèrent : on compare à une génération sous C avec LES MÊMES .deb.
"$publier" v9.9.9 --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$depot_loc/c" --dossier "$art_loc" > /dev/null 2>&1 || echec "génération sous C"
LC_ALL="$locale_autre" LANG="$locale_autre" "$publier" v9.9.9 --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$depot_loc/l" --dossier "$art_loc" > /dev/null 2>&1 || echec "génération sous $locale_autre"
cmp -s "$depot_loc/c/dists/stable/main/binary-amd64/Packages" "$depot_loc/l/dists/stable/main/binary-amd64/Packages" \
    || echec "Packages diffère selon la locale ($locale_autre)"

# --- Seconde version v9.9.10 : six entrées, triées par nom puis version ---
art_2="$(artefacts 9.9.10 a@example.invalid)"
lancer v9.9.10 "${a_blanc[@]}" --dossier "$art_2"
attendre_code 0 "seconde version"
[ "$(grep -c '^Package: ' "$paquets_idx")" -eq 6 ] || echec "Packages ne contient pas six entrées"
attendu="wimao 9.9.9-1
wimao 9.9.10-1
wimao-bridge 9.9.9-1
wimao-bridge 9.9.10-1
wimao-wine 9.9.9-1
wimao-wine 9.9.10-1"
obtenu="$(awk '/^Package: / { p = $2 } /^Version: / { print p, $2 }' "$paquets_idx")"
[ "$obtenu" = "$attendu" ] || echec "ordre des entrées inattendu :
$obtenu"
gpgv --keyring "$temporaire/trousseau.gpg" "$depot/dists/stable/InRelease" > /dev/null 2>&1 || echec "gpgv refuse InRelease (6 entrées)"
gpgv --keyring "$temporaire/trousseau.gpg" "$depot/dists/stable/Release.gpg" "$release" > /dev/null 2>&1 || echec "gpgv refuse Release.gpg (6 entrées)"

# --- Refus : la sortie doit rester intacte ---
empreinte_depot() { (cd "$depot" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum); }
avant="$(empreinte_depot)"

# SHA256SUMS.asc signé par une autre clé (B, absente du trousseau de contrôle)
art_b="$(artefacts 9.9.11 b@example.invalid)"
lancer v9.9.11 "${a_blanc[@]}" --dossier "$art_b"
attendre_code 1 "SHA256SUMS.asc d'une autre clé"

# .deb modifié après la signature
art_m="$(artefacts 9.9.12 a@example.invalid)"
echo "octets en trop" >> "$art_m/wimao_9.9.12-1_amd64.deb"
lancer v9.9.12 "${a_blanc[@]}" --dossier "$art_m"
attendre_code 1 ".deb modifié"

# .deb en trop (non listé dans SHA256SUMS)
art_t="$(artefacts 9.9.13 a@example.invalid)"
fabriquer_deb intrus 9.9.13-1 "$art_t"
lancer v9.9.13 "${a_blanc[@]}" --dossier "$art_t"
attendre_code 1 ".deb en trop"

# SHA256SUMS modifié après la signature
art_s="$(artefacts 9.9.14 a@example.invalid)"
echo "$(printf '0%.0s' {1..64})  faux.deb" >> "$art_s/SHA256SUMS"
lancer v9.9.14 "${a_blanc[@]}" --dossier "$art_s"
attendre_code 1 "SHA256SUMS modifié"

[ "$(empreinte_depot)" = "$avant" ] || echec "un refus a modifié le dépôt de sortie"

# SHA256SUMS écrit en mode binaire (« *nom ») : accepté
art_e="$(artefacts 9.9.15 a@example.invalid)"
sed -i 's/  / */' "$art_e/SHA256SUMS"
gpg --batch --yes --armor --detach-sign --local-user a@example.invalid --output "$art_e/SHA256SUMS.asc" "$art_e/SHA256SUMS" 2> /dev/null
lancer v9.9.15 --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$temporaire/depot-etoile" --dossier "$art_e"
attendre_code 0 "SHA256SUMS en mode binaire"

# --- Usage ---
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_1" --inconnue
attendre_code 2 "option inconnue"
lancer v1.2 "${a_blanc[@]}" --dossier "$art_1"
attendre_code 2 "étiquette v1.2"
lancer v9.9.9 --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$depot"
attendre_code 2 "--a-blanc sans --dossier"
lancer v9.9.9 --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --dossier "$art_1"
attendre_code 2 "--a-blanc sans --sortie"
lancer v9.9.9 --a-blanc --racine "$fausse_racine" --sortie "$depot" --dossier "$art_1"
attendre_code 2 "--a-blanc sans --cle"
lancer v9.9.9 --racine "$fausse_racine"
attendre_code 2 "--racine sans --a-blanc"
lancer
attendre_code 2 "version manquante"

# --- Aucun appel à gh ni à git ---
[ ! -s "$journal" ] || echec "gh ou git a été appelé à blanc : $(cat "$journal")"

echo "OK : tester-publier-depot-apt.sh"
