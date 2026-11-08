#!/usr/bin/env bash
# Fabrique trois FAUX RPM d'essai (wimao, wimao-bridge, wimao-wine, version donnée, un seul petit fichier chacun)
# dans un conteneur Fedora 45 figé. Ils servent aux essais de scripts/publier-depot-dnf.sh,
# scripts/assembler-site-apt.sh et de l'essai de bout en bout : de vrais RPM (createrepo_c lit leur en-tête,
# dnf peut les installer) sans la charge des vrais paquets de WiMao.
#
# Usage     : scripts/tests/fabriquer-faux-rpm.sh <dossier de sortie> <version>   (ex. /tmp/x 9.9.9)
# Écrit     : <sortie>/<paquet>-<version>-1.x86_64.rpm (trois fichiers) ; le reste vit dans le conteneur jetable.
# Code      : 0 = fabriqués ; 1 = échec ; 2 = usage.
# Dépend de : podman et d'un accès réseau (le conteneur installe rpm-build depuis les dépôts de Fedora).
set -euo pipefail

if [ "$#" -ne 2 ] || ! [[ "$2" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Usage : $0 <dossier de sortie> <version X.Y.Z>" >&2
    exit 2
fi
mkdir -p -- "$1"
sortie="$(cd -- "$1" && pwd)"
# Même image que scripts/publier-depot-dnf.sh (Fedora 45 figé par empreinte).
image="registry.fedoraproject.org/fedora:45@sha256:4a8a63ec2df9f26a96c35ff380237ad578cf42e326b979d9b5801c31c6daf2e8"

podman run --rm -v "$sortie:/d:Z" "$image" bash -euo pipefail -c '
    dnf install -y -q --setopt=install_weak_deps=False rpm-build > /tmp/dnf.log 2>&1 || { cat /tmp/dnf.log; exit 1; }
    for nom in wimao wimao-bridge wimao-wine; do
        cat > "/tmp/$nom.spec" << SPEC
Name: $nom
Version: $1
Release: 1
Summary: faux paquet d essai
License: MIT
BuildArch: x86_64
%description
Faux paquet d essai de WiMao, sans aucun contenu utile.
%install
mkdir -p %{buildroot}/usr/share/$nom
echo $nom > %{buildroot}/usr/share/$nom/note
%files
/usr/share/$nom/note
SPEC
        rpmbuild -bb --define "_rpmdir /tmp/rpms" --define "_topdir /tmp/top" "/tmp/$nom.spec" > /tmp/rpmbuild.log 2>&1 \
            || { cat /tmp/rpmbuild.log; exit 1; }
    done
    cp /tmp/rpms/x86_64/*.rpm /d/
' _ "$2"
