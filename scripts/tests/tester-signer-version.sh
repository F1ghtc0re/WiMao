#!/usr/bin/env bash
# Teste scripts/signer-version.sh en mode --a-blanc, SANS podman ni réseau : la refabrication est simulée
# par SIGNER_REFABRIQUER, le trousseau GnuPG est jetable et un faux `gh` (qui échoue bruyamment et se
# consigne) est placé en tête du PATH pour prouver qu'aucun appel à gh n'a lieu à blanc.
#
# Une version compte SIX paquets (trois .deb et trois .rpm) ; les RPM sont refabriqués depuis les .deb.
# Cas couverts : signature valide (gpg --verify) avec six paquets ; arrêt si un paquet ne correspond pas à
# SHA256SUMS ; arrêt si un .deb ou un .rpm est en trop ; arrêt si un RPM manque du brouillon (refus) ; arrêt
# si l'empreinte d'un .deb ou d'un RPM refabriqué diffère (message sur rpm-build/zstd) ; arrêt si un .deb ou un
# RPM refabriqué manque ; refus des usages invalides (mode réel sans étiquette, SIGNER_REFABRIQUER en mode réel, --ref
# sans --a-blanc, --a-blanc sans --dossier) ; aucun appel à gh nulle part en --a-blanc.
# Mode réel (dépôt de test, faux gh et faux podman qui se consignent, étiquette signée en OpenPGP par une
# sous-clé) : --source-digest reçoit le commit de l'étiquette et non HEAD ; les six paquets sont attestés ;
# la clé est déverrouillée (signature d'un texte jetable) AVANT tout appel à gh et à podman, et un échec du
# déverrouillage arrête le script ; les RPM sont refabriqués par podman dans l'image Fedora 45 du job `rpm`,
# sans appel à gh ; la clé publique est envoyée avec
# la signature ; une étiquette signée par une autre clé que la publiée dans SECURITY.md est refusée ; sans
# empreinte publiée, arrêt.
# La refabrication réelle par podman n'est PAS testée ici : le job `signature-a-blanc` de
# .github/workflows/paquets.yml la prouve de bout en bout sur les vrais paquets de la CI (.deb et RPM).
# Toutes les clés de ce test sont jetables : la vraie clé du propriétaire n'est jamais sollicitée.
#
# Usage     : scripts/tests/tester-signer-version.sh
# Variables : aucune (GNUPGHOME et PATH sont réglés par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~/.gnupg.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
signer="$racine/scripts/signer-version.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

# --- Trousseau GnuPG jetable : une clé sans phrase de passe ---
export GNUPGHOME="$temporaire/gnupg"
mkdir -m 700 "$GNUPGHOME"
cat > "$temporaire/cle.params" << 'EOF'
%no-protection
Key-Type: EDDSA
Key-Curve: ed25519
Key-Usage: sign
Name-Real: WiMao essai jetable
Name-Email: essai@example.invalid
Expire-Date: 1d
%commit
EOF
gpg --batch --quiet --gen-key "$temporaire/cle.params" 2> /dev/null

# --- Faux gh : consigne l'appel, échoue bruyamment ---
mkdir "$temporaire/bin"
journal_gh="$temporaire/appels-gh.log"
cat > "$temporaire/bin/gh" << EOF
#!/usr/bin/env bash
echo "gh \$*" >> "$journal_gh"
echo "FAUX GH APPELÉ : gh \$*" >&2
exit 99
EOF
chmod +x "$temporaire/bin/gh"
export PATH="$temporaire/bin:$PATH"

# --- Faux artefacts : trois .deb et trois .rpm, SHA256SUMS comme le fait le job `sommes` ---
originaux="$temporaire/originaux"
mkdir "$originaux"
for nom in wimao_0.4.0-1_amd64.deb wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb wimao-wine_11.20+wimao1_amd64.deb \
    wimao-0.4.0-1.x86_64.rpm wimao-bridge-5.1.1+dxvk3.1.1-1.x86_64.rpm wimao-wine-11.20+wimao1-1.x86_64.rpm; do
    echo "contenu de $nom" > "$originaux/$nom"
