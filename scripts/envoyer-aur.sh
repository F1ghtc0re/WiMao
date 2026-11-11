#!/usr/bin/env bash
# Envoie vers l'AUR (paquet wimao-bin) la recette fusionnée sur main. C'est la seconde étape de l'AUR (la
# première, scripts/publier-aur.sh, prépare la recette et ouvre la PR). À lancer depuis VOTRE machine, après la
# fusion de la PR : l'envoi utilise la clé SSH dédiée à l'AUR (~/.ssh/aur), qui ne va jamais sur GitHub ni en CI.
#
# Risque : l'AUR sert sans relecture ce que le compte du mainteneur y envoie. Les protections de ce script :
# il n'envoie que PKGBUILD et .SRCINFO tels qu'ils sont sur main (à jour, propre), la clé d'hôte de
# aur.archlinux.org est figée dans le dépôt (aucune confiance au premier contact) et le contenu publié est relu.
#
# Usage     : scripts/envoyer-aur.sh vX.Y.Z
#             scripts/envoyer-aur.sh vX.Y.Z --a-blanc --aur <dépôt git local> --racine <dossier du dépôt> --cle <empreinte>
#             scripts/envoyer-aur.sh --aide
#   vX.Y.Z             version à envoyer : exactement trois nombres, celle du pkgver de la recette (ex. v0.7.0).
#   --a-blanc          essai local (tests) : un dépôt git local tient lieu d'AUR, aucun réseau, aucun SSH ; ni
#                      contrôle de main ni de ~/.ssh/aur. Exige les deux options suivantes.
#   --aur <d>          (--a-blanc) dépôt git (nu) tenant lieu d'AUR ; il peut être vide, comme un nouveau paquet.
#   --racine <d>       (--a-blanc) dépôt git tenant lieu de racine : sa recette paquets/aur/wimao-bin/ est envoyée.
#   --cle <empreinte>  (--a-blanc) empreinte (40 hexadécimaux) que doit porter validpgpkeys, à la place de celle
#                      de SECURITY.md (le faux dépôt n'en a pas) ; sans elle, la racine doit avoir un SECURITY.md.
#
# Étapes :
#   1. préconditions : outils présents ; racine propre ; (réel) sur main, à jour avec origin/main, et clé
#      ~/.ssh/aur présente ;
#   2. recette (lue dans le commit HEAD) : pkgver = la version demandée, aucune marque d'essai local, .SRCINFO
#      identique à celui que produit makepkg --printsrcinfo ; dernière barrière, propriétés de sécurité :
#      validpgpkeys = l'empreinte publiée dans SECURITY.md, exactement 5 sources (Release GitHub de la version,
#      dont SHA256SUMS.asc), aucun `install=`, prepare() qui vérifie `sha256sum -c SHA256SUMS` ;
#   3. clone du dépôt de l'AUR dans un dossier temporaire (réel : ssh://aur@aur.archlinux.org/wimao-bin.git avec
#      la clé ~/.ssh/aur seule et la clé d'hôte figée de paquets/aur/aur.archlinux.org.known_hosts ; rien n'est
#      écrit dans ~/.ssh) ; un dépôt vide (premier envoi) reçoit la branche master ;
#   4. suppression de tout autre fichier suivi du dépôt AUR (seuls PKGBUILD et .SRCINFO y ont leur place), copie
#      de PKGBUILD et .SRCINFO, commit « wimao-bin X.Y.Z-R » (auteur : pseudonyme et adresse noreply,
#      jamais une adresse personnelle ; ni signature ni configuration git de la machine) et affichage du diff
#      complet ; rien à changer (et aucun fichier en trop) -> « rien à envoyer » ;
#   5. git push de master (réel : la fenêtre de phrase de passe de la clé SSH peut s'ouvrir : le script prévient) ;
#   6. relecture de ce que l'AUR publie (réel : https://aur.archlinux.org/cgit/aur.git/plain/…?h=wimao-bin ;
#      à blanc : le contenu du faux AUR), comparée octet pour octet à main.
#
# Variables : aucune. Lit : (dans le commit HEAD) paquets/aur/wimao-bin/{PKGBUILD,.SRCINFO}, SECURITY.md et
#             paquets/aur/aur.archlinux.org.known_hosts ; ~/.ssh/aur (lecture seule, réel). Écrit : dossiers temporaires supprimés en sortie ; l'AUR (réel).
# Code      : 0 = envoyé et relu, ou rien à envoyer ; 1 = contrôle échoué ou étape impossible ; 2 = usage.
# Dépend de : git, curl (réel), cmp, awk ; podman ou makepkg (régénération du .SRCINFO).
set -euo pipefail
# Langue neutre : tri, comparaisons et awk ne dépendent pas de la langue de la machine.
export LC_ALL=C

RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=paquets/aur/commun-aur.sh
source "$RACINE_DEPOT/paquets/aur/commun-aur.sh"
# shellcheck source=scripts/lib/publication.sh
source "$RACINE_DEPOT/scripts/lib/publication.sh"

# Valeurs figées (voir SECURITY.md et docs/publier-une-version.md).
ADRESSE_AUR="ssh://aur@aur.archlinux.org/wimao-bin.git"
CLE_SSH_AUR="$HOME/.ssh/aur"
# Clé d'hôte figée : relue dans le commit HEAD (copie extraite dans le dossier temporaire, voir l'étape 1).
CHEMIN_HOTES_CONNUS="paquets/aur/aur.archlinux.org.known_hosts"
HOTES_CONNUS=""
URL_RELECTURE="https://aur.archlinux.org/cgit/aur.git/plain"
AUTEUR_NOM="F1ghtc0re"
AUTEUR_COURRIEL="162480684+F1ghtc0re@users.noreply.github.com"

# --- Aide et erreurs ---

# Affiche le bloc de commentaires d'en-tête (de « Usage » à « Dépend de ») comme aide.
aide() {
    sed -n '/^# Usage/,/^# Dépend de/p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# arreter <message> : erreur en français sur la sortie d'erreur, arrêt (code 1).
arreter() {
    echo "ERREUR : $1" >&2
    exit 1
}

# erreur_usage <message> : erreur d'utilisation (code 2) avec rappel de l'aide.
erreur_usage() {
    echo "ERREUR : $1" >&2
    echo "Aide : $0 --aide" >&2
    exit 2
}

# --- Arguments ---
version=""
a_blanc=0
aur_essai=""
racine_essai=""
cle_essai=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --aide | -h | --help) aide; exit 0 ;;
        --a-blanc) a_blanc=1 ;;
        --aur | --racine | --cle)
            [ "$#" -ge 2 ] || erreur_usage "$1 attend une valeur"
            # Une valeur en « -… » serait prise pour une option par d'autres commandes.
            case "$2" in -*) erreur_usage "$1 : valeur invalide (commence par « - ») : $2" ;; esac
            case "$1" in
                --aur) aur_essai="$2" ;;
                --racine) racine_essai="$2" ;;
                --cle) cle_essai="$2" ;;
            esac
            shift
            ;;
        -*) erreur_usage "option inconnue : $1" ;;
        *)
            [ -z "$version" ] || erreur_usage "une seule version attendue (reçu : $version et $1)"
            version="$1"
            ;;
    esac
    shift
done

