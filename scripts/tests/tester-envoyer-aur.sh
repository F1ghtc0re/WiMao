#!/usr/bin/env bash
# Teste scripts/envoyer-aur.sh. Recette d'essai fabriquée par paquets/fabriquer-pkgbuild.sh (clé GnuPG jetable,
# faux SHA256SUMS) puis rangée dans un faux dépôt WiMao (git local) ; faux AUR = dépôt git NU local. Il faut
# podman : le .SRCINFO est produit (et contrôlé par le script) dans l'image Arch figée. Aucun réseau pour
# l'envoi, aucun SSH, aucune écriture hors du dossier temporaire.
#
# Cas, tous à blanc (--a-blanc) :
#   - faux AUR VIDE (premier envoi) : sa branche master contient exactement PKGBUILD et .SRCINFO de la racine,
#     commit « wimao-bin X.Y.Z-1 », auteur pseudonyme/noreply, même avec une configuration git globale
#     différente et une signature de commits exigée ;
#   - deuxième envoi identique : « rien à envoyer », code 0, aucun nouveau commit ;
#   - recette modifiée puis renvoyée : un second commit, dont le diff n'est que la modification ;
#   - refus (code 1) : pkgver différent de la version demandée, PKGBUILD avec la marque d'essai local (même
#     suivie d'un espace), .SRCINFO modifié d'une ligne, racine non propre (fichier suivi modifié), recette
#     absente ; dernière barrière (recette et .SRCINFO cohérents, mais) : validpgpkeys différent de l'empreinte
#     attendue, source hors de la Release de la version, nombre de sources différent de 5, `install=`,
#     prepare() sans `sha256sum -c SHA256SUMS` ; un refus ne touche pas au faux AUR ;
#   - faux AUR contenant un fichier en trop : retiré par l'envoi (et même quand la recette est déjà à jour) ;
#   - usage (code 2).
# Mode réel avec un faux git : refus hors de main, et clé ~/.ssh/aur absente (HOME d'essai) ; aucun clone, aucun
# envoi n'a eu lieu.
#
# Usage     : scripts/tests/tester-envoyer-aur.sh
# Variables : aucune (GNUPGHOME, HOME et PATH sont réglés par le script, pour lui seul).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin. Jamais dans ~ ni dans le dépôt.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine_depot="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
envoyer="$racine_depot/scripts/envoyer-aur.sh"
fabriquer="$racine_depot/paquets/fabriquer-pkgbuild.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

[ -f "$envoyer" ] || echec "scripts/envoyer-aur.sh est introuvable"
[ -f "$racine_depot/paquets/aur/aur.archlinux.org.known_hosts" ] || echec "paquets/aur/aur.archlinux.org.known_hosts est introuvable"
command -v podman > /dev/null || echec "podman est nécessaire à cet essai"

version="$(awk -F'"' '/^version = / { print $2; exit }' "$racine_depot/Cargo.toml")"
etiquette="v$version"

# --- Recette d'essai (clé jetable, faux SHA256SUMS à trois .deb) ---
export GNUPGHOME="$temporaire/gnupg"
mkdir -m 700 "$GNUPGHOME"
cat > "$temporaire/a.params" << 'PARAMS'
%no-protection
Key-Type: EDDSA
Key-Curve: ed25519
Key-Usage: sign
Name-Real: WiMao essai A
Name-Email: a@example.invalid
Expire-Date: 1d
%commit
PARAMS
gpg --batch --quiet --gen-key "$temporaire/a.params" 2> /dev/null
empreinte_a="$(gpg --with-colons --list-keys a@example.invalid 2> /dev/null | awk -F: '$1 == "fpr" { print $10; exit }')"
n=0
for deb in "wimao_${version}-1_amd64.deb" "wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb" "wimao-wine_11.20+wimao1_amd64.deb"; do
    n=$((n + 1))
    printf '%s  %s\n' "$(printf '%064d' 0 | tr 0 "$n")" "$deb" >> "$temporaire/SHA256SUMS"
