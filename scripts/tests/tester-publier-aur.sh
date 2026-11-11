#!/usr/bin/env bash
# Teste scripts/publier-aur.sh. Clé GnuPG jetable (A signe, B est « une autre clé »), faux SHA256SUMS (trois .deb
# et trois .rpm aux empreintes inventées, version de Cargo.toml), faux `gh` et `git` en tête du PATH. Il faut
# podman : le .SRCINFO est produit dans l'image Arch figée (paquets/aur/commun-aur.sh).
#
# Deux sortes d'essais :
#   - À BLANC (--a-blanc) : les faux gh et git se consignent et échouent, ce qui prouve qu'aucun appel à gh ni à
#     git n'a lieu, et le script n'a pas lancé l'essai de construction (aucun conteneur d'essai à blanc : les
#     .deb d'essai n'existent pas sur GitHub). Cas : réussite (PKGBUILD avec validpgpkeys = clé d'essai,
#     sources de la Release, aucune marque d'essai local, .SRCINFO présent ; relance : mêmes octets) ; refus
#     (code 1) : SHA256SUMS.asc absent, signé par une autre clé, SHA256SUMS modifié après la signature,
#     SHA256SUMS sans les .deb attendus, étiquette qui n'est pas la version de Cargo.toml ; un refus laisse
#     --sortie intacte ; usage (code 2) : étiquette mal formée (v0.7), options manquantes, option à blanc
#     sans --a-blanc.
#   - MODE RÉEL, avec des faux gh et git programmables : dépôt non propre -> 1, hors de main -> 1, branche
#     déjà existante -> 1, Release encore brouillon -> 1. Ces refus ont lieu avant tout téléchargement, tout
#     conteneur et toute écriture.
#
# Usage     : scripts/tests/tester-publier-aur.sh
# Variables : aucune (GNUPGHOME et PATH sont réglés par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~/.gnupg ni dans le dépôt.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
publier="$racine/scripts/publier-aur.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

[ -f "$publier" ] || echec "scripts/publier-aur.sh est introuvable"
command -v podman > /dev/null || echec "podman est nécessaire à cet essai"

# La recette annonce la version de Cargo.toml : l'étiquette d'essai est donc celle-là.
version="$(awk -F'"' '/^version = / { print $2; exit }' "$racine/Cargo.toml")"
etiquette="v$version"
deb_wimao="wimao_${version}-1_amd64.deb"
deb_bridge="wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb"
deb_wine="wimao-wine_11.20+wimao1_amd64.deb"

# --- Trousseau GnuPG jetable : clé A (celle qui signe) et clé B (une autre) ---
export GNUPGHOME="$temporaire/gnupg"
mkdir -m 700 "$GNUPGHOME"
cat > "$temporaire/a.params" << 'PARAMS'
%no-protection
Key-Type: EDDSA
Key-Curve: ed25519
Key-Usage: sign
Name-Real: WiMao essai A
Name-Email: a@example.invalid
Expire-Date: 1d
%commit
PARAMS
sed -e 's/essai A/essai B/' -e 's/a@example/b@example/' "$temporaire/a.params" > "$temporaire/b.params"
gpg --batch --quiet --gen-key "$temporaire/a.params" 2> /dev/null
gpg --batch --quiet --gen-key "$temporaire/b.params" 2> /dev/null
empreinte_a="$(gpg --with-colons --list-keys a@example.invalid 2> /dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"

# --- Faux dépôt : sa clé publique (celle de A) donne le trousseau de contrôle de gpgv ---
fausse_racine="$temporaire/racine"
mkdir -p "$fausse_racine/docs"
gpg --armor --export a@example.invalid > "$fausse_racine/docs/cle-publique.asc"

# --- Faux gh et faux git : consignent l'appel. À blanc ils échouent (code 99) ; en mode réel ils répondent
#     selon FAUX_STATUT (sortie de `git status --porcelain`), FAUX_BRANCHE, FAUX_BRANCHE_EXISTE et FAUX_RELEASE. ---
mkdir "$temporaire/bin"
journal="$temporaire/appels.log"
cat > "$temporaire/bin/git" << FAUX
#!/usr/bin/env bash
echo "git \$*" >> "$journal"
if [ "\${FAUX_MODE_REEL:-}" != "oui" ]; then echo "FAUX git APPELÉ : git \$*" >&2; exit 99; fi
case "\$*" in
    *"status --porcelain"*) printf '%s' "\${FAUX_STATUT:-}" ;;
    *"branch --show-current"*) echo "\${FAUX_BRANCHE:-main}" ;;
    *"fetch"*) ;;
    *"rev-parse HEAD"*|*"rev-parse origin/main"*) echo "0000000000000000000000000000000000000000" ;;
    *"rev-parse --verify"*) [ "\${FAUX_BRANCHE_EXISTE:-}" = "oui" ] || exit 1 ;;
    *"ls-remote"*) exit 2 ;;
    *) echo "FAUX git : commande non prévue : git \$*" >&2; exit 99 ;;