[ -n "$version" ] || erreur_usage "version manquante (ex. v0.7.0)"
[[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || erreur_usage "version « $version » invalide (forme attendue : vX.Y.Z, trois nombres, ex. v0.7.0)"
numero="${version#v}"
if [ "$a_blanc" -eq 1 ]; then
    [ -n "$aur_essai" ] || erreur_usage "--a-blanc exige --aur (le dépôt git local qui tient lieu d'AUR)"
    [ -n "$racine_essai" ] || erreur_usage "--a-blanc exige --racine (le dépôt qui contient la recette)"
else
    # En vrai envoi, seuls le vrai dépôt et le vrai AUR comptent : aucun contournement.
    [ -z "$aur_essai$racine_essai$cle_essai" ] || erreur_usage "--aur, --racine et --cle ne sont admis qu'avec --a-blanc (réservés aux essais)"
fi

if [ "$a_blanc" -eq 1 ]; then
    [ -d "$racine_essai" ] || arreter "dossier --racine introuvable : $racine_essai"
    racine="$(cd "$racine_essai" && pwd)"
    depot_aur="$aur_essai"
else
    racine="$RACINE_DEPOT"
    depot_aur="$ADRESSE_AUR"
fi
recette="$racine/paquets/aur/wimao-bin"

# --- Dossier temporaire et nettoyage ---
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

# --- 1. Préconditions ---
echo "== 1. Préconditions"
outils=(git cmp awk)
[ "$a_blanc" -eq 1 ] || outils+=(curl ssh)
for outil in "${outils[@]}"; do
    command -v "$outil" > /dev/null || arreter "la commande $outil est introuvable"
done
git -C "$racine" rev-parse --git-dir > /dev/null 2>&1 || arreter "$racine n'est pas un dépôt git"
[ -z "$(git -C "$racine" status --porcelain)" ] \
    || arreter "le dépôt contient des modifications non enregistrées : on n'envoie que ce qui est sur main"
if [ "$a_blanc" -eq 0 ]; then
    [ "$(git -C "$racine" branch --show-current)" = "main" ] || arreter "vous n'êtes pas sur la branche main"
    git -C "$racine" fetch --quiet origin main || arreter "git fetch origin main a échoué"
    [ "$(git -C "$racine" rev-parse HEAD)" = "$(git -C "$racine" rev-parse origin/main)" ] \
        || arreter "main n'est pas à jour avec origin/main (git pull --ff-only)"
    [ -f "$CLE_SSH_AUR" ] || arreter "la clé SSH de l'AUR ($CLE_SSH_AUR) est absente. Pour la créer :
  ssh-keygen -t ed25519 -f ~/.ssh/aur -C \"wimao-aur\"
puis collez ~/.ssh/aur.pub dans votre profil sur https://aur.archlinux.org (voir docs/publier-une-version.md)"
    # La clé d'hôte figée est lue dans le COMMIT (comme la recette), jamais dans le dossier de travail.
    HOTES_CONNUS="$temporaire/known_hosts"
    git -C "$racine" show "HEAD:$CHEMIN_HOTES_CONNUS" > "$HOTES_CONNUS" 2> /dev/null \
        || arreter "$CHEMIN_HOTES_CONNUS est absent du commit HEAD (clé d'hôte de l'AUR)"
    [ -s "$HOTES_CONNUS" ] || arreter "$CHEMIN_HOTES_CONNUS est vide dans le commit HEAD"
    # Git lance la commande SSH par `sh -c`, et ssh développe « % » et traite les blancs comme séparateurs :
    # on refuse ces chemins plutôt que de les laisser se couper.
    for chemin in "$CLE_SSH_AUR" "$HOTES_CONNUS"; do
        case "$chemin" in
            *[[:space:]%]*) arreter "le chemin « $chemin » contient un blanc ou un « % » : incompatible avec la commande SSH" ;;
        esac
    done
else
    echo "À blanc : pas de contrôle de main ni de ~/.ssh/aur, pas de réseau."
fi

# --- 2. Recette ---
echo "== 2. Recette (paquets/aur/wimao-bin)"
# La recette est lue dans le COMMIT (HEAD, déjà prouvé identique à origin/main en réel), jamais dans le dossier de
# travail : celui-ci peut différer de git sans que `git status` le montre (skip-worktree, assume-unchanged,
# fichier ignoré, filtres .gitattributes). Tous les contrôles, la copie vers l'AUR et la relecture utilisent
# cette copie extraite, octet pour octet.
recette_git="$recette"
recette="$temporaire/main"
mkdir "$recette"
for fichier in PKGBUILD .SRCINFO; do
    git -C "$racine" show "HEAD:paquets/aur/wimao-bin/$fichier" > "$recette/$fichier" 2> /dev/null \
        || arreter "la recette est absente de $recette_git ($fichier) : la PR de scripts/publier-aur.sh est-elle fusionnée ?"
done
pkgver="$(awk -F= '$1 == "pkgver" { print $2; exit }' "$recette/PKGBUILD")"
pkgrel="$(awk -F= '$1 == "pkgrel" { print $2; exit }' "$recette/PKGBUILD")"
[ "$pkgver" = "$numero" ] \
    || arreter "le PKGBUILD annonce pkgver=$pkgver et non $numero : la recette de $version n'est pas sur main"
[[ "$pkgrel" =~ ^[0-9]+$ ]] || arreter "pkgrel illisible dans le PKGBUILD (« $pkgrel »)"
# Recherche par sous-chaîne : une marque suivie d'un espace ou de texte est refusée aussi.
! grep -qF "$MARQUE_ESSAI_LOCAL" "$recette/PKGBUILD" \
    || arreter "le PKGBUILD porte la marque d'essai local : jamais à publier"
# Dernière barrière (avant la régénération du .SRCINFO, qui échouerait sur un install= absent) : propriétés de sécurité de la recette extraite de HEAD (positives : on vérifie ce qui DOIT
# y être, pas seulement l'absence de ce qu'on a pensé à interdire).
# (a) validpgpkeys = l'empreinte publiée dans SECURITY.md (du commit HEAD ; à blanc : --cle).
if [ "$a_blanc" -eq 1 ] && [ -n "$cle_essai" ]; then
    empreinte="$cle_essai"
else
    mkdir "$temporaire/secu"
    git -C "$racine" show "HEAD:SECURITY.md" > "$temporaire/secu/SECURITY.md" 2> /dev/null \
        || arreter "SECURITY.md est absent du commit HEAD : impossible de connaître l'empreinte de la clé"
    empreinte="$(empreinte_publiee "$temporaire/secu")" || arreter "empreinte de la clé illisible dans SECURITY.md"
fi
[[ "$empreinte" =~ ^[0-9A-F]{40}$ ]] || arreter "empreinte attendue mal formée : $empreinte"
cles_pkgbuild="$(sed -n 's/^validpgpkeys=(\(.*\))[[:space:]]*$/\1/p' "$recette/PKGBUILD" | tr -d "'\"")"
[ "$cles_pkgbuild" = "$empreinte" ] \
    || arreter "validpgpkeys du PKGBUILD (« $cles_pkgbuild ») n'est pas l'empreinte publiée ($empreinte) : recette refusée"
# (b) exactement cinq sources, toutes sur la Release de CETTE version, dont la signature de SHA256SUMS.
prefixe_sources="https://github.com/F1ghtc0re/WiMao/releases/download/v$pkgver/"
[ "$(grep -c '^[[:space:]]*source = ' "$recette/.SRCINFO")" -eq 5 ] \
    || arreter ".SRCINFO : il faut exactement 5 lignes « source = » (trois .deb, SHA256SUMS, SHA256SUMS.asc)"
while IFS= read -r ligne; do
    case "$ligne" in
        "source = $prefixe_sources"*) ;;
        *) arreter ".SRCINFO : source hors de la Release v$pkgver de F1ghtc0re/WiMao : $ligne" ;;
    esac
