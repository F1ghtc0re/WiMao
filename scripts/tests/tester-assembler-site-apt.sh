#!/usr/bin/env bash
# Teste scripts/assembler-site-apt.sh sans réseau. Un dépôt APT d'essai est produit par le VRAI
# scripts/publier-depot-apt.sh en mode --a-blanc (clé GnuPG jetable avec sous-clé de signature, faux .deb
# fabriqués par `dpkg-deb --build`, version 9.9.9) ; les « téléchargements » sont des copies depuis un dossier
# local (WIMAO_SOURCE_RELEASES). Un faux `curl` en tête du PATH se consigne et échoue : il prouve qu'un refus
# de Filename a lieu AVANT tout téléchargement. (Mise en place de clé et de faux .deb dupliquée de
# tester-publier-depot-apt.sh, volontairement minimale : l'extraire dans un fichier commun compliquerait
# les deux tests pour peu de gain.)
#
# Cas couverts : site complet (dists/, pool/v9.9.9/*.deb identiques, wimao.asc = clé publique, index.html avec
# l'adresse du dépôt, les commandes et l'empreinte en groupes de 4) ; InRelease altéré -> 1 ; Release.gpg
# altéré -> 1 ; index signé par une autre clé -> 1 ; Packages altéré (empreinte de Release fausse) -> 1 ;
# .deb téléchargé de taille ou de SHA256 différents -> 1 ; .deb absent de la source -> 1 ; Filename
# « pool/../x.deb », « /etc/x.deb », « pool/v1.2/x.deb » (index signé par la bonne clé) -> 1 sans aucun
# appel à curl ; paragraphe en double, sans Size, Packages.gz différent, deux lignes SHA256 dans Release
# (index re-signé) -> 1 ; fichier en trop, autre architecture, lien symbolique dans depot-apt/ -> 1 ;
# WIMAO_SOURCE_RELEASES sans --racine -> 2 ; --cle invalide avec dépôt vide -> 2 ; dossier de site non vide -> 1 ; un refus n'écrit rien dans le site ; depot-apt/ absent -> 3
# (rien écrit) ; option inconnue, site manquant, --cle sans --racine -> 2.
#
# Usage     : scripts/tests/tester-assembler-site-apt.sh
# Variables : aucune (GNUPGHOME, PATH et WIMAO_SOURCE_RELEASES sont réglés par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~/.gnupg.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
assembler="$racine/scripts/assembler-site-apt.sh"
publier="$racine/scripts/publier-depot-apt.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

[ -f "$assembler" ] || echec "scripts/assembler-site-apt.sh est introuvable"

# --- Trousseau jetable : clé A (avec sous-clé de signature, celle qui signe) et clé B (autre) ---
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

# --- Faux curl : consigne l'appel et échoue ---
mkdir "$temporaire/bin"
journal="$temporaire/curl.log"
cat > "$temporaire/bin/curl" << FAUX
#!/usr/bin/env bash
echo "curl \$*" >> "$journal"
echo "FAUX curl APPELÉ : curl \$*" >&2
exit 99
FAUX
chmod +x "$temporaire/bin/curl"
export PATH="$temporaire/bin:$PATH"

# --- Faux .deb et faux dépôt APT produit par publier-depot-apt.sh --a-blanc ---
art="$temporaire/artefacts"
mkdir "$art"
for nom in wimao wimao-bridge wimao-wine; do
    arbre="$temporaire/arbre-$nom"
    mkdir -p "$arbre/DEBIAN" "$arbre/usr/share/doc/$nom"
    printf 'Package: %s\nVersion: 9.9.9-1\nArchitecture: amd64\nMaintainer: Essai <essai@example.invalid>\nDescription: faux paquet d essai\n' \
        "$nom" > "$arbre/DEBIAN/control"
    echo "contenu de $nom" > "$arbre/usr/share/doc/$nom/note"
    dpkg-deb --root-owner-group --build "$arbre" "$art/${nom}_9.9.9-1_amd64.deb" > /dev/null
done
(cd "$art" && sha256sum -- *.deb > SHA256SUMS)
gpg --batch --armor --detach-sign --local-user a@example.invalid --output "$art/SHA256SUMS.asc" "$art/SHA256SUMS" 2> /dev/null

# Faux dépôt de référence : docs/cle-publique.asc (clé A) et depot-apt/ signé.
base="$temporaire/base"
mkdir -p "$base/docs"
gpg --armor --export a@example.invalid > "$base/docs/cle-publique.asc"
"$publier" v9.9.9 --a-blanc --racine "$base" --cle "$empreinte_a" --sortie "$base/depot-apt" --dossier "$art" > "$temporaire/publier.log" 2>&1 \
    || { cat "$temporaire/publier.log" >&2; echec "publier-depot-apt.sh n'a pas produit le dépôt d'essai"; }

