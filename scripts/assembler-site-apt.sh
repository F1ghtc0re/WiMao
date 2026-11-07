#!/usr/bin/env bash
# Assemble le site GitHub Pages du dépôt APT de WiMao. Tourne en CI (job « assembler » de
# .github/workflows/depot-apt.yml) et ne détient AUCUNE clé : il vérifie ce qui a été signé sur la machine du
# propriétaire (depot-apt/, voir scripts/publier-depot-apt.sh), télécharge les .deb depuis les Releases GitHub,
# contrôle chacun (taille et SHA256 annoncées par l'index signé), puis range le tout dans <dossier du site>.
#
# Usage     : scripts/assembler-site-apt.sh <dossier du site>
#             scripts/assembler-site-apt.sh <dossier du site> --cle <empreinte> --racine <dossier du dépôt>
#             scripts/assembler-site-apt.sh --aide
#   <dossier du site>  dossier à produire : absent ou vide (sinon refus, rien n'est écrasé).
#   --cle <empreinte>  (essais seulement) empreinte de la clé (40 hexadécimaux) : remplace celle de SECURITY.md.
#   --racine <d>       (essais seulement, exige --cle) dossier tenant lieu de racine du dépôt : on y lit
#                      docs/cle-publique.asc et depot-apt/. Même choix que publier-depot-apt.sh : un test
#                      fournit sa clé jetable sans toucher au vrai dépôt ni à ~/.gnupg.
#
# Étapes :
#   1. si depot-apt/dists/stable/InRelease est absent (avant la première publication) : message
#      « dépôt APT vide : rien à assembler », rien n'est écrit, code 3 (la CI saute alors l'installation et le
#      déploiement sans échouer) ;
#   2. clé attendue : empreinte de SECURITY.md ; trousseau gpgv construit depuis docs/cle-publique.asc ;
#   3. InRelease et Release.gpg doivent être signés par cette clé, et le texte signé d'InRelease doit être
#      identique à Release ;
#   4. les empreintes et tailles SHA256 de Release doivent correspondre à Packages et Packages.gz, et
#      Packages.gz doit contenir exactement Packages ;
#   5. chaque entrée de Packages : « Filename: pool/vX.Y.Z/<nom>.deb » (forme STRICTE, vérifiée pour TOUTES les
#      entrées avant le moindre téléchargement : ni « .. », ni chemin absolu, ni étiquette mal formée), Size et
#      SHA256 présents ;
#   6. téléchargement de https://github.com/F1ghtc0re/WiMao/releases/download/<étiquette>/<nom> (curl,
#      HTTPS seul), contrôle de la taille et du SHA256 annoncés ;
#      Avant l'étape 3, tout élément de depot-apt/ qui n'est pas l'un des cinq fichiers ordinaires attendus
#      (lien symbolique, fichier en trop, autre architecture...) est refusé (code 1) ;
#   7. assemblage : InRelease, Release, Release.gpg, Packages et Packages.gz (les seuls fichiers copiés), pool/<étiquette>/*.deb, wimao.asc (copie de
#      docs/cle-publique.asc) et index.html (page autonome : commandes d'installation et empreinte).
#   Tout est construit dans un dossier temporaire puis copié à la fin : un contrôle qui échoue n'écrit rien.
#
# Variables : WIMAO_SOURCE_RELEASES  (ESSAIS SEULEMENT) dossier local « <étiquette>/<nom> » qui remplace
#             l'adresse des Releases : les fichiers y sont copiés au lieu d'être téléchargés. Elle n'est admise
#             qu'avec --racine (mode essai) ; définie sans --racine, elle est REFUSÉE (code 2), jamais ignorée
#             en silence : une CI qui l'aurait par erreur n'assemblerait pas un site à partir d'un dossier local.
# Écrit     : <dossier du site> uniquement, en cas de succès (si la copie finale échoue, il est vidé) ; dossiers temporaires supprimés en sortie.
# Code      : 0 = site complet ; 1 = contrôle échoué ; 2 = usage ; 3 = dépôt APT vide (rien écrit).
# Dépend de : gpg, gpgv, curl (téléchargement réel), gzip, sha256sum, awk.
set -euo pipefail
# Langue neutre : comparaisons, tri et awk ne dépendent pas de la langue de la machine.
export LC_ALL=C

