#!/usr/bin/env bash
# Fabrique le paquet wimao_<version>-1_amd64.deb : l'application graphique (wimao), la ligne de
# commande (wimao-cli), les recettes, les traductions, les icônes, le .desktop et la fiche AppStream.
#
# Usage     : paquets/fabriquer-wimao.sh <dossier de sortie>
# Variables : SOURCE_DATE_EPOCH  date imposée aux fichiers (défaut : date du dernier commit)
# Écrit     : <sortie>/wimao_<version>-1_amd64.deb et un dossier de travail <sortie>/travail-wimao
#             (supprimé puis recréé à chaque fabrication). La compilation Rust écrit dans target/.
# Contenu   : /usr/bin/{wimao,wimao-cli}, /usr/share/wimao/recettes/*.toml,
#             /usr/share/locale/{fr,es}/LC_MESSAGES/wimao.mo, /usr/share/icons/hicolor/**,
#             /usr/share/applications/fr.wimao.WiMao.desktop,
#             /usr/share/metainfo/fr.wimao.WiMao.metainfo.xml,
#             /usr/share/doc/wimao/{copyright,changelog.Debian.gz} et
#             /usr/share/lintian/overrides/wimao.
#             Le recettes/README.md (documentation de développement) n'est pas livré.
# Version   : celle du workspace Cargo (lue avec `cargo metadata`, jamais écrite en dur), suivie de « -1 ».
# Dépend de : cargo, msgfmt (gettext), jq, strip, dpkg-deb et dpkg-shlibdeps (dpkg-dev).
#
# Choix : le paquet est assemblé avec construire_deb (comme wimao-wine et wimao-bridge) plutôt
# qu'avec cargo-deb : mêmes règles de reproductibilité, de changelog et de droits pour les trois
# paquets, et aucun outil supplémentaire à installer et figer. Les dépendances « $auto » sont
# calculées par dpkg-shlibdeps, comme le fait cargo-deb.
set -euo pipefail

# shellcheck source=paquets/commun.sh
source "$(dirname "${BASH_SOURCE[0]}")/commun.sh"

if [ "$#" -ne 1 ]; then
    echo "Usage : $0 <dossier de sortie>" >&2
    exit 2
fi
sortie="$(mkdir -p "$1" && cd "$1" && pwd)"
dossier_wimao="$RACINE_DEPOT/paquets/wimao"

date_source

# Version du paquet = version du workspace (une seule source de vérité : Cargo.toml).
version_workspace="$(cd "$RACINE_DEPOT" && cargo metadata --no-deps --format-version 1 --locked \
    | jq -r '.packages[] | select(.name == "wimao") | .version')"
if [ -z "$version_workspace" ] || [ "$version_workspace" = "null" ]; then
    echo "ERREUR : version du workspace introuvable" >&2
    exit 1
fi
VERSION_PAQUET="${version_workspace}-1"

# Le changelog Debian porte aussi la version (sa première ligne) : les deux doivent concorder,
# sinon le paquet annoncerait une version et son journal une autre.
version_changelog="$(dpkg-parsechangelog -l "$dossier_wimao/changelog" -SVersion)"
if [ "$version_changelog" != "$VERSION_PAQUET" ]; then
    echo "ERREUR : paquets/wimao/changelog annonce ${version_changelog}, Cargo.toml donne ${VERSION_PAQUET}." >&2
    echo "         Ajoutez une entrée au changelog (dch -v ${VERSION_PAQUET}) avant de fabriquer." >&2
    exit 1
fi

# --- Compilation ---
# Deux mécanismes complémentaires rendent le binaire indépendant du dossier de fabrication :
#  - WIMAO_PAQUET=1 pose le cfg `wimao_paquet` (build.rs de wimao, wimao-core et wimao-recettes) :
#    les replis de développement fondés sur env!("CARGO_MANIFEST_DIR") / OUT_DIR (icônes des
#    sources, recettes du dépôt, traductions de OUT_DIR) ne sont alors pas compilés du tout ;
#  - --remap-path-prefix retire les chemins que rustc insère de lui-même (dépôt, ~/.cargo,
#    chaîne Rust : messages de panique, informations de débogage).
# La vérification plus bas refuse tout chemin de la machine de fabrication restant dans un binaire.
racine_rust="$(rustc --print sysroot)"
(
    cd "$RACINE_DEPOT"
    export WIMAO_PAQUET=1
    export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=${RACINE_DEPOT}=/build/wimao --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/build/cargo --remap-path-prefix=${racine_rust}=/build/rust"
    cargo build --release --locked -p wimao -p wimao-cli
)
cible="$RACINE_DEPOT/target/release"

# --- Arborescence du paquet, reconstruite à neuf ---
travail="$sortie/travail-wimao"
racine_paquet="$travail/paquet"
rm -rf "$travail"
mkdir -p "$racine_paquet"/{DEBIAN,usr/bin,usr/share/wimao/recettes,usr/share/doc/wimao,usr/share/applications,usr/share/metainfo,usr/share/icons}

# Binaires, allégés (strip : exigence de lintian, résultat déterministe).
install -m 0755 "$cible/wimao" "$cible/wimao-cli" "$racine_paquet/usr/bin/"
strip --strip-unneeded "$racine_paquet/usr/bin/wimao" "$racine_paquet/usr/bin/wimao-cli"

