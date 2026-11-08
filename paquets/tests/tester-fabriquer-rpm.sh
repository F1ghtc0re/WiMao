#!/usr/bin/env bash
# Test de paquets/fabriquer-rpm.sh, à lancer DANS Fedora 45 (sans racine, via podman) :
#   podman run --rm -v "$PWD:/d:ro,Z" registry.fedoraproject.org/fedora:45@sha256:4a8a63ec2df9f26a96c35ff380237ad578cf42e326b979d9b5801c31c6daf2e8 \
#       bash /d/paquets/tests/tester-fabriquer-rpm.sh
# Le dépôt peut être monté en lecture seule : tout est écrit dans un dossier temporaire.
# Installe au besoin rpm-build, dpkg, zstd et cpio (dnf), ce qui demande l'accès réseau.
#
# Principe : pour chacun des trois noms de paquet connus, le test construit un FAUX .deb (un exécutable,
# un fichier 0644, un lien symbolique, un chemin avec espace, un fichier contenant « % »), le réemballe
# et vérifie : (a) la liste des chemins, (b) version et release, (c) droits, liens et contenus identiques
# à ceux du .deb, (d) fabrication reproductible, (e) refus sans SOURCE_DATE_EPOCH, (f) refus d'un .deb
# inconnu, (g) aucun script d'installation, (h) seules les dépendances des fichiers .deps.
# Sortie : « OK » et code 0 si tout passe ; sinon un message « ÉCHEC » et un code non nul.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
script="$racine/paquets/fabriquer-rpm.sh"

if ! command -v rpmbuild >/dev/null || ! command -v dpkg-deb >/dev/null || ! command -v cpio >/dev/null || ! command -v zstd >/dev/null; then
    dnf install -y --setopt=install_weak_deps=False rpm-build dpkg zstd cpio >/dev/null
fi

t="$(mktemp -d)"
trap 'rm -rf "$t"' EXIT
export SOURCE_DATE_EPOCH=1700000000

echec() { echo "ÉCHEC : $*" >&2; exit 1; }

# fabriquer_faux_deb <nom> <version> <fichier.deb> : construit le faux .deb décrit plus haut.
fabriquer_faux_deb() {
    local nom="$1" version="$2" deb="$3" r="$t/faux-$1"
    rm -rf "$r"
    mkdir -p "$r/DEBIAN" "$r/opt/wimao/$nom/sous dossier" "$r/usr/bin"
    printf '#!/bin/sh\necho bonjour\n' > "$r/usr/bin/essai-$nom"
    chmod 0755 "$r/usr/bin/essai-$nom"
    echo "donnees" > "$r/opt/wimao/$nom/donnees.txt"
    chmod 0644 "$r/opt/wimao/$nom/donnees.txt"
    ln -s donnees.txt "$r/opt/wimao/$nom/lien"
    echo "avec espace" > "$r/opt/wimao/$nom/sous dossier/fichier avec espace.txt"
    printf '100%% %%{name} %%post\n' > "$r/opt/wimao/$nom/pourcent%.txt"
    cat > "$r/DEBIAN/control" <<CTL
Package: $nom
Version: $version
Architecture: amd64
Maintainer: Essai <essai@example.invalid>
Homepage: https://example.invalid/wimao
Description: faux paquet d'essai
 Description de plusieurs lignes avec un pour cent : 100%.
CTL
    dpkg-deb --root-owner-group -Zxz --build "$r" "$deb" >/dev/null
}

# inventaire <dossier> : un inventaire des fichiers et liens (chemin, droits, type, cible), dossiers exclus.
inventaire() {
    (cd "$1" && find . -mindepth 1 ! -type d -printf '%p %m %y %l\n' | LC_ALL=C sort)
}