done
(cd "$originaux" && LC_ALL=C sha256sum -- *.deb *.rpm | LC_ALL=C sort -k2 > SHA256SUMS)

# Faux « refabricant » : recopie wimao et wimao-bridge (pas wimao-wine) dans $1. Le comportement se règle par
# FAUX_MODE : identique (défaut), altere (un .deb diffère), manquant (un .deb n'est pas produit).
cat > "$temporaire/refabriquer.sh" << EOF
#!/usr/bin/env bash
set -euo pipefail
cp "$originaux"/wimao_*.deb "$originaux"/wimao-bridge_*.deb "\$1/"
case "\${FAUX_MODE:-identique}" in
    altere) echo "octets en plus" >> "\$1"/wimao_*.deb ;;
    manquant) rm "\$1"/wimao-bridge_*.deb ;;
esac
EOF
chmod +x "$temporaire/refabriquer.sh"
export SIGNER_REFABRIQUER="$temporaire/refabriquer.sh"

# Faux « refabricant » de RPM : $1 = dossier des .deb vérifiés, $2 = sortie. Recopie les trois .rpm d'origine.
# FAUX_MODE_RPM : identique (défaut), altere (un RPM diffère), manquant (un RPM n'est pas produit).
# Il vérifie aussi que les .deb lui sont bien transmis (c'est d'eux que les RPM doivent venir).
cat > "$temporaire/refabriquer-rpm.sh" << EOF
#!/usr/bin/env bash
set -euo pipefail
[ "\$(ls "\$1"/*.deb | wc -l)" -eq 3 ] || { echo "les trois .deb vérifiés devaient être transmis" >&2; exit 1; }
cp "$originaux"/*.rpm "\$2/"
case "\${FAUX_MODE_RPM:-identique}" in
    altere) echo "octets en plus" >> "\$2"/wimao-wine-*.rpm ;;
    manquant) rm "\$2"/wimao-bridge-*.rpm ;;
esac
EOF
chmod +x "$temporaire/refabriquer-rpm.sh"
export SIGNER_REFABRIQUER_RPM="$temporaire/refabriquer-rpm.sh"

# Copie fraîche des artefacts dans un dossier de travail ; rend son chemin.
copie() {
    local d="$temporaire/travail-$1"
    rm -rf "$d"
    cp -r "$originaux" "$d"
    printf '%s' "$d"
}

# lancer <dossier-de-sortie-et-code> : exécute le script, sortie dans $temporaire/sortie.log, code dans $code.
code=0
lancer() {
    code=0
    "$signer" "$@" > "$temporaire/sortie.log" 2>&1 || code=$?
}

# Vérifie que la sortie du dernier lancement contient le motif (prouve le MOTIF du refus, pas seulement le code).
contient() {
    grep -qE -- "$1" "$temporaire/sortie.log" || { cat "$temporaire/sortie.log" >&2; echec "$2 : motif « $1 » absent de la sortie"; }
}

# --- 1. Cas nominal ---
d="$(copie nominal)"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -eq 0 ] || { cat "$temporaire/sortie.log" >&2; echec "cas nominal : code $code au lieu de 0"; }
[ -f "$d/SHA256SUMS.asc" ] || echec "cas nominal : SHA256SUMS.asc absent"
gpg --verify "$d/SHA256SUMS.asc" "$d/SHA256SUMS" 2> /dev/null || echec "cas nominal : signature invalide"
contient "attestations non vérifiées" "cas nominal"
contient "wimao-wine-11.20\+wimao1-1.x86_64.rpm" "cas nominal (RPM comparés)"
contient "clé non déverrouillée" "cas nominal"
echo "ok : signature valide en mode à blanc, six paquets"

# La signature ne couvre pas un SHA256SUMS modifié (le test est bien discriminant).
echo "ligne ajoutée" >> "$d/SHA256SUMS"
if gpg --verify "$d/SHA256SUMS.asc" "$d/SHA256SUMS" 2> /dev/null; then echec "une signature valide sur un fichier modifié"; fi
echo "ok : une signature ne couvre pas un SHA256SUMS modifié"

# --- 2. SHA256SUMS ne correspond pas ---
d="$(copie somme)"
echo "altéré" >> "$d/wimao_0.4.0-1_amd64.deb"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "somme fausse : accepté"
contient "ne correspond pas à SHA256SUMS" "somme fausse"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "somme fausse : signature écrite malgré l'erreur"
echo "ok : arrêt si un .deb ne correspond pas à SHA256SUMS"

# 2b. Idem pour un RPM dont le contenu ne correspond pas à sa somme.
d="$(copie somme-rpm)"
echo "altéré" >> "$d/wimao-0.4.0-1.x86_64.rpm"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "somme de RPM fausse : acceptée"
contient "ne correspond pas à SHA256SUMS" "somme de RPM fausse"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "somme de RPM fausse : signature écrite"
echo "ok : arrêt si un .rpm ne correspond pas à SHA256SUMS"

# --- 3. .deb en trop dans le dossier ---
d="$(copie en-trop)"
echo "intrus" > "$d/intrus_1_amd64.deb"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec ".deb en trop : accepté"
contient "pas exactement ceux de SHA256SUMS" ".deb en trop"
[ ! -e "$d/SHA256SUMS.asc" ] || echec ".deb en trop : signature écrite"
echo "ok : arrêt si un .deb n'est pas listé dans SHA256SUMS"

# 3b. .rpm en trop dans le dossier (non listé dans SHA256SUMS).
d="$(copie rpm-en-trop)"
echo "intrus" > "$d/intrus-1-1.x86_64.rpm"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec ".rpm en trop : accepté"
contient "pas exactement ceux de SHA256SUMS" ".rpm en trop"
[ ! -e "$d/SHA256SUMS.asc" ] || echec ".rpm en trop : signature écrite"
echo "ok : arrêt si un .rpm n'est pas listé dans SHA256SUMS"

# 3b-bis. Un .rpm remplacé par un lien symbolique vers un fichier de même contenu : sha256sum -c le suit et
# l'accepte, mais ce n'est pas un fichier ordinaire. Le contrôle (find -type f) doit le refuser, de façon
# explicite, plutôt que de signer une liste dont un élément n'est qu'un lien.
d="$(copie rpm-lien)"
cp "$d/wimao-0.4.0-1.x86_64.rpm" "$d/cible-du-lien"
rm "$d/wimao-0.4.0-1.x86_64.rpm"
ln -s cible-du-lien "$d/wimao-0.4.0-1.x86_64.rpm"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec ".rpm en lien symbolique : accepté"
contient "pas exactement ceux de SHA256SUMS" ".rpm en lien symbolique"
[ ! -e "$d/SHA256SUMS.asc" ] || echec ".rpm en lien symbolique : signature écrite"
echo "ok : arrêt si un .rpm est un lien symbolique"

# 3c. RPM absent du brouillon ET de SHA256SUMS (cohérents entre eux, mais incomplets) : refus, rien de signé.
d="$(copie rpm-absent)"
rm "$d"/*.rpm
grep -v '\.rpm' "$d/SHA256SUMS" | LC_ALL=C sort -k2 > "$d/SHA256SUMS.nouveau"
mv "$d/SHA256SUMS.nouveau" "$d/SHA256SUMS"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "RPM absent du brouillon : accepté"
contient "trois \.deb et trois \.rpm sont attendus, il y a 3 \.deb et 0 \.rpm" "RPM absent du brouillon"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "RPM absent du brouillon : signature écrite"
echo "ok : arrêt si les RPM manquent du brouillon"

# 3d. Un seul RPM manque (listé nulle part) : même refus.
d="$(copie rpm-manque-un)"
rm "$d/wimao-wine-11.20+wimao1-1.x86_64.rpm"
grep -v 'wimao-wine-11.20+wimao1-1.x86_64.rpm' "$d/SHA256SUMS" > "$d/SHA256SUMS.nouveau"
mv "$d/SHA256SUMS.nouveau" "$d/SHA256SUMS"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "un RPM manque : accepté"
contient "il y a 3 \.deb et 2 \.rpm" "un RPM manque"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "un RPM manque : signature écrite"
echo "ok : arrêt s'il manque un seul RPM"

# --- 4. Empreinte refabriquée différente ---
d="$(copie difference)"
FAUX_MODE=altere lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "refabrication différente : acceptée"
contient "wimao_0.4.0-1_amd64.deb n'est pas reproductible" "refabrication différente"
contient "rien n'est signé" "refabrication différente"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "refabrication différente : signature écrite"
echo "ok : arrêt si l'empreinte refabriquée diffère"

# --- 5. .deb refabriqué manquant ---
d="$(copie manquant)"
FAUX_MODE=manquant lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "refabrication incomplète : acceptée"
contient "deux \.deb refabriqués attendus" "refabrication incomplète"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "refabrication incomplète : signature écrite"
echo "ok : arrêt si un .deb refabriqué manque"

# 5b. RPM refabriqué différent de celui du brouillon (les .deb concordent) : refus, avec l'explication utile.
d="$(copie rpm-difference)"
FAUX_MODE_RPM=altere lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "RPM refabriqué différent : accepté"
contient "wimao-wine-11.20\+wimao1-1.x86_64.rpm n'est pas reproductible" "RPM refabriqué différent"
contient "rpm-build ou de zstd" "RPM refabriqué différent"
contient "relancez la CI, puis signez peu après" "RPM refabriqué différent"
contient "rien n'est signé" "RPM refabriqué différent"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "RPM refabriqué différent : signature écrite"
echo "ok : arrêt si un RPM refabriqué diffère du brouillon"

# 5c. RPM refabriqué manquant.
d="$(copie rpm-manquant)"
FAUX_MODE_RPM=manquant lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec "refabrication de RPM incomplète : acceptée"
contient "trois \.rpm refabriqués attendus" "refabrication de RPM incomplète"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "refabrication de RPM incomplète : signature écrite"
echo "ok : arrêt si un RPM refabriqué manque"

# --- 6. Usages refusés (code 2 ; mode réel : aucune étape lancée) ---
attendre_usage() {
    local nom="$1" motif="$2"
    shift 2
    lancer "$@"
    [ "$code" -eq 2 ] || { cat "$temporaire/sortie.log" >&2; echec "$nom : code $code au lieu de 2"; }
    contient "$motif" "$nom"
    echo "ok : $nom refusé"
}
d="$(copie usage)"
attendre_usage "sans version" "version manquante"
attendre_usage "version invalide" "invalide" v1.2 --a-blanc --dossier "$d"
attendre_usage "suffixe en mode réel" "invalide" v1.2.3-rc1
attendre_usage "--a-blanc sans --dossier" "exige --dossier" v0.0.0-a-blanc --a-blanc
attendre_usage "--ref sans --a-blanc" "--ref n'est admis qu'avec --a-blanc" v1.2.3 --ref HEAD
attendre_usage "SIGNER_REFABRIQUER en mode réel" "réservé aux tests" v1.2.3
# SIGNER_REFABRIQUER_RPM seul (l'autre variable retirée) est refusé lui aussi en mode réel.
code=0
env -u SIGNER_REFABRIQUER "$signer" v1.2.3 > "$temporaire/sortie.log" 2>&1 || code=$?
[ "$code" -eq 2 ] || { cat "$temporaire/sortie.log" >&2; echec "SIGNER_REFABRIQUER_RPM en mode réel : code $code au lieu de 2"; }
contient "SIGNER_REFABRIQUER_RPM n'est admis qu'avec --a-blanc" "SIGNER_REFABRIQUER_RPM en mode réel"
echo "ok : SIGNER_REFABRIQUER_RPM en mode réel refusé"
attendre_usage "option inconnue" "option inconnue" --nimporte-quoi
[ ! -e "$d/SHA256SUMS.asc" ] || echec "usage refusé : signature écrite"

# --- 7. Mode réel sans étiquette : refus avant tout appel (SIGNER_REFABRIQUER retiré pour passer l'usage) ---
lancer_reel() {
    code=0
    env -u SIGNER_REFABRIQUER -u SIGNER_REFABRIQUER_RPM "$signer" "$@" > "$temporaire/sortie.log" 2>&1 || code=$?
}
lancer_reel v9.9.9
[ "$code" -eq 1 ] || { cat "$temporaire/sortie.log" >&2; echec "mode réel sans étiquette : code $code au lieu de 1"; }
contient "modifications non enregistrées|n'existe pas localement" "mode réel sans étiquette"
echo "ok : mode réel refusé sans dépôt propre et étiquette"

# --- 8. Mode réel complet : empreinte publiée, commit attesté, clé publique jointe ---
# Un dépôt de test (le script y est copié : il se repère par son propre emplacement) avec une clé à sous-clé de
# signature comme celle du propriétaire, une étiquette signée EN OPENPGP, un faux `gh` qui consigne ses
# arguments (et affiche une aide qui annonce les options nécessaires) et un faux `podman` qui « refabrique ».
# Aucun réseau, aucun vrai gh ni podman. Configuration git globale ignorée (elle pourrait imposer une
# signature SSH des commits, comme chez le propriétaire).
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
gpg --batch --quiet --pinentry-mode loopback --passphrase '' --quick-gen-key "WiMao essai principale <principale@example.invalid>" ed25519 cert never 2> /dev/null
principale="$(gpg --list-keys --with-colons principale@example.invalid | awk -F: '$1 == "fpr" { print $10; exit }')"
gpg --batch --quiet --pinentry-mode loopback --passphrase '' --quick-add-key "$principale" ed25519 sign never 2> /dev/null
# Empreinte de la clé « autre » : la clé jetable sans sous-clé du début (signataire illégitime).
autre="$(gpg --list-keys --with-colons essai@example.invalid | awk -F: '$1 == "fpr" { print $10; exit }')"
if [ -z "$principale" ] || [ -z "$autre" ] || [ "$principale" = "$autre" ]; then
    echec "clés d'essai : empreintes inattendues"
fi

depot="$temporaire/depot-reel"
mkdir -p "$depot/scripts/lib" "$depot/docs" "$depot/.github/workflows"
cp "$signer" "$depot/scripts/signer-version.sh"
# La bibliothèque commune que signer-version.sh source (chemin relatif à lui-même).
cp "$racine/scripts/lib/publication.sh" "$depot/scripts/lib/publication.sh"
gpg --armor --export "$principale" > "$depot/docs/cle-publique.asc"
cat > "$depot/.github/workflows/paquets.yml" << 'EOF'
jobs:
  wimao:
    container:
      image: ubuntu:24.04@sha256:0000000000000000000000000000000000000000000000000000000000000000
    steps:
      - with:
          toolchain: 1.99.1
  rpm:
    container:
      image: registry.fedoraproject.org/fedora:45@sha256:1111111111111111111111111111111111111111111111111111111111111111
EOF
# Écrit SECURITY.md avec l'empreinte donnée ($1 ; vide = aucune ligne d'empreinte).
ecrire_securite() {
    { echo "# Politique de sécurité"; [ -z "$1" ] || echo "Empreinte de la clé de signature : $1"; } > "$depot/SECURITY.md"
}
git_depot() { git -C "$depot" -c user.name=essai -c user.email=essai@example.invalid -c commit.gpgsign=false "$@"; }
git_depot init --quiet
ecrire_securite "${principale:0:20} ${principale:20}"   # avec une espace au milieu : admise
git_depot add -A
git_depot commit --quiet -m "version à signer"
commit_version="$(git -C "$depot" rev-parse HEAD)"
# Étiquette : sous-clé de la clé principale, en OpenPGP (-u <empreinte principale> choisit la sous-clé de signature).
git_depot -c gpg.format=openpgp tag -s -u "$principale" v1.2.3 -m "WiMao 1.2.3" 2> /dev/null
# Un commit de plus APRÈS l'étiquette : HEAD n'est plus le commit de l'étiquette (le test est discriminant).
echo "suite" > "$depot/suite.txt"
git_depot add -A
git_depot commit --quiet -m "commit postérieur à l'étiquette"
[ "$(git -C "$depot" rev-parse HEAD)" != "$commit_version" ] || echec "dépôt de test : HEAD devrait différer du commit de l'étiquette"

mkdir "$temporaire/bin-reel"
journal_reel="$temporaire/appels-gh-reel.log"
cat > "$temporaire/bin-reel/gh" << EOF
#!/usr/bin/env bash
# Faux gh du mode réel : consigne chaque appel ; aide minimale pour la vérification des options.
if [ "\$1 \$2 \$3" = "attestation verify --help" ]; then
    echo "--signer-workflow --source-ref --source-digest --deny-self-hosted-runners"
    exit 0
fi
echo "gh \$*" >> "$journal_reel"
EOF
chmod +x "$temporaire/bin-reel/gh"
# Faux gpg : passe tout au vrai gpg (même trousseau jetable), sauf qu'il CONSIGNE le déverrouillage
# (--clearsign) dans le même journal que gh, pour prouver l'ORDRE : déverrouillage avant tout le reste. Avec
# FAUX_GPG_CLEARSIGN_ECHEC=1, ce déverrouillage échoue (phrase de passe refusée). Le texte à signer passe par
# l'entrée standard : le faux la consomme pour ne pas casser le tube.
vrai_gpg="$(type -P gpg)"
cat > "$temporaire/bin-reel/gpg" << EOF
#!/usr/bin/env bash
for argument in "\$@"; do
    if [ "\$argument" = "--clearsign" ]; then
        echo "gpg \$*" >> "$journal_reel"
        if [ "\${FAUX_GPG_CLEARSIGN_ECHEC:-0}" = "1" ]; then cat > /dev/null; exit 2; fi
        break
    fi
done
exec "$vrai_gpg" "\$@"
EOF
chmod +x "$temporaire/bin-reel/gpg"
# Faux podman : trouve les volumes montés. Avec « …:/debs:ro » c'est la refabrication des RPM (image Fedora) :
# il y recopie les trois .rpm « refabriqués » et consigne l'image utilisée ; sinon ce sont les deux .deb.
cat > "$temporaire/bin-reel/podman" << EOF
#!/usr/bin/env bash
sortie=""
debs=""
image=""
while [ "\$#" -gt 0 ]; do
    case "\$1" in
        --volume) case "\$2" in *:/sortie:rw) sortie="\${2%:/sortie:rw}" ;; *:/debs:ro) debs="\${2%:/debs:ro}" ;; esac; shift ;;
        --env) shift ;;
        run | --rm) ;;
        *) image="\$1"; break ;;
    esac
    shift
done
if [ -n "\$debs" ]; then
    echo "podman rpm \$image debs=\$(ls "\$debs"/*.deb | wc -l)" >> "$journal_reel"
    cp "$originaux"/*.rpm "\$sortie/"
else
    echo "podman deb \$image" >> "$journal_reel"
    cp "$originaux"/wimao_*.deb "$originaux"/wimao-bridge_*.deb "\$sortie/"
fi
EOF
chmod +x "$temporaire/bin-reel/podman"

lancer_depot() {
    code=0
    env -u SIGNER_REFABRIQUER -u SIGNER_REFABRIQUER_RPM PATH="$temporaire/bin-reel:$PATH" "$depot/scripts/signer-version.sh" "$@" > "$temporaire/sortie.log" 2>&1 || code=$?
}

# 8a. Cas nominal : tout passe, --source-digest reçoit le commit de l'ÉTIQUETTE (pas HEAD).
d="$(copie reel)"
: > "$journal_reel"
lancer_depot v1.2.3 --dossier "$d"
[ "$code" -eq 0 ] || { cat "$temporaire/sortie.log" >&2; echec "mode réel : code $code au lieu de 0"; }
verifs="$(grep -c '^gh attestation verify ' "$journal_reel" || true)"
[ "$verifs" -eq 6 ] || { cat "$journal_reel" >&2; echec "mode réel : 6 vérifications d'attestation attendues (3 .deb + 3 .rpm), $verifs faites"; }
for nom_paquet in wimao_0.4.0-1_amd64.deb wimao-wine_11.20+wimao1_amd64.deb wimao-0.4.0-1.x86_64.rpm wimao-bridge-5.1.1+dxvk3.1.1-1.x86_64.rpm wimao-wine-11.20+wimao1-1.x86_64.rpm; do
    grep -q "^gh attestation verify .*/$nom_paquet " "$journal_reel" \
        || { cat "$journal_reel" >&2; echec "mode réel : pas d'attestation vérifiée pour $nom_paquet"; }
