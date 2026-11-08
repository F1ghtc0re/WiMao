#!/usr/bin/env python3
"""Lit et contrôle un index dnf (dossier « repodata ») produit par createrepo_c, puis affiche les RPM qu'il décrit.

Rôle
    Ce programme est le SEUL endroit qui sait lire repomd.xml et le fichier « primary ». Il est appelé par
    scripts/publier-depot-dnf.sh (contrôle de ce qui vient d'être construit et signé) et par
    scripts/assembler-site-apt.sh (contrôle en CI de ce qui a été signé). Il ne vérifie PAS la signature :
    c'est le travail de l'appelant (gpgv, scripts/lib/publication.sh). Il vérifie la COHÉRENCE de l'index :
    si repomd.xml est signé, tout ce qu'il décrit est alors lui aussi digne de confiance.

Usage
    python3 -I scripts/lib/lire-index-dnf.py <dossier repodata>

Ce qui est contrôlé (le moindre écart arrête avec le code 1 et un message « ERREUR : … » sur la sortie d'erreur)
    1. le dossier ne contient que des fichiers ordinaires, aux noms simples : ni lien symbolique, ni sous-dossier ;
    2. il contient repomd.xml, éventuellement repomd.xml.asc, et EXACTEMENT les fichiers que repomd.xml décrit ;
    3. pour chaque fichier décrit : emplacement « repodata/<nom> », taille et SHA-256 annoncés = ceux du fichier ;
    4. un seul fichier de type « primary » ;
    5. le fichier primary est décompressé avec l'outil que son extension annonce (.zst -> zstd, .gz -> gzip,
       .xz -> xz, .bz2 -> bzip2, .xml -> rien) : createrepo_c compresse en zstd ou en gzip selon sa version,
       on ne suppose donc jamais gzip. Une autre extension est refusée. La taille décompressée est bornée ;
    6. chaque paquet décrit par primary a un emplacement (location href) sans attribut « xml:base » (qui
       détournerait le téléchargement ailleurs), une taille et une somme SHA-256.

Sortie standard (code 0)
    Une ligne par paquet : « <href>TAB<taille>TAB<sha256> », triée par href. Le href est tel qu'écrit dans
    l'index (adresse complète si createrepo_c a reçu --location-prefix). C'est à l'appelant de décider si cette
    adresse est celle qu'il attend : ce programme ne fait pas confiance à une adresse, il la rapporte.

Dépend de : python3 (bibliothèque standard seulement), et selon l'extension zstd, gzip, xz ou bzip2.
"""
import hashlib
import os
import re
import stat
import subprocess
import sys
import xml.etree.ElementTree as ET

# Nom de fichier admis dans repodata/ : lettres, chiffres, point, tiret, souligné. Pas de « / », donc pas de « .. »
# utilisable pour sortir du dossier ; le premier caractère n'est jamais un point.
MOTIF_NOM = re.compile(r"^[A-Za-z0-9_-][A-Za-z0-9._-]*$")
MOTIF_SOMME = re.compile(r"^[0-9a-f]{64}$")
# Taille maximale du XML décompressé : un index de quelques paquets pèse quelques Ko. Cette borne protège
# contre une « bombe de décompression » ; elle est très au-dessus de l'usage réel.
TAILLE_MAX_XML = 64 * 1024 * 1024
# Extension -> commande de décompression (lit le fichier, écrit sur la sortie standard). None = déjà en clair.
DECOMPRESSEURS = {
    ".zst": ["zstd", "-dc", "--"],
    ".gz": ["gzip", "-dc", "--"],
    ".xz": ["xz", "-dc", "--"],
    ".bz2": ["bzip2", "-dc", "--"],
    ".xml": None,
}


def refuser(message):
    """Affiche l'erreur en français sur la sortie d'erreur et arrête avec le code 1."""
    print("ERREUR : " + message, file=sys.stderr)
    sys.exit(1)