RACINE_DEPOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Adresse des Releases GitHub, et du dépôt de code (lien de la page d'accueil).
URL_RELEASES="https://github.com/F1ghtc0re/WiMao/releases/download"
URL_DEPOT="https://github.com/F1ghtc0re/WiMao"
# Forme stricte d'un Filename : pool/vX.Y.Z/<nom de .deb sans « / » ni « .. » grâce au jeu de caractères>.
MOTIF_FILENAME='^pool/v[0-9]+\.[0-9]+\.[0-9]+/[A-Za-z0-9.+~_-]+\.deb$'
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
site=""
cle=""
racine_essai=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --aide | -h | --help) aide; exit 0 ;;
        --cle | --racine)
            [ "$#" -ge 2 ] || erreur_usage "$1 attend une valeur"
            case "$2" in -*) erreur_usage "$1 : valeur invalide (commence par « - ») : $2" ;; esac
            if [ "$1" = "--cle" ]; then cle="$2"; else racine_essai="$2"; fi
            shift
            ;;
        -*) erreur_usage "option inconnue : $1" ;;
        *)
            [ -z "$site" ] || erreur_usage "un seul dossier de site attendu (reçu : $site et $1)"
            site="$1"
            ;;
    esac
    shift
done
[ -n "$site" ] || erreur_usage "dossier du site manquant"
# --cle et --racine vont ensemble : l'une sans l'autre ferait lire SECURITY.md d'un faux dépôt ou l'inverse.
if [ -n "$cle$racine_essai" ]; then
    [ -n "$cle" ] && [ -n "$racine_essai" ] || erreur_usage "--cle et --racine vont ensemble (réservés aux essais)"
    racine="$racine_essai"
    empreinte_essai="$(tr -d ' ' <<< "$cle" | tr 'a-f' 'A-F')"
    [[ "$empreinte_essai" =~ ^[0-9A-F]{40}$ ]] || erreur_usage "--cle : empreinte invalide (40 chiffres hexadécimaux attendus) : $cle"
else
    racine="$RACINE_DEPOT"
    # La source locale des Releases est réservée aux essais : hors essai on refuse, on n'ignore pas.
    [ -z "${WIMAO_SOURCE_RELEASES:-}" ] \
        || erreur_usage "WIMAO_SOURCE_RELEASES n'est admise qu'avec --racine (essais) : retirez-la de l'environnement"
fi
depot_apt="$racine/depot-apt"
dists="$depot_apt/dists/stable"

# --- 1. Dépôt APT vide ---
echo "== 1. Dépôt APT"
if [ ! -f "$dists/InRelease" ]; then
    echo "dépôt APT vide : rien à assembler"
    exit 3
fi

# Contenu de depot-apt/ : exactement cet arbre, fait de dossiers et de fichiers ORDINAIRES (le type vient de
# find : un lien symbolique est « l »). Tout le reste (fichier en trop, autre architecture, lien) est refusé,
# puisque seuls les cinq fichiers attendus seront copiés et que le reste n'est ni signé ni contrôlé.
attendus="d dists
d dists/stable
d dists/stable/main
d dists/stable/main/binary-amd64
f dists/stable/InRelease
f dists/stable/Release
f dists/stable/Release.gpg
f dists/stable/main/binary-amd64/Packages
f dists/stable/main/binary-amd64/Packages.gz"
trouves="$(find "$depot_apt" -mindepth 1 -printf '%y %P\n' | sort)"
if [ "$trouves" != "$(sort <<< "$attendus")" ]; then
    inattendus="$(comm -23 <(printf '%s\n' "$trouves") <(sort <<< "$attendus") | tr '\n' ';')"
    manquants="$(comm -13 <(printf '%s\n' "$trouves") <(sort <<< "$attendus") | tr '\n' ';')"
    arreter "depot-apt/ n'a pas le contenu attendu (inattendu : ${inattendus:-rien} ; manquant : ${manquants:-rien})"