done < <(sed -n 's/^[[:space:]]*\(source = .*\)$/\1/p' "$recette/.SRCINFO")
grep -q '^[[:space:]]*source = .*/SHA256SUMS\.asc$' "$recette/.SRCINFO" \
    || arreter ".SRCINFO : la source SHA256SUMS.asc (signature de la Release) manque"
# (c) aucun script d'installation (il tournerait en root à pacman -U).
! grep -q '^[[:space:]]*install = ' "$recette/.SRCINFO" || arreter ".SRCINFO déclare un script install= : refusé"
! grep -Eq '^[[:space:]]*install=' "$recette/PKGBUILD" || arreter "le PKGBUILD déclare install= : refusé"
# (d) prepare() vérifie les empreintes des sources.
grep -q 'sha256sum -c SHA256SUMS' "$recette/PKGBUILD" \
    || arreter "le PKGBUILD ne vérifie pas les empreintes (sha256sum -c SHA256SUMS absent de prepare())"

# Le .SRCINFO est ce que lit l'AUR : il doit être exactement celui que makepkg tire du PKGBUILD.
mkdir "$temporaire/controle"
cp -- "$recette/PKGBUILD" "$temporaire/controle/PKGBUILD"
srcinfo "$temporaire/controle" || arreter "impossible de régénérer le .SRCINFO"
cmp -s "$temporaire/controle/.SRCINFO" "$recette/.SRCINFO" \
    || arreter ".SRCINFO ne correspond pas au PKGBUILD (modifié à la main ?) : relancez scripts/publier-aur.sh"

