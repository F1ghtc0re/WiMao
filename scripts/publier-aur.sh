#!/usr/bin/env bash
# Prépare la recette AUR de WiMao (paquet wimao-bin) à partir d'une Release GitHub publiée, l'ESSAIE pour de
# vrai dans un Arch Linux jetable, puis ouvre une demande de fusion (PR). C'est la première des deux étapes de
# l'AUR (la seconde, scripts/envoyer-aur.sh, envoie la recette fusionnée vers l'AUR). Ce script ne fusionne
# jamais, ne signe rien et ne demande aucune phrase de passe : la signature qui compte est celle de la Release
# (SHA256SUMS.asc), déjà faite ; la recette l'exige et la vérifie chez chaque utilisateur.
#
# Ce qui est publié : SEULEMENT la recette (PKGBUILD et .SRCINFO, quelques Ko) dans paquets/aur/wimao-bin/.
# Elle pointe vers les .deb de la Release GitHub et en contient les empreintes SHA-256.
#
# Usage     : scripts/publier-aur.sh vX.Y.Z
#             scripts/publier-aur.sh vX.Y.Z --a-blanc --dossier <artefacts> --cle <empreinte> \
#                 --sortie <dossier de la recette> --racine <dossier du dépôt>
#             scripts/publier-aur.sh --aide
#   vX.Y.Z             étiquette de la Release : exactement trois nombres, celle de Cargo.toml (ex. v0.7.0).
#   --a-blanc          essai local (tests, CI) : aucun appel à gh ni à git, ni essai dans Arch (les .deb d'essai
#                      n'existent pas sur GitHub) ; arrêt une fois la recette écrite. Exige les quatre options
#                      suivantes. N'utilise jamais votre vraie clé : la signature est contrôlée avec --cle.
#   --dossier <d>      (--a-blanc) dossier tenant lieu de Release : SHA256SUMS et SHA256SUMS.asc.
#   --cle <empreinte>  (--a-blanc) empreinte de la clé (40 hexadécimaux) : remplace celle de SECURITY.md.
#   --sortie <d>       (--a-blanc) dossier tenant lieu de paquets/aur/wimao-bin/ (créé s'il manque).
#   --racine <d>       (--a-blanc) dossier tenant lieu de racine du dépôt : son docs/cle-publique.asc donne
#                      le trousseau de contrôle de gpgv.
#
# Étapes :
#   1. (réel) préconditions : outils présents ; dépôt propre, sur main, à jour avec origin/main ; la branche
#      aur-vX.Y.Z n'existe ni ici ni sur origin ; la Release vX.Y.Z existe et n'est PAS un brouillon ;
#   2. empreinte attendue : celle publiée dans SECURITY.md (ou --cle à blanc) ;
#   3. (réel) téléchargement de SHA256SUMS et SHA256SUMS.asc dans un dossier temporaire ; l'étiquette doit être
#      la version de Cargo.toml ; SHA256SUMS.asc doit être signé par la clé publiée (gpgv, trousseau tiré de
#      docs/cle-publique.asc, jamais de ~/.gnupg) ;
#   4. recette : paquets/fabriquer-pkgbuild.sh (les trois .deb de SHA256SUMS, validpgpkeys = la clé publiée),
#      construite dans un dossier temporaire ;
#   5. (réel) ESSAI dans le conteneur Arch figé (IMAGE_ARCH) : la clé est importée depuis docs/cle-publique.asc,
#      `makepkg` TÉLÉCHARGE les .deb depuis la Release et en vérifie les empreintes ET la signature, `pacman -U`
#      installe le paquet, `wimao-cli --version` répond la bonne version ;
#   6. la recette est copiée dans la sortie (jamais avant : un contrôle qui échoue la laisse intacte) ;
#   7. (réel) branche aur-vX.Y.Z, commit des SEULS fichiers de paquets/aur/wimao-bin/, envoi, `gh pr create`.
#
# Variables : aucune. Écrit : paquets/aur/wimao-bin/{PKGBUILD,.SRCINFO} (ou <sortie>/ à blanc). Dossiers
#             temporaires supprimés en sortie. Rien dans votre dossier personnel.
# Code      : 0 = recette prête (réel : PR ouverte) ; 1 = contrôle échoué ou étape impossible ; 2 = usage.
# Dépend de : gpg, gpgv, podman (essai et .SRCINFO), sha256sum, awk ; en mode réel git et gh. Le conteneur a
#             besoin d'un accès au réseau (mises à jour d'Arch et Release GitHub).
set -euo pipefail
# Langue neutre : tri, comparaisons et awk ne dépendent pas de la langue de la machine.
export LC_ALL=C

RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Dépôt GitHub qui porte les Releases.
DEPOT_GITHUB="F1ghtc0re/WiMao"
# shellcheck source=paquets/aur/commun-aur.sh
source "$RACINE_DEPOT/paquets/aur/commun-aur.sh"
# shellcheck source=scripts/lib/publication.sh
source "$RACINE_DEPOT/scripts/lib/publication.sh"

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
dossier=""
cle=""
sortie=""
racine_essai=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --aide | -h | --help) aide; exit 0 ;;
        --a-blanc) a_blanc=1 ;;
        --dossier | --cle | --sortie | --racine)
            [ "$#" -ge 2 ] || erreur_usage "$1 attend une valeur"
            # Une valeur en « -… » serait prise pour une option par d'autres commandes.
            case "$2" in -*) erreur_usage "$1 : valeur invalide (commence par « - ») : $2" ;; esac
            case "$1" in
                --dossier) dossier="$2" ;;
                --cle) cle="$2" ;;
                --sortie) sortie="$2" ;;
                --racine) racine_essai="$2" ;;
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
if [ "$a_blanc" -eq 1 ]; then
    [ -n "$dossier" ] || erreur_usage "--a-blanc exige --dossier (aucun téléchargement n'est fait à blanc)"
    [ -n "$cle" ] || erreur_usage "--a-blanc exige --cle (l'empreinte de la clé d'essai)"
    [ -n "$sortie" ] || erreur_usage "--a-blanc exige --sortie (la vraie recette n'est jamais touchée à blanc)"
    [ -n "$racine_essai" ] || erreur_usage "--a-blanc exige --racine (dossier dont docs/cle-publique.asc est la clé d'essai)"
else
    # En vraie publication, seuls SECURITY.md et le vrai dépôt comptent : aucun contournement.
    [ -z "$dossier$cle$sortie$racine_essai" ] \
        || erreur_usage "--dossier, --cle, --sortie et --racine ne sont admis qu'avec --a-blanc (réservés aux essais)"
fi

# Racine dont on lit docs/cle-publique.asc, et dossier de la recette à mettre à jour.
if [ "$a_blanc" -eq 1 ]; then
    racine_cle="$racine_essai"
    recette_finale="$sortie"
else
    racine_cle="$RACINE_DEPOT"
    recette_finale="$RACINE_DEPOT/paquets/aur/wimao-bin"
fi
nom_branche="aur-$version"

# --- Dossier temporaire et nettoyage ---
temporaire="$(mktemp -d)"
branche=""            # branche de la PR (mode réel), posée dès sa création
pr_ouverte=0          # passe à 1 quand la PR est ouverte : plus rien à défaire
# Sortie : supprime le temporaire ; si une étape a échoué APRÈS la création de la branche, AFFICHE la marche
# à suivre pour revenir à l'état de départ. Elle ne l'exécute jamais : rien n'est détruit sans que vous le voyiez.
nettoyer() {
    local code=$?
    rm -rf "$temporaire"
    if [ "$code" -ne 0 ] && [ -n "$branche" ] && [ "$pr_ouverte" -eq 0 ]; then
        {
            echo
            echo "La publication a échoué après la création de la branche $branche. Pour revenir à l'état de départ :"
            echo "  git switch main"
            echo "  git restore --source=HEAD --staged --worktree -- paquets/aur/wimao-bin"
            echo "  git clean -fd -- paquets/aur/wimao-bin"
            echo "  git branch -D $branche"
            echo "  git push origin --delete $branche   # seulement si l'envoi avait réussi"
        } >&2
    fi
}
trap nettoyer EXIT

# --- 1. Préconditions ---
echo "== 1. Préconditions"
outils=(gpg gpgv podman sha256sum awk)
[ "$a_blanc" -eq 1 ] || outils+=(git gh)
for outil in "${outils[@]}"; do
    command -v "$outil" > /dev/null || arreter "la commande $outil est introuvable"