fi

for outil in gpg gpgv gzip sha256sum awk; do
    command -v "$outil" > /dev/null || arreter "la commande $outil est introuvable"
done
if [ -z "${WIMAO_SOURCE_RELEASES:-}" ]; then
    command -v curl > /dev/null || arreter "la commande curl est introuvable"
fi
# Le dossier du site doit être absent ou vide : on n'écrase jamais un site existant.
if [ -e "$site" ]; then
    [ -d "$site" ] || arreter "$site existe et n'est pas un dossier"
    [ -z "$(ls -A -- "$site")" ] || arreter "le dossier $site n'est pas vide"
fi

temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

# --- 2. Clé attendue et trousseau ---
echo "== 2. Clé attendue"
if [ -n "$cle" ]; then
    empreinte="$empreinte_essai"  # déjà validée avec les arguments
    echo "Essai : clé $empreinte (SECURITY.md ignoré)"
else
    empreinte="$(empreinte_publiee "$racine")" || exit 1
    echo "Clé de signature attendue (SECURITY.md) : $empreinte"
fi
trousseau_cle_publiee "$racine" "$empreinte" "$temporaire/trousseau.gpg" || exit 1

# --- 3. Signatures ---
echo "== 3. Signatures"
[ -f "$dists/Release" ] || arreter "$dists/Release est absent"
[ -f "$dists/Release.gpg" ] || arreter "$dists/Release.gpg est absent"
verifier_signature "$temporaire/trousseau.gpg" "$empreinte" "$dists/InRelease" || exit 1
verifier_signature "$temporaire/trousseau.gpg" "$empreinte" "$dists/Release.gpg" "$dists/Release" || exit 1
# Le texte signé d'InRelease (celui qu'apt lira) doit être EXACTEMENT Release (celui dont on contrôle les
# empreintes) : sinon une Release.gpg valide pourrait cacher un InRelease différent. gpgv --output écrit le
# texte signé (après avoir de nouveau vérifié la signature).
gpgv --keyring "$temporaire/trousseau.gpg" --output "$temporaire/InRelease.texte" "$dists/InRelease" > /dev/null 2>&1 \
    || arreter "gpgv n'a pas pu extraire le texte signé d'InRelease"
cmp -s "$temporaire/InRelease.texte" "$dists/Release" || arreter "le texte signé d'InRelease diffère de Release"
echo "InRelease et Release.gpg signés par la clé publiée."

# --- 4. Empreintes de Release ---
echo "== 4. Empreintes de Release"
paquets="$dists/main/binary-amd64/Packages"
paquets_gz="$dists/main/binary-amd64/Packages.gz"
[ -f "$paquets" ] || arreter "$paquets est absent"
[ -f "$paquets_gz" ] || arreter "$paquets_gz est absent"
# controler_release <chemin relatif à dists/stable> : la section SHA256 de Release (lignes « hash taille chemin »,
# commençant par une espace) doit donner pour ce fichier exactement son SHA256 et sa taille. Une seule ligne.
controler_release() {
    local rel="$1" fichier="$dists/$1" attendu trouve
    trouve="$(awk -v chemin="$rel" '
        /^[^ ]/ { section = ($0 == "SHA256:") }
        section && $3 == chemin { print $1 " " $2 }
    ' "$dists/Release")"
    [ "$(wc -l <<< "$trouve")" -eq 1 ] && [ -n "$trouve" ] \
        || arreter "Release ne donne pas exactement une empreinte SHA256 pour $rel"
    attendu="$(sha256sum -- "$fichier" | cut -d' ' -f1) $(stat -c %s -- "$fichier")"
    [ "$trouve" = "$attendu" ] || arreter "Release : empreinte ou taille fausse pour $rel (Release : $trouve ; fichier : $attendu)"
}
controler_release main/binary-amd64/Packages
controler_release main/binary-amd64/Packages.gz
# Packages.gz doit contenir exactement Packages (apt peut lire l'un ou l'autre).
gzip -dc -- "$paquets_gz" | cmp -s - "$paquets" || arreter "Packages.gz ne contient pas exactement Packages"
echo "Packages et Packages.gz conformes à Release."

