#!/usr/bin/env bash
# Réemballe un paquet .deb de WiMao en RPM (Fedora) : même contenu, mêmes chemins, aucune recompilation.
#
# Usage     : paquets/fabriquer-rpm.sh <paquet.deb> <dossier de sortie>
#             À lancer DANS un conteneur Fedora 45 (paquets : rpm-build, dpkg, zstd), par exemple
#             podman run --rm -v "$PWD:/d:Z" -e SOURCE_DATE_EPOCH=… <image Fedora 45> \
#                 bash /d/paquets/fabriquer-rpm.sh /d/sortie/wimao_0.5.0-1_amd64.deb /d/sortie
# Variables : SOURCE_DATE_EPOCH  OBLIGATOIRE : date imposée à la fabrication et aux fichiers. Aucune
#             valeur par défaut, pour que deux fabrications ne diffèrent jamais par oubli.
# Lit       : le .deb (nom, version, architecture et description via `dpkg-deb -f`) et, dans
#             paquets/rpm/, pour le paquet <nom> : <nom>.deps (dépendances), <nom>.recommends
#             (dépendances facultatives), <nom>.resume (résumé d'une ligne) et wimao.spec.in (modèle).
#             Un .deb sans fichier <nom>.deps est refusé : seuls les trois paquets de WiMao sont connus.
# Écrit     : <sortie>/<nom>-<version>-1.x86_64.rpm. Tout le reste (extraction, .spec, arborescence de
#             rpmbuild) vit dans un dossier temporaire supprimé en fin de script.
# Version   : celle du .deb sans son éventuel suffixe « -1 » (révision Debian) : 0.5.0-1 -> 0.5.0,
#             5.1.1+dxvk3.1.1-1 -> 5.1.1+dxvk3.1.1, 11.20+wimao1 -> 11.20+wimao1. La « Release » RPM est 1.
# Dépend de : dpkg-deb (dpkg), rpmbuild (rpm-build) et zstd, ainsi que des outils habituels (find, sort).
#
# Choix : on réemballe les .deb déjà publiés plutôt que de recompiler, de sorte que le logiciel installé
# soit rigoureusement le même sous Debian/Ubuntu et sous Fedora. Les réglages qui rendent le RPM
# reproductible sont dans le modèle paquets/rpm/wimao.spec.in, où ils sont commentés un par un.
set -euo pipefail

# shellcheck source=paquets/commun.sh
source "$(dirname "${BASH_SOURCE[0]}")/commun.sh"

if [ "$#" -ne 2 ]; then
    echo "Usage : $0 <paquet.deb> <dossier de sortie>" >&2
    exit 2
fi
fichier_deb="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
if [ ! -f "$fichier_deb" ]; then
    echo "ERREUR : $1 introuvable" >&2
    exit 1
fi
sortie="$(mkdir -p "$2" && cd "$2" && pwd)"
dossier_rpm="$RACINE_DEPOT/paquets/rpm"

# La date de fabrication est exigée explicitement (pas de repli sur le dernier commit, contrairement
# aux .deb) : un RPM sans date imposée ne serait pas reproductible.
if ! [[ "${SOURCE_DATE_EPOCH:-}" =~ ^[0-9]+$ ]]; then
    echo "ERREUR : SOURCE_DATE_EPOCH doit être défini (nombre de secondes depuis 1970)." >&2
    exit 1
fi
export SOURCE_DATE_EPOCH

# --- Identité du paquet, lue dans le .deb ---
nom="$(dpkg-deb -f "$fichier_deb" Package)"
version_deb="$(dpkg-deb -f "$fichier_deb" Version)"
architecture="$(dpkg-deb -f "$fichier_deb" Architecture)"
adresse="$(dpkg-deb -f "$fichier_deb" Homepage)"

# Seuls les paquets décrits dans paquets/rpm/ sont réemballés (liste de dépendances connue).
for suffixe in deps recommends resume; do
    if [ ! -f "$dossier_rpm/$nom.$suffixe" ]; then
        echo "ERREUR : paquet « $nom » inconnu : paquets/rpm/$nom.$suffixe est absent." >&2
        exit 1
    fi
done
if [ "$architecture" != "amd64" ]; then
    echo "ERREUR : $nom est en architecture « $architecture » ; seul amd64 (x86_64) est géré." >&2
    exit 1
fi

# Version RPM = version Debian moins le « -1 » final. Un « - » restant est interdit dans une version RPM.
version="${version_deb%-1}"
if [[ "$version" == *-* ]]; then
    echo "ERREUR : la version « $version_deb » contient un tiret que RPM refuse." >&2
    exit 1
fi