esac
FAUX
cat > "$temporaire/bin/gh" << FAUX
#!/usr/bin/env bash
echo "gh \$*" >> "$journal"
if [ "\${FAUX_MODE_REEL:-}" != "oui" ]; then echo "FAUX gh APPELÉ : gh \$*" >&2; exit 99; fi
case "\$*" in
    "release view"*) printf '%s\n' "\${FAUX_RELEASE:-false	2026-01-01T00:00:00Z}" ;;
    *) echo "FAUX gh : commande non prévue : gh \$*" >&2; exit 99 ;;
esac
FAUX
chmod +x "$temporaire/bin/git" "$temporaire/bin/gh"
export PATH="$temporaire/bin:$PATH"

# artefacts <nom> <clé de signature> : dossier de faux artefacts (SHA256SUMS à six lignes signé ; aucun fichier
# de paquet : à blanc, seuls SHA256SUMS et SHA256SUMS.asc comptent). Rend son chemin.
artefacts() {
    local d="$temporaire/artefacts-$1" c
    rm -rf "$d"
    mkdir -p "$d"
    : > "$d/SHA256SUMS"
    for c in "1 $deb_wimao" "2 $deb_bridge" "3 $deb_wine" "4 wimao-$version-1.x86_64.rpm" \
        "5 wimao-bridge-5.1.1+dxvk3.1.1-1.x86_64.rpm" "6 wimao-wine-11.20+wimao1-1.x86_64.rpm"; do
        printf '%s  %s\n' "$(printf '%064d' 0 | tr 0 "${c%% *}")" "${c#* }" >> "$d/SHA256SUMS"
    done
    signer "$d" "$2"
    printf '%s' "$d"
}