done
# Déverrouillage : premier événement du journal (avant gh et podman), signature jetable vers la sortie nulle.
premier="$(head -n 1 "$journal_reel")"
case "$premier" in
    "gpg "*"--clearsign"*) ;;
    *) cat "$journal_reel" >&2; echec "mode réel : la clé doit être déverrouillée avant tout autre appel (premier : $premier)" ;;
esac
case "$premier" in *"-u $principale"*) ;; *) echec "mode réel : déverrouillage sans la clé publiée dans SECURITY.md : $premier" ;; esac
nb_clearsign="$(grep -c -- '--clearsign' "$journal_reel" || true)"
[ "$nb_clearsign" -eq 1 ] || echec "mode réel : un seul déverrouillage attendu, $nb_clearsign"
# Refabrication : .deb puis RPM, ces derniers dans l'image Fedora 45 du job `rpm`, avec les trois .deb vérifiés,
# et sans que gh y participe.
grep -q '^podman deb ' "$journal_reel" || echec "mode réel : refabrication des .deb absente"
grep -q '^podman rpm registry.fedoraproject.org/fedora:45@sha256:1111111111111111111111111111111111111111111111111111111111111111 debs=3$' "$journal_reel" \
    || { cat "$journal_reel" >&2; echec "mode réel : refabrication des RPM absente ou hors de l'image du job rpm"; }
