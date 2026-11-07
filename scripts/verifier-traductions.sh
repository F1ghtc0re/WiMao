#!/usr/bin/env bash
# Vérifie que les traductions françaises et espagnoles sont complètes.
#
# 1. régénère po/POTFILES (tous les .rs des crates) hors tests d intégration (sans chaîne) puis po/wimao.pot (xgettext) ;
# 2. met à jour po/fr.po et po/es.po avec msgmerge (nouvelles chaînes ajoutées vides) ;
# 3. échoue (code 1) en listant les chaînes non traduites ou approximatives (fuzzy).
# Utilisé par la CI et à lancer avant chaque commit qui touche une chaîne visible.
# Voir CONTRIBUTING.md (« Ajouter une langue », « Ajouter une chaîne traduisible »).
# Prérequis : paquet gettext (xgettext, msgmerge, msgfmt, msgattrib) et git.
set -euo pipefail

# Langues vérifiées : ajouter ici le code d'une nouvelle langue (et créer po/<code>.po).
LANGUES=(fr es)

cd "$(dirname "$0")/.."

# Les .rs sont listés par git (--others : fichiers neufs pas encore ajoutés) ;
# LC_ALL=C pour un ordre de tri identique sur toutes les machines.
git ls-files --cached --others --exclude-standard 'crates/*.rs' ':!crates/*/tests/*' | LC_ALL=C sort > po/POTFILES

# trn:1,2 : pluriels (singulier et pluriel sont les arguments 1 et 2) ; trc:1c,2 : texte avec contexte (argument 1 = contexte, 2 = msgid) ; tr_noop : marque une chaîne à extraire sans la traduire sur place (messages de regles.toml).
xgettext --language=C --keyword=tr --keyword=trf --keyword=tr_noop --keyword=trn:1,2 --keyword=trc:1c,2 \
    --from-code=UTF-8 --add-comments --add-location=file \
    --package-name=wimao -f po/POTFILES -o po/wimao.pot

# Lit un fichier .po sur l'entrée standard et écrit le msgid de chaque entrée, hors
# en-tête (l'entrée qui contient « Content-Type: »). Un msgid peut tenir sur plusieurs
# lignes (`msgid ""` puis des lignes `"…"`, par exemple quand il contient \n) : elles
# sont recollées, sinon une telle chaîne passerait inaperçue.
msgids_sans_entete() {
    awk 'BEGIN { RS = "" }
         !/Content-Type: / {
             m = ""; dedans = 0; n = split($0, l, "\n")
             for (i = 1; i <= n; i++) {
                 if (l[i] ~ /^msgid /) { m = l[i]; dedans = 1; continue }
                 if (dedans && l[i] ~ /^"/) { m = m l[i]; continue }
                 dedans = 0
             }
             if (m != "") print m
         }'
}

# Lit un fichier .po sur l'entrée standard et signale (sur la sortie d'erreur) chaque entrée
# traduite dont les marqueurs {nom} diffèrent de ceux du msgid : un marqueur oublié, ajouté
# ou renommé afficherait « {nom} » tel quel (ou perdrait une valeur). Règles :
# - singulier : msgstr comparé à msgid ; pluriels : chaque msgstr[i] comparé à l'union
#   msgid / msgid_plural (le singulier peut omettre {n}, par exemple « un plugin ») ;
# - entrées obsolètes (#~) et msgstr vides (non traduits, déjà signalés) ignorés.
# Code de sortie 1 s'il y a au moins un écart.
placeholders_coherents() {
    python3 -c '
import re, sys
nom = sys.argv[1]
MARQUEUR = re.compile(r"\{[A-Za-z_0-9]+\}")
ecarts = 0

def texte(lignes):
    """Recolle les chaînes entre guillemets (un msgid peut tenir sur plusieurs lignes)."""
    return "".join(l.strip()[1:-1] for l in lignes)

for bloc in sys.stdin.read().split("\n\n"):
    champs, courant = {}, None
    for ligne in bloc.splitlines():
        if ligne.startswith("#"):
            continue  # commentaires, dont les entrées obsolètes (#~)
        m = re.match(r"(msgid_plural|msgid|msgstr(?:\[\d+\])?) (.*)", ligne)
        if m:
            courant = m.group(1)
            champs[courant] = [m.group(2)]
        elif courant and ligne.startswith("\""):
            champs[courant].append(ligne)
    if "msgid" not in champs:
        continue
    msgid = texte(champs["msgid"])
    if msgid == "":
        continue  # en-tête du fichier
    attendu = set(MARQUEUR.findall(msgid))
    if "msgid_plural" in champs:
        attendu |= set(MARQUEUR.findall(texte(champs["msgid_plural"])))
    for cle, lignes in champs.items():
        if not cle.startswith("msgstr"):
            continue
        traduit = texte(lignes)
        if traduit == "":
            continue  # non traduit : signalé ailleurs
        trouve = set(MARQUEUR.findall(traduit))
        if trouve != attendu:
            ecarts += 1
            print(f"[{nom}] marqueurs différents dans {cle} de : {msgid}", file=sys.stderr)
            print(f"    attendus {sorted(attendu)}, trouvés {sorted(trouve)}", file=sys.stderr)
sys.exit(1 if ecarts else 0)
' "$1"
}

erreurs=0
for langue in "${LANGUES[@]}"; do
    fichier="po/${langue}.po"
    msgmerge --update --quiet --add-location=file --backup=none "$fichier" po/wimao.pot
    # Chaînes sans traduction, puis chaînes approximatives : msgattrib les liste.
    manquantes=$(msgattrib --no-wrap --untranslated --no-obsolete "$fichier" | msgids_sans_entete)
    approx=$(msgattrib --no-wrap --only-fuzzy --no-obsolete "$fichier" | msgids_sans_entete)
    if [[ -n "$manquantes" ]]; then
        echo "[$langue] chaînes non traduites :" >&2
        echo "$manquantes" >&2
        erreurs=1
    fi
    if [[ -n "$approx" ]]; then
        echo "[$langue] chaînes approximatives (fuzzy) à relire :" >&2
        echo "$approx" >&2
        erreurs=1
    fi
    # Marqueurs {nom} identiques entre msgid et msgstr.
    if ! msgattrib --no-wrap --translated --no-obsolete "$fichier" | placeholders_coherents "$langue"; then
        erreurs=1
    fi
    # Contrôle de syntaxe final du fichier ; affiche le bilan.
    msgfmt --statistics --check -o /dev/null "$fichier"
done
exit "$erreurs"