echo "wimao-bin $pkgver-$pkgrel : pkgver conforme, .SRCINFO conforme au PKGBUILD, clé, sources et vérifications conformes."

# --- 3. Clone du dépôt de l'AUR ---
echo "== 3. Clone de l'AUR"
clone="$temporaire/aur"
if [ "$a_blanc" -eq 0 ]; then
    # Clé dédiée seule (pas d'agent : IdentitiesOnly), clé d'hôte figée et exigée : aucun
    # « trust on first use », rien dans ~/.ssh/config ni ~/.ssh/known_hosts.
    # Exclusif : -F /dev/null ignore ~/.ssh/config et /etc/ssh/ssh_config (HostName, IdentityFile,
    # KnownHostsCommand…) ; GlobalKnownHostsFile=/dev/null ignore /etc/ssh/ssh_known_hosts ; UpdateHostKeys=no
    # interdit d'ajouter des clés d'hôte. L'agent SSH reste joignable (variable SSH_AUTH_SOCK, pas la config).
    # Git lance cette commande par `sh -c` : les chemins sont protégés par printf %q (refus des blancs et « % » :
    # voir les préconditions).
    cle_q="$(printf '%q' "$CLE_SSH_AUR")"
    hotes_q="$(printf '%q' "$HOTES_CONNUS")"
    GIT_SSH_COMMAND="ssh -F /dev/null -i $cle_q -o IdentitiesOnly=yes -o UserKnownHostsFile=$hotes_q -o GlobalKnownHostsFile=/dev/null -o UpdateHostKeys=no -o StrictHostKeyChecking=yes"
    export GIT_SSH_COMMAND
fi
git clone --quiet "$depot_aur" "$clone" || arreter "le clone de $depot_aur a échoué"
# Premier envoi : le dépôt de l'AUR est vide (aucune branche). L'AUR n'accepte que master : on la crée ici
# (ou on s'y place si elle existe déjà).
git -C "$clone" checkout --quiet -B master
# L'AUR ne doit contenir QUE PKGBUILD et .SRCINFO : tout autre fichier suivi (source ajoutée, script...) est
# retiré ici ; il serait sinon conservé tel quel dans le dépôt et pourrait être servi aux utilisateurs.
if git -C "$clone" rev-parse --verify --quiet HEAD > /dev/null; then
    while IFS= read -r -d '' suivi; do
        case "$suivi" in
            PKGBUILD | .SRCINFO) ;;
            *)
                echo "Fichier en trop sur l'AUR, retiré : $suivi"
                git -C "$clone" rm --quiet -r -f -- "$suivi"
                ;;
        esac
    done < <(git -C "$clone" ls-files -z)
