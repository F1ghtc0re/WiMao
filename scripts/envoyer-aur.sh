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
#             scripts/envoyer-aur.sh vX.Y.Z --a-blanc --aur <dépôt git local> --racine <dossier du dépôt>
#             scripts/envoyer-aur.sh --aide
#   vX.Y.Z             version à envoyer : exactement trois nombres, celle du pkgver de la recette (ex. v0.7.0).
#   --a-blanc          essai local (tests) : un dépôt git local tient lieu d'AUR, aucun réseau, aucun SSH ; ni
#                      contrôle de main ni de ~/.ssh/aur. Exige les deux options suivantes.
#   --aur <d>          (--a-blanc) dépôt git (nu) tenant lieu d'AUR ; il peut être vide, comme un nouveau paquet.
#   --racine <d>       (--a-blanc) dépôt git tenant lieu de racine : sa recette paquets/aur/wimao-bin/ est envoyée.
#
# Étapes :
#   1. préconditions : outils présents ; racine propre ; (réel) sur main, à jour avec origin/main, et clé
#      ~/.ssh/aur présente ;
#   2. recette : pkgver = la version demandée, aucune ligne d'essai local, .SRCINFO identique à celui que
#      produit makepkg --printsrcinfo à partir du PKGBUILD ;
#   3. clone du dépôt de l'AUR dans un dossier temporaire (réel : ssh://aur@aur.archlinux.org/wimao-bin.git avec
#      la clé ~/.ssh/aur seule et la clé d'hôte figée de paquets/aur/aur.archlinux.org.known_hosts ; rien n'est
#      écrit dans ~/.ssh) ; un dépôt vide (premier envoi) reçoit la branche master ;
#   4. copie de PKGBUILD et .SRCINFO, commit « wimao-bin X.Y.Z-R » (auteur : pseudonyme et adresse noreply,
#      jamais une adresse personnelle ; ni signature ni configuration git de la machine) et affichage du diff ;
#      rien à changer -> « rien à envoyer » ;
#   5. git push de master (réel : la fenêtre de phrase de passe de la clé SSH peut s'ouvrir : le script prévient) ;
#   6. relecture de ce que l'AUR publie (réel : https://aur.archlinux.org/cgit/aur.git/plain/…?h=wimao-bin ;
#      à blanc : le contenu du faux AUR), comparée octet pour octet à main.
#
# Variables : aucune. Lit : paquets/aur/wimao-bin/{PKGBUILD,.SRCINFO}, paquets/aur/aur.archlinux.org.known_hosts,
#             ~/.ssh/aur (lecture seule, réel). Écrit : dossiers temporaires supprimés en sortie ; l'AUR (réel).
# Code      : 0 = envoyé et relu, ou rien à envoyer ; 1 = contrôle échoué ou étape impossible ; 2 = usage.
# Dépend de : git, curl (réel), cmp, awk ; podman ou makepkg (régénération du .SRCINFO).
set -euo pipefail
# Langue neutre : tri, comparaisons et awk ne dépendent pas de la langue de la machine.
export LC_ALL=C

RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=paquets/aur/commun-aur.sh
source "$RACINE_DEPOT/paquets/aur/commun-aur.sh"

# Valeurs figées (voir SECURITY.md et docs/publier-une-version.md).
ADRESSE_AUR="ssh://aur@aur.archlinux.org/wimao-bin.git"
CLE_SSH_AUR="$HOME/.ssh/aur"
HOTES_CONNUS="$RACINE_DEPOT/paquets/aur/aur.archlinux.org.known_hosts"
URL_RELECTURE="https://aur.archlinux.org/cgit/aur.git/plain"
AUTEUR_NOM="F1ghtc0re"
AUTEUR_COURRIEL="162480684+F1ghtc0re@users.noreply.github.com"
MARQUE_ESSAI_LOCAL="# ESSAI LOCAL, NE PAS PUBLIER"

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
while [ "$#" -gt 0 ]; do
    case "$1" in
        --aide | -h | --help) aide; exit 0 ;;
        --a-blanc) a_blanc=1 ;;
        --aur | --racine)
            [ "$#" -ge 2 ] || erreur_usage "$1 attend une valeur"
            # Une valeur en « -… » serait prise pour une option par d'autres commandes.
            case "$2" in -*) erreur_usage "$1 : valeur invalide (commence par « - ») : $2" ;; esac
            if [ "$1" = "--aur" ]; then aur_essai="$2"; else racine_essai="$2"; fi
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
    [ -z "$aur_essai$racine_essai" ] || erreur_usage "--aur et --racine ne sont admis qu'avec --a-blanc (réservés aux essais)"
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
    # Git lance la commande SSH par `sh -c`, et ssh développe « % » et traite les blancs comme séparateurs :
    # on refuse ces chemins plutôt que de les laisser se couper.
    for chemin in "$CLE_SSH_AUR" "$HOTES_CONNUS"; do
        case "$chemin" in
            *[[:space:]%]*) arreter "le chemin « $chemin » contient un blanc ou un « % » : incompatible avec la commande SSH" ;;
        esac
    done
    [ -f "$HOTES_CONNUS" ] || arreter "$HOTES_CONNUS introuvable (clé d'hôte de l'AUR)"
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
! grep -qxF "$MARQUE_ESSAI_LOCAL" "$recette/PKGBUILD" \
    || arreter "le PKGBUILD porte la marque d'essai local : jamais à publier"
# Le .SRCINFO est ce que lit l'AUR : il doit être exactement celui que makepkg tire du PKGBUILD.
mkdir "$temporaire/controle"
cp -- "$recette/PKGBUILD" "$temporaire/controle/PKGBUILD"
srcinfo "$temporaire/controle" || arreter "impossible de régénérer le .SRCINFO"
cmp -s "$temporaire/controle/.SRCINFO" "$recette/.SRCINFO" \
    || arreter ".SRCINFO ne correspond pas au PKGBUILD (modifié à la main ?) : relancez scripts/publier-aur.sh"
echo "wimao-bin $pkgver-$pkgrel : pkgver conforme, .SRCINFO conforme au PKGBUILD."

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

# --- 4. Copie, commit, diff ---
echo "== 4. Commit"
cp -- "$recette/PKGBUILD" "$recette/.SRCINFO" "$clone/"
git -C "$clone" add -- PKGBUILD .SRCINFO
envoye=0
if [ -z "$(git -C "$clone" status --porcelain)" ] && git -C "$clone" rev-parse --verify --quiet HEAD > /dev/null; then
    echo "Rien à envoyer : l'AUR a déjà cette recette."
else
    # Configuration donnée sur la ligne de commande seulement : ce clone est un dépôt à part, qui ne doit
    # dépendre ni de l'identité ni de la signature de la machine.
    git -C "$clone" -c "user.name=$AUTEUR_NOM" -c "user.email=$AUTEUR_COURRIEL" -c commit.gpgsign=false \
        commit --quiet -m "wimao-bin $pkgver-$pkgrel"
    echo "Commit « wimao-bin $pkgver-$pkgrel » (auteur : $AUTEUR_NOM <$AUTEUR_COURRIEL>). Changements :"
    git -C "$clone" show --stat --format= HEAD
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