done
if [ "$a_blanc" -eq 0 ]; then
    [ -z "$(git -C "$RACINE_DEPOT" status --porcelain)" ] \
        || arreter "le dépôt contient des modifications non enregistrées : commitez-les ou rangez-les d'abord"
    [ "$(git -C "$RACINE_DEPOT" branch --show-current)" = "main" ] \
        || arreter "vous n'êtes pas sur la branche main"
    git -C "$RACINE_DEPOT" fetch --quiet origin main || arreter "git fetch origin main a échoué"
    [ "$(git -C "$RACINE_DEPOT" rev-parse HEAD)" = "$(git -C "$RACINE_DEPOT" rev-parse origin/main)" ] \
        || arreter "main n'est pas à jour avec origin/main (git pull --ff-only)"
    # La branche de la PR ne doit exister ni ici ni sur origin : sinon une publication précédente a été laissée en route.
    if git -C "$RACINE_DEPOT" rev-parse --verify --quiet "refs/heads/$nom_branche" > /dev/null; then
        arreter "la branche locale $nom_branche existe déjà. Si elle est périmée : git branch -D $nom_branche"
    fi
    if git -C "$RACINE_DEPOT" ls-remote --exit-code --heads origin "$nom_branche" > /dev/null 2>&1; then
        arreter "la branche $nom_branche existe déjà sur origin. Si elle est périmée : git push origin --delete $nom_branche"
    fi
    # Une Release brouillon n'est pas téléchargeable par makepkg : on exige une Release PUBLIÉE.
    infos="$(gh release view "$version" --repo "$DEPOT_GITHUB" --json isDraft,publishedAt --jq '[.isDraft, .publishedAt] | @tsv')" \
        || arreter "la Release $version est introuvable sur $DEPOT_GITHUB"
    IFS=$'\t' read -r brouillon _ <<< "$infos"
    [ "$brouillon" = "false" ] \
        || arreter "la Release $version est encore un brouillon : relisez-la et publiez-la d'abord (docs/publier-une-version.md)"
else
    echo "À blanc : aucun appel à git ni à gh."
fi

# --- 2. Empreinte attendue ---
echo "== 2. Clé attendue"
if [ "$a_blanc" -eq 1 ]; then
    empreinte="$(tr -d ' ' <<< "$cle" | tr 'a-f' 'A-F')"
    [[ "$empreinte" =~ ^[0-9A-F]{40}$ ]] || erreur_usage "--cle : empreinte invalide (40 chiffres hexadécimaux attendus) : $cle"
    echo "À blanc : clé d'essai $empreinte (SECURITY.md ignoré)"
else
    empreinte="$(empreinte_publiee "$RACINE_DEPOT")" || exit 1
    echo "Clé de signature attendue (SECURITY.md) : $empreinte"
fi
# Trousseau de contrôle de gpgv : uniquement la clé publiée (docs/cle-publique.asc), jamais ~/.gnupg.
trousseau="$temporaire/trousseau.gpg"
trousseau_cle_publiee "$racine_cle" "$empreinte" "$trousseau" || exit 1

# --- 3. SHA256SUMS et sa signature ---
if [ "$a_blanc" -eq 0 ]; then
    echo "== 3. Téléchargement de SHA256SUMS ($version)"
    dossier="$temporaire/artefacts"
    mkdir -p "$dossier"
    gh release download "$version" --repo "$DEPOT_GITHUB" --dir "$dossier" \
        --pattern SHA256SUMS --pattern SHA256SUMS.asc \
        || arreter "téléchargement de la Release $version impossible"
else
    echo "== 3. À blanc : SHA256SUMS pris dans $dossier"
fi
[ -d "$dossier" ] || arreter "dossier d'artefacts introuvable : $dossier"
dossier="$(cd "$dossier" && pwd)"
for fichier in SHA256SUMS SHA256SUMS.asc; do
    [ -f "$dossier/$fichier" ] || arreter "$dossier/$fichier introuvable (la Release $version est-elle signée ?)"
done
# L'étiquette est la version de WiMao : celle de Cargo.toml (fabriquer-pkgbuild.sh y compare le .deb de wimao).
version_cargo="$(awk -F'"' '/^version = / { print $2; exit }' "$RACINE_DEPOT/Cargo.toml")"
[ "v$version_cargo" = "$version" ] \
    || arreter "l'étiquette $version n'est pas la version de Cargo.toml (v$version_cargo) : la recette est celle de ce dépôt"
verifier_signature "$trousseau" "$empreinte" "$dossier/SHA256SUMS.asc" "$dossier/SHA256SUMS" \
    || arreter "SHA256SUMS n'est pas signé par la clé publiée ($empreinte)"
echo "SHA256SUMS est signé par la clé publiée."

# --- 4. Recette ---
echo "== 4. Recette (fabriquer-pkgbuild.sh)"
recette="$temporaire/recette"
"$RACINE_DEPOT/paquets/fabriquer-pkgbuild.sh" "$dossier/SHA256SUMS" "$recette" --cle "$empreinte" \
    || arreter "la fabrication de la recette a échoué (voir ci-dessus)"
[ -f "$recette/PKGBUILD" ] && [ -f "$recette/.SRCINFO" ] || arreter "la recette est incomplète"
# Jamais de recette d'essai local dans ce qu'on publie.
! grep -qF "$MARQUE_ESSAI_LOCAL" "$recette/PKGBUILD" || arreter "la recette porte la marque d'essai local"

