#!/usr/bin/env bash
# Publie une version de WiMao dans le dépôt APT signé (dossier depot-apt/) : vérifie la Release GitHub
# publiée, construit l'index APT, le signe avec VOTRE clé OpenPGP, puis ouvre une demande de fusion (PR).
# C'est l'étape « propriétaire » qui suit la publication de la Release (docs/publier-une-version.md) : le
# site (GitHub Pages) n'est déployé que quand vous fusionnez cette PR ; ce script ne fusionne jamais.
#
# Usage     : scripts/publier-depot-apt.sh vX.Y.Z
#             scripts/publier-depot-apt.sh vX.Y.Z --a-blanc --dossier <artefacts> --cle <empreinte> \
#                 --sortie <dossier depot-apt> --racine <dossier du dépôt>
#             scripts/publier-depot-apt.sh --aide
#   vX.Y.Z             étiquette de la Release à publier : exactement trois nombres (ex. v0.4.0).
#   --a-blanc          essai local (tests, CI) : aucun appel à gh ni à git, arrêt après le contrôle de
#                      l'étape 7 (ni branche, ni commit, ni PR). Exige les quatre options suivantes.
#   --dossier <d>      (--a-blanc) dossier tenant lieu de Release : les .deb, SHA256SUMS, SHA256SUMS.asc.
#   --cle <empreinte>  (--a-blanc) empreinte de la clé (40 hexadécimaux) : remplace celle de SECURITY.md.
#   --sortie <d>       (--a-blanc) dossier tenant lieu de depot-apt/ (créé s'il manque, sinon mis à jour).
#   --racine <d>       (--a-blanc) dossier tenant lieu de racine du dépôt : son docs/cle-publique.asc donne
#                      le trousseau de contrôle de gpgv. Choix de conception : un test fournit ainsi sa
#                      propre clé jetable SANS toucher au vrai dépôt ni à ~/.gnupg, et la règle « le
#                      trousseau vient toujours de docs/cle-publique.asc » reste la même qu'en mode réel.
#
# Étapes :
#   1. (réel) préconditions : outils présents ; dépôt propre, sur main, à jour avec origin/main ; la Release
#      vX.Y.Z existe et n'est PAS un brouillon (publiez-la d'abord) ;
#   2. empreinte attendue : celle publiée dans SECURITY.md (ou --cle à blanc) ;
#   3. (réel) téléchargement des .deb, de SHA256SUMS et de SHA256SUMS.asc dans un dossier temporaire ;
#   4. vérification : SHA256SUMS.asc doit être signé par la clé publiée (gpgv, trousseau tiré de
#      docs/cle-publique.asc, jamais de ~/.gnupg) ; `sha256sum -c` ; les .deb présents sont exactement ceux
#      listés ;
#   5. index : `apt-ftparchive packages` sur pool/vX.Y.Z/, fusionné avec le Packages existant (relancer la
#      même version REMPLACE ses entrées au lieu de les dupliquer), trié par nom puis version (dpkg), puis
#      Packages.gz (gzip -9n) et Release (apt-ftparchive release, SHA256 seulement, pas de Valid-Until) ;
#   6. signature de Release avec la clé publiée : InRelease (clearsign) et Release.gpg (détachée). La phrase
#      de passe est demandée par GnuPG/pinentry : ce script ne la lit ni ne la reçoit jamais ;
#   7. contrôle local : gpgv accepte InRelease et Release.gpg, et les empreintes de Release correspondent
#      aux fichiers Packages* ;
#   8. (réel) branche depot-apt-vX.Y.Z, commit des SEULS fichiers de depot-apt/, envoi, `gh pr create`.
#   Tout est construit dans un dossier temporaire : un contrôle qui échoue laisse depot-apt/ intact.
#
# Variables : GNUPGHOME, GPG_TTY  lus par GnuPG (trousseau personnel de la signature, terminal de la phrase
#             de passe). Aucune autre.
# Écrit     : depot-apt/dists/stable/{Release,InRelease,Release.gpg,main/binary-amd64/{Packages,Packages.gz}}
#             (ou <sortie> à blanc) ; dossiers temporaires supprimés en sortie. Rien dans votre dossier personnel.
# Code      : 0 = publié (PR ouverte ; à blanc : index prêt et contrôlé) ; 1 = contrôle échoué ou étape
#             impossible ; 2 = usage.
# Dépend de : gpg, gpgv, apt-ftparchive (paquet apt-utils), dpkg, gzip, sha256sum ; en mode réel git et gh.
set -euo pipefail

RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Dépôt GitHub qui porte les Releases.
DEPOT_GITHUB="F1ghtc0re/WiMao"
# Fonctions communes de vérification de signature (empreinte_publiee, trousseau_cle_publiee, verifier_signature).
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

