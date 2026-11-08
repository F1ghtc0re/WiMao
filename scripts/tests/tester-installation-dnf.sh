#!/usr/bin/env bash
# Prouve, DANS UN CONTENEUR FEDORA JETABLE, que le dépôt dnf d'un site assemblé (scripts/assembler-site-apt.sh)
# fonctionne comme le promet WiMao : dnf installe `wimao` (et ses deux paquets frères) depuis ce site, en
# contrôlant la signature de l'index (repo_gpgcheck=1), ET refuse un index altéré d'un seul caractère.
# Lancé par le job `installation-dnf` de .github/workflows/depot-apt.yml (Fedora 44 et 45) et par
# scripts/tests/tester-depot-dnf-bout-en-bout.sh (essai local avec de faux RPM).
#
# ATTENTION : ce script installe des paquets et écrit dans /etc/yum.repos.d. Il se lance en root DANS UN
# CONTENEUR JETABLE, jamais sur une machine de travail (il refuse de tourner hors d'un conteneur).
#
# Usage     : scripts/tests/tester-installation-dnf.sh <dossier du site> [<dossier de RPM à servir>]
#   <dossier du site>        site assemblé : rpm/repodata/, wimao.repo et wimao.asc.
#   <dossier de RPM>         (essai local seulement) dossier servi sur http://127.0.0.1:8081/ : l'index doit alors
#                            avoir été construit avec --adresse-rpm http://127.0.0.1:8081/ (publier-depot-dnf.sh
#                            à blanc). Sans ce dossier (cas de la CI), dnf télécharge les vrais RPM sur les
#                            Releases GitHub, aux adresses de l'index signé.
# Variables : aucune.
# Écrit     : /etc/yum.repos.d/wimao.repo, des copies du site sous /tmp, les paquets installés. Rien d'autre.
# Code      : 0 = tout est prouvé ; 1 = écart (message « ERREUR : … ») ; 2 = usage ou hors conteneur.
#
# Ce que fait le script (le .repo du site est réécrit pour pointer vers un petit serveur local, le site réel
# n'étant pas encore en ligne ; c'est la seule différence avec ce que verra un utilisateur) :
#   1. serveur http.server sur 127.0.0.1:8080 (le site) et, s'il y a lieu, sur 8081 (les RPM d'essai) ;
#   2. PREUVE DU REFUS (en premier, tant que rien n'est installé) : une copie du site où UN caractère de
#      repomd.xml est modifié (le chiffre de <revision>) est servie sur le port 8082 ; `dnf install wimao` doit
#      échouer, `wimao` ne doit pas s'installer, et la sortie doit parler de signature ou d'absence du paquet ;
#   3. installation : avec le vrai index, `dnf install -y wimao wimao-bridge wimao-wine` réussit ; les trois paquets sont installés, au
#      même numéro de version que celui annoncé par l'index ; dnf a bien pris wimao dans le dépôt « wimao ».
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage : $0 <dossier du site> [<dossier de RPM à servir>]" >&2
    exit 2
fi
if [ ! -e /run/.containerenv ] && [ ! -e /.dockerenv ]; then
    echo "ERREUR : ce script installe des paquets : à lancer seulement dans un conteneur jetable." >&2
    exit 2
fi
[ "$(id -u)" -eq 0 ] || { echo "ERREUR : à lancer en root (dans le conteneur)." >&2; exit 2; }

arreter() {
    echo "ERREUR : $1" >&2
    exit 1
}

site="$(cd "$1" && pwd)"
rpms=""
if [ -n "${2:-}" ]; then rpms="$(cd "$2" && pwd)"; fi
for f in rpm/repodata/repomd.xml rpm/repodata/repomd.xml.asc wimao.repo wimao.asc; do
    [ -f "$site/$f" ] || arreter "$site/$f est absent : le site n'a pas de dépôt dnf"
done

echo "== 0. Outils"
# python3 sert de serveur HTTP (python3 -m http.server) et diffutils fournit cmp ; les autres outils (curl, rpm, dnf) sont dans l'image.
dnf install -y -q --setopt=install_weak_deps=False python3 diffutils > /tmp/outils.log 2>&1 || { cat /tmp/outils.log >&2; arreter "installation de python3 impossible"; }

# servir <dossier> <port> : serveur HTTP local en arrière-plan, attendu prêt. Arrêté à la sortie.
pids=()
nettoyer() { for p in "${pids[@]}"; do kill "$p" 2> /dev/null || true; done; }
trap nettoyer EXIT
servir() {
    (cd "$1" && exec python3 -m http.server "$2" --bind 127.0.0.1 > "/tmp/serveur-$2.log" 2>&1) &
    pids+=("$!")
    for _ in $(seq 1 50); do
        curl -fsS -o /dev/null "http://127.0.0.1:$2/" 2> /dev/null && return 0
        sleep 0.2
    done
    arreter "le serveur du port $2 ne répond pas"
}