done
"$fabriquer" "$temporaire/SHA256SUMS" "$temporaire/recette" --cle "$empreinte_a" > /dev/null \
    || echec "la fabrication de la recette d'essai a échoué"

# Identité git de l'essai : volontairement différente de l'auteur attendu, avec signature exigée mais sans clé.
# Si le script en dépendait, ses commits échoueraient ou porteraient cet auteur.
export GIT_CONFIG_GLOBAL="$temporaire/gitconfig"
export GIT_CONFIG_SYSTEM=/dev/null
git config --file "$GIT_CONFIG_GLOBAL" user.name "Quelqu'un d'autre"
git config --file "$GIT_CONFIG_GLOBAL" user.email "autre@example.invalid"
git config --file "$GIT_CONFIG_GLOBAL" commit.gpgsign true
git config --file "$GIT_CONFIG_GLOBAL" gpg.program /bin/false

# git_essai <dossier> <arguments> : git pour construire les dépôts d'essai (sans signature).
git_essai() {
    local d="$1"
    shift
    git -C "$d" -c user.name=essai -c user.email=essai@example.invalid -c commit.gpgsign=false "$@"
}

# nouvelle_racine <nom> : faux dépôt WiMao (git, branche main) avec la recette d'essai enregistrée. Rend son chemin.
nouvelle_racine() {
    local r="$temporaire/racine-$1"
    rm -rf "$r"
    mkdir -p "$r/paquets/aur/wimao-bin"
    cp -- "$temporaire/recette/PKGBUILD" "$temporaire/recette/.SRCINFO" "$r/paquets/aur/wimao-bin/"
    git init --quiet --initial-branch=main "$r"
    git_essai "$r" add -A
    git_essai "$r" commit --quiet -m "recette d'essai"
    printf '%s' "$r"
}

# nouvel_aur <nom> : faux AUR, dépôt nu VIDE (comme un paquet qui n'a jamais été envoyé).
nouvel_aur() {
    local a="$temporaire/aur-$1.git"
    rm -rf "$a"
    git init --quiet --bare --initial-branch=master "$a"
    printf '%s' "$a"
}

# lancer <arguments> : exécute le script ; sortie dans $temporaire/sortie.log, code dans $code.
# En mode à blanc, l'empreinte attendue (celle de la clé jetable) est donnée par --cle, sauf si l'appel la fixe.
lancer() {
    local args=("$@") arg
    for arg in "$@"; do
        if [ "$arg" = "--a-blanc" ] && [[ " $* " != *" --cle "* ]]; then
            args+=(--cle "$empreinte_a")
        fi
    done
    code=0
    "$envoyer" "${args[@]}" > "$temporaire/sortie.log" 2>&1 || code=$?
}

# racine_modifiee <nom> <expression sed sur le PKGBUILD> : comme nouvelle_racine, mais le PKGBUILD est modifié
# puis le .SRCINFO REGÉNÉRÉ (donc cohérent) et le tout enregistré : seule la dernière barrière peut refuser.
racine_modifiee() {
    local r
    r="$(nouvelle_racine "$1")"
    sed -i "$2" "$r/paquets/aur/wimao-bin/PKGBUILD"
    ! git_essai "$r" diff --quiet || echec "$1 : l'expression sed n'a rien modifié (motif périmé)"
    srcinfo_de "$r/paquets/aur/wimao-bin"
    git_essai "$r" commit --quiet -am "$1"
    printf '%s' "$r"
}
# srcinfo_de <dossier> : régénère <dossier>/.SRCINFO depuis son PKGBUILD (conteneur Arch figé, comme le script).
srcinfo_de() {
    (
        # shellcheck source=paquets/aur/commun-aur.sh
        source "$racine_depot/paquets/aur/commun-aur.sh"
        srcinfo "$1"
    )
}
attendre_code() {
    # shellcheck disable=SC2031  # faux positif : `code` est réglé par lancer, jamais dans un sous-shell
    [ "$code" -eq "$1" ] || { cat "$temporaire/sortie.log" >&2; echec "$2 : code $code au lieu de $1"; }
}
attendre_message() {
    grep -qF -- "$1" "$temporaire/sortie.log" || { cat "$temporaire/sortie.log" >&2; echec "$2 : message « $1 » absent"; }
}
# nb_commits <faux AUR> : nombre de commits de master (0 si la branche n'existe pas).
nb_commits() {
    git -C "$1" rev-list --count master 2> /dev/null || echo 0
}