def sans_espace_de_noms(element):
    """Retire l'espace de noms XML des étiquettes (« {http://…}data » devient « data »), sur tout l'arbre."""
    for e in element.iter():
        if isinstance(e.tag, str) and "}" in e.tag:
            e.tag = e.tag.split("}", 1)[1]
    return element


def somme_et_taille(chemin):
    """Rend (SHA-256 hexadécimal, taille en octets) d'un fichier."""
    h = hashlib.sha256()
    taille = 0
    with open(chemin, "rb") as f:
        for bloc in iter(lambda: f.read(1 << 20), b""):
            h.update(bloc)
            taille += len(bloc)
    return h.hexdigest(), taille


def lire_xml_decompresse(chemin):
    """Rend le contenu (octets) du fichier XML, décompressé selon son extension. Refuse une extension inconnue."""
    extension = os.path.splitext(chemin)[1]
    if extension not in DECOMPRESSEURS:
        refuser("extension « %s » non gérée pour %s (admises : %s)"
                % (extension, os.path.basename(chemin), " ".join(sorted(DECOMPRESSEURS))))
    commande = DECOMPRESSEURS[extension]
    if commande is None:
        with open(chemin, "rb") as f:
            donnees = f.read(TAILLE_MAX_XML + 1)
    else:
        try:
            processus = subprocess.Popen(commande + [chemin], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        except FileNotFoundError:
            refuser("l'outil %s est introuvable (nécessaire pour lire %s)" % (commande[0], os.path.basename(chemin)))
        donnees = processus.stdout.read(TAILLE_MAX_XML + 1)
        if len(donnees) > TAILLE_MAX_XML:
            processus.kill()
        processus.stdout.close()
        if processus.wait() != 0 and len(donnees) <= TAILLE_MAX_XML:
            refuser("%s : décompression impossible (%s)" % (os.path.basename(chemin), commande[0]))
    if len(donnees) > TAILLE_MAX_XML:
        refuser("%s : plus de %d octets une fois décompressé" % (os.path.basename(chemin), TAILLE_MAX_XML))
    return donnees


def analyser_xml(donnees, nom):
    """Analyse un document XML ; une erreur de syntaxe est un refus, pas une trace d'exception."""
    try:
        return sans_espace_de_noms(ET.fromstring(donnees))
    except ET.ParseError as erreur:
        refuser("%s n'est pas un XML valide (%s)" % (nom, erreur))


def main():
    if len(sys.argv) != 2:
        print("Usage : lire-index-dnf.py <dossier repodata>", file=sys.stderr)
        sys.exit(2)
    dossier = sys.argv[1]
    if not os.path.isdir(dossier) or os.path.islink(dossier):
        refuser("%s n'est pas un dossier ordinaire" % dossier)

    # 1. Contenu du dossier : que des fichiers ordinaires, aux noms simples.
    presents = set()
    for nom in os.listdir(dossier):
        if not MOTIF_NOM.match(nom):
            refuser("nom de fichier refusé dans repodata : %r" % nom)
        if not stat.S_ISREG(os.lstat(os.path.join(dossier, nom)).st_mode):
            refuser("repodata/%s n'est pas un fichier ordinaire (lien, dossier...)" % nom)
        presents.add(nom)
    if "repomd.xml" not in presents:
        refuser("repodata/repomd.xml est absent")

    # 2 et 3. repomd.xml et les fichiers qu'il décrit.
    with open(os.path.join(dossier, "repomd.xml"), "rb") as f:
        racine = analyser_xml(f.read(TAILLE_MAX_XML + 1), "repomd.xml")
    if racine.tag != "repomd":
        refuser("repomd.xml : élément racine « %s » au lieu de « repomd »" % racine.tag)
    decrits = {}  # nom de fichier -> type
    primary = None
    for data in racine.findall("data"):
        type_donnees = data.get("type", "")
        emplacement = data.find("location")
        somme = data.find("checksum")
        taille = data.find("size")
        if emplacement is None or somme is None or taille is None:
            refuser("repomd.xml : entrée « %s » sans location, checksum ou size" % type_donnees)
        if emplacement.get("href") is None or any(a.endswith("base") for a in emplacement.attrib):
            refuser("repomd.xml : entrée « %s » : location sans href, ou avec un attribut « base »" % type_donnees)
        href = emplacement.get("href")
        if not href.startswith("repodata/") or not MOTIF_NOM.match(href[len("repodata/"):]):
            refuser("repomd.xml : emplacement refusé (attendu repodata/<nom>) : %r" % href)
        nom = href[len("repodata/"):]
        if nom in decrits:
            refuser("repomd.xml : le fichier %s est décrit deux fois" % nom)
        if nom == "repomd.xml" or nom == "repomd.xml.asc":
            refuser("repomd.xml ne doit pas se décrire lui-même ni décrire sa signature")
        if somme.get("type") != "sha256" or not MOTIF_SOMME.match((somme.text or "").strip()):
            refuser("repomd.xml : somme de %s absente ou pas en SHA-256" % nom)
        if not (taille.text or "").strip().isdigit():
            refuser("repomd.xml : taille de %s invalide" % nom)
        if nom not in presents:
            refuser("repomd.xml décrit %s, absent du dossier" % nom)
        trouvee, octets = somme_et_taille(os.path.join(dossier, nom))
        if trouvee != somme.text.strip():
            refuser("%s : SHA-256 différent de celui de repomd.xml" % nom)
        if octets != int(taille.text.strip()):
            refuser("%s : taille %d différente de celle de repomd.xml" % (nom, octets))
        decrits[nom] = type_donnees
        if type_donnees == "primary":
            if primary is not None:
                refuser("repomd.xml : plusieurs fichiers de type primary")
            primary = nom
    if primary is None:
        refuser("repomd.xml ne décrit aucun fichier de type primary")
    # Rien d'autre que repomd.xml, sa signature et les fichiers décrits : un fichier en trop n'est ni signé ni contrôlé.
    en_trop = presents - set(decrits) - {"repomd.xml", "repomd.xml.asc"}
    if en_trop:
        refuser("fichiers non décrits par repomd.xml dans repodata : %s" % " ".join(sorted(en_trop)))

    # 5 et 6. Le fichier primary : un paquet par élément <package>.
    document = analyser_xml(lire_xml_decompresse(os.path.join(dossier, primary)), primary)
    if document.tag != "metadata":
        refuser("%s : élément racine « %s » au lieu de « metadata »" % (primary, document.tag))
    lignes = []
    for paquet in document.findall("package"):
        emplacement = paquet.find("location")
        taille = paquet.find("size")
        somme = paquet.find("checksum")
        if emplacement is None or taille is None or somme is None:
            refuser("%s : paquet sans location, size ou checksum" % primary)
        # Un attribut xml:base ferait télécharger le paquet à une adresse que le href ne montre pas.
        if set(emplacement.attrib) != {"href"}:
            refuser("%s : location avec d'autres attributs que href : %s" % (primary, sorted(emplacement.attrib)))
        octets = taille.get("package", "")
        valeur = (somme.text or "").strip()
        if not octets.isdigit() or somme.get("type") != "sha256" or not MOTIF_SOMME.match(valeur):
            refuser("%s : taille ou somme SHA-256 invalide pour %s" % (primary, emplacement.get("href")))
        href = emplacement.get("href")
        # Le href ne doit contenir ni tabulation ni saut de ligne : la sortie est une ligne par paquet.
        if re.search(r"[\t\r\n]", href):
            refuser("%s : href contenant un caractère de contrôle" % primary)
        lignes.append("%s\t%s\t%s" % (href, octets, valeur))
    if not lignes:
        refuser("%s ne décrit aucun paquet" % primary)
    for ligne in sorted(lignes):
        print(ligne)


if __name__ == "__main__":
    main()
