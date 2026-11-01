#!/usr/bin/env bash
# Teste scripts/signer-version.sh en mode --a-blanc, SANS podman ni réseau : la refabrication est simulée
# par SIGNER_REFABRIQUER, le trousseau GnuPG est jetable et un faux `gh` (qui échoue bruyamment et se
# consigne) est placé en tête du PATH pour prouver qu'aucun appel à gh n'a lieu à blanc.
#
# Cas couverts : signature valide (gpg --verify) ; arrêt si un .deb ne correspond pas à SHA256SUMS ;
# arrêt si un .deb est en trop ; arrêt si l'empreinte refabriquée diffère ; arrêt si un .deb refabriqué
# manque ; refus des usages invalides (mode réel sans étiquette, SIGNER_REFABRIQUER en mode réel, --ref
# sans --a-blanc, --a-blanc sans --dossier) ; aucun appel à gh nulle part en --a-blanc.
# Mode réel (dépôt de test, faux gh et faux podman qui se consignent, étiquette signée en OpenPGP par une
# sous-clé) : --source-digest reçoit le commit de l'étiquette et non HEAD ; la clé publique est envoyée avec
# la signature ; une étiquette signée par une autre clé que la publiée dans SECURITY.md est refusée ; sans
# empreinte publiée, arrêt.
# La refabrication réelle par podman n'est PAS testée ici : le job `signature-a-blanc` de
# .github/workflows/paquets.yml la prouve de bout en bout sur les vrais .deb de la CI.
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

# --- Faux artefacts : trois .deb, SHA256SUMS comme le fait le job `sommes` ---
originaux="$temporaire/originaux"
mkdir "$originaux"
for nom in wimao_0.4.0-1_amd64.deb wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb wimao-wine_11.20+wimao1_amd64.deb; do
    echo "contenu de $nom" > "$originaux/$nom"
done
(cd "$originaux" && LC_ALL=C sha256sum -- *.deb | LC_ALL=C sort -k2 > SHA256SUMS)

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
echo "ok : signature valide en mode à blanc"

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

# --- 3. .deb en trop dans le dossier ---
d="$(copie en-trop)"
echo "intrus" > "$d/intrus_1_amd64.deb"
lancer v0.0.0-a-blanc --a-blanc --dossier "$d"
[ "$code" -ne 0 ] || echec ".deb en trop : accepté"
contient "pas exactement ceux de SHA256SUMS" ".deb en trop"
[ ! -e "$d/SHA256SUMS.asc" ] || echec ".deb en trop : signature écrite"
echo "ok : arrêt si un .deb n'est pas listé dans SHA256SUMS"

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
attendre_usage "option inconnue" "option inconnue" --nimporte-quoi
[ ! -e "$d/SHA256SUMS.asc" ] || echec "usage refusé : signature écrite"

# --- 7. Mode réel sans étiquette : refus avant tout appel (SIGNER_REFABRIQUER retiré pour passer l'usage) ---
lancer_reel() {
    code=0
    env -u SIGNER_REFABRIQUER "$signer" "$@" > "$temporaire/sortie.log" 2>&1 || code=$?
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
mkdir -p "$depot/scripts" "$depot/docs" "$depot/.github/workflows"
cp "$signer" "$depot/scripts/signer-version.sh"
gpg --armor --export "$principale" > "$depot/docs/cle-publique.asc"
cat > "$depot/.github/workflows/paquets.yml" << 'EOF'
jobs:
  wimao:
    container:
      image: ubuntu:24.04@sha256:0000000000000000000000000000000000000000000000000000000000000000
    steps:
      - with:
          toolchain: 1.99.1
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
# Faux podman : trouve « --volume <dossier>:/sortie:rw » et y recopie les deux .deb « refabriqués ».
cat > "$temporaire/bin-reel/podman" << EOF
#!/usr/bin/env bash
while [ "\$#" -gt 0 ]; do
    case "\$1" in
        --volume) case "\$2" in *:/sortie:rw) sortie="\${2%:/sortie:rw}" ;; esac; shift ;;
    esac
    shift
done
cp "$originaux"/wimao_*.deb "$originaux"/wimao-bridge_*.deb "\$sortie/"
EOF
chmod +x "$temporaire/bin-reel/podman"

lancer_depot() {
    code=0
    env -u SIGNER_REFABRIQUER PATH="$temporaire/bin-reel:$PATH" "$depot/scripts/signer-version.sh" "$@" > "$temporaire/sortie.log" 2>&1 || code=$?
}

# 8a. Cas nominal : tout passe, --source-digest reçoit le commit de l'ÉTIQUETTE (pas HEAD).
d="$(copie reel)"
: > "$journal_reel"
lancer_depot v1.2.3 --dossier "$d"
[ "$code" -eq 0 ] || { cat "$temporaire/sortie.log" >&2; echec "mode réel : code $code au lieu de 0"; }
verifs="$(grep -c '^gh attestation verify ' "$journal_reel" || true)"
[ "$verifs" -eq 3 ] || { cat "$journal_reel" >&2; echec "mode réel : 3 vérifications d'attestation attendues, $verifs faites"; }
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
