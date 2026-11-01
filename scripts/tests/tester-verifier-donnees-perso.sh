#!/usr/bin/env bash
# Teste scripts/verifier-donnees-perso.sh sur des dépôts git JETABLES, sans dépendre de la machine.
#
# Principe : chaque cas crée un petit dépôt dans un dossier temporaire, y suit (git add) des fichiers,
# puis lance le contrôle et compare le code de retour et la sortie. Sauf pour le cas du fichier
# `.git/donnees-perso`, on passe toujours `--seulement --terme …` : le script ne cherche alors que les
# termes donnés, et non le nom d'utilisateur, la machine ou l'e-mail git réels de celui qui lance le test.
#
# Cas couverts :
#   - un terme présent : code 1, la sortie donne `fichier:ligne` mais JAMAIS le terme (casse comprise) ;
#   - terme absent : code 0 ;
#   - fichier BINAIRE suivi (octets NUL) : avec le terme → code 1, la sortie donne `fichier (binaire)` sans
#     le terme ; sans le terme → code 0 ;
#   - terme de moins de 3 caractères : ignoré avec un message, code 0 ;
#   - terme `runner` (nom de l'utilisateur des runners CI, trop courant) : ignoré, code 0 ;
#   - recherche littérale : `machine.test` ne trouve pas `machineXtest` mais trouve `machine.test` ;
#   - fichier non suivi par git : jamais examiné, code 0 ;
#   - `.git/donnees-perso` (commentaires et lignes vides ignorés), avec USER=runner et un faux
#     `hostname` qui répond `runner` : seul le terme du fichier est cherché, code 1 ;
#   - e-mail git en `noreply.github.com` : ignoré, code 0 ;
#   - ligne d'espaces ou CRLF dans .git/donnees-perso ; erreur de git grep (faux git) : code 2, sans le terme ;
#   - --dossier hors dépôt git, --terme sans valeur, option inconnue : code 2.
#
# Usage     : scripts/tests/tester-verifier-donnees-perso.sh
# Variables : aucune (PATH et USER sont réglés par le script, pour les cas qui en ont besoin).
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
controle="$racine/scripts/verifier-donnees-perso.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

# Crée un dépôt git neuf dans $temporaire/<nom> et affiche son chemin.
# (Aucune identité git n'est nécessaire : on ne fait jamais de commit, seulement `git add`.)
nouveau_depot() {
    local depot="$temporaire/$1"
    mkdir "$depot"
    git -C "$depot" init --quiet
    echo "$depot"
}

# Lance le contrôle et range code et sortie (stdout + stderr) dans $code et $sortie.
# Le `|| code=$?` évite que `set -e` n'arrête le test sur un code non nul attendu.
lancer() {
    code=0
    sortie="$("$controle" "$@" 2>&1)" || code=$?
}

# Vérifie le code de retour du dernier `lancer` ($1 = attendu, $2 = nom du cas).
attendre_code() {
    [[ "$code" -eq "$1" ]] || echec "$2 : code $code au lieu de $1 (sortie : $sortie)"
}

# --- Terme présent : fichier:ligne, jamais le terme lui-même ---
depot="$(nouveau_depot present)"
printf 'ligne un\nBonjour ALICE\n' > "$depot/a.txt"
git -C "$depot" add a.txt
lancer --dossier "$depot" --seulement --terme alice
attendre_code 1 "terme présent"
[[ "$sortie" == *"a.txt:2"* ]] || echec "terme présent : a.txt:2 absent de la sortie ($sortie)"
if grep -qi "alice" <<< "$sortie"; then
    echec "terme présent : la sortie répète le terme ($sortie)"
fi

# --- Terme absent ---
lancer --dossier "$depot" --seulement --terme introuvable
attendre_code 0 "terme absent"

# --- Terme trop court : ignoré avec un message ---
lancer --dossier "$depot" --seulement --terme ab
attendre_code 0 "terme trop court"
[[ "$sortie" == *"ignoré"* ]] || echec "terme trop court : pas de message « ignoré » ($sortie)"

# --- Terme `runner` : ignoré même s'il est présent ---
depot="$(nouveau_depot runner)"
echo "tourne sous runner" > "$depot/r.txt"
git -C "$depot" add r.txt
lancer --dossier "$depot" --seulement --terme runner
attendre_code 0 "terme runner"
[[ "$sortie" == *"ignoré"* ]] || echec "terme runner : pas de message « ignoré » ($sortie)"

# --- Recherche littérale : le point n'est pas un joker ---
depot="$(nouveau_depot litteral)"
echo "machineXtest" > "$depot/x.txt"
git -C "$depot" add x.txt
lancer --dossier "$depot" --seulement --terme machine.test
attendre_code 0 "recherche littérale (machineXtest)"
echo "machine.test" > "$depot/y.txt"
git -C "$depot" add y.txt
lancer --dossier "$depot" --seulement --terme machine.test
attendre_code 1 "recherche littérale (machine.test)"
[[ "$sortie" == *"y.txt:1"* ]] || echec "recherche littérale : y.txt:1 absent ($sortie)"