# Faux « Releases GitHub » : <étiquette>/<nom>.
source_ok="$temporaire/releases"
mkdir -p "$source_ok/v9.9.9"
cp "$art"/*.deb "$source_ok/v9.9.9/"

# nouvelle_racine <nom> : copie du faux dépôt de référence, modifiable sans toucher à la référence ; rend son chemin.
nouvelle_racine() {
    rm -rf "${temporaire:?}/$1"
    cp -r "$base" "$temporaire/$1"
    printf '%s' "$temporaire/$1"
}

# lancer <racine> <site> [<source des Releases>] : exécute le script ; sortie dans sortie.log, code dans $code.
lancer() {
    code=0
    if [ -n "${3:-}" ]; then export WIMAO_SOURCE_RELEASES="$3"; else unset WIMAO_SOURCE_RELEASES; fi
    "$assembler" "$2" --cle "$empreinte_a" --racine "$1" > "$temporaire/sortie.log" 2>&1 || code=$?
}

# attendre_code <attendu> <description> : échec (avec la sortie du script) si $code diffère.
attendre_code() {
    [ "$code" -eq "$1" ] || { cat "$temporaire/sortie.log" >&2; echec "$2 : code $code au lieu de $1"; }
}

# attendre_vide <dossier> <description> : le dossier n'existe pas ou est vide (un refus n'écrit rien).
attendre_vide() {
    [ ! -e "$1" ] || [ -z "$(ls -A "$1")" ] || echec "$2 : le site a été écrit malgré le refus"
}

# attendre_message <texte> <description> : la sortie du script contient ce texte (refus pour la bonne raison).
attendre_message() {
    grep -qF -- "$1" "$temporaire/sortie.log" || { cat "$temporaire/sortie.log" >&2; echec "$2 : message « $1 » absent"; }
}

# resigner <racine> [gz-deja-fait] : après modification de Packages* ou de Release, recalcule ce qu'il faut pour que l'index
# reste signé PAR LA BONNE CLÉ : empreintes de Release mises à jour, puis InRelease et Release.gpg refaits.
# Sert à prouver qu'un refus vient du CONTENU (Filename) et non d'une signature cassée.
resigner() {
    local d="$1/depot-apt/dists/stable" fichier
    [ "${2:-}" = "gz-deja-fait" ] || gzip -9n -c "$d/main/binary-amd64/Packages" > "$d/main/binary-amd64/Packages.gz"
    for fichier in Packages Packages.gz; do
        sed -i -E "s#^ [0-9a-f]{64} +[0-9]+ main/binary-amd64/$fichier\$# $(sha256sum "$d/main/binary-amd64/$fichier" | cut -d' ' -f1) $(stat -c %s "$d/main/binary-amd64/$fichier") main/binary-amd64/$fichier#" "$d/Release"
    done
    rm -f "$d/InRelease" "$d/Release.gpg"
    gpg --batch --yes --clearsign --local-user a@example.invalid --output "$d/InRelease" "$d/Release" 2> /dev/null
    gpg --batch --yes --armor --detach-sign --local-user a@example.invalid --output "$d/Release.gpg" "$d/Release" 2> /dev/null
}

# --- Cas nominal : site complet ---
site="$temporaire/site-ok"
lancer "$base" "$site" "$source_ok"
attendre_code 0 "site complet"
[ -f "$site/dists/stable/InRelease" ] && [ -f "$site/dists/stable/Release.gpg" ] && [ -f "$site/dists/stable/Release" ] \
    || echec "dists/stable incomplet"
cmp -s "$base/depot-apt/dists/stable/main/binary-amd64/Packages" "$site/dists/stable/main/binary-amd64/Packages" || echec "Packages différent"
cmp -s "$base/depot-apt/dists/stable/main/binary-amd64/Packages.gz" "$site/dists/stable/main/binary-amd64/Packages.gz" || echec "Packages.gz différent"
for deb in "$art"/*.deb; do
    cmp -s "$deb" "$site/pool/v9.9.9/$(basename "$deb")" || echec "pool/v9.9.9/$(basename "$deb") absent ou différent"
done
cmp -s "$base/docs/cle-publique.asc" "$site/wimao.asc" || echec "wimao.asc diffère de docs/cle-publique.asc"
page="$site/index.html"
grep -q '<title>Dépôt APT de WiMao</title>' "$page" || echec "index.html : titre"
grep -qF 'https://f1ghtc0re.github.io/WiMao' "$page" || echec "index.html : adresse du dépôt"
grep -qF 'curl -fsSL https://f1ghtc0re.github.io/WiMao/wimao.asc | sudo tee /etc/apt/keyrings/wimao.asc &gt; /dev/null' "$page" || echec "index.html : commande curl"
grep -qF 'Si <code>curl</code> n'"'"'est pas installé : <code>sudo apt install curl</code>' "$page" || echec "index.html : note curl"
grep -qF 'sudo install -d -m 0755 /etc/apt/keyrings' "$page" || echec "index.html : commande install"
grep -qF 'sudo tee /etc/apt/sources.list.d/wimao.list' "$page" || echec "index.html : commande tee de la source"
grep -qF 'sudo apt update &amp;&amp; sudo apt install wimao' "$page" || echec "index.html : commande apt"
grep -qF 'stable main"' "$page" || echec "index.html : ligne deb"
groupes="$(sed 's/.\{4\}/& /g; s/ $//' <<< "$empreinte_a")"
grep -qF "$groupes" "$page" || echec "index.html : empreinte en groupes de 4"
grep -q 'prefers-color-scheme: dark' "$page" || echec "index.html : thème sombre"
! grep -qE '(src|href)="https?://[^"]*"' <(grep -v 'github.com/F1ghtc0re/WiMao' "$page") || echec "index.html : ressource externe"
[ ! -s "$journal" ] || echec "curl appelé en mode d'essai"

# --- Site non vide : refusé, contenu intact ---
mkdir "$temporaire/site-plein"
echo x > "$temporaire/site-plein/existant"
lancer "$base" "$temporaire/site-plein" "$source_ok"
attendre_code 1 "site non vide"
[ "$(ls -A "$temporaire/site-plein")" = "existant" ] || echec "site non vide modifié"

# --- InRelease altéré (un caractère du texte signé) ---
r="$(nouvelle_racine r-inrelease)"
sed -i 's/^Origin: WiMao$/Origin: WiMaX/' "$r/depot-apt/dists/stable/InRelease"
lancer "$r" "$temporaire/s-inrelease" "$source_ok"
attendre_code 1 "InRelease altéré"
attendre_vide "$temporaire/s-inrelease" "InRelease altéré"

# --- Release.gpg altéré (signature d'un autre texte : celle de B sur Release) ---
r="$(nouvelle_racine r-gpg)"
rm "$r/depot-apt/dists/stable/Release.gpg"
gpg --batch --armor --detach-sign --local-user b@example.invalid --output "$r/depot-apt/dists/stable/Release.gpg" "$r/depot-apt/dists/stable/Release" 2> /dev/null
lancer "$r" "$temporaire/s-gpg" "$source_ok"
attendre_code 1 "Release.gpg d'une autre clé"
attendre_vide "$temporaire/s-gpg" "Release.gpg d'une autre clé"

# --- Index entièrement signé par une autre clé (B) ---
r="$(nouvelle_racine r-autre-cle)"
d="$r/depot-apt/dists/stable"
rm "$d/InRelease" "$d/Release.gpg"
gpg --batch --clearsign --local-user b@example.invalid --output "$d/InRelease" "$d/Release" 2> /dev/null
gpg --batch --armor --detach-sign --local-user b@example.invalid --output "$d/Release.gpg" "$d/Release" 2> /dev/null
lancer "$r" "$temporaire/s-autre-cle" "$source_ok"
attendre_code 1 "index signé par une autre clé"
attendre_vide "$temporaire/s-autre-cle" "index signé par une autre clé"

# --- InRelease valide mais différent de Release (Release et Release.gpg intacts) ---
r="$(nouvelle_racine r-decale)"
d="$r/depot-apt/dists/stable"
sed 's/^Label: WiMao$/Label: Autre/' "$d/Release" > "$temporaire/Release.autre"
rm "$d/InRelease"
gpg --batch --clearsign --local-user a@example.invalid --output "$d/InRelease" "$temporaire/Release.autre" 2> /dev/null
lancer "$r" "$temporaire/s-decale" "$source_ok"
attendre_code 1 "InRelease signé différent de Release"
attendre_vide "$temporaire/s-decale" "InRelease différent de Release"

# --- Packages altéré : l'empreinte de Release ne correspond plus ---
r="$(nouvelle_racine r-packages)"
echo "# altéré" >> "$r/depot-apt/dists/stable/main/binary-amd64/Packages"
lancer "$r" "$temporaire/s-packages" "$source_ok"
attendre_code 1 "Packages altéré"
attendre_vide "$temporaire/s-packages" "Packages altéré"

# --- .deb téléchargé : taille différente, SHA256 différent (même taille), absent ---
premier=""
for deb in "$art"/*.deb; do premier="$(basename "$deb")"; break; done
taille_premier="$(stat -c %s "$art/$premier")"
cp -r "$source_ok" "$temporaire/releases-taille"
echo "en trop" >> "$temporaire/releases-taille/v9.9.9/$premier"
lancer "$base" "$temporaire/s-taille" "$temporaire/releases-taille"
attendre_code 1 ".deb de taille différente"
attendre_vide "$temporaire/s-taille" ".deb de taille différente"

cp -r "$source_ok" "$temporaire/releases-somme"
head -c "$taille_premier" /dev/zero > "$temporaire/releases-somme/v9.9.9/$premier"
lancer "$base" "$temporaire/s-somme" "$temporaire/releases-somme"
attendre_code 1 ".deb de SHA256 différent"
attendre_vide "$temporaire/s-somme" ".deb de SHA256 différent"

cp -r "$source_ok" "$temporaire/releases-absent"
rm "$temporaire/releases-absent/v9.9.9/$premier"
lancer "$base" "$temporaire/s-absent" "$temporaire/releases-absent"
attendre_code 1 ".deb absent de la source"
attendre_vide "$temporaire/s-absent" ".deb absent de la source"

# --- Filename hors de pool/vX.Y.Z/ : index signé par la bonne clé, refus AVANT tout téléchargement ---
for mauvais in 'pool/../x.deb' '/etc/x.deb' 'pool/v1.2/x.deb' 'pool/v1.2.3/../x.deb' 'pool/v1.2.3/a/b.deb' 'pool/v1.2.3/x.rpm'; do
    r="$(nouvelle_racine r-filename)"
    : > "$journal"
    # On remplace le Filename de la première entrée seulement (sed 0,/…/ : première occurrence).
    sed -i "0,/^Filename: .*/s##Filename: $mauvais#" "$r/depot-apt/dists/stable/main/binary-amd64/Packages"
    resigner "$r"
    # Contrôle préalable : l'index ainsi modifié est bien signé par A (le refus ne vient pas de la signature).
    gpg --export a@example.invalid > "$temporaire/trousseau.gpg"
    gpgv --keyring "$temporaire/trousseau.gpg" "$r/depot-apt/dists/stable/InRelease" > /dev/null 2>&1 || echec "essai mal monté : InRelease non signé ($mauvais)"
    # Sans WIMAO_SOURCE_RELEASES : un téléchargement passerait par le faux curl, qui le consigne.
    lancer "$r" "$temporaire/s-filename" ""
    attendre_code 1 "Filename $mauvais"
    grep -q 'Filename refusé' "$temporaire/sortie.log" || { cat "$temporaire/sortie.log" >&2; echec "Filename $mauvais : refus pour une autre raison"; }
    [ ! -s "$journal" ] || echec "Filename $mauvais : curl a été appelé"
    attendre_vide "$temporaire/s-filename" "Filename $mauvais"
