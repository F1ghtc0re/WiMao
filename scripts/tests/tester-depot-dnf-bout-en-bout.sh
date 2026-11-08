#!/usr/bin/env bash
# Essai de BOUT EN BOUT du dépôt dnf, sur la machine de développement, avec de FAUX RPM et une clé JETABLE :
#   faux RPM -> scripts/publier-depot-dnf.sh --a-blanc (index signé par la clé jetable, adresses vers un serveur
#   local) -> site minimal (rpm/, wimao.repo, wimao.asc) -> dnf dans des conteneurs Fedora 44 et 45 qui installent
#   `wimao` depuis ce site (repo_gpgcheck=1) puis refusent un index altéré (scripts/tests/tester-installation-dnf.sh).
#
# Ce que cet essai prouve : la chaîne publication -> index signé -> dnf de Fedora 44 et 45, avec le vrai
# `publier-depot-dnf.sh` et le vrai `wimao.repo`. Ce qu'il NE prouve PAS : le téléchargement des RPM depuis
# github.com (ici un second serveur local les fournit, par --adresse-rpm, réservé à l'essai) et l'assemblage du
# site, qui exige les adresses de GitHub (il est testé à part, par tester-assembler-site-apt.sh). Dans la CI,
# le job `installation-dnf` fait l'installation avec le site réellement assemblé, quand un index dnf existe.
#
# Usage     : scripts/tests/tester-depot-dnf-bout-en-bout.sh      (podman et le réseau sont nécessaires, ~5 min)
# Variables : aucune (GNUPGHOME est réglé par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~/.gnupg ni dans le dépôt.
# Code      : 0 si tout est prouvé, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT
echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}
command -v podman > /dev/null || echec "podman est nécessaire"

# Clé jetable (avec sous-clé de signature) : jamais la vraie clé.
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
Name-Real: WiMao essai bout en bout
Name-Email: a@example.invalid
Expire-Date: 1d
%commit
PARAMS
gpg --batch --quiet --gen-key "$temporaire/a.params" 2> /dev/null
empreinte="$(gpg --with-colons --list-keys a@example.invalid 2> /dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"
mkdir -p "$temporaire/racine/docs"
gpg --armor --export a@example.invalid > "$temporaire/racine/docs/cle-publique.asc"

# Faux RPM et SHA256SUMS signé, comme une Release.
art="$temporaire/artefacts"
"$racine/scripts/tests/fabriquer-faux-rpm.sh" "$art" 9.9.9 > "$temporaire/fabrication.log" 2>&1 \
    || { cat "$temporaire/fabrication.log" >&2; echec "fabrication des faux RPM impossible"; }
(cd "$art" && sha256sum -- *.rpm > SHA256SUMS)
gpg --batch --armor --detach-sign --local-user a@example.invalid --output "$art/SHA256SUMS.asc" "$art/SHA256SUMS" 2> /dev/null

# Index signé, dont les adresses visent le second serveur du conteneur (port 8081).
"$racine/scripts/publier-depot-dnf.sh" v9.9.9 --a-blanc --racine "$temporaire/racine" --cle "$empreinte" \
    --sortie "$temporaire/depot-dnf" --dossier "$art" --adresse-rpm http://127.0.0.1:8081/ > "$temporaire/publier.log" 2>&1 \
    || { cat "$temporaire/publier.log" >&2; echec "publier-depot-dnf.sh a échoué"; }

# Site minimal : ce que l'assemblage ajoute au site (rpm/, wimao.repo), plus la clé publique (wimao.asc).
site="$temporaire/site"
mkdir -p "$site"
cp -r "$temporaire/depot-dnf/rpm" "$site/rpm"
cp "$racine/depot-dnf/wimao.repo" "$site/wimao.repo"
cp "$temporaire/racine/docs/cle-publique.asc" "$site/wimao.asc"
chmod -R a+rX "$site" "$art"

# Les images : les mêmes que le workflow (Fedora 44 et 45, figées par empreinte).
for image in \
    "registry.fedoraproject.org/fedora:44@sha256:4b39e947a92d0ac2df1db9ebbf7672256d828a73e6e0219f8a6e774f84f34d64" \
    "registry.fedoraproject.org/fedora:45@sha256:4a8a63ec2df9f26a96c35ff380237ad578cf42e326b979d9b5801c31c6daf2e8"; do
    echo "######## ${image%%@*}"
    mkdir -p "$temporaire/rpms-servis"
    cp "$art"/*.rpm "$temporaire/rpms-servis/"
    podman run --rm -v "$site:/site:ro,Z" -v "$temporaire/rpms-servis:/rpms:ro,Z" \
        -v "$racine/scripts/tests/tester-installation-dnf.sh:/essai.sh:ro,Z" \
        "$image" bash /essai.sh /site /rpms || echec "l'essai d'installation a échoué sur ${image%%@*}"
done
echo "OK : tester-depot-dnf-bout-en-bout.sh"