# --- 1. Premier envoi : faux AUR vide ---
racine="$(nouvelle_racine ok)"
aur="$(nouvel_aur ok)"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$racine"
attendre_code 0 "premier envoi"
[ "$(nb_commits "$aur")" -eq 1 ] || echec "le faux AUR devrait avoir 1 commit, il en a $(nb_commits "$aur")"
[ "$(git -C "$aur" ls-tree --name-only master | LC_ALL=C sort | paste -sd" ")" = ".SRCINFO PKGBUILD" ] \
    || echec "master ne contient pas exactement PKGBUILD et .SRCINFO"
for f in PKGBUILD .SRCINFO; do
    git -C "$aur" show "master:$f" | cmp -s - "$racine/paquets/aur/wimao-bin/$f" || echec "$f diffère de la racine"
done
[ "$(git -C "$aur" log -1 --format=%s master)" = "wimao-bin $version-1" ] || echec "message de commit incorrect : $(git -C "$aur" log -1 --format=%s master)"
[ "$(git -C "$aur" log -1 --format='%an <%ae>' master)" = "F1ghtc0re <162480684+F1ghtc0re@users.noreply.github.com>" ] \
    || echec "auteur incorrect : $(git -C "$aur" log -1 --format='%an <%ae>' master)"
[ "$(git -C "$aur" log -1 --format='%cn <%ce>' master)" = "F1ghtc0re <162480684+F1ghtc0re@users.noreply.github.com>" ] \
    || echec "committer incorrect"
[ "$(git -C "$aur" log -1 --format=%G? master)" = "N" ] || echec "le commit de l'AUR ne devrait pas être signé"

# --- 2. Deuxième envoi identique : rien à envoyer ---
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$racine"
attendre_code 0 "second envoi identique"
attendre_message "Rien à envoyer" "second envoi identique"
[ "$(nb_commits "$aur")" -eq 1 ] || echec "un envoi identique a créé un commit"

# --- 3. Recette modifiée (un autre pkgrel) : un second commit ---
sed -i 's/^pkgrel=1$/pkgrel=2/' "$racine/paquets/aur/wimao-bin/PKGBUILD"
cp -- "$racine/paquets/aur/wimao-bin/PKGBUILD" "$temporaire/majour-PKGBUILD"
mkdir "$temporaire/majour"
mv "$temporaire/majour-PKGBUILD" "$temporaire/majour/PKGBUILD"
(
    # shellcheck source=paquets/aur/commun-aur.sh
    source "$racine_depot/paquets/aur/commun-aur.sh"
    srcinfo "$temporaire/majour"
)
cp -- "$temporaire/majour/.SRCINFO" "$racine/paquets/aur/wimao-bin/.SRCINFO"
git_essai "$racine" commit --quiet -am "pkgrel 2"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$racine"
attendre_code 0 "envoi d'une recette modifiée"
[ "$(nb_commits "$aur")" -eq 2 ] || echec "le faux AUR devrait avoir 2 commits"
[ "$(git -C "$aur" log -1 --format=%s master)" = "wimao-bin $version-2" ] || echec "message du second commit incorrect"

# --- 4. Refus : le faux AUR doit rester intact ---
avant="$(git -C "$aur" rev-parse master)"
# pkgver différent de la version demandée
lancer v9.9.9 --a-blanc --aur "$aur" --racine "$racine"
attendre_code 1 "pkgver différent"
attendre_message "annonce pkgver=$version et non 9.9.9" "pkgver différent"