fi

# --- 4. Copie, commit, diff ---
echo "== 4. Commit"
cp -- "$recette/PKGBUILD" "$recette/.SRCINFO" "$clone/"
git -C "$clone" add -- PKGBUILD .SRCINFO
# Contrôle : le dépôt AUR ne suivra que ces deux fichiers (y compris quand il n'y a « rien à envoyer »).
[ "$(git -C "$clone" ls-files | LC_ALL=C sort | paste -sd' ')" = ".SRCINFO PKGBUILD" ] \
    || arreter "le dépôt AUR ne serait pas limité à PKGBUILD et .SRCINFO : envoi annulé"
envoye=0
if [ -z "$(git -C "$clone" status --porcelain)" ] && git -C "$clone" rev-parse --verify --quiet HEAD > /dev/null; then
    echo "Rien à envoyer : l'AUR a déjà cette recette."
else
    # Configuration donnée sur la ligne de commande seulement : ce clone est un dépôt à part, qui ne doit
    # dépendre ni de l'identité ni de la signature de la machine.
    git -C "$clone" -c "user.name=$AUTEUR_NOM" -c "user.email=$AUTEUR_COURRIEL" -c commit.gpgsign=false \
        commit --quiet -m "wimao-bin $pkgver-$pkgrel"
    echo "Commit « wimao-bin $pkgver-$pkgrel » (auteur : $AUTEUR_NOM <$AUTEUR_COURRIEL>). Changements :"
    # Diff complet (pas seulement --stat) : vous voyez exactement ce qui remplace le contenu actuel de l'AUR.
    git -C "$clone" show --stat --format= HEAD
    git -C "$clone" show --format= HEAD
    envoye=1
fi

# --- 5. Envoi ---
if [ "$envoye" -eq 1 ]; then
    echo "== 5. Envoi (git push)"
    if [ "$a_blanc" -eq 0 ]; then
        echo "Attention : une fenêtre de phrase de passe de la clé SSH (~/.ssh/aur) peut s'ouvrir. Saisissez-la pour continuer."
    fi
    git -C "$clone" push --quiet origin master || arreter "git push a échoué (clé SSH refusée ? phrase de passe annulée ?)"
    echo "Envoyé."
else
    echo "== 5. Pas d'envoi"
fi

# --- 6. Relecture ---
echo "== 6. Relecture de ce que l'AUR publie"
# Commit exact du clone : l'adresse cgit porte cet identifiant, ce qui évite une réponse en cache d'avant l'envoi.
sha_envoye="$(git -C "$clone" rev-parse HEAD)"
for fichier in PKGBUILD .SRCINFO; do
    publie="$temporaire/relu-$fichier"
    if [ "$a_blanc" -eq 1 ]; then
        git -C "$depot_aur" show "master:$fichier" > "$publie" || arreter "$fichier absent de master dans $depot_aur"
    else
        # cgit peut mettre quelques secondes à refléter l'envoi : trois essais.
        ok=0
        for _ in 1 2 3; do
            if curl -fsSL -o "$publie" "$URL_RELECTURE/$fichier?h=wimao-bin&id=$sha_envoye" && cmp -s "$publie" "$recette/$fichier"; then
                ok=1
                break
            fi
            sleep 5
        done
        [ "$ok" -eq 1 ] || arreter "$fichier publié sur l'AUR diffère de main (ou illisible) : vérifiez https://aur.archlinux.org/packages/wimao-bin"
    fi
    cmp -s "$publie" "$recette/$fichier" || arreter "$fichier publié sur l'AUR diffère de main"
    echo "ok : $fichier identique à main."
done
echo "Terminé : wimao-bin $pkgver-$pkgrel est sur l'AUR."
