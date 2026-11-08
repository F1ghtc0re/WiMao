#!/usr/bin/env bash
# Teste scripts/publier-depot-dnf.sh. Clé GnuPG jetable (avec sous-clé de signature), faux RPM fabriqués par
# scripts/tests/fabriquer-faux-rpm.sh (de vrais RPM minuscules, version 9.9.9), faux `gh` et `git` en tête du
# PATH. Il faut podman et le réseau : le conteneur Fedora 45 installe rpm-build puis createrepo_c.
#
# Deux sortes d'essais :
#   - À BLANC (--a-blanc) : les faux gh et git se consignent et échouent, ce qui prouve qu'aucun appel à gh ni à
#     git n'a lieu. Cas : réussite (trois entrées, adresses = <Release>/<nom>, signature acceptée par gpgv,
#     aucun RPM copié dans la sortie, SHA256SUMS à six lignes dont les .deb sont ignorés) ; relance de la
#     même version : mêmes octets pour tout sauf la signature, et un reste de l'ancien index disparaît ;
#     SHA256SUMS.asc absent, signé par une autre clé ou modifié -> 1 ; RPM à l'empreinte fausse -> 1 ; RPM en
#     trop, ou nombre de RPM différent de trois (deux, quatre) -> 1 ; nom de RPM inattendu -> 1 ;
#     un refus laisse --sortie intacte ; version mal formée, options manquantes, --adresse-rpm mal formée
#     ou hors --a-blanc -> 2.
#   - MODE RÉEL, avec des faux gh et git programmables : dépôt non propre -> 1, hors de main -> 1, Release
#     encore brouillon -> 1. Ces refus ont lieu avant tout téléchargement, toute signature et toute écriture.
#
# Usage     : scripts/tests/tester-publier-depot-dnf.sh
# Variables : aucune (GNUPGHOME et PATH sont réglés par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~/.gnupg ni dans le dépôt.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
publier="$racine/scripts/publier-depot-dnf.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

[ -f "$publier" ] || echec "scripts/publier-depot-dnf.sh est introuvable"
command -v podman > /dev/null || echec "podman est nécessaire à cet essai"

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
gpg --export a@example.invalid > "$temporaire/trousseau.gpg"

# --- Faux gh et faux git : consignent l'appel. À blanc ils échouent (code 99) ; en mode réel ils répondent
#     selon FAUX_STATUT (sortie de `git status --porcelain`), FAUX_BRANCHE et FAUX_RELEASE. ---
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
    *"rev-parse --verify"*) exit 1 ;;
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

# --- Faux RPM (une seule fabrication, 13 s) ---
rpm_sources="$temporaire/rpm-sources"
"$racine/scripts/tests/fabriquer-faux-rpm.sh" "$rpm_sources" 9.9.9 > "$temporaire/fabrication.log" 2>&1 \
    || { cat "$temporaire/fabrication.log" >&2; echec "fabrication des faux RPM impossible"; }