# --- 5. Entrées de Packages (TOUTES vérifiées avant tout téléchargement) ---
echo "== 5. Entrées de Packages"
# Un paragraphe = un paquet. On sort une ligne « Filename<TAB>Size<TAB>SHA256 » par paragraphe ; un champ
# manquant ou en double dans un paragraphe sort une ligne marquée « !! » qui est refusée plus bas.
awk '
    function vider() {
        if (vu) {
            if (nf == 1 && ns == 1 && nh == 1) print fichier "\t" taille "\t" somme
            else print "!!\tchamp Filename, Size ou SHA256 manquant ou en double"
        }
        vu = 0; nf = ns = nh = 0; fichier = taille = somme = ""
    }
    /^$/ { vider(); next }
    { vu = 1 }
    /^Filename: / { nf++; fichier = substr($0, 11) }
    /^Size: /     { ns++; taille = substr($0, 7) }
    /^SHA256: /   { nh++; somme = substr($0, 9) }
    END { vider() }
' "$paquets" > "$temporaire/entrees.tsv"
[ -s "$temporaire/entrees.tsv" ] || arreter "Packages ne contient aucun paquet"

while IFS=$'\t' read -r fichier taille somme; do
    [ "$fichier" != "!!" ] || arreter "Packages : $taille"
    [[ "$fichier" =~ $MOTIF_FILENAME ]] \
        || arreter "Filename refusé (attendu pool/vX.Y.Z/<nom>.deb) : $fichier"
    [[ "$taille" =~ ^[0-9]+$ ]] || arreter "Size invalide pour $fichier : $taille"
    [[ "$somme" =~ ^[0-9a-f]{64}$ ]] || arreter "SHA256 invalide pour $fichier : $somme"
done < "$temporaire/entrees.tsv"
doublons="$(cut -f1 "$temporaire/entrees.tsv" | sort | uniq -d)"
[ -z "$doublons" ] || arreter "Filename en double dans Packages : $doublons"
echo "$(wc -l < "$temporaire/entrees.tsv") entrée(s), toutes en pool/vX.Y.Z/."

# --- 6. Téléchargement et contrôle des .deb ---
echo "== 6. Téléchargement des paquets"
construit="$temporaire/site"
mkdir -p "$construit"
# La liste est lue sur le descripteur 3, pas sur l'entrée standard : curl (ou toute commande du corps de
# boucle) ne peut ainsi pas en avaler des lignes.
while IFS=$'\t' read -r fichier taille somme <&3; do
    # Filename a la forme stricte pool/<étiquette>/<nom> : on en tire l'étiquette et le nom.
    etiquette="$(cut -d/ -f2 <<< "$fichier")"
    nom="$(cut -d/ -f3 <<< "$fichier")"
    mkdir -p "$construit/pool/$etiquette"
    destination="$construit/$fichier"
    if [ -n "${WIMAO_SOURCE_RELEASES:-}" ]; then
        echo "  copie (essai) $etiquette/$nom"
        cp -- "$WIMAO_SOURCE_RELEASES/$etiquette/$nom" "$destination" 2> /dev/null \
            || arreter "fichier d'essai introuvable : $WIMAO_SOURCE_RELEASES/$etiquette/$nom"
    else
        echo "  téléchargement $etiquette/$nom"
        curl -fsSL --proto '=https' --retry 3 -o "$destination" "$URL_RELEASES/$etiquette/$nom" \
            || arreter "téléchargement impossible : $URL_RELEASES/$etiquette/$nom"
    fi
    [ "$(stat -c %s -- "$destination")" = "$taille" ] \
        || arreter "$nom : taille $(stat -c %s -- "$destination") au lieu de $taille annoncée"
    [ "$(sha256sum -- "$destination" | cut -d' ' -f1)" = "$somme" ] \
        || arreter "$nom : SHA256 différent de celui de l'index signé"