# poser_repo <port> : installe le .repo du site réécrit vers le serveur local de ce port. Chaque réécriture
# doit toucher exactement une ligne : sinon le .repo du site a changé de forme et l'essai ne prouverait plus rien.
poser_repo() {
    local source="$site/wimao.repo" cible=/etc/yum.repos.d/wimao.repo
    [ "$(grep -c '^baseurl=https://f1ghtc0re.github.io/WiMao/rpm$' "$source")" -eq 1 ] || arreter "wimao.repo : ligne baseurl inattendue"
    [ "$(grep -c '^gpgkey=https://f1ghtc0re.github.io/WiMao/wimao.asc$' "$source")" -eq 1 ] || arreter "wimao.repo : ligne gpgkey inattendue"
    grep -qx 'repo_gpgcheck=1' "$source" || arreter "wimao.repo : repo_gpgcheck=1 absent"
    sed -e "s#^baseurl=.*#baseurl=http://127.0.0.1:$1/rpm#" -e "s#^gpgkey=.*#gpgkey=http://127.0.0.1:8080/wimao.asc#" "$source" > "$cible"
    dnf clean all > /dev/null 2>&1
}

echo "== 1. Serveurs locaux"
servir "$site" 8080
if [ -n "$rpms" ]; then servir "$rpms" 8081; fi

echo "== 2. Un index altéré d'un caractère est refusé"
altere=/tmp/site-altere
rm -rf "$altere"
cp -r "$site" "$altere"
# Un seul caractère change : un chiffre de <revision>. Le XML reste valide, seule la signature ne correspond plus.
python3 -I - "$altere/rpm/repodata/repomd.xml" << 'PY'
import sys
chemin = sys.argv[1]
octets = bytearray(open(chemin, "rb").read())
debut = octets.index(b"<revision>") + len(b"<revision>")
octets[debut] = ord("0") + (octets[debut] - ord("0") + 1) % 10
open(chemin, "wb").write(bytes(octets))
PY
cmp -s "$site/rpm/repodata/repomd.xml" "$altere/rpm/repodata/repomd.xml" && arreter "essai mal monté : repomd.xml n'a pas été modifié"
[ "$(cmp -l "$site/rpm/repodata/repomd.xml" "$altere/rpm/repodata/repomd.xml" | wc -l)" -eq 1 ] || arreter "essai mal monté : plus d'un octet modifié"
servir "$altere" 8082
poser_repo 8082
code=0
dnf install -y wimao > /tmp/refus.log 2>&1 || code=$?
cat /tmp/refus.log
[ "$code" -ne 0 ] || arreter "dnf a accepté un index altéré"
if rpm -q wimao > /dev/null 2>&1; then arreter "wimao s'est installé depuis un index altéré"; fi
grep -qiE 'signature|GPG' /tmp/refus.log || arreter "l'échec de dnf ne parle pas de signature : ce n'est peut-être pas la bonne raison"
grep -q 'No match for argument' /tmp/refus.log || arreter "dnf n'a pas déclaré wimao introuvable (« No match for argument » attendu)"
echo "ok : l'index altéré est refusé (code $code), wimao n'est pas installé."

echo "== 3. Installation depuis l'index intact"
poser_repo 8080
# Les trois paquets sont nommés : wimao tire déjà les deux autres dans la vraie version, mais les nommer prouve
# aussi que les trois sont dans l'index (les faux RPM d'essai, eux, n'ont aucune dépendance).
dnf install -y wimao wimao-bridge wimao-wine 2>&1 | tee /tmp/installation.log
for paquet in wimao wimao-bridge wimao-wine; do
    rpm -q "$paquet" > /dev/null || arreter "$paquet n'est pas installé"
done
# La version installée de wimao est celle que l'index annonce (nom-version-1.x86_64.rpm dans primary).
annoncee="$(python3 -I - "$site/rpm/repodata" << 'PY'
import glob, re, subprocess, sys
fichier = glob.glob(sys.argv[1] + "/*-primary.xml.*")[0]
texte = subprocess.run(["zstd", "-dc", fichier] if fichier.endswith(".zst") else ["gzip", "-dc", fichier],
                       capture_output=True, check=True).stdout.decode()
m = re.search(r'<name>wimao</name>\s*<arch>[^<]*</arch>\s*<version epoch="[0-9]+" ver="([^"]+)"', texte)
print(m.group(1))
PY
)"
installee="$(rpm -q --qf '%{VERSION}' wimao)"
[ "$installee" = "$annoncee" ] || arreter "wimao installé en $installee, l'index annonce $annoncee"
# dnf a pris wimao dans le dépôt « wimao » (et non dans un autre dépôt de la distribution).
grep -E '^ *wimao +x86_64 .* wimao ' /tmp/installation.log > /dev/null || arreter "le journal de dnf n'indique pas le dépôt « wimao » pour le paquet wimao"
echo "ok : wimao $installee installé depuis le dépôt local, trois paquets présents."
echo "OK : tester-installation-dnf.sh"
