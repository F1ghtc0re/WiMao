#!/usr/bin/env bash
# Signe une version de WiMao : vérifie le brouillon de Release produit par la CI, REFABRIQUE les paquets
# soi-même, et ne signe SHA256SUMS que si tout concorde. C'est l'étape « propriétaire » de la chaîne de
# confiance : la CI atteste d'où viennent les .deb et les .rpm, cette signature atteste qu'une personne les
# a vérifiés. Une version compte SIX paquets : trois .deb et trois .rpm, tous couverts par SHA256SUMS.
#
# Usage     : scripts/signer-version.sh vX.Y.Z [--dossier <artefacts>]
#             scripts/signer-version.sh <pseudo-version> --a-blanc --dossier <artefacts> [--ref <commit>]
#             scripts/signer-version.sh --aide
#   vX.Y.Z             version à signer (mode réel : exactement trois nombres, ex. v0.4.0). En --a-blanc,
#                      un suffixe est admis (ex. v0.0.0-a-blanc) : c'est une simple étiquette de message.
#   --dossier <d>      dossier contenant les trois .deb, les trois .rpm et SHA256SUMS (artefact
#                      `paquets-<sha court>` de la CI).
#                      Facultatif en mode réel (défaut : téléchargement depuis le brouillon de Release) ;
#                      OBLIGATOIRE en --a-blanc.
#   --a-blanc          essai sans aucun appel à `gh` (ni téléchargement, ni vérification d'attestation,
#                      ni envoi) ni déverrouillage de la clé : seules les étapes locales sont jouées.
#                      ATTENTION : la refabrication par podman, elle, a besoin du réseau (images, paquets
#                      Ubuntu et Fedora, Rust, crates) ; elle n'utilise PAS gh, donc elle marche à blanc.
#   --ref <commit>     --a-blanc seulement : commit (ou étiquette) à refabriquer, au lieu de l'étiquette
#                      vX.Y.Z (qui n'existe pas encore pour un essai sur une demande de fusion). En mode
#                      réel, le code refabriqué est TOUJOURS celui de l'étiquette signée.
#
# Étapes :
#   1. (réel) dépôt propre ; étiquette vX.Y.Z locale, signée OpenPGP, par la clé dont l'empreinte est
#      PUBLIÉE dans SECURITY.md (ligne « Empreinte de la clé de signature : … ») : une signature valide
#      d'une AUTRE clé du trousseau est refusée. Sans empreinte publiée, le script s'arrête. Puis
#      DÉVERROUILLAGE de la sous-clé : signature d'un texte jetable (rien n'est écrit), pour que la fenêtre
#      de phrase de passe s'ouvre tout de suite et non après de longues minutes de refabrication ;
#   2. (réel, sans --dossier) `gh release download` du brouillon : les .deb, les .rpm et SHA256SUMS ;
#   3. `sha256sum -c SHA256SUMS`, et exactement les .deb et .rpm listés dans SHA256SUMS : trois de chaque ;
#   4. (réel) `gh attestation verify` de chaque .deb et de chaque .rpm : provenance Sigstore/GitHub, du
#      workflow paquets.yml, sur la référence de l'étiquette ET sur le COMMIT de l'étiquette signée
#      (--source-digest) : sans cela, une étiquette distante déplacée sur un autre commit passerait ;
#   5. refabrication de wimao et wimao-bridge depuis l'étiquette (ou --ref) dans un conteneur podman SANS
#      racine, avec l'image Ubuntu, la version de Rust et les paquets de la CI (voir plus bas), puis
#      comparaison octet pour octet (SHA-256) avec les .deb du brouillon ; toute différence = arrêt ;
#   5b. refabrication des trois RPM À PARTIR DES .deb du brouillon, ceux qu'on vient de vérifier (jamais
#      d'une autre source), dans un conteneur Fedora 45, puis comparaison octet pour octet avec les .rpm ;
#   6. `gpg --armor --detach-sign --local-user <empreinte publiée>` de SHA256SUMS vers SHA256SUMS.asc (la
#      phrase de passe est demandée par GnuPG/pinentry : ce script ne la lit ni ne la reçoit jamais) ; il
#      affiche l'empreinte de la clé qui a signé ;
#   7. (réel) `gh release upload` de SHA256SUMS.asc ET de la clé publique docs/cle-publique.asc. La Release
#      reste un BROUILLON : ce script ne la publie jamais, c'est à vous de la relire puis de la publier.
#   wimao-wine n'est pas refabriqué ici (plusieurs heures de compilation) : la CI le refabrique sans
#   cache sur chaque étiquette (job `reproductible-wine`) ; il reste vérifié par somme et attestation.
#   Son RPM, lui, est bien refabriqué (simple réemballage du .deb, quelques secondes).
#
# Pourquoi podman : la machine du propriétaire n'est pas l'Ubuntu 24.04 de la CI ; une refabrication
# native ne pourrait jamais correspondre. Le conteneur reprend, depuis le dépôt (sources uniques) :
#   - l'image `ubuntu:24.04@sha256:…` et la version de Rust : lues dans le job `wimao` de
#     .github/workflows/paquets.yml (clés `image:` et `toolchain:`) ;
#   - les paquets Ubuntu : paquets/{wimao,bridge}/paquets-compilation.txt, comme la CI ;
#   - Rust : rustup-init (version et SHA-256 figés dans ce script) téléchargé sur static.rust-lang.org et
#     vérifié, puis `--default-toolchain <version> --profile minimal` (chaîne vérifiée par rustup).
#   La date des fichiers (SOURCE_DATE_EPOCH) est celle du dernier commit, calculée comme le fait la CI
#   (paquets/commun.sh), et le dépôt est monté dans un git worktree jetable, supprimé en sortie.
#   Limite : les paquets Ubuntu sont ceux d'AUJOURD'HUI (apt-get update) ; si Ubuntu a publié une mise à jour
#   d'une bibliothèque depuis la CI, les dépendances relevées par dpkg-shlibdeps peuvent différer : signer
#   alors peu après la CI, ou relancer la CI.
#   RPM : l'image Fedora 45 (clé `image:` du job `rpm` de paquets.yml, figée par empreinte) est la même que
#   la CI ; `rpm-build`, `dpkg` et `zstd` y sont installés depuis les dépôts VIVANTS de Fedora, comme dans la
#   CI. Une mise à jour de rpm-build ou de zstd entre la CI et la signature peut donc changer les octets d'un
#   RPM : relancer la CI, puis signer peu après. SOURCE_DATE_EPOCH est calculé comme dans le job `rpm`
#   (date du dernier commit du code refabriqué).
#
# Variables : SIGNER_REFABRIQUER  (tests, --a-blanc seulement) exécutable remplaçant TOUTE la refabrication :
#                                 il reçoit en $1 un dossier de sortie et y écrit les deux .deb.
#             SIGNER_REFABRIQUER_RPM (tests, --a-blanc seulement) exécutable remplaçant la refabrication des
#                                 RPM : il reçoit en $1 le dossier des .deb vérifiés, en $2 un dossier de
#                                 sortie où il écrit les trois .rpm.
#             SIGNER_EMPREINTE    (tests, --a-blanc seulement) empreinte de la clé qui signe, à la place de
#                                 celle de SECURITY.md ; sans elle, --a-blanc signe avec la clé par défaut.
#             GNUPGHOME, GPG_TTY  lus par GnuPG (trousseau et terminal de la phrase de passe).
# Écrit     : <dossier>/SHA256SUMS.asc (la signature) ; dossiers temporaires (téléchargement, worktree,
#             refabrication) supprimés en sortie. Rien dans le dépôt, rien dans votre dossier personnel.
# Code      : 0 = signé (et envoyé en mode réel) ; 1 = vérification échouée ou étape impossible ; 2 = usage.
# Dépend de : git, gpg, sha256sum, et pour la refabrication podman ; gh (mode réel seulement).
set -euo pipefail

RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Dépôt GitHub qui porte les Releases et les attestations.
DEPOT_GITHUB="F1ghtc0re/WiMao"
# Fonctions communes de vérification de signature (empreinte_publiee, …), partagées avec le dépôt APT.
# shellcheck source=scripts/lib/publication.sh
source "$RACINE_DEPOT/scripts/lib/publication.sh"
# rustup-init (Linux x86_64) : version figée, adresse VERSIONNÉE (archive/) et empreinte SHA-256 écrite ici,
# vérifiée avant toute exécution. Pour changer de version : télécharger la nouvelle, recalculer sha256sum.
# La chaîne Rust elle-même est ensuite vérifiée par rustup (manifestes signés et empreintes).
VERSION_RUSTUP=1.29.2
SHA256_RUSTUP=5be191e8c70a5007074254cb74d78dbfdd25cd5a61c27de7b8e5410122ec24f4
URL_RUSTUP="https://static.rust-lang.org/rustup/archive/${VERSION_RUSTUP}/x86_64-unknown-linux-gnu/rustup-init"

# --- Aide et erreurs ---

# Affiche le bloc de commentaires d'en-tête (de « Usage » à « Code ») comme aide.
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
ref=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --aide | -h | --help) aide; exit 0 ;;
        --a-blanc) a_blanc=1 ;;
        --dossier | --ref)
            [ "$#" -ge 2 ] || erreur_usage "$1 attend une valeur"
            # Une valeur en « -… » serait prise pour une option par git ou par d'autres commandes.
            case "$2" in -*) erreur_usage "$1 : valeur invalide (commence par « - ») : $2" ;; esac
            if [ "$1" = "--dossier" ]; then dossier="$2"; else ref="$2"; fi
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
if [ "$a_blanc" -eq 1 ]; then
    [[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$ ]] \
        || erreur_usage "version « $version » invalide (forme attendue : vX.Y.Z, suffixe admis en --a-blanc)"
    [ -n "$dossier" ] || erreur_usage "--a-blanc exige --dossier (aucun téléchargement n'est fait à blanc)"
