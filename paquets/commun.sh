#!/usr/bin/env bash
# Fonctions partagées par les scripts de fabrication des paquets .deb (paquets/fabriquer-*.sh).
#
# Ce fichier ne se lance pas : il se charge avec
#     source "$(dirname "$0")/commun.sh"
# (il ne fixe donc ni `set -e` ni autre option du shell appelant, qui doit déjà avoir
# `set -euo pipefail`).
#
# Fonctions fournies :
#   date_source                          exporte SOURCE_DATE_EPOCH (date du dernier commit) si absente
#   telecharger_verifie <url> <fichier> <sha256>
#                                        télécharge (curl) si besoin, puis vérifie l'empreinte
#   verifier_empreinte <fichier> <sha256>
#                                        s'arrête si l'empreinte SHA-256 ne correspond pas
#   normaliser_arborescence <dossier>    droits 0755/0644 et dates fixes, pour un paquet reproductible
#   remplacer_marqueur <fichier> <marqueur> <valeur>
#                                        remplace toutes les occurrences de <marqueur> par <valeur>, sans
#                                        interpréter la valeur (« | », « & », « \\ » y sont admis)
#   construire_deb <dossier> <sortie.deb>
#                                        assemble le paquet (propriétaire root:root, compression xz)
#
# Variables lues : SOURCE_DATE_EPOCH (facultative, voir date_source).
# Écrit : uniquement les chemins passés en arguments.

# Racine du dépôt (le dossier qui contient « paquets/ »), calculée une fois pour toutes.
RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Exporte SOURCE_DATE_EPOCH : la date du dernier commit, sauf si l'appelant l'a déjà fixée
# (la CI ou un essai à la main peuvent l'imposer). Toutes les dates du paquet en dérivent,
# ce qui rend deux fabrications successives identiques octet pour octet.
date_source() {
    if [ -z "${SOURCE_DATE_EPOCH:-}" ]; then
        SOURCE_DATE_EPOCH="$(git -C "$RACINE_DEPOT" log -1 --format=%ct)"
    fi
    export SOURCE_DATE_EPOCH
}

# verifier_empreinte <fichier> <sha256> : arrêt immédiat si l'empreinte diffère.
# Toute archive téléchargée passe par ici avant d'être utilisée.
verifier_empreinte() {
    local fichier="$1" attendue="$2" reelle
    reelle="$(sha256sum "$fichier" | cut -d' ' -f1)"
    if [ "$reelle" != "$attendue" ]; then
        echo "ERREUR : empreinte SHA-256 inattendue pour $fichier" >&2
        echo "  attendue : $attendue" >&2
        echo "  obtenue  : $reelle" >&2
        return 1
    fi
}

# telecharger_verifie <url> <fichier> <sha256> : télécharge <url> vers <fichier> si ce
# dernier n'existe pas déjà (cache), puis vérifie l'empreinte. Un fichier en cache dont
# l'empreinte est mauvaise est supprimé (il sera retéléchargé au prochain essai).
telecharger_verifie() {
    local url="$1" fichier="$2" sha256="$3"
    if [ ! -f "$fichier" ]; then
        mkdir -p "$(dirname "$fichier")"
        # Téléchargement vers un nom temporaire : un échec ne laisse pas de fichier tronqué dans le cache.
        curl -fsSL -o "$fichier.part" "$url"
        mv "$fichier.part" "$fichier"
    fi
    if ! verifier_empreinte "$fichier" "$sha256"; then
        rm -f "$fichier"
        return 1
    fi
}

# normaliser_arborescence <dossier> : dossiers en 0755, fichiers en 0644 (0755 s'ils étaient
# exécutables), toutes les dates ramenées à SOURCE_DATE_EPOCH (appeler date_source avant).
# Les liens symboliques ne sont pas suivis : seule leur date est fixée.
normaliser_arborescence() {
    local dossier="$1"
    find "$dossier" -type d -exec chmod 0755 {} +
    # Un fichier avec un bit d'exécution quelconque devient 0755, les autres 0644.
    find "$dossier" -type f -perm /111 -exec chmod 0755 {} +
    find "$dossier" -type f ! -perm /111 -exec chmod 0644 {} +
    find "$dossier" -exec touch -h -d "@${SOURCE_DATE_EPOCH}" {} +
}

# construire_deb <dossier> <sortie.deb> : assemble le paquet. --root-owner-group donne le
# propriétaire root:root sans être root ; -Zxz fixe la compression pour que le résultat
# ne dépende pas du réglage par défaut de la machine.
construire_deb() {
    local dossier="$1" sortie="$2"
    mkdir -p "$(dirname "$sortie")"
    dpkg-deb --root-owner-group -Zxz --build "$dossier" "$sortie"
}

# remplacer_marqueur <fichier> <marqueur> <valeur> : remplace en place chaque <marqueur> de <fichier>
# (ex. @DEPENDANCES@) par <valeur>. Substitution faite par le shell et non par sed : une valeur
# comme « a | b, c (>= 1) » casserait un `sed s|…|…|`. `patsub_replacement` (bash >= 5.2) est
# désactivé car il donnerait un sens spécial à « & » dans la valeur.
remplacer_marqueur() {
    local fichier="$1" marqueur="$2" valeur="$3" contenu
    shopt -u patsub_replacement 2>/dev/null || true
    contenu="$(cat "$fichier")"
    contenu="${contenu//"$marqueur"/"$valeur"}"
    printf '%s\n' "$contenu" > "$fichier"
}