done 3< "$temporaire/entrees.tsv"

# --- 7. Assemblage ---
echo "== 7. Assemblage du site"
# Seuls les cinq fichiers vérifiés sont copiés (l'arbre de depot-apt/ a été contrôlé plus haut).
mkdir -p "$construit/dists/stable/main/binary-amd64"
cp -- "$dists/InRelease" "$dists/Release" "$dists/Release.gpg" "$construit/dists/stable/"
cp -- "$paquets" "$paquets_gz" "$construit/dists/stable/main/binary-amd64/"
cp -- "$racine/docs/cle-publique.asc" "$construit/wimao.asc"
# Empreinte en groupes de 4 (lisible, comparable à l'œil) ; hexadécimal pur, donc rien à échapper en HTML.
empreinte_groupes="$(sed 's/.\{4\}/& /g; s/ $//' <<< "$empreinte")"
# Page d'accueil autonome : aucune ressource externe, thème clair ou sombre selon le système.
cat > "$construit/index.html" << HTML
<!doctype html>
<html lang="fr">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Dépôt APT de WiMao</title>
<style>
  :root { color-scheme: light dark; --fond: #fafafa; --texte: #1c1c1e; --cadre: #e4e4e7; --code: #f0f0f3; --lien: #1a5fb4; }
  @media (prefers-color-scheme: dark) {
    :root { --fond: #1e1e22; --texte: #ececf0; --cadre: #3a3a42; --code: #2a2a30; --lien: #78aeed; }
  }
  body { background: var(--fond); color: var(--texte); font: 16px/1.55 system-ui, sans-serif; margin: 0; }
  main { max-width: 46rem; margin: 0 auto; padding: 2rem 1rem 3rem; }
  pre { background: var(--code); border: 1px solid var(--cadre); border-radius: 8px; padding: 1rem; overflow-x: auto; font-size: 0.9rem; }
  code { font-family: ui-monospace, monospace; }
  a { color: var(--lien); }
</style>
</head>
<body>
<main>
<h1>Dépôt APT de WiMao</h1>
<p>WiMao rend Wine utilisable pour la musique assistée par ordinateur. Ce site est son dépôt APT : une fois
ajouté, <code>apt</code> installe WiMao et le met à jour avec le reste du système. L'index est signé avec la clé
OpenPGP du propriétaire ; <code>apt</code> refuse le dépôt si un octet de l'index ou d'un paquet est modifié.</p>
<h2>Installation (Ubuntu 24.04, Debian 13)</h2>
<pre><code>sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://f1ghtc0re.github.io/WiMao/wimao.asc | sudo tee /etc/apt/keyrings/wimao.asc &gt; /dev/null
echo "deb [signed-by=/etc/apt/keyrings/wimao.asc] https://f1ghtc0re.github.io/WiMao stable main" \\
  | sudo tee /etc/apt/sources.list.d/wimao.list
sudo apt update &amp;&amp; sudo apt install wimao</code></pre>
<h2>Empreinte de la clé de signature</h2>
<p>Comparez-la avec celle de <code>SECURITY.md</code> dans le dépôt :</p>
<pre><code>$empreinte_groupes</code></pre>
<p>Clé publique : <a href="wimao.asc">wimao.asc</a>. Code source et documentation :
<a href="$URL_DEPOT">$URL_DEPOT</a>.</p>
</main>
</body>
</html>
HTML

# Rien n'a échoué : on copie dans le dossier du site (créé au besoin).
# Si la copie échoue en route (disque plein...), le site resterait À MOITIÉ rempli et pourrait être publié tel
# quel : on le vide (il était absent ou vide au départ) et on sort en code 1.
if ! { mkdir -p -- "$site" && cp -r -- "$construit/." "$site/"; }; then
    find "$site" -mindepth 1 -delete 2> /dev/null || true
    arreter "la copie vers $site a échoué ; le dossier a été vidé"
fi
echo "Site assemblé dans $site."
