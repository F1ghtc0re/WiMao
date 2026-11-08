#!/usr/bin/env bash
# Publie une version de WiMao dans le dépôt dnf signé (dossier depot-dnf/) : vérifie la Release GitHub
# publiée, construit l'index dnf (createrepo_c, dans un conteneur Fedora 45 figé), le signe avec VOTRE clé
# OpenPGP, puis ouvre une demande de fusion (PR). C'est le pendant Fedora de scripts/publier-depot-apt.sh : il
# vient juste après lui dans docs/publier-une-version.md. Le site (GitHub Pages) n'est déployé que quand vous
# fusionnez la PR ; ce script ne fusionne jamais.
#
# Ce qui est publié : SEULEMENT l'index (quelques Ko), jamais les RPM. L'index pointe vers les RPM de la
# Release GitHub (createrepo_c --location-prefix) et contient l'empreinte SHA-256 de chacun ; c'est lui qui
# est signé (repomd.xml.asc). Un RPM falsifié ou détourné est donc refusé par dnf (repo_gpgcheck=1).
#
# Usage     : scripts/publier-depot-dnf.sh vX.Y.Z
#             scripts/publier-depot-dnf.sh vX.Y.Z --a-blanc --dossier <artefacts> --cle <empreinte> \
#                 --sortie <dossier depot-dnf> --racine <dossier du dépôt> [--adresse-rpm http://127.0.0.1:PORT/]
#             scripts/publier-depot-dnf.sh --aide
#   vX.Y.Z             étiquette de la Release à publier : exactement trois nombres (ex. v0.5.0).
#   --a-blanc          essai local (tests, CI) : aucun appel à gh ni à git, arrêt après le contrôle de
#                      l'étape 7 (ni branche, ni commit, ni PR). Exige les quatre options suivantes. N'appelle
#                      jamais votre vraie clé : la signature utilise la clé d'essai de --cle.
#   --dossier <d>      (--a-blanc) dossier tenant lieu de Release : les .rpm, SHA256SUMS, SHA256SUMS.asc.
#   --cle <empreinte>  (--a-blanc) empreinte de la clé (40 hexadécimaux) : remplace celle de SECURITY.md.
#   --sortie <d>       (--a-blanc) dossier tenant lieu de depot-dnf/ (créé s'il manque, sinon mis à jour).
#   --racine <d>       (--a-blanc) dossier tenant lieu de racine du dépôt : son docs/cle-publique.asc donne
#                      le trousseau de contrôle de gpgv. Même choix de conception que publier-depot-apt.sh.
#   --adresse-rpm <u>  (--a-blanc, facultatif) remplace l'adresse des Releases GitHub dans l'index par
#                      http://127.0.0.1:PORT/ (forme stricte, la machine locale seulement). Sert à l'essai de
#                      bout en bout, où dnf télécharge les RPM d'un second serveur local
#                      (scripts/tests/tester-depot-dnf-bout-en-bout.sh). Refusée hors --a-blanc, et un index
#                      ainsi produit est REFUSÉ par l'assemblage du site : il ne peut pas être publié.
#
# Étapes :
#   1. (réel) préconditions : outils présents ; dépôt propre, sur main, à jour avec origin/main ; la Release
#      vX.Y.Z existe et n'est PAS un brouillon (publiez-la d'abord) ; date de publication de la Release lue ;
#   2. empreinte attendue : celle publiée dans SECURITY.md (ou --cle à blanc) ;
#   3. (réel) téléchargement des .rpm, de SHA256SUMS et de SHA256SUMS.asc dans un dossier temporaire ;
#   4. vérification : SHA256SUMS.asc doit être signé par la clé publiée (gpgv, trousseau tiré de
#      docs/cle-publique.asc, jamais de ~/.gnupg) ; les lignes « .rpm » de SHA256SUMS (qui liste aussi les
#      .deb, non téléchargés ici) sont au nombre de TROIS et vérifiées par `sha256sum -c` ; les .rpm présents
#      sont exactement ceux listés, et ce sont wimao, wimao-bridge et wimao-wine ;
#   5. index : `createrepo_c --location-prefix <adresse de la Release> --revision <date de publication>
#      --set-timestamp-to-revision` dans le conteneur Fedora 45 (image figée par empreinte). La date des RPM
#      copiés est ramenée à la même date : l'index ne dépend ni de l'heure de l'exécution ni de celle du
#      téléchargement, relancer la même version redonne les mêmes octets ;
#   6. signature de repomd.xml avec la sous-clé de signature (détachée, ASCII) : repomd.xml.asc. La phrase de
#      passe est demandée par GnuPG/pinentry : ce script ne la lit ni ne la reçoit jamais ;
#   7. contrôle local : gpgv accepte repomd.xml.asc ; la cohérence de l'index est relue (lire-index-dnf.py) ;
#      CHAQUE adresse de l'index est exactement <adresse de la Release><nom du RPM> ; taille et SHA-256 de
#      chaque RPM dans l'index sont ceux des fichiers vérifiés à l'étape 4 ;
#   8. (réel) branche depot-dnf-vX.Y.Z (refusée si elle existe déjà, ici ou sur origin), commit des SEULS
#      fichiers de depot-dnf/, envoi, `gh pr create`.
#   Tout est construit dans un dossier temporaire : un contrôle qui échoue laisse depot-dnf/ intact.
#
# Variables : GNUPGHOME, GPG_TTY  lus par GnuPG (trousseau personnel de la signature, terminal de la phrase
#             de passe). SOURCE_DATE_EPOCH (à blanc seulement) : date de l'index ; à défaut, l'heure actuelle.
#             En mode réel la date est celle de la publication de la Release et la variable est ignorée.
# Écrit     : depot-dnf/rpm/repodata/* (ou <sortie>/rpm/repodata/* à blanc), qui REMPLACE l'index précédent :
#             l'index ne décrit que la dernière version. depot-dnf/wimao.repo (fichier fixe) n'est pas touché.
#             Dossiers temporaires supprimés en sortie. Rien dans votre dossier personnel.
# Code      : 0 = publié (PR ouverte ; à blanc : index prêt et contrôlé) ; 1 = contrôle échoué ou étape
#             impossible ; 2 = usage.
# Dépend de : gpg, gpgv, podman, python3, zstd, sha256sum, awk ; en mode réel git et gh. Le conteneur a besoin
#             d'un accès au réseau (il installe createrepo_c depuis les dépôts de Fedora).
set -euo pipefail
# Langue neutre : tri, comparaisons et awk ne dépendent pas de la langue de la machine.
export LC_ALL=C

RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Dépôt GitHub qui porte les Releases.
DEPOT_GITHUB="F1ghtc0re/WiMao"
# Image Fedora 45 figée par empreinte (la même que paquets.yml) : elle fournit createrepo_c.
IMAGE_FEDORA="registry.fedoraproject.org/fedora:45@sha256:4a8a63ec2df9f26a96c35ff380237ad578cf42e326b979d9b5801c31c6daf2e8"
# Sous-clé de signature de la clé publiée (SECURITY.md). Le « ! » qui suit à la signature l'oblige à signer
# avec CETTE sous-clé et pas une autre ; le contrôle de l'étape 7 vérifie ensuite qu'elle appartient bien à
# la clé principale publiée.
SOUS_CLE_SIGNATURE="5F1D4F6002B80C81"
# Motif d'un nom de RPM de WiMao : <paquet>-<version>-1.x86_64.rpm. La version n'a pas de « - » (la « Release »
# RPM est toujours 1, voir paquets/fabriquer-rpm.sh). Le nom entre tel quel dans une adresse de téléchargement.
MOTIF_RPM='^(wimao|wimao-bridge|wimao-wine)-([0-9][A-Za-z0-9.+~_^]*)-1\.x86_64\.rpm$'
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
adresse_essai=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --aide | -h | --help) aide; exit 0 ;;
        --a-blanc) a_blanc=1 ;;
        --dossier | --cle | --sortie | --racine | --adresse-rpm)
            [ "$#" -ge 2 ] || erreur_usage "$1 attend une valeur"
            # Une valeur en « -… » serait prise pour une option par d'autres commandes.
            case "$2" in -*) erreur_usage "$1 : valeur invalide (commence par « - ») : $2" ;; esac
            case "$1" in
                --dossier) dossier="$2" ;;
                --cle) cle="$2" ;;
                --sortie) sortie="$2" ;;
                --racine) racine_essai="$2" ;;
                --adresse-rpm) adresse_essai="$2" ;;
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