# Licence : celle indiquée dans le fichier copyright de chaque paquet (identifiants SPDX).
case "$nom" in
    wimao)        licence="GPL-3.0-or-later" ;;
    wimao-wine)   licence="LGPL-2.1-or-later" ;;
    wimao-bridge) licence="GPL-3.0-or-later AND Zlib" ;;
    *)
        echo "ERREUR : licence du paquet « $nom » non déclarée dans $0." >&2
        exit 1
        ;;
esac

# --- Dossier de travail temporaire, supprimé à la sortie quoi qu'il arrive ---
travail="$(mktemp -d)"
trap 'rm -rf "$travail"' EXIT
arborescence="$travail/arborescence"
mkdir -p "$arborescence"

# Extraction du contenu du .deb (droits et liens symboliques conservés).
dpkg-deb -x "$fichier_deb" "$arborescence"

# lire_liste <fichier> : affiche les lignes utiles d'un .deps/.recommends (sans commentaires ni lignes vides).
lire_liste() {
    grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$1" || true
}

# lignes_spec <étiquette> <fichier> : une ligne « Étiquette: valeur » du .spec par dépendance du fichier.
lignes_spec() {
    local etiquette="$1" ligne
    while IFS= read -r ligne; do
        printf '%s: %s\n' "$etiquette" "$ligne"
    done < <(lire_liste "$2")
}

# --- Liste %files, générée depuis l'arborescence ---
# Chaque chemin est entre guillemets (espaces admis) et chaque « % » est doublé (sinon rpm y verrait
# une macro). Seuls les dossiers propres au paquet sont déclarés (%dir) : jamais /usr/bin, /usr/share,
# etc., qui appartiennent au système. Les caractères que rpm interprète dans un nom de fichier (guillemet,
# antislash, jokers) ne sont pas gérés : le paquet est refusé plutôt que mal emballé.
liste_fichiers() {
    local chemin absolu echappe
    while IFS= read -r -d '' chemin; do
        absolu="/${chemin#./}"
        if [[ "$absolu" == *[\"\\*?\[\]]* || "$absolu" == *$'\n'* ]]; then
            echo "ERREUR : chemin non géré dans le paquet : $absolu" >&2
            return 1
        fi
        echappe="${absolu//%/%%}"
        if [ -d "$arborescence/$chemin" ] && [ ! -L "$arborescence/$chemin" ]; then
            case "$absolu" in
                /opt/wimao | /opt/wimao/* | /usr/share/wimao | /usr/share/wimao/* | "/usr/share/doc/$nom" | "/usr/share/doc/$nom/"*)
                    printf '%%dir "%s"\n' "$echappe" ;;
            esac
        else
            printf '"%s"\n' "$echappe"
        fi
    done < <(cd "$arborescence" && LC_ALL=C find . -mindepth 1 -print0 | LC_ALL=C sort -z)
}

# --- Description : le corps de la description Debian, sans la première ligne (le résumé) ---
# Format Debian : lignes de suite commençant par une espace, « . » seul pour une ligne vide.
# « % » est doublé pour ne pas être pris pour une macro.
description="$(dpkg-deb -f "$fichier_deb" Description | tail -n +2 | sed -e 's/^ //' -e 's/^\.$//' -e 's/%/%%/g')"

# --- Écriture du .spec depuis le modèle ---
# Les marqueurs simples d'abord, puis la description et la liste de fichiers (texte libre, qui
# pourrait contenir un marqueur) en dernier.
spec="$travail/$nom.spec"
cp "$dossier_rpm/wimao.spec.in" "$spec"
remplacer_marqueur "$spec" "@NOM@" "$nom"
remplacer_marqueur "$spec" "@VERSION@" "$version"
remplacer_marqueur "$spec" "@RESUME@" "$(head -n 1 "$dossier_rpm/$nom.resume")"
remplacer_marqueur "$spec" "@LICENCE@" "$licence"
remplacer_marqueur "$spec" "@URL@" "$adresse"
remplacer_marqueur "$spec" "@DEPENDANCES@" "$(lignes_spec Requires "$dossier_rpm/$nom.deps")"
remplacer_marqueur "$spec" "@RECOMMANDES@" "$(lignes_spec Recommends "$dossier_rpm/$nom.recommends")"
remplacer_marqueur "$spec" "@DESCRIPTION@" "$description"
remplacer_marqueur "$spec" "@FICHIERS@" "$(liste_fichiers)"

# --- Fabrication ---
# HOME est redirigé pour qu'aucun ~/.rpmmacros de la machine ne modifie le résultat. Le dossier source
# est transmis au %install du .spec par l'environnement. Ni signature ni vérification des sources ici.
export WIMAO_ARBORESCENCE="$arborescence"
HOME="$travail" rpmbuild -bb "$spec" \
    --target x86_64 \
    --define "_topdir $travail/rpmbuild" \
    --define "_rpmdir $sortie"

echo "RPM écrit : $sortie/$nom-$version-1.x86_64.rpm"