done

# --- Paragraphe de Packages en double (même Filename), index re-signé ---
r="$(nouvelle_racine r-double)"
idx="$r/depot-apt/dists/stable/main/binary-amd64/Packages"
{ awk '/^$/ { exit } { print }' "$idx"; echo; cat "$idx"; } > "$temporaire/Packages.double"
cp "$temporaire/Packages.double" "$idx"
resigner "$r"
lancer "$r" "$temporaire/s-double" "$source_ok"
attendre_code 1 "paragraphe en double"
attendre_message "Filename en double" "paragraphe en double"
attendre_vide "$temporaire/s-double" "paragraphe en double"

# --- Paragraphe sans Size ---
r="$(nouvelle_racine r-sans-size)"
sed -i '0,/^Size: /{/^Size: /d}' "$r/depot-apt/dists/stable/main/binary-amd64/Packages"
resigner "$r"
lancer "$r" "$temporaire/s-sans-size" "$source_ok"
attendre_code 1 "paragraphe sans Size"
attendre_message "manquant ou en double" "paragraphe sans Size"
attendre_vide "$temporaire/s-sans-size" "paragraphe sans Size"

# --- Packages.gz différent de Packages (Release recalculée et re-signée) ---
r="$(nouvelle_racine r-gz)"
echo "Package: intrus" | gzip -9n > "$r/depot-apt/dists/stable/main/binary-amd64/Packages.gz"
resigner "$r" gz-deja-fait
lancer "$r" "$temporaire/s-gz" "$source_ok"
attendre_code 1 "Packages.gz différent"
attendre_message "Packages.gz ne contient pas exactement Packages" "Packages.gz différent"
attendre_vide "$temporaire/s-gz" "Packages.gz différent"