# --- Fichier binaire suivi : seconde passe (-a), nom seul dans la sortie ---
depot="$(nouveau_depot binaire)"
printf 'debut\0 contient secretnom \0fin\n' > "$depot/image.bin"
git -C "$depot" add image.bin
lancer --dossier "$depot" --seulement --terme secretnom
attendre_code 1 "binaire contenant le terme"
[[ "$sortie" == *"image.bin (binaire)"* ]] || echec "binaire : « image.bin (binaire) » absent ($sortie)"
if grep -qi "secretnom" <<< "$sortie"; then
    echec "binaire : la sortie répète le terme ($sortie)"
fi
printf 'debut\0 rien de special \0fin\n' > "$depot/image.bin"
git -C "$depot" add image.bin
lancer --dossier "$depot" --seulement --terme secretnom
attendre_code 0 "binaire sans le terme"

# --- Fichier non suivi : jamais examiné ---
depot="$(nouveau_depot nonsuivi)"
echo "contient secretnom" > "$depot/pas-suivi.txt"
lancer --dossier "$depot" --seulement --terme secretnom
attendre_code 0 "fichier non suivi"

# --- .git/donnees-perso, sans --seulement ni --terme ---
# USER=runner et un faux `hostname` qui répond `runner` : ces deux termes sont ignorés, il ne reste
# que ceux du fichier. Le faux hostname est un script placé en tête du PATH.
mkdir "$temporaire/bin"
printf '#!/usr/bin/env bash\necho runner\n' > "$temporaire/bin/hostname"
chmod +x "$temporaire/bin/hostname"
depot="$(nouveau_depot fichier)"
printf '# commentaire\n\nsecretnom\n' > "$depot/.git/donnees-perso"
echo "ici secretnom" > "$depot/f.txt"
git -C "$depot" add f.txt
code=0
sortie="$(USER=runner PATH="$temporaire/bin:$PATH" "$controle" --dossier "$depot" 2>&1)" || code=$?
attendre_code 1 ".git/donnees-perso"
[[ "$sortie" == *"f.txt:1"* ]] || echec ".git/donnees-perso : f.txt:1 absent ($sortie)"

# --- E-mail git en noreply.github.com : ignoré (c'est une adresse publique par construction) ---
depot="$(nouveau_depot noreply)"
git -C "$depot" config user.email alice@users.noreply.github.com
echo "contact alice@users.noreply.github.com" > "$depot/n.txt"
git -C "$depot" add n.txt
code=0
sortie="$(USER=runner PATH="$temporaire/bin:$PATH" "$controle" --dossier "$depot" 2>&1)" || code=$?
attendre_code 0 "e-mail noreply"

# --- .git/donnees-perso : espaces et fin de ligne CRLF ---
# Une ligne d'espaces ne doit rien chercher (sinon elle trouverait tout) ; « secretnom\r » doit trouver
# secretnom.
depot="$(nouveau_depot crlf)"
printf '   \n\t\n' > "$depot/.git/donnees-perso"
echo "ici secretnom" > "$depot/f.txt"
git -C "$depot" add f.txt
code=0
sortie="$(USER=runner PATH="$temporaire/bin:$PATH" "$controle" --dossier "$depot" 2>&1)" || code=$?
attendre_code 0 "ligne d'espaces"
printf '  secretnom\r\n' > "$depot/.git/donnees-perso"
code=0
sortie="$(USER=runner PATH="$temporaire/bin:$PATH" "$controle" --dossier "$depot" 2>&1)" || code=$?
attendre_code 1 "secretnom CRLF"

# --- Erreur de git grep : jamais prise pour « rien trouvé » ---
# (Le dépôt courant, `crlf`, contient un fichier texte avec le terme : le faux git doit faire échouer
# la recherche dès la première passe.)
# Faux git : renvoie 128 pour `grep`, délègue au vrai git pour le reste.
vrai_git="$(command -v git)"
mkdir "$temporaire/bin-git"
cat > "$temporaire/bin-git/git" << EOF2
#!/usr/bin/env bash
for argument in "\$@"; do
    if [[ "\$argument" == grep ]]; then exit 128; fi
done
exec "$vrai_git" "\$@"
EOF2
chmod +x "$temporaire/bin-git/git"
code=0
sortie="$(PATH="$temporaire/bin-git:$PATH" "$controle" --dossier "$depot" --seulement --terme secretnom 2>&1)" || code=$?
attendre_code 2 "erreur de git grep"
if grep -qi "secretnom" <<< "$sortie"; then
    echec "erreur de git grep : le message répète le terme ($sortie)"
fi

# --- Usages invalides ---
lancer --dossier "$temporaire/inexistant-pas-git" --seulement --terme secretnom
attendre_code 2 "--dossier qui n'est pas un dépôt"
lancer --terme
attendre_code 2 "--terme sans valeur"

# --- Option inconnue : erreur d'usage ---
lancer --dossier "$depot" --nimporte-quoi
attendre_code 2 "option inconnue"

echo "OK : verifier-donnees-perso.sh se comporte comme prévu."