# marque d'essai local (enregistrée : la racine est propre, c'est la marque qui est refusée)
r="$(nouvelle_racine essai-local)"
sed -i '2i # ESSAI LOCAL, NE PAS PUBLIER' "$r/paquets/aur/wimao-bin/PKGBUILD"
git_essai "$r" commit --quiet -am "marque"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "marque d'essai local"
attendre_message "marque d'essai local" "marque d'essai local"

# .SRCINFO modifié d'une ligne (enregistré)
r="$(nouvelle_racine srcinfo-modifie)"
sed -i 's/^\tpkgdesc = .*/\tpkgdesc = modifié à la main/' "$r/paquets/aur/wimao-bin/.SRCINFO"
git_essai "$r" commit --quiet -am "srcinfo modifié"
! git_essai "$r" diff --quiet HEAD~1 HEAD || echec "l'essai n'a rien modifié dans .SRCINFO (motif périmé)"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 ".SRCINFO modifié"
attendre_message ".SRCINFO ne correspond pas au PKGBUILD" ".SRCINFO modifié"

# racine non propre (fichier suivi modifié, non enregistré)
r="$(nouvelle_racine pas-propre)"
echo "# ajout" >> "$r/paquets/aur/wimao-bin/PKGBUILD"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "racine pas propre"
attendre_message "modifications non enregistrées" "racine pas propre"

# marque d'essai local suivie d'un espace (la recherche est par sous-chaîne, pas par ligne exacte)
r="$(racine_modifiee marque-espace '2i # ESSAI LOCAL, NE PAS PUBLIER ')"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "marque avec espace final"
attendre_message "marque d'essai local" "marque avec espace final"

# (a) validpgpkeys du PKGBUILD différent de l'empreinte attendue (recette cohérente, .SRCINFO régénéré)
r="$(racine_modifiee autre-cle "s/^validpgpkeys=(.*)\$/validpgpkeys=(0000000000000000000000000000000000000000)/")"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "validpgpkeys différent"
attendre_message "n'est pas l'empreinte publiée" "validpgpkeys différent"
# (a) idem par l'empreinte attendue : la recette saine est refusée si l'empreinte attendue est une autre
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$racine" --cle 1111111111111111111111111111111111111111
attendre_code 1 "empreinte attendue différente"
attendre_message "n'est pas l'empreinte publiée" "empreinte attendue différente"
# (b) source hors de la Release de la version (adresse de la première source changée, dans le PKGBUILD)
r="$(racine_modifiee source-etrangere 's#^  https://github.com/F1ghtc0re/WiMao/releases/download/\(.*\.deb\)$#  https://example.invalid/\1#')"
# (b) une source de trop (6 au lieu de 5), à l'adresse pourtant correcte
r="$(nouvelle_racine source-en-trop)"
sed -i '0,/^\tsource = /s##\tsource = https://github.com/F1ghtc0re/WiMao/releases/download/v'"$version"'/en-trop.txt\n&#' "$r/paquets/aur/wimao-bin/.SRCINFO"
git_essai "$r" commit --quiet -am "source en trop"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "source en trop"
attendre_message "exactement 5 lignes" "source en trop"
# (c) install= déclaré dans le PKGBUILD seul, puis « install = » dans le .SRCINFO seul (modifications directes :
# makepkg refuserait de régénérer un .SRCINFO avec un fichier install absent ; la barrière passe avant)
r="$(nouvelle_racine avec-install)"
sed -i 's/^pkgrel=1$/pkgrel=1\ninstall=wimao.install/' "$r/paquets/aur/wimao-bin/PKGBUILD"
git_essai "$r" commit --quiet -am "install= dans le PKGBUILD"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "install= dans le PKGBUILD"
attendre_message "déclare install=" "install= dans le PKGBUILD"
r="$(nouvelle_racine avec-install-srcinfo)"
sed -i 's/^\tpkgdesc = .*/&\n\tinstall = wimao.install/' "$r/paquets/aur/wimao-bin/.SRCINFO"
git_essai "$r" commit --quiet -am "install dans le .SRCINFO"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "install dans le .SRCINFO"
attendre_message "script install=" "install dans le .SRCINFO"
# (d) prepare() sans la vérification des empreintes
r="$(racine_modifiee sans-verification 's/sha256sum -c SHA256SUMS --ignore-missing/true/')"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "prepare sans sha256sum"
attendre_message "sha256sum -c SHA256SUMS" "prepare sans sha256sum"