# --- Deux lignes SHA256 pour Packages dans Release, re-signé ---
r="$(nouvelle_racine r-release-double)"
sed -i -E '/ main\/binary-amd64\/Packages$/p' "$r/depot-apt/dists/stable/Release"
resigner "$r"
lancer "$r" "$temporaire/s-release-double" "$source_ok"
attendre_code 1 "deux lignes SHA256 pour Packages"
attendre_message "exactement une empreinte SHA256" "deux lignes SHA256"
attendre_vide "$temporaire/s-release-double" "deux lignes SHA256"

# --- Éléments inattendus dans depot-apt/ : fichier en trop, autre architecture, lien symbolique ---
r="$(nouvelle_racine r-en-trop)"
echo x > "$r/depot-apt/dists/stable/en-trop"
lancer "$r" "$temporaire/s-en-trop" "$source_ok"
attendre_code 1 "fichier en trop"
attendre_message "contenu attendu" "fichier en trop"
attendre_vide "$temporaire/s-en-trop" "fichier en trop"
r="$(nouvelle_racine r-arch)"
mkdir "$r/depot-apt/dists/stable/main/binary-arm64"
lancer "$r" "$temporaire/s-arch" "$source_ok"
attendre_code 1 "autre architecture"
attendre_vide "$temporaire/s-arch" "autre architecture"
r="$(nouvelle_racine r-lien)"
ln -s Release "$r/depot-apt/dists/stable/lien"
lancer "$r" "$temporaire/s-lien" "$source_ok"
attendre_code 1 "lien symbolique"
attendre_vide "$temporaire/s-lien" "lien symbolique"