[ -n "$version" ] || erreur_usage "version manquante (ex. v0.5.0)"
[[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || erreur_usage "version « $version » invalide (forme attendue : vX.Y.Z, trois nombres, ex. v0.5.0)"
if [ "$a_blanc" -eq 1 ]; then
    [ -n "$dossier" ] || erreur_usage "--a-blanc exige --dossier (aucun téléchargement n'est fait à blanc)"
    [ -n "$cle" ] || erreur_usage "--a-blanc exige --cle (l'empreinte de la clé d'essai)"
    [ -n "$sortie" ] || erreur_usage "--a-blanc exige --sortie (le vrai depot-dnf/ n'est jamais touché à blanc)"
    [ -n "$racine_essai" ] || erreur_usage "--a-blanc exige --racine (dossier dont docs/cle-publique.asc est la clé d'essai)"
    # Adresse d'essai : la machine locale et rien d'autre, sinon un essai pourrait fabriquer un index qui
    # pointe ailleurs que vers les Releases.
    if [ -n "$adresse_essai" ]; then
        [[ "$adresse_essai" =~ ^http://127\.0\.0\.1:[0-9]{1,5}/$ ]] \
            || erreur_usage "--adresse-rpm : forme http://127.0.0.1:PORT/ exigée (reçu : $adresse_essai)"
    fi
else
    # En vraie publication, seuls SECURITY.md et le vrai dépôt comptent : aucun contournement.
    [ -z "$dossier$cle$sortie$racine_essai$adresse_essai" ] \
        || erreur_usage "--dossier, --cle, --sortie, --racine et --adresse-rpm ne sont admis qu'avec --a-blanc (réservés aux essais)"
fi

# Racine dont on lit docs/cle-publique.asc, et dossier du dépôt dnf à mettre à jour.
if [ "$a_blanc" -eq 1 ]; then
    racine_cle="$racine_essai"
    depot_dnf="$sortie"
else
    racine_cle="$RACINE_DEPOT"
    depot_dnf="$RACINE_DEPOT/depot-dnf"
fi
# Adresse où dnf ira chercher les RPM : le dossier de la Release (ou, à blanc seulement, l'adresse d'essai).
prefixe="https://github.com/$DEPOT_GITHUB/releases/download/$version/"
[ -z "$adresse_essai" ] || prefixe="$adresse_essai"

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
            echo "  git restore --source=HEAD --staged --worktree -- depot-dnf"
            echo "  git clean -fd -- depot-dnf"
            echo "  git branch -D $branche"
            echo "  git push origin --delete $branche   # seulement si l'envoi avait réussi"
        } >&2
    fi
}
trap nettoyer EXIT

# --- 1. Préconditions ---
echo "== 1. Préconditions"
outils=(gpg gpgv podman python3 zstd sha256sum awk)
[ "$a_blanc" -eq 1 ] || outils+=(git gh)
for outil in "${outils[@]}"; do
    command -v "$outil" > /dev/null || arreter "la commande $outil est introuvable"
done
epoque=""
if [ "$a_blanc" -eq 0 ]; then
    [ -z "$(git -C "$RACINE_DEPOT" status --porcelain)" ] \
        || arreter "le dépôt contient des modifications non enregistrées : commitez-les ou rangez-les d'abord"
    [ "$(git -C "$RACINE_DEPOT" branch --show-current)" = "main" ] \
        || arreter "vous n'êtes pas sur la branche main"
    git -C "$RACINE_DEPOT" fetch --quiet origin main || arreter "git fetch origin main a échoué"
    [ "$(git -C "$RACINE_DEPOT" rev-parse HEAD)" = "$(git -C "$RACINE_DEPOT" rev-parse origin/main)" ] \
        || arreter "main n'est pas à jour avec origin/main (git pull --ff-only)"
    [ -f "$depot_dnf/wimao.repo" ] || arreter "depot-dnf/wimao.repo est absent du dépôt"
    # La branche de la PR ne doit exister ni ici ni sur origin : sinon une publication précédente a été laissée en route.
    nom_branche="depot-dnf-$version"
    if git -C "$RACINE_DEPOT" rev-parse --verify --quiet "refs/heads/$nom_branche" > /dev/null; then
        arreter "la branche locale $nom_branche existe déjà. Si elle est périmée : git branch -D $nom_branche"
    fi
    if git -C "$RACINE_DEPOT" ls-remote --exit-code --heads origin "$nom_branche" > /dev/null 2>&1; then
        arreter "la branche $nom_branche existe déjà sur origin. Si elle est périmée : git push origin --delete $nom_branche"
    fi
    # Une Release brouillon n'est pas téléchargeable par la CI du site : on exige une Release PUBLIÉE.
    infos="$(gh release view "$version" --repo "$DEPOT_GITHUB" --json isDraft,publishedAt --jq '[.isDraft, .publishedAt] | @tsv')" \
        || arreter "la Release $version est introuvable sur $DEPOT_GITHUB"
    IFS=$'\t' read -r brouillon publiee <<< "$infos"
    [ "$brouillon" = "false" ] \
        || arreter "la Release $version est encore un brouillon : relisez-la et publiez-la d'abord (docs/publier-une-version.md)"
    epoque="$(date -u -d "$publiee" +%s 2> /dev/null)" || arreter "date de publication illisible : « $publiee »"
else
    echo "À blanc : aucun appel à git ni à gh."
    # Date de l'index à blanc : SOURCE_DATE_EPOCH si définie (essais reproductibles), sinon l'heure actuelle.
    epoque="${SOURCE_DATE_EPOCH:-$(date +%s)}"
fi
[[ "$epoque" =~ ^[0-9]+$ ]] || arreter "date de l'index invalide : « $epoque »"

# --- 2. Empreinte attendue ---
echo "== 2. Clé attendue"
if [ "$a_blanc" -eq 1 ]; then
    empreinte="$(tr -d ' ' <<< "$cle" | tr 'a-f' 'A-F')"
    [[ "$empreinte" =~ ^[0-9A-F]{40}$ ]] || erreur_usage "--cle : empreinte invalide (40 chiffres hexadécimaux attendus) : $cle"
    echo "À blanc : clé d'essai $empreinte (SECURITY.md ignoré)"
    cle_signature="$empreinte"
else
    empreinte="$(empreinte_publiee "$RACINE_DEPOT")" || exit 1
    echo "Clé de signature attendue (SECURITY.md) : $empreinte"
    cle_signature="${SOUS_CLE_SIGNATURE}!"
fi
# Trousseau de contrôle de gpgv : uniquement la clé publiée (docs/cle-publique.asc), jamais ~/.gnupg.
trousseau="$temporaire/trousseau.gpg"
trousseau_cle_publiee "$racine_cle" "$empreinte" "$trousseau" || exit 1

# --- 3. Artefacts ---
if [ "$a_blanc" -eq 0 ]; then
    echo "== 3. Téléchargement de la Release $version"
    dossier="$temporaire/artefacts"
    mkdir -p "$dossier"
    gh release download "$version" --repo "$DEPOT_GITHUB" --dir "$dossier" \
        --pattern '*.rpm' --pattern SHA256SUMS --pattern SHA256SUMS.asc \
        || arreter "téléchargement de la Release $version impossible"
else
    echo "== 3. À blanc : artefacts pris dans $dossier"
fi
[ -d "$dossier" ] || arreter "dossier d'artefacts introuvable : $dossier"
dossier="$(cd "$dossier" && pwd)"
for fichier in SHA256SUMS SHA256SUMS.asc; do
    [ -f "$dossier/$fichier" ] || arreter "$dossier/$fichier introuvable (la Release $version est-elle signée ?)"
done

# --- 4. Vérification ---
echo "== 4. Signature et sommes SHA-256"
verifier_signature "$trousseau" "$empreinte" "$dossier/SHA256SUMS.asc" "$dossier/SHA256SUMS" \
    || arreter "SHA256SUMS n'est pas signé par la clé publiée ($empreinte)"
# SHA256SUMS liste six paquets (trois .deb et trois .rpm) : seuls les .rpm nous concernent. On en extrait les
# lignes « .rpm » (un « * » devant le nom est la marque du mode binaire de sha256sum : on le retire pour le nom).
awk '{ n = $2; sub(/^\*/, "", n); if (n ~ /\.rpm$/) print }' "$dossier/SHA256SUMS" > "$temporaire/SHA256SUMS.rpm"
listes="$(awk '{ n = $2; sub(/^\*/, "", n); print n }' "$temporaire/SHA256SUMS.rpm" | sort)"
nombre_liste="$(grep -c . <<< "$listes" || true)"
[ "$nombre_liste" -eq 3 ] \
    || arreter "SHA256SUMS liste $nombre_liste RPM au lieu de trois (wimao, wimao-bridge, wimao-wine)"
presents="$(find "$dossier" -maxdepth 1 -name '*.rpm' -printf '%f\n' | sort)"
[ "$listes" = "$presents" ] || arreter "les .rpm du dossier ne sont pas exactement ceux de SHA256SUMS
  listés   : $(echo "$listes" | tr '\n' ' ')
  présents : $(echo "$presents" | tr '\n' ' ')"
# Noms sûrs : ils entrent tels quels dans l'adresse de téléchargement écrite dans l'index. Les trois paquets
# attendus, chacun une fois.
vus=""
while IFS= read -r nom; do
    [[ "$nom" =~ $MOTIF_RPM ]] \
        || arreter "nom de RPM inattendu dans la Release : « $nom » (forme <paquet>-<version>-1.x86_64.rpm, paquets wimao, wimao-bridge, wimao-wine)"
    vus+="${BASH_REMATCH[1]} "
done <<< "$presents"
[ "$(tr ' ' '\n' <<< "$vus" | grep -v '^$' | sort | tr '\n' ' ')" = "wimao wimao-bridge wimao-wine " ] \
    || arreter "il faut un RPM de chacun des trois paquets wimao, wimao-bridge et wimao-wine (reçu : $vus)"
(cd "$dossier" && sha256sum -c --strict "$temporaire/SHA256SUMS.rpm") || arreter "un .rpm ne correspond pas à SHA256SUMS (voir ci-dessus)"

# --- 5. Index ---
echo "== 5. Index dnf (createrepo_c, Fedora 45)"
# Copie de travail des seuls RPM, ramenés à la date de l'index : createrepo_c consigne la date des fichiers
# (« time file= » de primary), qui serait sinon celle du téléchargement.
travail="$temporaire/travail"
mkdir -p "$travail"
cp -- "$dossier"/*.rpm "$travail/"
touch -d "@$epoque" -- "$travail"/*.rpm
# Le dossier de travail est monté dans le conteneur (:Z pour SELinux). createrepo_c y crée repodata/. L'installation
# de createrepo_c est silencieuse ; en cas d'échec, tout le journal est affiché.
if ! podman run --rm -v "$travail:/d:Z" "$IMAGE_FEDORA" bash -euo pipefail -c '
    dnf install -y -q --setopt=install_weak_deps=False createrepo_c > /tmp/dnf.log 2>&1 || { cat /tmp/dnf.log; exit 1; }
    createrepo_c --location-prefix "$1" --revision "$2" --set-timestamp-to-revision /d
' _ "$prefixe" "$epoque" > "$temporaire/createrepo.log" 2>&1; then
    cat "$temporaire/createrepo.log" >&2
    arreter "la construction de l'index (podman, createrepo_c) a échoué"
fi
repodata="$travail/repodata"
[ -f "$repodata/repomd.xml" ] || arreter "createrepo_c n'a pas produit repomd.xml"

# --- 6. Signature ---
echo "== 6. Signature de repomd.xml"
echo "Une fenêtre de phrase de passe va s'ouvrir (une seule signature) ; elle attend environ une minute."
gpg --yes --local-user "$cle_signature" --armor --detach-sign --output "$repodata/repomd.xml.asc" "$repodata/repomd.xml" \
    || arreter "la signature de repomd.xml a échoué (phrase de passe refusée ou annulée ?)"

# --- 7. Contrôle local ---
echo "== 7. Contrôle local"
verifier_signature "$trousseau" "$empreinte" "$repodata/repomd.xml.asc" "$repodata/repomd.xml" \
    || arreter "repomd.xml.asc ne passe pas gpgv avec la clé publiée"
index="$(lire_index_dnf "$repodata")" || exit 1
nb_index=0
while IFS=$'\t' read -r href taille somme; do
    nom="${href##*/}"
    # L'adresse doit être EXACTEMENT <adresse de la Release><nom> : on la reconstruit et on compare.
    [ "$href" = "$prefixe$nom" ] || arreter "l'index annonce « $href » au lieu de « $prefixe$nom »"
    [ -f "$dossier/$nom" ] || arreter "l'index décrit un RPM inconnu : $nom"
    [ "$taille" = "$(stat -c %s -- "$dossier/$nom")" ] || arreter "$nom : taille de l'index ($taille) différente de celle du fichier"
    [ "$somme" = "$(sha256sum -- "$dossier/$nom" | cut -d' ' -f1)" ] || arreter "$nom : SHA-256 de l'index différent de celui du fichier"
    nb_index=$((nb_index + 1))
done <<< "$index"
[ "$nb_index" -eq 3 ] || arreter "l'index décrit $nb_index RPM au lieu de trois"
[ "$(cut -f1 <<< "$index" | sort -u | wc -l)" -eq 3 ] || arreter "l'index décrit deux fois le même RPM"
echo "Signature, adresses, tailles et empreintes concordent ($nb_index RPM)."

# Tout est bon : on installe le résultat dans le dépôt dnf (jamais avant). En mode réel, la branche de la PR
# est créée AVANT d'écrire dans depot-dnf/ : main n'est jamais modifié, et en cas d'échec le nettoyage affiche
# comment revenir en arrière.
if [ "$a_blanc" -eq 0 ]; then
    echo "== 8. Branche et demande de fusion (PR)"
    git -C "$RACINE_DEPOT" switch -c "$nom_branche"
    branche="$nom_branche"
fi
# L'ancien index est remplacé EN ENTIER (les noms de fichiers portent leur empreinte : un reste de l'ancien
# index ne serait plus décrit par repomd.xml, et l'assemblage du site le refuserait).
mkdir -p "$depot_dnf/rpm"
rm -rf "${depot_dnf:?}/rpm/repodata"
mkdir "$depot_dnf/rpm/repodata"
cp -- "$repodata"/* "$depot_dnf/rpm/repodata/"

if [ "$a_blanc" -eq 1 ]; then
    echo "== 8. À blanc : ni branche, ni commit, ni PR. Terminé."
    exit 0
fi
git -C "$RACINE_DEPOT" add -- depot-dnf
git -C "$RACINE_DEPOT" commit -m "Dépôt dnf : version $version"
git -C "$RACINE_DEPOT" push -u origin "$branche"
# Corps de la PR : RPM de cette version avec leurs empreintes SHA-256.
corps="$temporaire/corps.md"
{
    echo "Ajoute la version $version au dépôt dnf signé (index signé par la clé $empreinte)."
    echo
    echo "RPM de cette version (l'index pointe vers la Release, il ne les contient pas) :"
    echo
    (cd "$dossier" && sha256sum -- *.rpm | awk '{ print "- `" $2 "` : SHA-256 `" $1 "`" }')
    echo
    echo "Le site n'est déployé qu'à la fusion de cette PR (workflow depot-apt)."
} > "$corps"
gh pr create --repo "$DEPOT_GITHUB" --base main --head "$branche" \
    --title "Dépôt dnf : version $version" --body-file "$corps"
pr_ouverte=1
echo "PR ouverte. Relisez-la, attendez la CI, puis fusionnez-la vous-même."
echo "Vous êtes sur la branche $branche ; « git switch main » pour revenir."