# --- 5. Essai réel dans Arch ---
if [ "$a_blanc" -eq 0 ]; then
    echo "== 5. Essai dans Arch Linux (conteneur jetable) : makepkg, pacman -U, wimao-cli --version"
    echo "makepkg télécharge les .deb depuis la Release (87 Mo) : cela prend quelques minutes."
    journaux="$temporaire/journaux"
    mkdir -p "$journaux"
    cp -- "$racine_cle/docs/cle-publique.asc" "$temporaire/cle-publique.asc"
    # Montages : la bibliothèque (fonctions d'essai), la recette et la clé en lecture seule, les journaux en
    # écriture (:Z pour SELinux). Dans le conteneur on est root : la clé est copiée lisible par « constructeur »,
    # qui l'importe dans SON trousseau (makepkg y cherche la signature). La recette est copiée par
    # construire_installer_recette : les sources ne sont PAS fournies, makepkg les télécharge.
    if ! podman run --rm \
        -v "$DOSSIER_LIB_AUR:/lib-aur:ro,Z" -v "$recette:/recette:ro,Z" \
        -v "$temporaire/cle-publique.asc:/cle-publique.asc:ro,Z" -v "$journaux:/journaux:Z" \
        "$IMAGE_ARCH" bash -euo pipefail -c '
            source /lib-aur/commun-aur.sh
            pacman -Syu --noconfirm --needed > /journaux/pacman.log 2>&1 || { cat /journaux/pacman.log; echec "pacman -Syu a échoué"; }
            utilisateur_constructeur
            importer_cle_constructeur /cle-publique.asc
            construire_installer_recette /recette /journaux
            attendu="wimao-cli $1"
            obtenu="$(env -i LANG=C.UTF-8 PATH=/usr/bin:/bin wimao-cli --version)"
            echo "wimao-cli --version : $obtenu"
            [ "$obtenu" = "$attendu" ] || echec "wimao-cli --version rend « $obtenu », attendu « $attendu »"
        ' _ "$version_cargo"; then
        arreter "l'essai dans Arch a échoué (journaux ci-dessus ; la recette n'est pas publiée)"
    fi
    echo "Essai réussi : la recette télécharge, vérifie, construit et installe WiMao $version_cargo."
else
    echo "== 5. À blanc : pas d'essai dans Arch."
fi

# --- 6. Mise en place de la recette ---
# Tout est bon : on installe le résultat dans le dépôt (jamais avant). En mode réel, la branche de la PR est
# créée AVANT d'écrire dans paquets/aur/wimao-bin/ : main n'est jamais modifié, et en cas d'échec le
# nettoyage affiche comment revenir en arrière.
if [ "$a_blanc" -eq 0 ]; then
    echo "== 6. Branche et recette"
    git -C "$RACINE_DEPOT" switch -c "$nom_branche"
    branche="$nom_branche"
fi
mkdir -p "$recette_finale"
cp -- "$recette/PKGBUILD" "$recette/.SRCINFO" "$recette_finale/"

if [ "$a_blanc" -eq 1 ]; then
    echo "== 6. À blanc : ni branche, ni commit, ni PR. Terminé."
    exit 0
fi

# --- 7. Commit et PR ---
echo "== 7. Commit et demande de fusion (PR)"
git -C "$RACINE_DEPOT" add -- paquets/aur/wimao-bin/PKGBUILD paquets/aur/wimao-bin/.SRCINFO
git -C "$RACINE_DEPOT" commit -m "AUR : recette wimao-bin $version"
git -C "$RACINE_DEPOT" push -u origin "$branche"
# Corps de la PR : .deb de cette version avec leurs empreintes SHA-256.
corps="$temporaire/corps.md"
{
    echo "Met à jour la recette AUR de wimao-bin pour la version $version (sources vérifiées par la clé $empreinte)."
    echo
    echo "La recette a été essayée dans Arch Linux (makepkg avec téléchargement et contrôle de la signature,"
    echo "pacman -U, wimao-cli --version). Les .deb de cette version :"
    echo
    awk '{ n = $2; sub(/^\*/, "", n); if (n ~ /\.deb$/) print "- `" n "` : SHA-256 `" $1 "`" }' "$dossier/SHA256SUMS"
    echo
    echo "Après la fusion : scripts/envoyer-aur.sh $version (depuis votre machine, avec la clé SSH de l'AUR)."
} > "$corps"
gh pr create --repo "$DEPOT_GITHUB" --base main --head "$branche" \
    --title "AUR : recette wimao-bin $version" --body-file "$corps"
pr_ouverte=1
echo "PR ouverte. Relisez-la, attendez la CI, puis fusionnez-la vous-même."
echo "Vous êtes sur la branche $branche ; « git switch main » pour revenir."