else
    [[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
        || erreur_usage "version « $version » invalide (forme attendue : vX.Y.Z, ex. v0.4.0)"
    [ -z "$ref" ] || erreur_usage "--ref n'est admis qu'avec --a-blanc (le mode réel refabrique l'étiquette $version)"
    # Un contournement de la refabrication n'a pas sa place dans une vraie signature.
    [ -z "${SIGNER_REFABRIQUER:-}" ] || erreur_usage "SIGNER_REFABRIQUER n'est admis qu'avec --a-blanc (réservé aux tests)"
    [ -z "${SIGNER_REFABRIQUER_RPM:-}" ] || erreur_usage "SIGNER_REFABRIQUER_RPM n'est admis qu'avec --a-blanc (réservé aux tests)"
    # Idem pour l'empreinte : en vraie signature, seule celle publiée dans SECURITY.md compte.
    [ -z "${SIGNER_EMPREINTE:-}" ] || erreur_usage "SIGNER_EMPREINTE n'est admis qu'avec --a-blanc (réservé aux tests)"
fi

# --- Dossier temporaire et nettoyage ---
temporaire="$(mktemp -d)"
worktree=""
nettoyer() {
    # Le worktree git doit être déclaré supprimé auprès du dépôt avant que son dossier disparaisse.
    if [ -n "$worktree" ]; then
        git -C "$RACINE_DEPOT" worktree remove --force "$worktree" > /dev/null 2>&1 || true
        git -C "$RACINE_DEPOT" worktree prune > /dev/null 2>&1 || true
    fi
    rm -rf "$temporaire"
}
trap nettoyer EXIT

# --- 1. Mode réel : dépôt propre et étiquette signée ---
empreinte=""          # clé de signature attendue (mode réel : lue dans SECURITY.md)
commit_etiquette=""   # commit de l'étiquette signée (mode réel)
if [ "$a_blanc" -eq 0 ]; then
    echo "== 1. Dépôt propre et étiquette $version signée"
    [ -z "$(git -C "$RACINE_DEPOT" status --porcelain)" ] \
        || arreter "le dépôt contient des modifications non enregistrées : commitez-les ou rangez-les d'abord"
    git -C "$RACINE_DEPOT" rev-parse -q --verify "refs/tags/$version" > /dev/null \
        || arreter "l'étiquette $version n'existe pas localement (git fetch --tags ?)"
    # Empreinte PUBLIÉE de la clé de signature : seule cette clé a le droit de signer une version.
    empreinte="$(empreinte_publiee "$RACINE_DEPOT")" || exit 1
    echo "Clé de signature attendue (SECURITY.md) : $empreinte"
    # Vérification de l'étiquette en OpenPGP (l'étiquette est signée par la sous-clé, alors que les
    # commits le sont par une clé SSH : voir docs/publier-une-version.md). --raw donne les lignes
    # [GNUPG:] de GnuPG, bien plus sûres à lire que le texte traduit de `gpg --verify`.
    verif="$(git -C "$RACINE_DEPOT" -c gpg.format=openpgp verify-tag --raw "$version" 2>&1)" \
        || { echo "$verif" >&2; arreter "la signature de l'étiquette $version est invalide ou la clé publique du signataire manque"; }
    # Une signature valide : GOODSIG, et VALIDSIG dont le 12e champ est l'empreinte de la clé PRINCIPALE
    # (la signature elle-même est faite par une sous-clé, dont l'empreinte est le 3e champ).
    grep -q '^\[GNUPG:\] GOODSIG ' <<< "$verif" || { echo "$verif" >&2; arreter "pas de signature valide (GOODSIG) sur l'étiquette $version"; }
    signataire="$(awk '$1 == "[GNUPG:]" && $2 == "VALIDSIG" { print $12; exit }' <<< "$verif")"
    [ "$signataire" = "$empreinte" ] \
        || arreter "l'étiquette $version est signée par la clé « ${signataire:-inconnue} », pas par la clé publiée dans SECURITY.md ($empreinte)"
    echo "Étiquette $version : signature valide de la clé $signataire"
    # Commit de l'étiquette signée : l'attestation de la CI devra porter sur CE commit (étape 4).
    commit_etiquette="$(git -C "$RACINE_DEPOT" rev-parse "$version^{commit}")"
    # La clé publique est jointe à la Release (étape 7) : autant l'exiger tout de suite, et de la bonne clé.
    [ -f "$RACINE_DEPOT/docs/cle-publique.asc" ] || arreter "docs/cle-publique.asc introuvable : exportez la clé publique (voir docs/publier-une-version.md)"
    publiee="$(gpg --show-keys --with-colons "$RACINE_DEPOT/docs/cle-publique.asc" 2> /dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"
    [ "$publiee" = "$empreinte" ] \
        || arreter "docs/cle-publique.asc contient la clé « ${publiee:-illisible} », pas celle de SECURITY.md ($empreinte)"
    # `gh attestation` (avec --signer-workflow et --source-ref) n'existe pas dans les anciennes
    # versions de gh, dont celle des dépôts d'Ubuntu (2.46, constaté en 2026) : on le vérifie avant
    # tout appel à gh, avec un message qui dit quoi faire. Aide lue dans une variable : avec pipefail,
    # `gh … | grep -q` pourrait échouer à tort (SIGPIPE quand grep s'arrête au premier résultat).
    aide_gh="$(gh attestation verify --help 2> /dev/null || true)"
    for option in --signer-workflow --source-digest --deny-self-hosted-runners; do
        grep -q -- "$option" <<< "$aide_gh" \
            || arreter "votre gh ne sait pas vérifier les attestations (gh attestation verify $option) :
installez GitHub CLI depuis son dépôt officiel (https://github.com/cli/cli/blob/trunk/docs/install_linux.md)"
    done

    # Déverrouillage de la clé AVANT la refabrication (longue) : sans cela, la fenêtre de phrase de passe
    # n'apparaîtrait qu'à l'étape 6, et GnuPG pourrait avoir oublié le reste. On signe un texte jetable, sans
    # écrire aucun fichier (sortie jetée) : la phrase de passe est demandée par GnuPG/pinentry, jamais lue ici.
    # `-u "$empreinte"` choisit la même sous-clé de signature que l'étape 6 : c'est celle-là qu'il faut
    # déverrouiller pour que la signature finale n'ait plus à redemander la phrase.
    echo "Déverrouillage de la clé de signature (une fenêtre de phrase de passe peut s'ouvrir maintenant)"
    if terminal="$(tty 2> /dev/null)"; then
        export GPG_TTY="$terminal"
    fi
    echo "déverrouillage de la clé de signature WiMao $version" | gpg --batch --yes -u "$empreinte" --clearsign > /dev/null \
        || arreter "impossible de déverrouiller la clé de signature $empreinte (phrase de passe refusée, fenêtre fermée ou clé absente du trousseau) :
rien n'a été téléchargé ni refabriqué, relancez le script"
else
    echo "== 1. À blanc : étiquette non vérifiée, clé non déverrouillée"
fi

# --- 2. Artefacts ---
if [ -z "$dossier" ]; then
    echo "== 2. Téléchargement du brouillon de Release $version"
    command -v gh > /dev/null || arreter "la commande gh est introuvable (https://cli.github.com)"
    dossier="$temporaire/artefacts"
    mkdir -p "$dossier"
    gh release download "$version" --repo "$DEPOT_GITHUB" --pattern '*.deb' --pattern '*.rpm' --pattern SHA256SUMS --dir "$dossier"
else
    echo "== 2. Artefacts pris dans $dossier"
fi
[ -d "$dossier" ] || arreter "dossier d'artefacts introuvable : $dossier"
dossier="$(cd "$dossier" && pwd)"
[ -f "$dossier/SHA256SUMS" ] || arreter "$dossier/SHA256SUMS introuvable"

# --- 3. Sommes SHA-256 ---
echo "== 3. Sommes SHA-256"
(cd "$dossier" && sha256sum -c SHA256SUMS) || arreter "un paquet ne correspond pas à SHA256SUMS (voir ci-dessus)"
# sha256sum -c ne regarde que les fichiers LISTÉS : un paquet de plus dans le dossier passerait inaperçu.
listes="$(awk '{print $2}' "$dossier/SHA256SUMS" | LC_ALL=C sort)"
presents="$(find "$dossier" -maxdepth 1 \( -name '*.deb' -o -name '*.rpm' \) -printf '%f\n' | LC_ALL=C sort)"
[ "$listes" = "$presents" ] || arreter "les .deb et .rpm du dossier ne sont pas exactement ceux de SHA256SUMS
  listés   : $(echo "$listes" | tr '\n' ' ')
  présents : $(echo "$presents" | tr '\n' ' ')"
# Une version complète compte exactement trois .deb ET trois .rpm : un brouillon incomplet (RPM oubliés)
# ne doit jamais être signé, même si SHA256SUMS et le dossier sont cohérents entre eux.
nb_deb="$(grep -c '\.deb$' <<< "$presents" || true)"
nb_rpm="$(grep -c '\.rpm$' <<< "$presents" || true)"
{ [ "$nb_deb" -eq 3 ] && [ "$nb_rpm" -eq 3 ]; } || arreter "trois .deb et trois .rpm sont attendus, il y a $nb_deb .deb et $nb_rpm .rpm :
  $(echo "$presents" | tr '\n' ' ')"

# --- 4. Attestations de provenance ---
if [ "$a_blanc" -eq 0 ]; then
    echo "== 4. Attestations de provenance (gh attestation verify)"
    for paquet in "$dossier"/*.deb "$dossier"/*.rpm; do
        # Attestation exigée du workflow des paquets, de l'étiquette de cette version ET du commit de
        # l'étiquette SIGNÉE localement (--source-digest) : une attestation d'un autre workflow, d'une autre
        # référence ou d'un autre commit (étiquette distante déplacée) est refusée, tout comme un runner
        # auto-hébergé (--deny-self-hosted-runners).
        gh attestation verify "$paquet" --repo "$DEPOT_GITHUB" \
            --signer-workflow "$DEPOT_GITHUB/.github/workflows/paquets.yml" \
            --source-ref "refs/tags/$version" \
            --source-digest "$commit_etiquette" \
            --deny-self-hosted-runners \
            || arreter "attestation absente ou invalide pour $(basename "$paquet")"
    done
else
    echo "== 4. À blanc : attestations non vérifiées (aucun appel à gh)"
fi

# --- 5. Refabrication et comparaison ---

# Valeur de la clé <cle> (image, toolchain) dans le job <job> du workflow <fichier> : source unique
# de l'image et de la version de Rust, partagée avec la CI (job `wimao` pour les .deb, job `rpm` pour les
# RPM). Commentaires ignorés ; premier résultat.
lire_cle_job() {
    local fichier="$1" job_voulu="$2" cle="$3"
    awk -v cle="$cle" -v job_voulu="$job_voulu" '
        /^[[:space:]]*#/ { next }
        /^[^ ]/ { enjobs = ($0 ~ /^jobs:/); job = ""; next }
        enjobs && /^  [^ ]/ { job = $0; sub(/^  /, "", job); sub(/:.*/, "", job); next }
        job == job_voulu && $0 ~ ("^[[:space:]]+(-[[:space:]]+)?" cle ":[[:space:]]+") {
            valeur = $0
            sub("^[[:space:]]+(-[[:space:]]+)?" cle ":[[:space:]]+", "", valeur)
            sub(/[[:space:]]+#.*$/, "", valeur)
            sub(/[[:space:]]+$/, "", valeur)
            print valeur
            exit
        }' "$fichier"
}

# Script exécuté DANS le conteneur (root du conteneur = vous, podman étant sans racine). Reçoit
# RUST_VERSION et SOURCE_DATE_EPOCH par l'environnement ; le dépôt est monté sur /depot, la sortie sur /sortie.
# shellcheck disable=SC2016  # les « $ » sont volontairement interprétés par le shell du conteneur
SCRIPT_CONTENEUR='
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
# Comme dtolnay/rust-toolchain dans la CI.
export CARGO_INCREMENTAL=0
cd /depot
apt-get update
# Mêmes paquets que les jobs `wimao` et `bridge` (lecture : commentaires retirés).
for liste in paquets/wimao/paquets-compilation.txt paquets/bridge/paquets-compilation.txt; do
    sed "s/#.*//" "$liste"
done | xargs apt-get install -y --no-install-recommends
# Rust : rustup-init versionné, empreinte figée dans le script, vérifiée avant toute exécution.
cd /tmp
curl -fsSL -o rustup-init "$URL_RUSTUP"
echo "$SHA256_RUSTUP  rustup-init" | sha256sum -c -
chmod +x rustup-init
./rustup-init --default-toolchain "$RUST_VERSION" --profile minimal -y
export PATH="$HOME/.cargo/bin:$PATH"
rustc --version
cd /depot
paquets/fabriquer-bridge.sh /sortie
paquets/fabriquer-wimao.sh /sortie
'

# Prépare (une seule fois par lancement) la copie de travail jetable du code à refabriquer. Renseigne
# `depot` (son dossier), `commit` et `epoch` (SOURCE_DATE_EPOCH : même calcul que paquets/commun.sh, date du
# dernier commit du code refabriqué). Partagée par la refabrication des .deb et celle des RPM, pour qu'elles
# partent exactement du même commit. Ne fait rien si c'est déjà fait.
depot=""
commit=""
epoch=""
preparer_depot() {
    [ -z "$depot" ] || return 0
    local cible
    command -v podman > /dev/null || arreter "podman est introuvable : installez-le (sudo apt install podman) pour refabriquer les paquets"
    cible="${ref:-$version}"
    commit="$(git -C "$RACINE_DEPOT" rev-parse -q --verify "${cible}^{commit}")" \
        || arreter "référence introuvable dans le dépôt : $cible (en --a-blanc, passez --ref <commit>)"
    # Copie de travail jetable : le dépôt de l'utilisateur n'est ni modifié ni sali par la compilation.
    depot="$temporaire/depot"
    git -C "$RACINE_DEPOT" worktree add --detach --quiet "$depot" "$commit"
    worktree="$depot"
    epoch="$(git -C "$depot" log -1 --format=%ct)"
}

# Refabrication des .deb dans un conteneur podman sans racine. $1 = dossier de sortie (reçoit les deux .deb).
refabriquer_avec_podman() {
    local sortie="$1" image rust
    preparer_depot
    image="$(lire_cle_job "$depot/.github/workflows/paquets.yml" wimao image)"
    rust="$(lire_cle_job "$depot/.github/workflows/paquets.yml" wimao toolchain)"
    [[ "$image" =~ ^[A-Za-z0-9._/:-]+@sha256:[0-9a-f]{64}$ ]] \
        || arreter "image du job wimao illisible dans paquets.yml : « $image »"
    [[ "$rust" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] \
        || arreter "version de Rust du job wimao illisible dans paquets.yml : « $rust »"
    # Nom complet exigé par podman (sans quoi il consulte la liste des registres de l'utilisateur).
    case "$image" in */*) ;; *) image="docker.io/library/$image" ;; esac
    echo "Conteneur : $image ; Rust $rust ; commit ${commit:0:12} ; SOURCE_DATE_EPOCH=$epoch"
    podman run --rm \
        --volume "$depot:/depot:rw" --volume "$sortie:/sortie:rw" \
        --env "SOURCE_DATE_EPOCH=$epoch" --env "RUST_VERSION=$rust" --env "URL_RUSTUP=$URL_RUSTUP" --env "SHA256_RUSTUP=$SHA256_RUSTUP" \
        "$image" bash -c "$SCRIPT_CONTENEUR"
}

# Script exécuté DANS le conteneur Fedora 45 pour refabriquer les RPM. Mêmes paquets que l'étape
# « Paquets de base » du job `rpm` de la CI (git-core et tar y servent à actions/checkout ; on les installe
# aussi pour rester strictement identique), et même boucle : un RPM par .deb. Reçoit SOURCE_DATE_EPOCH par
# l'environnement ; le dépôt est monté (lecture seule) sur /depot, les .deb vérifiés sur /debs (lecture
# seule) et la sortie sur /sortie.
# shellcheck disable=SC2016  # (aucun « $ » à protéger ici, mais on garde la même forme que SCRIPT_CONTENEUR)
SCRIPT_CONTENEUR_RPM='
set -euo pipefail
dnf install -y --setopt=install_weak_deps=False git-core tar rpm-build dpkg zstd
cd /depot
for deb in /debs/*.deb; do
    paquets/fabriquer-rpm.sh "$deb" /sortie
done
'

# Refabrication des RPM dans un conteneur podman sans racine, À PARTIR DES .deb de $dossier (déjà vérifiés
# par somme, attestation et refabrication : aucune autre source). Fonctionne sans gh, donc à blanc aussi.
# $1 = dossier de sortie (reçoit les trois .rpm).
refabriquer_rpm_avec_podman() {
    local sortie="$1" image
    preparer_depot
    image="$(lire_cle_job "$depot/.github/workflows/paquets.yml" rpm image)"
    [[ "$image" =~ ^[A-Za-z0-9._/:-]+@sha256:[0-9a-f]{64}$ ]] \
        || arreter "image du job rpm illisible dans paquets.yml : « $image »"
    case "$image" in */*) ;; *) image="docker.io/library/$image" ;; esac
    echo "Conteneur : $image ; commit ${commit:0:12} ; SOURCE_DATE_EPOCH=$epoch"
    podman run --rm \
        --volume "$depot:/depot:ro" --volume "$dossier:/debs:ro" --volume "$sortie:/sortie:rw" \
        --env "SOURCE_DATE_EPOCH=$epoch" \
        "$image" bash -c "$SCRIPT_CONTENEUR_RPM"
}

