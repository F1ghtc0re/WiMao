#!/usr/bin/env bash
# Teste scripts/lib/publication.sh : les fonctions communes de vérification de signature du dépôt APT
# (publier-depot-apt.sh et assembler-site-apt.sh les réutilisent), sans réseau.
#
# Cas couverts : empreinte_publiee lit l'empreinte écrite avec espaces et la rend en 40 majuscules ;
# ligne absente ou mal formée -> 1 ; trousseau_cle_publiee accepte l'empreinte de la clé A et refuse
# celle de la clé B ; verifier_signature accepte une signature détachée faite par la SOUS-clé de A et une
# signature en clair (clearsign) de A ; refuse une signature valide faite par une autre clé (B, pourtant
# présente dans le trousseau) ; refuse un fichier modifié après signature et une signature d'une clé expirée ;
# trousseau_cle_publiee refuse un cle-publique.asc contenant deux clés.
#
# Usage     : scripts/tests/tester-publication.sh
# Variables : aucune (GNUPGHOME est réglé par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~/.gnupg.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

# Bibliothèque testée : son absence est l'échec attendu avant son écriture.
# shellcheck source=scripts/lib/publication.sh
source "$racine/scripts/lib/publication.sh" || echec "scripts/lib/publication.sh est introuvable"

# --- Trousseau GnuPG jetable : clé A (avec sous-clé de signature) et clé B (sans) ---
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
empreinte_b="$(gpg --with-colons --list-keys b@example.invalid 2> /dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"

# --- Faux dépôt : SECURITY.md (empreinte de A avec espaces, en minuscules) et la clé publique de A ---
depot="$temporaire/depot"
mkdir -p "$depot/docs"
espacee="$(sed 's/..../& /g; s/ $//' <<< "${empreinte_a,,}")"
printf 'Titre\nEmpreinte de la clé de signature : %s\n' "$espacee" > "$depot/SECURITY.md"
gpg --armor --export a@example.invalid > "$depot/docs/cle-publique.asc"

# --- empreinte_publiee ---
[ "$(empreinte_publiee "$depot")" = "$empreinte_a" ] || echec "empreinte_publiee n'a pas rendu l'empreinte de A en majuscules"
vide="$temporaire/vide"
mkdir "$vide"
printf 'Rien ici\n' > "$vide/SECURITY.md"
if empreinte_publiee "$vide" 2> /dev/null; then echec "empreinte_publiee accepte un SECURITY.md sans ligne d'empreinte"; fi
printf 'Empreinte de la clé de signature : 1234 abcd\n' > "$vide/SECURITY.md"
if empreinte_publiee "$vide" 2> /dev/null; then echec "empreinte_publiee accepte une empreinte trop courte"; fi

# --- trousseau_cle_publiee ---
trousseau_a="$temporaire/trousseau-a.gpg"
trousseau_cle_publiee "$depot" "$empreinte_a" "$trousseau_a" || echec "trousseau_cle_publiee refuse la bonne empreinte"
[ -s "$trousseau_a" ] || echec "le trousseau de A n'a pas été créé"
if trousseau_cle_publiee "$depot" "$empreinte_b" "$temporaire/trousseau-x.gpg" 2> /dev/null; then
    echec "trousseau_cle_publiee accepte l'empreinte de B pour la clé de A"
fi

# Un cle-publique.asc contenant A puis B : refusé (exactement une clé principale).
mkdir -p "$temporaire/depot-deux/docs"
cat "$depot/docs/cle-publique.asc" > "$temporaire/depot-deux/docs/cle-publique.asc"
gpg --armor --export b@example.invalid >> "$temporaire/depot-deux/docs/cle-publique.asc" 2> /dev/null
if trousseau_cle_publiee "$temporaire/depot-deux" "$empreinte_a" "$temporaire/trousseau-deux.gpg" 2> /dev/null; then
    echec "trousseau_cle_publiee accepte un cle-publique.asc contenant deux clés"