# --- depot-apt/ absent : code 3, rien écrit ---
r="$(nouvelle_racine r-vide)"
rm -rf "$r/depot-apt"
lancer "$r" "$temporaire/s-vide" "$source_ok"
attendre_code 3 "depot-apt/ absent"
grep -q 'dépôt APT vide : rien à assembler' "$temporaire/sortie.log" || echec "message « dépôt APT vide » absent"
[ ! -e "$temporaire/s-vide" ] || echec "depot-apt/ absent : le site a été créé"
# Dossier depot-apt/ présent mais sans InRelease : même chose.
mkdir -p "$r/depot-apt/dists/stable"
lancer "$r" "$temporaire/s-vide" "$source_ok"
attendre_code 3 "depot-apt/ sans InRelease"

# --- Usage ---
# --cle invalide : code 2 même si le dépôt est vide (arguments validés avant le test « dépôt vide »).
code=0
"$assembler" "$temporaire/s-usage" --cle zzz --racine "$r" > /dev/null 2>&1 || code=$?
attendre_code 2 "--cle invalide avec dépôt vide"
# WIMAO_SOURCE_RELEASES hors mode essai (sans --racine) : refusée, jamais ignorée.
code=0
WIMAO_SOURCE_RELEASES="$source_ok" "$assembler" "$temporaire/s-usage" > "$temporaire/sortie.log" 2>&1 || code=$?
attendre_code 2 "WIMAO_SOURCE_RELEASES hors essai"
attendre_message "WIMAO_SOURCE_RELEASES" "WIMAO_SOURCE_RELEASES hors essai"
[ ! -e "$temporaire/s-usage" ] || echec "WIMAO_SOURCE_RELEASES hors essai : site créé"
code=0
"$assembler" --inconnue > /dev/null 2>&1 || code=$?
attendre_code 2 "option inconnue"
code=0
"$assembler" > /dev/null 2>&1 || code=$?
attendre_code 2 "site manquant"
code=0
"$assembler" "$temporaire/s-usage" --cle "$empreinte_a" > /dev/null 2>&1 || code=$?
attendre_code 2 "--cle sans --racine"
code=0
"$assembler" "$temporaire/s-usage" --racine "$base" > /dev/null 2>&1 || code=$?
attendre_code 2 "--racine sans --cle"

# --- Release qui liste un autre fichier (ici elle-même), re-signé ---
r="$(nouvelle_racine r-release-soi)"
echo " $(printf '0%.0s' {1..64}) 0 Release" >> "$r/depot-apt/dists/stable/Release"
resigner "$r"
lancer "$r" "$temporaire/s-release-soi" "$source_ok"
attendre_code 1 "Release qui se décrit elle-même"
attendre_message "d'autres fichiers que Packages et Packages.gz" "Release qui se décrit elle-même"
attendre_vide "$temporaire/s-release-soi" "Release qui se décrit elle-même"

echo "Tous les cas de assembler-site-apt.sh se comportent comme prévu."