# Un cas : nom du paquet, version du .deb, « version-release » attendue côté RPM.
tester_paquet() {
    local nom="$1" version_deb="$2" attendu="$3"
    local deb="$t/$nom.deb" s1="$t/s1-$nom" s2="$t/s2-$nom" rpm
    fabriquer_faux_deb "$nom" "$version_deb" "$deb"

    bash "$script" "$deb" "$s1" >/dev/null 2>"$t/err" || { cat "$t/err" >&2; echec "$nom : la fabrication a échoué"; }
    rpm="$s1/$nom-$attendu.x86_64.rpm"
    [ -f "$rpm" ] || echec "$nom : $rpm absent (sortie : $(ls "$s1"))"

    # (a) tous les chemins du .deb sont dans le RPM
    rpm -qlp "$rpm" | sort > "$t/liste-rpm"
    for chemin in "/usr/bin/essai-$nom" "/opt/wimao/$nom/donnees.txt" "/opt/wimao/$nom/lien" \
                  "/opt/wimao/$nom/sous dossier/fichier avec espace.txt" "/opt/wimao/$nom/pourcent%.txt"; do
        grep -qxF -- "$chemin" "$t/liste-rpm" || echec "$nom (a) : « $chemin » absent de la liste du RPM"
    done
    # Aucun dossier système ne doit être possédé par le RPM.
    for chemin in /usr /usr/bin /opt; do
        if grep -qxF -- "$chemin" "$t/liste-rpm"; then echec "$nom (a) : le RPM possède $chemin"; fi
    done

    # (b) version et release
    [ "$(rpm -qp --qf '%{VERSION}-%{RELEASE}' "$rpm")" = "$attendu" ] || echec "$nom (b) : version-release inattendue : $(rpm -qp --qf '%{VERSION}-%{RELEASE}' "$rpm")"

    # (c) droits, liens et contenus identiques à ceux du .deb
    rm -rf "$t/ref-$nom" "$t/rpm-$nom"
    mkdir "$t/ref-$nom" "$t/rpm-$nom"
    dpkg-deb -x "$deb" "$t/ref-$nom"
    (cd "$t/rpm-$nom" && rpm2cpio "$rpm" | cpio -id --quiet)
    diff -r "$t/ref-$nom" "$t/rpm-$nom" || echec "$nom (c) : contenus différents"
    [ "$(inventaire "$t/ref-$nom")" = "$(inventaire "$t/rpm-$nom")" ] || {
        diff <(inventaire "$t/ref-$nom") <(inventaire "$t/rpm-$nom") >&2 || true
        echec "$nom (c) : droits, types ou liens différents"
    }

    # (d) deux fabrications identiques octet pour octet
    bash "$script" "$deb" "$s2" >/dev/null 2>&1 || echec "$nom (d) : seconde fabrication échouée"
    cmp "$rpm" "$s2/$nom-$attendu.x86_64.rpm" || echec "$nom (d) : deux fabrications différentes"

    # (e) SOURCE_DATE_EPOCH absent : refus
    if (unset SOURCE_DATE_EPOCH; bash "$script" "$deb" "$t/s3-$nom" >/dev/null 2>&1); then
        echec "$nom (e) : fabrication acceptée sans SOURCE_DATE_EPOCH"
    fi

    # (g) aucun script d'installation (rpm affiche « (aucun) » ou rien selon la langue)
    if rpm -qp --scripts "$rpm" | grep -q .; then
        echec "$nom (g) : le RPM contient des scripts : $(rpm -qp --scripts "$rpm")"
    fi

    # (h) dépendances : exactement celles de .deps (plus les rpmlib(...) ajoutés par rpm)
    grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$racine/paquets/rpm/$nom.deps" | LC_ALL=C sort > "$t/deps-attendues"
    rpm -qp --requires "$rpm" | grep -v '^rpmlib(' | LC_ALL=C sort > "$t/deps-reelles"
    diff "$t/deps-attendues" "$t/deps-reelles" >&2 || echec "$nom (h) : dépendances différentes de $nom.deps"
    echo "  ok : $nom ($attendu)"
}

echo "Test de fabriquer-rpm.sh"
tester_paquet wimao 0.5.0-1 0.5.0-1
tester_paquet wimao-wine 11.20+wimao1 11.20+wimao1-1
tester_paquet wimao-bridge 5.1.1+dxvk3.1.1-1 5.1.1+dxvk3.1.1-1

# (f) .deb inconnu (aucun fichier .deps) : refus
fabriquer_faux_deb inconnu 1.0-1 "$t/inconnu.deb"
if bash "$script" "$t/inconnu.deb" "$t/s-inconnu" >/dev/null 2>&1; then
    echec "(f) : un .deb inconnu a été accepté"
fi
echo "  ok : refus du .deb inconnu et de l'absence de SOURCE_DATE_EPOCH"
echo "OK"