fi

# Trousseau de B, puis trousseau combiné A + B (des trousseaux binaires se concatènent) : B est connue de gpgv.
gpg --export b@example.invalid > "$temporaire/trousseau-b.gpg"
cat "$trousseau_a" "$temporaire/trousseau-b.gpg" > "$temporaire/trousseau-ab.gpg"

# --- verifier_signature ---
echo "contenu à signer" > "$temporaire/fichier"
# Signature détachée par la sous-clé de A : on désigne la sous-clé par son identifiant suivi de « ! ».
sous_cle="$(gpg --with-colons --list-keys a@example.invalid 2> /dev/null | awk -F: '$1 == "sub" { print $5; exit }')"
gpg --batch --yes --quiet --local-user "${sous_cle}!" --detach-sign -o "$temporaire/fichier.sig-a" "$temporaire/fichier" 2> /dev/null
verifier_signature "$trousseau_a" "$empreinte_a" "$temporaire/fichier.sig-a" "$temporaire/fichier" \
    || echec "signature détachée de la sous-clé de A refusée"
# Signature en clair par A.
gpg --batch --yes --quiet --local-user "$empreinte_a" --clearsign -o "$temporaire/fichier.clair-a" "$temporaire/fichier" 2> /dev/null
verifier_signature "$trousseau_a" "$empreinte_a" "$temporaire/fichier.clair-a" || echec "signature en clair de A refusée"
# Signature valide mais faite par B, dont la clé est dans le trousseau : refusée.
gpg --batch --yes --quiet --local-user "$empreinte_b" --detach-sign -o "$temporaire/fichier.sig-b" "$temporaire/fichier" 2> /dev/null
if verifier_signature "$temporaire/trousseau-ab.gpg" "$empreinte_a" "$temporaire/fichier.sig-b" "$temporaire/fichier" 2> /dev/null; then
    echec "signature valide de B acceptée alors que l'empreinte attendue est celle de A"
fi
# Contrôle : la même signature de B passe si on attend B (prouve que le refus ci-dessus vient de l'empreinte).
verifier_signature "$temporaire/trousseau-ab.gpg" "$empreinte_b" "$temporaire/fichier.sig-b" "$temporaire/fichier" \
    || echec "signature de B refusée alors qu'on attend B"
# Fichier modifié après signature : refusé.
echo "modifié" >> "$temporaire/fichier"
if verifier_signature "$trousseau_a" "$empreinte_a" "$temporaire/fichier.sig-a" "$temporaire/fichier" 2> /dev/null; then
    echec "fichier modifié après signature accepté"
fi

# Clé C expirée : créée et utilisée pour signer il y a 20 jours (validité d'un jour), donc expirée aujourd'hui.
# gpgv rend 0 avec EXPKEYSIG + VALIDSIG ; verifier_signature doit refuser.
passe="$(date -d '20 days ago' +%s)"
sed -e 's/essai A/essai C/' -e 's/a@example/c@example/' -e '/^Subkey-/d' "$temporaire/a.params" > "$temporaire/c.params"
gpg --batch --quiet --faked-system-time "$passe" --gen-key "$temporaire/c.params" 2> /dev/null
empreinte_c="$(gpg --with-colons --list-keys c@example.invalid 2> /dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"
echo "contenu C" > "$temporaire/fichier-c"
gpg --batch --yes --quiet --faked-system-time "$passe" --local-user "$empreinte_c" --detach-sign \
    -o "$temporaire/fichier-c.sig" "$temporaire/fichier-c" 2> /dev/null
gpg --export c@example.invalid > "$temporaire/trousseau-c.gpg" 2> /dev/null
if verifier_signature "$temporaire/trousseau-c.gpg" "$empreinte_c" "$temporaire/fichier-c.sig" "$temporaire/fichier-c" 2> /dev/null; then
    echec "signature d'une clé expirée acceptée"
fi

echo "OK : scripts/lib/publication.sh se comporte comme prévu."