echo "== 5. Refabrication de wimao et wimao-bridge, puis comparaison"
refait="$temporaire/refait"
mkdir -p "$refait"
if [ -n "${SIGNER_REFABRIQUER:-}" ]; then
    echo "Refabrication simulée par SIGNER_REFABRIQUER (essais)."
    "$SIGNER_REFABRIQUER" "$refait" || arreter "SIGNER_REFABRIQUER a échoué"
else
    refabriquer_avec_podman "$refait"
fi

differences=0
nombre=0
for nouveau in "$refait"/*.deb; do
    [ -e "$nouveau" ] || arreter "la refabrication n'a produit aucun .deb"
    nom="$(basename "$nouveau")"
    if [ ! -f "$dossier/$nom" ]; then
        echo "ERREUR : $nom refabriqué n'existe pas dans les artefacts" >&2
        differences=1
        continue
    fi
    a="$(sha256sum "$dossier/$nom" | cut -d' ' -f1)"
    b="$(sha256sum "$nouveau" | cut -d' ' -f1)"
    echo "$nom"
    echo "  artefact  : $a"
    echo "  refabriqué: $b"
    if [ "$a" != "$b" ]; then
        echo "ERREUR : $nom n'est pas reproductible (empreintes différentes)" >&2
        differences=1
    fi
    nombre=$((nombre + 1))
done
[ "$nombre" -eq 2 ] || { echo "ERREUR : deux .deb refabriqués attendus (wimao, wimao-bridge), $nombre trouvé(s)" >&2; differences=1; }
[ "$differences" -eq 0 ] || arreter "refabrication différente du brouillon : rien n'est signé"

# --- 5b. Refabrication des RPM depuis les .deb vérifiés ---
# Placée APRÈS la comparaison des .deb : les RPM ne sont refabriqués qu'à partir de .deb dont l'origine est
# établie (somme, attestation, refabrication de wimao et wimao-bridge ; wimao-wine par attestation et par le
# job reproductible-wine de la CI).
echo "== 5b. Refabrication des trois RPM depuis les .deb vérifiés, puis comparaison"
refait_rpm="$temporaire/refait-rpm"
mkdir -p "$refait_rpm"
if [ -n "${SIGNER_REFABRIQUER_RPM:-}" ]; then
    echo "Refabrication des RPM simulée par SIGNER_REFABRIQUER_RPM (essais)."
    "$SIGNER_REFABRIQUER_RPM" "$dossier" "$refait_rpm" || arreter "SIGNER_REFABRIQUER_RPM a échoué"
else
    refabriquer_rpm_avec_podman "$refait_rpm" || arreter "la refabrication des RPM a échoué (podman, réseau ou dépôts Fedora ?)"
fi

differences=0
nombre=0
for nouveau in "$refait_rpm"/*.rpm; do
    [ -e "$nouveau" ] || arreter "la refabrication n'a produit aucun .rpm"
    nom="$(basename "$nouveau")"
    if [ ! -f "$dossier/$nom" ]; then
        echo "ERREUR : $nom refabriqué n'existe pas dans les artefacts" >&2
        differences=1
        continue
    fi
    a="$(sha256sum "$dossier/$nom" | cut -d' ' -f1)"
    b="$(sha256sum "$nouveau" | cut -d' ' -f1)"
    echo "$nom"
    echo "  artefact  : $a"
    echo "  refabriqué: $b"
    if [ "$a" != "$b" ]; then
        echo "ERREUR : $nom n'est pas reproductible (empreintes différentes)" >&2
        differences=1
    fi
    nombre=$((nombre + 1))
done
[ "$nombre" -eq 3 ] || { echo "ERREUR : trois .rpm refabriqués attendus (wimao, wimao-bridge, wimao-wine), $nombre trouvé(s)" >&2; differences=1; }
if [ "$differences" -ne 0 ]; then
    # Les paquets système (rpm-build, zstd…) viennent des dépôts VIVANTS de Fedora, pas d'une version figée.
    echo "Si seul un RPM diffère alors que les .deb concordent, une mise à jour de rpm-build ou de zstd" >&2
    echo "publiée depuis la CI peut en être la cause : relancez la CI, puis signez peu après." >&2
    arreter "RPM refabriqués différents du brouillon : rien n'est signé"
fi

# --- 6. Signature ---
echo "== 6. Signature de SHA256SUMS (GnuPG demandera la phrase de passe si la clé en a une)"
# Pour que pinentry en terminal sache où s'afficher.
if terminal="$(tty 2> /dev/null)"; then
    export GPG_TTY="$terminal"
fi
rm -f "$dossier/SHA256SUMS.asc"
# Clé qui signe : en réel, TOUJOURS celle publiée dans SECURITY.md (jamais « la clé par défaut du trousseau »,
# qui pourrait être une autre si vous en avez plusieurs) ; à blanc, SIGNER_EMPREINTE si fournie (tests).
if [ "$a_blanc" -eq 1 ]; then empreinte="${SIGNER_EMPREINTE:-}"; empreinte="${empreinte^^}"; fi
option_cle=()
if [ -n "$empreinte" ]; then option_cle=(--local-user "$empreinte"); fi
(cd "$dossier" && gpg --armor --detach-sign "${option_cle[@]}" --output SHA256SUMS.asc SHA256SUMS) \
    || arreter "la signature GnuPG a échoué"
statut="$(cd "$dossier" && gpg --status-fd 1 --verify SHA256SUMS.asc SHA256SUMS 2> /dev/null)" \
    || arreter "la signature produite ne se vérifie pas"
# Empreinte de la clé PRINCIPALE du signataire (12e champ de VALIDSIG), affichée pour que vous la compariez.
signataire="$(awk '$1 == "[GNUPG:]" && $2 == "VALIDSIG" { print $12; exit }' <<< "$statut")"
[ -n "$signataire" ] || arreter "la signature produite ne contient pas de VALIDSIG"
if [ -n "$empreinte" ] && [ "$signataire" != "$empreinte" ]; then
    arreter "SHA256SUMS signé par « $signataire » au lieu de la clé attendue « $empreinte »"
fi
echo "Signature écrite : $dossier/SHA256SUMS.asc"
echo "Clé qui a signé (empreinte de la clé principale) : $signataire"

# --- 7. Envoi ---
if [ "$a_blanc" -eq 0 ]; then
    echo "== 7. Envoi de SHA256SUMS.asc et de la clé publique vers le brouillon $version"
    gh release upload "$version" "$dossier/SHA256SUMS.asc" "$RACINE_DEPOT/docs/cle-publique.asc" --repo "$DEPOT_GITHUB"
    echo "Terminé. La Release $version reste un BROUILLON : relisez-la sur GitHub puis publiez-la vous-même."
else
    echo "== 7. À blanc : SHA256SUMS.asc et clé publique non envoyés (aucun appel à gh)"
    echo "Essai à blanc terminé : tout concorde."
fi