[ -n "$version" ] || erreur_usage "version manquante (ex. v0.4.0)"
[[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
    || erreur_usage "version « $version » invalide (forme attendue : vX.Y.Z, trois nombres, ex. v0.4.0)"
if [ "$a_blanc" -eq 1 ]; then
    [ -n "$dossier" ] || erreur_usage "--a-blanc exige --dossier (aucun téléchargement n'est fait à blanc)"
    [ -n "$cle" ] || erreur_usage "--a-blanc exige --cle (l'empreinte de la clé d'essai)"
    [ -n "$sortie" ] || erreur_usage "--a-blanc exige --sortie (le vrai depot-apt/ n'est jamais touché à blanc)"
    [ -n "$racine_essai" ] || erreur_usage "--a-blanc exige --racine (dossier dont docs/cle-publique.asc est la clé d'essai)"
else
    # En vraie publication, seuls SECURITY.md et le vrai dépôt comptent : aucun contournement.
    [ -z "$dossier$cle$sortie$racine_essai" ] \
        || erreur_usage "--dossier, --cle, --sortie et --racine ne sont admis qu'avec --a-blanc (réservés aux essais)"
fi

# Racine dont on lit docs/cle-publique.asc, et dossier du dépôt APT à mettre à jour.
if [ "$a_blanc" -eq 1 ]; then
    racine_cle="$racine_essai"
    depot_apt="$sortie"
else
    racine_cle="$RACINE_DEPOT"
    depot_apt="$RACINE_DEPOT/depot-apt"
fi

# --- Dossier temporaire et nettoyage ---
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

# --- 1. Préconditions ---
echo "== 1. Préconditions"
outils=(gpg gpgv apt-ftparchive dpkg gzip sha256sum)
[ "$a_blanc" -eq 1 ] || outils+=(git gh)
for outil in "${outils[@]}"; do
    command -v "$outil" > /dev/null \
        || arreter "la commande $outil est introuvable$([ "$outil" != apt-ftparchive ] || echo ' (sudo apt install apt-utils)')"
done
if [ "$a_blanc" -eq 0 ]; then
    [ -z "$(git -C "$RACINE_DEPOT" status --porcelain)" ] \
        || arreter "le dépôt contient des modifications non enregistrées : commitez-les ou rangez-les d'abord"
    [ "$(git -C "$RACINE_DEPOT" branch --show-current)" = "main" ] \
        || arreter "vous n'êtes pas sur la branche main"
    git -C "$RACINE_DEPOT" fetch --quiet origin main || arreter "git fetch origin main a échoué"
    [ "$(git -C "$RACINE_DEPOT" rev-parse HEAD)" = "$(git -C "$RACINE_DEPOT" rev-parse origin/main)" ] \
        || arreter "main n'est pas à jour avec origin/main (git pull --ff-only)"
    # Le dépôt APT ne publie que des Releases PUBLIÉES : un brouillon n'est pas téléchargeable par la CI du site.
    brouillon="$(gh release view "$version" --repo "$DEPOT_GITHUB" --json isDraft --jq .isDraft)" \
        || arreter "la Release $version est introuvable sur $DEPOT_GITHUB"
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

# --- 3. Artefacts ---
if [ "$a_blanc" -eq 0 ]; then
    echo "== 3. Téléchargement de la Release $version"
    dossier="$temporaire/artefacts"
    mkdir -p "$dossier"
    gh release download "$version" --repo "$DEPOT_GITHUB" --dir "$dossier" \
        --pattern '*.deb' --pattern SHA256SUMS --pattern SHA256SUMS.asc \
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
(cd "$dossier" && sha256sum -c SHA256SUMS) || arreter "un .deb ne correspond pas à SHA256SUMS (voir ci-dessus)"
# sha256sum -c ne regarde que les fichiers LISTÉS : un .deb de plus dans le dossier passerait inaperçu.
listes="$(awk '{print $2}' "$dossier/SHA256SUMS" | LC_ALL=C sort)"
presents="$(find "$dossier" -maxdepth 1 -name '*.deb' -printf '%f\n' | LC_ALL=C sort)"
[ "$listes" = "$presents" ] || arreter "les .deb du dossier ne sont pas exactement ceux de SHA256SUMS
  listés   : $(echo "$listes" | tr '\n' ' ')
  présents : $(echo "$presents" | tr '\n' ' ')"
[ -n "$presents" ] || arreter "aucun .deb dans la Release $version"
# Noms sûrs : ils entrent tels quels dans Filename (pool/vX.Y.Z/<nom>) puis dans une adresse de téléchargement.
while IFS= read -r nom; do
    [[ "$nom" =~ ^[a-z0-9][a-z0-9.+-]*_[A-Za-z0-9.+~:-]+_amd64\.deb$ ]] \
        || arreter "nom de paquet inattendu dans la Release : « $nom » (forme <paquet>_<version>_amd64.deb)"
done <<< "$presents"

# --- 5. Index ---
echo "== 5. Index APT"
# Arbre de travail : copie du dépôt APT existant (si présent) + les .deb de cette version sous pool/vX.Y.Z/.
travail="$temporaire/travail"
indices="dists/stable/main/binary-amd64"
mkdir -p "$travail/$indices" "$travail/pool/$version"
if [ -f "$depot_apt/$indices/Packages" ]; then
    cp "$depot_apt/$indices/Packages" "$travail/Packages.ancien"
else
    : > "$travail/Packages.ancien"
fi
cp "$dossier"/*.deb "$travail/pool/$version/"
# Packages ne garde que SHA256 (et Size) : ni MD5sum, ni SHA1, ni SHA512, comme le demande la spécification.
options_packages=(-o APT::FTPArchive::MD5=false -o APT::FTPArchive::SHA1=false -o APT::FTPArchive::SHA512=false)
(cd "$travail" && apt-ftparchive "${options_packages[@]}" packages "pool/$version") > "$travail/Packages.nouveau" \
    || arreter "apt-ftparchive packages a échoué"

# Découpe un fichier Packages en un fichier par paragraphe : <dossier>/<préfixe>NNNNN.
decouper() {
    local fichier="$1" cible="$2" prefixe="$3"
    mkdir -p "$cible"
    awk -v RS='' -v dir="$cible" -v pre="$prefixe" '{ f = sprintf("%s/%s%05d", dir, pre, NR); print $0 > f; close(f) }' "$fichier"
}
paragraphes="$travail/paragraphes"
decouper "$travail/Packages.ancien" "$paragraphes" "a"
decouper "$travail/Packages.nouveau" "$paragraphes" "n"

# champ <fichier> <nom> : valeur du champ de contrôle <nom> d'un paragraphe.
champ() {
    awk -v n="$2" 'index($0, n ": ") == 1 { print substr($0, length(n) + 3); exit }' "$1"
}

# Entrées retenues : toutes sauf les anciennes de pool/vX.Y.Z/ (remplacées par les nouvelles).
declare -a fichiers=() noms=() versions=() chemins=()
for f in "$paragraphes"/*; do
    [ -e "$f" ] || continue
    chemin="$(champ "$f" Filename)"
    case "$chemin" in "pool/$version/"*) [[ "$(basename "$f")" == n* ]] || continue ;; esac
    fichiers+=("$f")
    noms+=("$(champ "$f" Package)")
    versions+=("$(champ "$f" Version)")
    chemins+=("$chemin")
done

# avant <i> <j> : succès si l'entrée i se place avant l'entrée j (nom en ordre d'octets, puis version selon
# dpkg, puis chemin pour départager deux entrées identiques). Pas de `sort -V` : il ne connaît pas les règles de dpkg.
avant() {
    local i="$1" j="$2"
    if [ "${noms[i]}" != "${noms[j]}" ]; then
        [[ "${noms[i]}" < "${noms[j]}" ]]
    elif [ "${versions[i]}" != "${versions[j]}" ]; then
        dpkg --compare-versions "${versions[i]}" lt "${versions[j]}"
    else
        [[ "${chemins[i]}" < "${chemins[j]}" ]]
    fi
}
# Tri par insertion sur les indices (quelques dizaines d'entrées au plus).
declare -a ordre=()
for i in "${!fichiers[@]}"; do
    position="${#ordre[@]}"
    for k in "${!ordre[@]}"; do
        if avant "$i" "${ordre[k]}"; then position="$k"; break; fi
    done
    ordre=("${ordre[@]:0:position}" "$i" "${ordre[@]:position}")
done

# Écriture : chaque paragraphe suivi d'une ligne vide, comme apt-ftparchive.
: > "$travail/$indices/Packages"
for i in "${ordre[@]}"; do
    cat "${fichiers[i]}" >> "$travail/$indices/Packages"
    printf '\n\n' >> "$travail/$indices/Packages"
done
# -n : ni nom ni date dans l'en-tête gzip, donc un même contenu donne toujours les mêmes octets.
gzip -9n < "$travail/$indices/Packages" > "$travail/$indices/Packages.gz"

# Release : champs du dépôt, SHA256 seulement. Le fichier Release de l'ancien dépôt n'est volontairement pas
# repris (il serait listé dans lui-même) ; la date est posée par apt-ftparchive.
(cd "$travail" && apt-ftparchive \
    -o APT::FTPArchive::Release::Origin=WiMao \
    -o APT::FTPArchive::Release::Label=WiMao \
    -o APT::FTPArchive::Release::Suite=stable \
    -o APT::FTPArchive::Release::Codename=stable \
    -o APT::FTPArchive::Release::Architectures=amd64 \
    -o APT::FTPArchive::Release::Components=main \
    -o "APT::FTPArchive::Release::Description=WiMao — Wine pour la MAO (paquets signés)" \
    -o APT::FTPArchive::MD5=false -o APT::FTPArchive::SHA1=false -o APT::FTPArchive::SHA512=false \
    -o APT::FTPArchive::Release::MD5=false -o APT::FTPArchive::Release::SHA1=false -o APT::FTPArchive::Release::SHA512=false \
    release dists/stable) > "$travail/dists/stable/Release" \
    || arreter "apt-ftparchive release a échoué"
echo "Packages : ${#ordre[@]} entrée(s)."

# --- 6. Signature ---
echo "== 6. Signature de Release"
echo "La fenêtre de la phrase de passe va s'ouvrir ; elle attend environ une minute."
gpg --yes --local-user "$empreinte" --clearsign --output "$travail/dists/stable/InRelease" "$travail/dists/stable/Release" \
    || arreter "la signature de InRelease a échoué (phrase de passe refusée ou annulée ?)"
gpg --yes --local-user "$empreinte" --armor --detach-sign --output "$travail/dists/stable/Release.gpg" "$travail/dists/stable/Release" \
    || arreter "la signature de Release.gpg a échoué (phrase de passe refusée ou annulée ?)"

# --- 7. Contrôle local ---
echo "== 7. Contrôle local"
verifier_signature "$trousseau" "$empreinte" "$travail/dists/stable/InRelease" \
    || arreter "InRelease ne passe pas gpgv avec la clé publiée"
verifier_signature "$trousseau" "$empreinte" "$travail/dists/stable/Release.gpg" "$travail/dists/stable/Release" \
    || arreter "Release.gpg ne passe pas gpgv avec la clé publiée"
for fichier in main/binary-amd64/Packages main/binary-amd64/Packages.gz; do
    attendu_somme="$(sha256sum "$travail/dists/stable/$fichier" | cut -d' ' -f1)"
    attendu_taille="$(stat -c %s "$travail/dists/stable/$fichier")"
    # Section « SHA256: » de Release : lignes « <somme> <taille> <chemin> ». On lit l'empreinte annoncée.
    annoncee="$(awk -v f="$fichier" '/^[^ ]/ { section = $1 } section == "SHA256:" && $3 == f { print $1, $2; exit }' "$travail/dists/stable/Release")"
    [ "$annoncee" = "$attendu_somme $attendu_taille" ] \
        || arreter "Release annonce « ${annoncee:-rien} » pour $fichier, le fichier vaut « $attendu_somme $attendu_taille »"
done
echo "Signatures et empreintes concordent."

# Tout est bon : on installe le résultat dans le dépôt APT (jamais avant).
mkdir -p "$depot_apt/dists/stable/main/binary-amd64"
cp "$travail/dists/stable/Release" "$travail/dists/stable/InRelease" "$travail/dists/stable/Release.gpg" "$depot_apt/dists/stable/"
cp "$travail/$indices/Packages" "$travail/$indices/Packages.gz" "$depot_apt/$indices/"

# --- 8. Demande de fusion ---
if [ "$a_blanc" -eq 1 ]; then
    echo "== 8. À blanc : ni branche, ni commit, ni PR. Terminé."
    exit 0
fi
echo "== 8. Demande de fusion (PR)"
branche="depot-apt-$version"
git -C "$RACINE_DEPOT" switch -c "$branche"
git -C "$RACINE_DEPOT" add -- depot-apt
git -C "$RACINE_DEPOT" commit -m "Dépôt APT : version $version"
git -C "$RACINE_DEPOT" push -u origin "$branche"
# Corps de la PR : paquets de cette version avec leurs empreintes SHA-256.
corps="$temporaire/corps.md"
{
    echo "Ajoute la version $version au dépôt APT signé (index signé par la clé $empreinte)."
    echo
    echo "Paquets de cette version :"
    echo
    (cd "$dossier" && sha256sum -- *.deb | awk '{ print "- `" $2 "` : SHA-256 `" $1 "`" }')
    echo
    echo "Le site n'est déployé qu'à la fusion de cette PR (workflow depot-apt)."
} > "$corps"
gh pr create --repo "$DEPOT_GITHUB" --base main --head "$branche" \
    --title "Dépôt APT : version $version" --body-file "$corps"
echo "PR ouverte. Relisez-la, attendez la CI, puis fusionnez-la vous-même."