# artefacts <nom> <clé de signature> : dossier de faux artefacts (3 .rpm, SHA256SUMS à six lignes signé) ; rend son chemin.
# SHA256SUMS liste aussi trois faux .deb (somme sans fichier) : le script ne doit regarder que les .rpm.
artefacts() {
    local d="$temporaire/artefacts-$1"
    rm -rf "$d"
    mkdir -p "$d"
    cp "$rpm_sources"/*.rpm "$d/"
    {
        for nom in wimao wimao-bridge wimao-wine; do
            echo "$(printf '%s' "faux $nom" | sha256sum | cut -d' ' -f1)  ${nom}_9.9.9-1_amd64.deb"
        done
        (cd "$d" && sha256sum -- *.rpm)
    } | LC_ALL=C sort -k2 > "$d/SHA256SUMS"
    gpg --batch --armor --detach-sign --local-user "$2" --output "$d/SHA256SUMS.asc" "$d/SHA256SUMS" 2> /dev/null
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

# attendre_message <texte> <description> : la sortie du script contient ce texte (refus pour la bonne raison).
attendre_message() {
    grep -qF -- "$1" "$temporaire/sortie.log" || { cat "$temporaire/sortie.log" >&2; echec "$2 : message « $1 » absent"; }
}

depot="$temporaire/depot-dnf"
repodata="$depot/rpm/repodata"
a_blanc=(--a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$depot")
prefixe="https://github.com/F1ghtc0re/WiMao/releases/download/v9.9.9/"
art="$(artefacts ok a@example.invalid)"

# --- Premier passage ---
export SOURCE_DATE_EPOCH=1700000000
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art"
attendre_code 0 "premier passage"
for f in repomd.xml repomd.xml.asc; do
    [ -f "$repodata/$f" ] || echec "$f absent"
done
grep -qx '  <revision>1700000000</revision>' "$repodata/repomd.xml" || echec "repomd.xml : revision différente de SOURCE_DATE_EPOCH"
# Aucun RPM ne doit être copié dans la sortie (l'index pointe vers la Release) et rien d'autre que repodata/.
[ -z "$(find "$depot" -name '*.rpm')" ] || echec "un RPM a été copié dans la sortie"
[ "$(find "$depot" -mindepth 1 -maxdepth 1 | wc -l)" -eq 1 ] || echec "la sortie contient autre chose que rpm/"
# gpgv (trousseau tiré de la clé publique de A, jamais le trousseau personnel) accepte la signature.
gpgv --keyring "$temporaire/trousseau.gpg" "$repodata/repomd.xml.asc" "$repodata/repomd.xml" > /dev/null 2>&1 || echec "gpgv refuse repomd.xml.asc"
# Adresses, tailles et sommes de l'index (lues comme le fait l'assemblage du site).
index="$(python3 -I "$racine/scripts/lib/lire-index-dnf.py" "$repodata")" || echec "lire-index-dnf.py refuse l'index produit"
[ "$(wc -l <<< "$index")" -eq 3 ] || echec "l'index ne décrit pas trois RPM"
for rpm in "$art"/*.rpm; do
    nom="$(basename "$rpm")"
    attendu="$prefixe$nom	$(stat -c %s "$rpm")	$(sha256sum "$rpm" | cut -d' ' -f1)"
    grep -qxF "$attendu" <<< "$index" || echec "index : ligne attendue absente pour $nom (« $attendu »)"
done
# Octets d'une relance : tout sauf la signature (sa date de création change) doit être identique.
empreinte_depot() { (cd "$depot" && find . -type f ! -name 'repomd.xml.asc' -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum); }
avant="$(empreinte_depot)"
touch "$repodata/reste-de-l-ancien-index.xml"

# --- Même version relancée : mêmes octets, et l'ancien index est entièrement remplacé ---
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art"
attendre_code 0 "même version relancée"
[ ! -e "$repodata/reste-de-l-ancien-index.xml" ] || echec "un reste de l'ancien index a survécu"
[ "$(empreinte_depot)" = "$avant" ] || echec "l'index diffère entre deux fabrications identiques (non reproductible)"
gpgv --keyring "$temporaire/trousseau.gpg" "$repodata/repomd.xml.asc" "$repodata/repomd.xml" > /dev/null 2>&1 || echec "gpgv refuse repomd.xml.asc après relance"

# --- Refus à blanc : la sortie doit rester intacte ---
avant_complet="$(cd "$depot" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum)"

# SHA256SUMS.asc absent
art_x="$(artefacts sans-asc a@example.invalid)"
rm "$art_x/SHA256SUMS.asc"
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_x"
attendre_code 1 "SHA256SUMS.asc absent"
attendre_message "SHA256SUMS.asc introuvable" "SHA256SUMS.asc absent"

# SHA256SUMS.asc signé par une autre clé (B, absente du trousseau de contrôle)
art_b="$(artefacts autre-cle b@example.invalid)"
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_b"
attendre_code 1 "SHA256SUMS.asc d'une autre clé"
attendre_message "SHA256SUMS n'est pas signé par la clé publiée" "SHA256SUMS.asc d'une autre clé"

# SHA256SUMS modifié après la signature
art_s="$(artefacts somme-modifiee a@example.invalid)"
echo "$(printf '0%.0s' {1..64})  faux.rpm" >> "$art_s/SHA256SUMS"
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_s"
attendre_code 1 "SHA256SUMS modifié"
attendre_message "SHA256SUMS n'est pas signé par la clé publiée" "SHA256SUMS modifié"

# RPM modifié après la signature (empreinte fausse)
art_m="$(artefacts rpm-modifie a@example.invalid)"
echo "octets en trop" >> "$art_m/wimao-9.9.9-1.x86_64.rpm"
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_m"
attendre_code 1 "RPM à l'empreinte fausse"
attendre_message "ne correspond pas à SHA256SUMS" "RPM à l'empreinte fausse"

# RPM en trop (présent, non listé)
art_t="$(artefacts rpm-en-trop a@example.invalid)"
cp "$art_t/wimao-9.9.9-1.x86_64.rpm" "$art_t/wimao-9.9.10-1.x86_64.rpm"
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_t"
attendre_code 1 "RPM en trop"
attendre_message "ne sont pas exactement ceux de SHA256SUMS" "RPM en trop"

# Deux RPM seulement (SHA256SUMS signé en conséquence)
art_d="$(artefacts deux-rpm a@example.invalid)"
rm "$art_d/wimao-wine-9.9.9-1.x86_64.rpm" "$art_d/SHA256SUMS.asc"
grep -v 'wimao-wine-9.9.9-1.x86_64.rpm' "$art_d/SHA256SUMS" > "$art_d/SHA256SUMS.neuf"
mv "$art_d/SHA256SUMS.neuf" "$art_d/SHA256SUMS"
gpg --batch --armor --detach-sign --local-user a@example.invalid --output "$art_d/SHA256SUMS.asc" "$art_d/SHA256SUMS" 2> /dev/null
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_d"
attendre_code 1 "deux RPM"
attendre_message "liste 2 RPM au lieu de trois" "deux RPM"

# Quatre RPM (un de plus, listé et signé)
art_q="$(artefacts quatre-rpm a@example.invalid)"
cp "$art_q/wimao-9.9.9-1.x86_64.rpm" "$art_q/wimao-bridge-9.9.10-1.x86_64.rpm"
(cd "$art_q" && sha256sum -- wimao-bridge-9.9.10-1.x86_64.rpm >> SHA256SUMS)
gpg --batch --yes --armor --detach-sign --local-user a@example.invalid --output "$art_q/SHA256SUMS.asc" "$art_q/SHA256SUMS" 2> /dev/null
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_q"
attendre_code 1 "quatre RPM"
attendre_message "liste 4 RPM au lieu de trois" "quatre RPM"

# Nom de RPM inattendu (un paquet qui n'est pas de WiMao), listé et signé : trois RPM mais pas les bons
art_n="$(artefacts nom-inattendu a@example.invalid)"
mv "$art_n/wimao-wine-9.9.9-1.x86_64.rpm" "$art_n/intrus-9.9.9-1.x86_64.rpm"
sed -i 's/wimao-wine-9.9.9-1.x86_64.rpm/intrus-9.9.9-1.x86_64.rpm/' "$art_n/SHA256SUMS"
gpg --batch --yes --armor --detach-sign --local-user a@example.invalid --output "$art_n/SHA256SUMS.asc" "$art_n/SHA256SUMS" 2> /dev/null
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art_n"
attendre_code 1 "nom de RPM inattendu"
attendre_message "nom de RPM inattendu" "nom de RPM inattendu"

[ "$(cd "$depot" && find . -type f -print0 | LC_ALL=C sort -z | xargs -0 sha256sum | sha256sum)" = "$avant_complet" ] \
    || echec "un refus a modifié le dépôt de sortie"

# --- Usage ---
lancer v9.9.9 "${a_blanc[@]}" --dossier "$art" --inconnue
attendre_code 2 "option inconnue"
lancer v1.2 "${a_blanc[@]}" --dossier "$art"
attendre_code 2 "étiquette v1.2"
lancer 9.9.9 "${a_blanc[@]}" --dossier "$art"
attendre_code 2 "étiquette sans v"
lancer v9.9.9 --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --sortie "$depot"
attendre_code 2 "--a-blanc sans --dossier"
lancer v9.9.9 --a-blanc --racine "$fausse_racine" --cle "$empreinte_a" --dossier "$art"
attendre_code 2 "--a-blanc sans --sortie"
lancer v9.9.9 --a-blanc --racine "$fausse_racine" --sortie "$depot" --dossier "$art"
attendre_code 2 "--a-blanc sans --cle"
lancer v9.9.9 --a-blanc --cle "$empreinte_a" --sortie "$depot" --dossier "$art"
attendre_code 2 "--a-blanc sans --racine"
lancer v9.9.9 --racine "$fausse_racine"
attendre_code 2 "--racine sans --a-blanc"
lancer v9.9.9 --adresse-rpm http://127.0.0.1:8081/
attendre_code 2 "--adresse-rpm sans --a-blanc"
for mauvaise in http://example.com/ https://127.0.0.1:8081/ http://127.0.0.1:8081 http://127.0.0.1:80a/ 'http://127.0.0.1:8081/x/'; do
    lancer v9.9.9 "${a_blanc[@]}" --dossier "$art" --adresse-rpm "$mauvaise"
    attendre_code 2 "--adresse-rpm $mauvaise"
done
lancer
attendre_code 2 "version manquante"

# --- Aucun appel à gh ni à git à blanc (avant d'activer les faux du mode réel) ---
[ ! -s "$journal" ] || echec "gh ou git a été appelé à blanc : $(cat "$journal")"

# --- Mode réel simulé : les faux gh et git répondent ; chaque refus a lieu avant tout téléchargement ---
export FAUX_MODE_REEL=oui
# Dépôt non propre
FAUX_STATUT=" M fichier.txt" lancer v9.9.9
attendre_code 1 "dépôt non propre"
attendre_message "modifications non enregistrées" "dépôt non propre"
# Hors de la branche main
FAUX_BRANCHE=autre lancer v9.9.9
attendre_code 1 "hors de main"
attendre_message "pas sur la branche main" "hors de main"
# Release encore brouillon
FAUX_RELEASE="$(printf 'true\t2026-01-01T00:00:00Z')" lancer v9.9.9
attendre_code 1 "Release brouillon"
attendre_message "encore un brouillon" "Release brouillon"
unset FAUX_MODE_REEL
# Ni téléchargement, ni branche, ni commit, ni envoi, ni PR n'ont eu lieu.
! grep -qE 'release download|switch|commit|push|pr create' "$journal" || echec "un refus du mode réel est allé trop loin : $(cat "$journal")"
grep -q 'release view' "$journal" || echec "le mode réel n'a pas interrogé la Release (essai mal monté)"

echo "OK : tester-publier-depot-dnf.sh"