[ "$(grep -n '^podman deb ' "$journal_reel" | cut -d: -f1)" -lt "$(grep -n '^podman rpm ' "$journal_reel" | cut -d: -f1)" ] \
    || echec "mode réel : les RPM doivent être refabriqués après les .deb"
contient "Déverrouillage de la clé" "mode réel"
contient "wimao-wine-11.20\+wimao1-1.x86_64.rpm" "mode réel (RPM comparés)"
while IFS= read -r ligne; do
    case "$ligne" in *"--source-digest $commit_version "* | *"--source-digest $commit_version") ;; *) echec "mode réel : --source-digest n'a pas reçu le commit de l'étiquette : $ligne" ;; esac
    case "$ligne" in *"--source-ref refs/tags/v1.2.3"*) ;; *) echec "mode réel : --source-ref absent : $ligne" ;; esac
    case "$ligne" in *"--deny-self-hosted-runners"*) ;; *) echec "mode réel : --deny-self-hosted-runners absent : $ligne" ;; esac
done < <(grep '^gh attestation verify ' "$journal_reel")
# Envoi : la signature ET la clé publique.
grep -q "^gh release upload v1.2.3 .*SHA256SUMS.asc .*docs/cle-publique.asc " "$journal_reel" \
    || { cat "$journal_reel" >&2; echec "mode réel : SHA256SUMS.asc et cle-publique.asc devraient être envoyés ensemble"; }