# signer <dossier> <clé> : (re)signe <dossier>/SHA256SUMS.
signer() {
    gpg --batch --yes --armor --detach-sign --local-user "$2" --output "$1/SHA256SUMS.asc" "$1/SHA256SUMS" 2> /dev/null
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

# attendre_message <texte> <description> : la sortie du script contient ce texte (refus pour la bonne raison).
attendre_message() {
    grep -qF -- "$1" "$temporaire/sortie.log" || { cat "$temporaire/sortie.log" >&2; echec "$2 : message « $1 » absent"; }
}

sortie="$temporaire/sortie-aur"
a_blanc=(--a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$sortie")
art="$(artefacts ok a@example.invalid)"

# --- Premier passage ---
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art"
attendre_code 0 "premier passage"
pkg="$sortie/PKGBUILD"
[ -f "$pkg" ] || echec "PKGBUILD absent de la sortie"
[ -f "$sortie/.SRCINFO" ] || echec ".SRCINFO absent de la sortie"
[ "$(find "$sortie" -mindepth 1 | wc -l)" -eq 2 ] || echec "la sortie contient autre chose que PKGBUILD et .SRCINFO"
grep -qxF "validpgpkeys=($empreinte_a)" "$pkg" || echec "validpgpkeys ne porte pas la clé d'essai"
grep -qxF "pkgver=$version" "$pkg" || echec "pkgver incorrect"
grep -qF "releases/download/v\${pkgver}/SHA256SUMS.asc" "$pkg" || echec "la signature de la Release n'est pas dans source="
! grep -q 'ESSAI LOCAL' "$pkg" || echec "marque d'essai local dans la recette à publier"
! grep -q 'rpm' "$pkg" || echec "un .rpm est entré dans le PKGBUILD"
grep -qP '^\toptdepends = vulkan-driver: ' "$sortie/.SRCINFO" || echec ".SRCINFO sans « optdepends = vulkan-driver: … »"
# Relance : mêmes octets.
avant="$(cd "$sortie" && sha256sum PKGBUILD .SRCINFO)"
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art"
attendre_code 0 "relance"
[ "$(cd "$sortie" && sha256sum PKGBUILD .SRCINFO)" = "$avant" ] || echec "la recette diffère entre deux fabrications identiques"

# --- Refus à blanc : la sortie doit rester intacte ---
# SHA256SUMS.asc absent
art_x="$(artefacts sans-asc a@example.invalid)"
rm "$art_x/SHA256SUMS.asc"
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art_x"
attendre_code 1 "SHA256SUMS.asc absent"
attendre_message "SHA256SUMS.asc introuvable" "SHA256SUMS.asc absent"

# SHA256SUMS.asc signé par une autre clé (B, absente du trousseau de contrôle)
art_b="$(artefacts autre-cle b@example.invalid)"
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art_b"
attendre_code 1 "SHA256SUMS.asc d'une autre clé"
attendre_message "SHA256SUMS n'est pas signé par la clé publiée" "SHA256SUMS.asc d'une autre clé"

# SHA256SUMS modifié après la signature
art_s="$(artefacts somme-modifiee a@example.invalid)"
printf '%064d  faux.deb\n' 0 >> "$art_s/SHA256SUMS"
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art_s"
attendre_code 1 "SHA256SUMS modifié"
attendre_message "SHA256SUMS n'est pas signé par la clé publiée" "SHA256SUMS modifié"

# SHA256SUMS bien signé mais sans les .deb attendus (que des .rpm)
art_r="$(artefacts sans-deb a@example.invalid)"
grep -v '\.deb$' "$art_r/SHA256SUMS" > "$art_r/SHA256SUMS.neuf"
mv "$art_r/SHA256SUMS.neuf" "$art_r/SHA256SUMS"
signer "$art_r" a@example.invalid
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art_r"
attendre_code 1 "SHA256SUMS sans .deb"
attendre_message "SHA256SUMS ne contient pas le .deb de wimao" "SHA256SUMS sans .deb"

# Un seul .deb manque (celui de wine)
art_w="$(artefacts sans-deb-wine a@example.invalid)"
grep -v "$deb_wine" "$art_w/SHA256SUMS" > "$art_w/SHA256SUMS.neuf"
mv "$art_w/SHA256SUMS.neuf" "$art_w/SHA256SUMS"
signer "$art_w" a@example.invalid
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art_w"
attendre_code 1 "SHA256SUMS sans le .deb de wine"
attendre_message "SHA256SUMS ne contient pas le .deb de wimao-wine" "SHA256SUMS sans le .deb de wine"

# Étiquette bien formée mais qui n'est pas la version de Cargo.toml
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art"
attendre_code 1 "étiquette différente de Cargo.toml"
attendre_message "Cargo.toml" "étiquette différente de Cargo.toml"

# --dossier introuvable
lancer "$etiquette" "${a_blanc[@]}" --dossier "$temporaire/n-existe-pas"
attendre_code 1 "dossier introuvable"
attendre_message "dossier d'artefacts introuvable" "dossier introuvable"

[ "$(cd "$sortie" && sha256sum PKGBUILD .SRCINFO)" = "$avant" ] || echec "un refus a modifié la sortie"
[ "$(find "$sortie" -mindepth 1 | wc -l)" -eq 2 ] || echec "un refus a laissé un fichier dans la sortie"

# --- Usage ---
lancer "$etiquette" "${a_blanc[@]}" --dossier "$art" --inconnue
attendre_code 2 "option inconnue"
lancer v0.7 "${a_blanc[@]}" --dossier "$art"
attendre_code 2 "étiquette v0.7"
attendre_message "invalide (forme attendue : vX.Y.Z" "étiquette v0.7"
lancer "$version" "${a_blanc[@]}" --dossier "$art"
attendre_code 2 "étiquette sans v"
lancer "$etiquette" --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$sortie"
attendre_code 2 "--a-blanc sans --dossier"
lancer "$etiquette" --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --dossier "$art"
attendre_code 2 "--a-blanc sans --sortie"
lancer "$etiquette" --a-blanc --racine "$fausse_racine" --sortie "$sortie" --dossier "$art"
attendre_code 2 "--a-blanc sans --cle"
lancer "$etiquette" --a-blanc --cle "$empreinte_a" --sortie "$sortie" --dossier "$art"
attendre_code 2 "--a-blanc sans --racine"
lancer "$etiquette" --racine "$fausse_racine"
attendre_code 2 "--racine sans --a-blanc"
lancer "$etiquette" --dossier "$art"
attendre_code 2 "--dossier sans --a-blanc"
lancer "$etiquette" --sortie "$sortie"
attendre_code 2 "--sortie sans --a-blanc"
lancer "$etiquette" --cle "$empreinte_a"
attendre_code 2 "--cle sans --a-blanc"
lancer
attendre_code 2 "version manquante"

# --- Aucun appel à gh ni à git à blanc (avant d'activer les faux du mode réel) ---
[ ! -s "$journal" ] || echec "gh ou git a été appelé à blanc : $(cat "$journal")"

# --- Mode réel simulé : les faux gh et git répondent ; chaque refus a lieu avant tout téléchargement ---
export FAUX_MODE_REEL=oui
# Dépôt non propre
FAUX_STATUT=" M fichier.txt" lancer "$etiquette"
attendre_code 1 "dépôt non propre"
attendre_message "modifications non enregistrées" "dépôt non propre"
# Hors de la branche main
FAUX_BRANCHE=autre lancer "$etiquette"
attendre_code 1 "hors de main"
attendre_message "pas sur la branche main" "hors de main"
# Branche de la PR déjà là
FAUX_BRANCHE_EXISTE=oui lancer "$etiquette"
attendre_code 1 "branche existante"
attendre_message "aur-$etiquette existe déjà" "branche existante"
# Release encore brouillon
FAUX_RELEASE="$(printf 'true\t2026-01-01T00:00:00Z')" lancer "$etiquette"
attendre_code 1 "Release brouillon"
attendre_message "encore un brouillon" "Release brouillon"
unset FAUX_MODE_REEL
# Ni téléchargement, ni branche, ni commit, ni envoi, ni PR n'ont eu lieu.
! grep -qE 'release download|switch|commit|push|pr create' "$journal" || echec "un refus du mode réel est allé trop loin : $(cat "$journal")"
grep -q 'release view' "$journal" || echec "le mode réel n'a pas interrogé la Release (essai mal monté)"

echo "OK : tester-publier-aur.sh"
