#!/usr/bin/env bash
# Bibliothèque SOURCÉE (pas exécutée : sans bit d'exécution) des fonctions communes de vérification de
# signature, partagées par scripts/signer-version.sh, scripts/publier-depot-apt.sh et
# scripts/assembler-site-apt.sh. Elle n'a pas de `set -e` propre : elle hérite des options de l'appelant.
#
# Principe : toute signature n'est jamais jugée « valide » toute seule. Elle doit venir de LA clé dont
# l'empreinte est publiée dans SECURITY.md ; une signature correcte faite par une autre clé est refusée.
#
# Fonctions :
#   empreinte_publiee <racine du dépôt>
#   trousseau_cle_publiee <racine du dépôt> <empreinte> <fichier trousseau à créer>
#   verifier_signature <trousseau> <empreinte> <signature> [<fichier signé>]
# Dépend de : gpg, gpgv, awk, tr, grep, mktemp.

# empreinte_publiee : affiche l'empreinte (40 chiffres hexadécimaux, majuscules) de la clé de signature publiée
# dans <racine>/SECURITY.md, ligne « Empreinte de la clé de signature : XXXX XXXX … » (espaces admis). Rend 1
# avec un message si la ligne manque ou est mal formée : tant que la clé n'est pas créée et publiée, on ne signe pas.
empreinte_publiee() {
    local racine="$1" ligne valeur
    ligne="$(grep -m1 '^Empreinte de la clé de signature : ' "$racine/SECURITY.md" 2> /dev/null || true)"
    if [ -z "$ligne" ]; then
        echo "ERREUR : SECURITY.md ne publie pas encore l'empreinte de la clé de signature." >&2
        echo "  Ajoutez-y une ligne « Empreinte de la clé de signature : <40 chiffres hexadécimaux> » (voir docs/publier-une-version.md)." >&2
        return 1
    fi
    valeur="${ligne#Empreinte de la clé de signature : }"
    valeur="$(tr -d ' ' <<< "$valeur" | tr 'a-f' 'A-F')"
    if [[ ! "$valeur" =~ ^[0-9A-F]{40}$ ]]; then
        echo "ERREUR : l'empreinte publiée dans SECURITY.md n'a pas 40 chiffres hexadécimaux : « $ligne »" >&2
        return 1
    fi
    echo "$valeur"
}

# trousseau_cle_publiee : crée, dans <fichier trousseau>, le trousseau que `gpgv` utilisera, à partir de
# <racine>/docs/cle-publique.asc. Rend 1 avec un message si ce fichier manque, est illisible ou si sa clé
# principale n'a pas l'<empreinte> attendue (celle de SECURITY.md) : on ne fait confiance qu'à la clé publiée.
# L'import se fait dans un GNUPGHOME jetable, jamais dans le trousseau de l'utilisateur.
trousseau_cle_publiee() {
    local racine="$1" attendue="$2" trousseau="$3" cle="$1/docs/cle-publique.asc" jetable trouvee liste nombre statut=0
    if [ ! -f "$cle" ]; then
        echo "ERREUR : $cle introuvable (voir docs/publier-une-version.md)." >&2
        return 1
    fi
    jetable="$(mktemp -d)"
    chmod 700 "$jetable"
    # Le premier enregistrement « fpr » de la sortie est celui de la clé principale (avant ses sous-clés).
    liste="$(GNUPGHOME="$jetable" gpg --show-keys --with-colons "$cle" 2> /dev/null)" || true
    trouvee="$(awk -F: '$1 == "fpr" { print $10; exit }' <<< "$liste")"
    # Exactement UNE clé principale (enregistrement « pub ») : un fichier A puis B ferait entrer B dans le
    # trousseau, et une clé non publiée n'a rien à y faire.
    nombre="$(awk -F: '$1 == "pub" { n++ } END { print n + 0 }' <<< "$liste")"
    if [ "$nombre" -gt 1 ]; then
        echo "ERREUR : docs/cle-publique.asc contient $nombre clés principales ; une seule est admise (la clé publiée $attendue)." >&2
        statut=1
    elif [ "$trouvee" != "$attendue" ]; then
        echo "ERREUR : docs/cle-publique.asc contient la clé « ${trouvee:-illisible} », pas la clé publiée ($attendue)." >&2
        statut=1
    elif ! { GNUPGHOME="$jetable" gpg --batch --quiet --import "$cle" 2> /dev/null \
        && GNUPGHOME="$jetable" gpg --batch --export > "$trousseau" 2> /dev/null; }; then
        echo "ERREUR : impossible de construire le trousseau gpgv depuis docs/cle-publique.asc." >&2
        statut=1
    fi
    rm -rf "$jetable"
    return "$statut"
}

# verifier_signature : vérifie avec `gpgv` (et le seul <trousseau>) la <signature> d'un <fichier signé>, ou
# d'une signature en clair (clearsign) quand le fichier signé est omis. Rend 0 seulement si gpgv réussit ET
# rapporte une ligne « [GNUPG:] VALIDSIG » dont le DERNIER champ, l'empreinte de la clé principale, vaut
# <empreinte> (la signature elle-même peut venir d'une sous-clé, dont l'empreinte est le 3e champ). Sinon 1.
# gpgv rend 0 même pour une clé expirée ou révoquée (EXPKEYSIG, REVKEYSIG, EXPSIG, suivis d'un VALIDSIG) : on
# exige donc aussi une ligne GOODSIG, que ces cas n'émettent pas, et UNE SEULE ligne VALIDSIG (plusieurs
# signatures dans le même fichier : refus). Quand la sous-clé de signature réelle expirera, la vérification
# échouera : c'est voulu, il faudra alors la prolonger (docs/publier-une-version.md).
verifier_signature() {
    local trousseau="$1" attendue="$2" signature="$3" fichier="${4:-}" etat signataire nombre
    local -a args=(--keyring "$trousseau" --status-fd 1 "$signature")
    [ -z "$fichier" ] || args+=("$fichier")
    if ! etat="$(gpgv "${args[@]}" 2> /dev/null)"; then
        echo "ERREUR : signature invalide ou clé inconnue : $signature" >&2
        return 1
    fi
    if ! grep -q '^\[GNUPG:\] GOODSIG ' <<< "$etat"; then
        echo "ERREUR : $signature n'est pas une signature entièrement valide (clé expirée, révoquée ou signature expirée ?)." >&2
        return 1
    fi
    nombre="$(awk '$1 == "[GNUPG:]" && $2 == "VALIDSIG" { n++ } END { print n + 0 }' <<< "$etat")"
    if [ "$nombre" -ne 1 ]; then
        echo "ERREUR : $signature porte $nombre signatures valides ; une seule est admise." >&2
        return 1
    fi
    signataire="$(awk '$1 == "[GNUPG:]" && $2 == "VALIDSIG" { print $NF; exit }' <<< "$etat")"
    if [ "$signataire" != "$attendue" ]; then
        echo "ERREUR : $signature est signée par la clé « ${signataire:-inconnue} », pas par la clé publiée ($attendue)." >&2
        return 1
    fi
}