# recette absente
r="$temporaire/racine-vide"
mkdir "$r"
git init --quiet --initial-branch=main "$r"
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$r"
attendre_code 1 "recette absente"
attendre_message "la recette est absente" "recette absente"

[ "$(git -C "$aur" rev-parse master)" = "$avant" ] || echec "un refus a modifié le faux AUR"
# Un refus avant le clone n'écrit rien dans un faux AUR vide
aur_vide="$(nouvel_aur vide)"
lancer v9.9.9 --a-blanc --aur "$aur_vide" --racine "$racine"
attendre_code 1 "refus sur faux AUR vide"
[ "$(nb_commits "$aur_vide")" -eq 0 ] || echec "un refus a rempli le faux AUR vide"

# Fichier en trop dans le faux AUR : retiré par l'envoi, et un AUR à jour mais encombré n'est PAS « rien à envoyer ».
r="$(nouvelle_racine fichier-en-trop)"
a_trop="$(nouvel_aur fichier-en-trop)"
lancer "$etiquette" --a-blanc --aur "$a_trop" --racine "$r"
attendre_code 0 "premier envoi (fichier en trop)"
trop="$temporaire/clone-trop"
git clone --quiet "$a_trop" "$trop"
echo "intrus" > "$trop/extra.sh"
git_essai "$trop" add extra.sh
git_essai "$trop" commit --quiet -m "fichier ajouté"
git_essai "$trop" push --quiet origin master
lancer "$etiquette" --a-blanc --aur "$a_trop" --racine "$r"
attendre_code 0 "AUR avec un fichier en trop"
attendre_message "Fichier en trop sur l'AUR, retiré : extra.sh" "AUR avec un fichier en trop"
! grep -q "Rien à envoyer" "$temporaire/sortie.log" || echec "un AUR avec un fichier en trop a été pris pour « rien à envoyer »"
[ "$(git -C "$a_trop" ls-tree -r --name-only master | LC_ALL=C sort | paste -sd' ')" = ".SRCINFO PKGBUILD" ] \
    || echec "le fichier en trop est toujours dans le faux AUR"
[ "$(nb_commits "$a_trop")" -eq 3 ] || echec "le faux AUR devrait avoir 3 commits"

# Modification locale cachée à git (skip-worktree, puis assume-unchanged) : `git status` est propre, mais ce qui part
# vers l'AUR doit être le contenu du commit, jamais le fichier du dossier de travail.
for drapeau in skip-worktree assume-unchanged; do
    r="$(nouvelle_racine "cache-$drapeau")"
    a_cache="$(nouvel_aur "cache-$drapeau")"
    git_essai "$r" update-index "--$drapeau" paquets/aur/wimao-bin/PKGBUILD
    sed -i 's/^pkgrel=1$/pkgrel=7/' "$r/paquets/aur/wimao-bin/PKGBUILD"
    lancer "$etiquette" --a-blanc --aur "$a_cache" --racine "$r"
    attendre_code 0 "modification cachée ($drapeau)"
    git -C "$r" show HEAD:paquets/aur/wimao-bin/PKGBUILD | cmp -s - <(git -C "$a_cache" show master:PKGBUILD) \
        || echec "$drapeau : le PKGBUILD envoyé n'est pas celui du commit"
    ! git -C "$a_cache" show master:PKGBUILD | grep -q '^pkgrel=7$' || echec "$drapeau : la modification locale a été envoyée"
    [ "$(git -C "$a_cache" log -1 --format=%s master)" = "wimao-bin $version-1" ] || echec "$drapeau : message de commit incorrect"