# Signature faite par la clé publiée (clé principale, via sa sous-clé), et affichée.
signataire="$(gpg --status-fd 1 --verify "$d/SHA256SUMS.asc" "$d/SHA256SUMS" 2> /dev/null | awk '$2 == "VALIDSIG" { print $12; exit }')"
[ "$signataire" = "$principale" ] || echec "mode réel : SHA256SUMS signé par « $signataire » au lieu de $principale"
contient "Clé qui a signé .*: $principale" "mode réel"
echo "ok : mode réel : --source-digest = commit de l'étiquette, clé publique jointe, signature de la clé publiée"

# 8a'. RPM refabriqué différent en mode réel : refus, rien n'est envoyé. Le faux podman recopie les RPM
# d'origine ; on altère le brouillon pour qu'ils ne concordent plus (la somme est recalculée, donc l'écart
# n'est visible qu'à la refabrication, comme une mise à jour de rpm-build).
d="$(copie reel-rpm-ecart)"
echo "autre octets" >> "$d/wimao-wine-11.20+wimao1-1.x86_64.rpm"
(cd "$d" && LC_ALL=C sha256sum -- *.deb *.rpm | LC_ALL=C sort -k2 > SHA256SUMS)
: > "$journal_reel"
lancer_depot v1.2.3 --dossier "$d"
[ "$code" -eq 1 ] || { cat "$temporaire/sortie.log" >&2; echec "mode réel, écart de RPM : code $code au lieu de 1"; }
contient "wimao-wine-11.20\+wimao1-1.x86_64.rpm n'est pas reproductible" "mode réel, écart de RPM"
contient "rpm-build ou de zstd" "mode réel, écart de RPM"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "mode réel, écart de RPM : signature écrite"
if grep -q '^gh release upload' "$journal_reel"; then echec "mode réel, écart de RPM : envoi malgré l'écart"; fi
echo "ok : mode réel : un écart de RPM arrête avant la signature et l'envoi"