# Recettes : les .toml seulement.
install -m 0644 "$RACINE_DEPOT"/recettes/*.toml "$racine_paquet/usr/share/wimao/recettes/"

# Traductions : le domaine gettext est « wimao » (crates/wimao-core/src/i18n.rs).
for po in "$RACINE_DEPOT"/po/*.po; do
    langue="$(basename "$po" .po)"
    mkdir -p "$racine_paquet/usr/share/locale/$langue/LC_MESSAGES"
    msgfmt --check -o "$racine_paquet/usr/share/locale/$langue/LC_MESSAGES/wimao.mo" "$po"
done

# Icônes (hicolor/scalable/apps et hicolor/symbolic/apps), .desktop et fiche AppStream.
cp -r "$RACINE_DEPOT/crates/wimao/data/icons/hicolor" "$racine_paquet/usr/share/icons/hicolor"
install -m 0644 "$RACINE_DEPOT/crates/wimao/data/fr.wimao.WiMao.desktop" "$racine_paquet/usr/share/applications/"
install -m 0644 "$RACINE_DEPOT/crates/wimao/data/fr.wimao.WiMao.metainfo.xml" "$racine_paquet/usr/share/metainfo/"

# Écarts volontaires à la charte Debian, avec leur justification (lisibles par lintian).
mkdir -p "$racine_paquet/usr/share/lintian/overrides"
install -m 0644 "$dossier_wimao/lintian-overrides" "$racine_paquet/usr/share/lintian/overrides/wimao"

# Documentation légale et journal (gzip -n : sans date ni nom, donc reproductible).
install -m 0644 "$dossier_wimao/copyright" "$racine_paquet/usr/share/doc/wimao/copyright"
gzip -9n -c "$dossier_wimao/changelog" > "$racine_paquet/usr/share/doc/wimao/changelog.Debian.gz"

# --- Dépendances sur les bibliothèques système (le « $auto » de cargo-deb) ---
# dpkg-shlibdeps exige un fichier debian/control dans le dossier courant : on en fabrique un
# minimal dans un dossier jetable.
analyse="$travail/analyse"
mkdir -p "$analyse/debian"
printf 'Source: wimao\n\nPackage: wimao\nArchitecture: amd64\n' > "$analyse/debian/control"
# Les avertissements de dpkg-shlibdeps restent visibles (stderr non masqué) pour le diagnostic.
dependances="$(cd "$analyse" && dpkg-shlibdeps -O -e"$racine_paquet/usr/bin/wimao" -e"$racine_paquet/usr/bin/wimao-cli" | sed -n 's/^shlibs:Depends=//p')"
# Une liste vide donnerait un paquet installable sans GTK ni libadwaita : on refuse.
if [ -z "$dependances" ]; then
    echo "ERREUR : dpkg-shlibdeps n'a relevé aucune dépendance de bibliothèque." >&2
    exit 1
fi
echo "Dépendances relevées : ${dependances}"

# --- Fichier control ---
cp "$dossier_wimao/control.in" "$racine_paquet/DEBIAN/control"
remplacer_marqueur "$racine_paquet/DEBIAN/control" "@VERSION@" "${VERSION_PAQUET}"
remplacer_marqueur "$racine_paquet/DEBIAN/control" "@DEPENDANCES@" "${dependances:+, ${dependances}}"
# Un marqueur @...@ resté dans le fichier produirait un paquet faux : on s'arrête.
if grep -q '@[A-Z]*@' "$racine_paquet/DEBIAN/control"; then
    echo "ERREUR : marqueur non remplacé dans DEBIAN/control" >&2
    exit 1
fi
chmod 0644 "$racine_paquet/DEBIAN/control"

# --- Aucun chemin de la machine de fabrication dans les binaires ---
# Contrôle strict : un chemin /home/, /__w/ (conteneur de la CI), /github/workspace ou le dossier
# du dépôt est une fuite (il rendrait le binaire dépendant du dossier de fabrication) = erreur.
for binaire in wimao wimao-cli; do
    # -o : n'afficher que le chemin trouvé (les chaînes du binaire sont collées les unes aux autres).
    # Le chemin du dépôt est cherché à part, avec grep -F : il pourrait contenir des caractères
    # spéciaux d'expression régulière (+, (, [ …) qui fausseraient un motif -E.
    chaines="$(strings -a "$racine_paquet/usr/bin/$binaire")"
    restes="$(
        {
            grep -oE '(/home/|/__w/|/github/workspace)[A-Za-z0-9_./-]*' <<< "$chaines" || true
            grep -oF -- "${RACINE_DEPOT}" <<< "$chaines" || true
        } | sort -u
    )"
    if [ -n "$restes" ]; then
        echo "ERREUR : chemins de la machine de fabrication dans $binaire :" >&2
        printf '%s\n' "$restes" | sed 's/^/  /' >&2
        exit 1
    fi
done

normaliser_arborescence "$racine_paquet"
construire_deb "$racine_paquet" "$sortie/wimao_${VERSION_PAQUET}_amd64.deb"
rm -rf "$travail"
echo "Paquet : $sortie/wimao_${VERSION_PAQUET}_amd64.deb"