done

# --- 5. Usage ---
lancer "$etiquette" --a-blanc --aur "$aur" --racine "$racine" --inconnue
attendre_code 2 "option inconnue"
lancer v0.7 --a-blanc --aur "$aur" --racine "$racine"
attendre_code 2 "étiquette v0.7"
attendre_message "invalide (forme attendue : vX.Y.Z" "étiquette v0.7"
lancer "$version" --a-blanc --aur "$aur" --racine "$racine"
attendre_code 2 "étiquette sans v"
lancer "$etiquette" --a-blanc --racine "$racine"
attendre_code 2 "--a-blanc sans --aur"
lancer "$etiquette" --a-blanc --aur "$aur"
attendre_code 2 "--a-blanc sans --racine"
lancer "$etiquette" --aur "$aur"
attendre_code 2 "--aur sans --a-blanc"
lancer "$etiquette" --racine "$racine"
attendre_code 2 "--racine sans --a-blanc"
lancer "$etiquette" "$etiquette"
attendre_code 2 "deux versions"
lancer
attendre_code 2 "version manquante"

# --- 6. Mode réel avec un faux git : refus avant tout clone ou envoi ---
mkdir "$temporaire/bin"
journal="$temporaire/appels.log"
cat > "$temporaire/bin/git" << FAUX
#!/usr/bin/env bash
echo "git \$*" >> "$journal"
case "\$*" in
    *"rev-parse --git-dir"*) echo .git ;;
    *"status --porcelain"*) ;;
    *"branch --show-current"*) echo "\${FAUX_BRANCHE:-main}" ;;
    *"fetch"*) ;;
    *"show HEAD:paquets/aur/aur.archlinux.org.known_hosts"*) echo "aur.archlinux.org ssh-ed25519 AAAAfaux" ;;
    *"rev-parse HEAD"*|*"rev-parse origin/main"*) echo "0000000000000000000000000000000000000000" ;;
    *) echo "FAUX git : commande non prévue : git \$*" >&2; exit 99 ;;
esac
FAUX
cat > "$temporaire/bin/ssh" << FAUX
#!/usr/bin/env bash
echo "ssh \$*" >> "$journal"
exit 99
FAUX
chmod +x "$temporaire/bin/git" "$temporaire/bin/ssh"
export HOME="$temporaire/maison"
mkdir -p "$HOME"
# Hors de main
FAUX_BRANCHE=autre PATH="$temporaire/bin:$PATH" lancer "$etiquette"
attendre_code 1 "hors de main"
attendre_message "pas sur la branche main" "hors de main"
# Clé SSH de l'AUR absente (HOME d'essai vide)
PATH="$temporaire/bin:$PATH" lancer "$etiquette"
attendre_code 1 "clé SSH absente"
attendre_message "clé SSH de l'AUR" "clé SSH absente"
attendre_message "docs/publier-une-version.md" "clé SSH absente"
# Chemin de clé SSH avec un blanc : refusé (git le couperait dans GIT_SSH_COMMAND)
HOME="$temporaire/ma maison"
mkdir -p "$HOME/.ssh"
: > "$HOME/.ssh/aur"
PATH="$temporaire/bin:$PATH" lancer "$etiquette"
attendre_code 1 "chemin avec un blanc"
attendre_message "contient un blanc" "chemin avec un blanc"
export HOME="$temporaire/maison"
! grep -qE 'clone|push|commit|^ssh ' "$journal" || echec "un refus du mode réel est allé trop loin : $(cat "$journal")"
[ ! -e "$HOME/.ssh" ] || echec "le script a écrit dans ~/.ssh"

echo "OK : tester-envoyer-aur.sh"