# 8a''. Le déverrouillage échoue : arrêt immédiat, avant téléchargement, refabrication et signature.
d="$(copie reel-deverrouillage)"
: > "$journal_reel"
FAUX_GPG_CLEARSIGN_ECHEC=1 lancer_depot v1.2.3 --dossier "$d"
[ "$code" -eq 1 ] || { cat "$temporaire/sortie.log" >&2; echec "déverrouillage refusé : code $code au lieu de 1"; }
contient "impossible de déverrouiller la clé de signature" "déverrouillage refusé"
if grep -qE '^(gh|podman)' "$journal_reel"; then cat "$journal_reel" >&2; echec "déverrouillage refusé : gh ou podman appelé malgré l'échec"; fi
[ ! -e "$d/SHA256SUMS.asc" ] || echec "déverrouillage refusé : signature écrite"
echo "ok : mode réel : un déverrouillage refusé arrête le script tout de suite"

# 8b. Étiquette signée par une AUTRE clé du trousseau (valide, mais pas la publiée) : refus.
git_depot tag -d v1.2.3 > /dev/null
git_depot -c gpg.format=openpgp tag -s -u "$autre" v1.2.3 "$commit_version" -m "WiMao 1.2.3" 2> /dev/null
d="$(copie autre-cle)"
: > "$journal_reel"
lancer_depot v1.2.3 --dossier "$d"
[ "$code" -eq 1 ] || { cat "$temporaire/sortie.log" >&2; echec "autre clé : code $code au lieu de 1"; }
contient "pas par la clé publiée dans SECURITY.md" "autre clé"
[ ! -s "$journal_reel" ] || echec "autre clé : gh appelé malgré le refus"
[ ! -e "$d/SHA256SUMS.asc" ] || echec "autre clé : signature écrite"
echo "ok : étiquette signée par une autre clé que la publiée refusée"

# 8c. SECURITY.md sans empreinte publiée : le mode réel s'arrête avec un message clair.
ecrire_securite ""
git_depot add -A
git_depot commit --quiet -m "SECURITY.md sans empreinte"
lancer_depot v1.2.3 --dossier "$d"
[ "$code" -eq 1 ] || { cat "$temporaire/sortie.log" >&2; echec "sans empreinte : code $code au lieu de 1"; }
contient "ne publie pas encore l'empreinte" "sans empreinte"
echo "ok : mode réel refusé tant que SECURITY.md ne publie pas d'empreinte"

# --- 9. Aucun appel à gh en --a-blanc, dans aucun des cas ---
if [ -s "$journal_gh" ]; then
    cat "$journal_gh" >&2
    echec "gh a été appelé (voir ci-dessus)"
fi
echo "ok : aucun appel à gh"
echo "Tous les cas de signer-version.sh se comportent comme prévu."
