#!/usr/bin/env bash
# Teste paquets/verifier-workflows.sh : les vrais workflows passent, et chaque forme non figée
# ci-dessous est REFUSÉE (code 1). Lancé par le job `chaine` de la CI, à côté du script testé.
#
# Usage     : paquets/tester-verifier-workflows.sh
# Variables : aucune.
# Écrit     : uniquement dans un dossier temporaire, supprimé à la fin.
# Code      : 0 si tout se comporte comme prévu, 1 au premier écart.
set -euo pipefail

racine="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verifier="$racine/paquets/verifier-workflows.sh"
temporaire="$(mktemp -d)"
trap 'rm -rf "$temporaire"' EXIT

SHA40=0123456789abcdef0123456789abcdef01234567
SHA256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef

echec() {
    echo "ÉCHEC : $1" >&2
    exit 1
}

# Les vrais workflows doivent passer.
"$verifier" > /dev/null || echec "les workflows du dépôt sont refusés"
echo "ok : workflows du dépôt acceptés"

# Écrit un workflow d'essai complet (corps du job donné en argument) et rend son dossier.
fabriquer() {
    local dossier="$temporaire/$1"
    mkdir -p "$dossier"
    {
        printf 'name: essai\non: push\npermissions:\n  contents: read\njobs:\n  essai:\n'
        printf '%s\n' "$2"
    } > "$dossier/essai.yml"
    printf '%s' "$dossier"
}

# Vérifie que le contrôle accepte (attendu=0) ou refuse (attendu=1) le workflow d'essai.
attendre() {
    local nom="$1" attendu="$2" corps="$3" code=0 dossier
    dossier="$(fabriquer "$nom" "$corps")"
    "$verifier" "$dossier" > /dev/null 2>&1 || code=$?
    [ "$code" -eq "$attendu" ] || echec "$nom : code $code au lieu de $attendu"
    echo "ok : $nom"
}

# Témoin : un workflow figé est accepté (sinon les refus ci-dessous ne prouveraient rien).
attendre temoin 0 "    runs-on: ubuntu-24.04
    container:
      image: ubuntu:24.04@sha256:$SHA256
    steps:
      - uses: actions/checkout@$SHA40 # v4"

attendre image-non-figee 1 "    container:
      image: ubuntu:24.04
    steps:
      - run: echo"

attendre image-de-matrice-sans-empreinte 1 "    container:
      image: \${{ matrix.image }}
    strategy:
      matrix:
        include:
          - image: debian:13
    steps:
      - run: echo"

attendre matrice-fromjson 1 "    container:
      image: \${{ matrix.image }}
    strategy:
      matrix: \${{ fromJSON(vars.MATRICE) }}
    steps:
      - run: echo"

attendre action-non-figee 1 "    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4"

# Droits d'écriture : écrits en entier (racine et en-tête propres au cas), nom de fichier au choix.
# Arguments : nom du cas, code attendu, fichier (essai.yml ou paquets.yml), contenu complet du workflow.
attendre_droits() {
    local nom="$1" attendu="$2" fichier="$3" contenu="$4" code=0 dossier="$temporaire/$1" sortie
    mkdir -p "$dossier"
    printf '%s\n' "$contenu" > "$dossier/$fichier"
    sortie="$("$verifier" "$dossier" 2>&1)" || code=$?
    [ "$code" -eq "$attendu" ] || echec "$nom : code $code au lieu de $attendu"
    # Un refus doit venir du contrôle 5 (droits) et non d'un autre contrôle cassé par mégarde.
    if [ "$attendu" -ne 0 ]; then
        grep -q "droit" <<< "$sortie" || echec "$nom : refusé, mais pas pour une question de droits :
$sortie"
    fi
    echo "ok : $nom"
}

# Le job brouillon de paquets.yml peut écrire contents, id-token et attestations.
attendre_droits droits-brouillon-permis 0 paquets.yml "name: p
on: push
permissions:
  contents: read
jobs:
  brouillon:
    runs-on: ubuntu-24.04
    permissions:
      contents: write
      id-token: write
      attestations: write
    steps:
      - run: echo"

attendre_droits droits-ecriture-autre-job 1 paquets.yml "name: p
on: push
permissions:
  contents: read
jobs:
  autre:
    runs-on: ubuntu-24.04
    permissions:
      contents: write
    steps:
      - run: echo"

attendre_droits droits-write-all-racine 1 paquets.yml "name: p
on: push
permissions: write-all
jobs:
  brouillon:
    runs-on: ubuntu-24.04
    steps:
      - run: echo"

attendre_droits droits-ecriture-racine-bloc 1 paquets.yml "name: p
on: push
permissions:
  contents: write
jobs:
  brouillon:
    runs-on: ubuntu-24.04
    steps:
      - run: echo"

attendre_droits droits-write-all-job 1 paquets.yml "name: p
on: push
permissions:
  contents: read
jobs:
  brouillon:
    runs-on: ubuntu-24.04
    permissions: write-all
    steps:
      - run: echo"

attendre_droits droits-brouillon-portee-en-trop 1 paquets.yml "name: p
on: push
permissions:
  contents: read
jobs:
  brouillon:
    runs-on: ubuntu-24.04
    permissions:
      contents: write
      packages: write
    steps:
      - run: echo"

# Un job nommé brouillon dans un autre fichier que paquets.yml n'a aucun droit d'écriture.
attendre_droits droits-brouillon-autre-fichier 1 autre.yml "name: p
on: push
permissions:
  contents: read
jobs:
  brouillon:
    runs-on: ubuntu-24.04
    permissions:
      contents: write
    steps:
      - run: echo"

# Le job brouillon-a-blanc (demande de fusion) n'a PAS le droit d'écrire, même dans paquets.yml.
attendre_droits droits-brouillon-a-blanc-sans-ecriture 1 paquets.yml "name: p
on: push
permissions:
  contents: read
jobs:
  brouillon-a-blanc:
    runs-on: ubuntu-24.04
    permissions:
      contents: write
    steps:
      - run: echo"

# Job `deployer` de depot-apt.yml (publication GitHub Pages) : seul à pouvoir écrire pages et id-token.
attendre_droits droits-deployer-pages-permis 0 depot-apt.yml "name: d
on: push
permissions:
  contents: read
jobs:
  deployer:
    runs-on: ubuntu-24.04
    permissions:
      pages: write
      id-token: write
    steps:
      - run: echo"

# Même droits dans un AUTRE job de depot-apt.yml : refusés.
attendre_droits droits-pages-autre-job-depot-apt 1 depot-apt.yml "name: d
on: push
permissions:
  contents: read
jobs:
  assembler:
    runs-on: ubuntu-24.04
    permissions:
      pages: write
    steps:
      - run: echo"

attendre_droits droits-id-token-autre-job-depot-apt 1 depot-apt.yml "name: d
on: push
permissions:
  contents: read
jobs:
  installation:
    runs-on: ubuntu-24.04
    permissions:
      id-token: write
    steps:
      - run: echo"

# Un job `deployer` d'un autre fichier n'a aucun droit d'écriture.
attendre_droits droits-deployer-autre-fichier 1 autre.yml "name: d
on: push
permissions:
  contents: read
jobs:
  deployer:
    runs-on: ubuntu-24.04
    permissions:
      pages: write
      id-token: write
    steps:
      - run: echo"

# Dans `deployer`, les autres droits d'écriture restent refusés (contents, packages, attestations...).
attendre_droits droits-deployer-contents-write 1 depot-apt.yml "name: d
on: push
permissions:
  contents: read
jobs:
  deployer:
    runs-on: ubuntu-24.04
    permissions:
      pages: write
      id-token: write
      contents: write
    steps:
      - run: echo"

attendre_droits droits-deployer-attestations-write 1 depot-apt.yml "name: d
on: push
permissions:
  contents: read
jobs:
  deployer:
    runs-on: ubuntu-24.04
    permissions:
      attestations: write
    steps:
      - run: echo"

# Au niveau racine, même `pages: write` est refusé.
attendre_droits droits-pages-racine-depot-apt 1 depot-apt.yml "name: d
on: push
permissions:
  pages: write
jobs:
  deployer:
    runs-on: ubuntu-24.04
    steps:
      - run: echo"

# `deployer` de paquets.yml ne gagne pas non plus ces droits.
attendre_droits droits-pages-paquets-yml 1 paquets.yml "name: d
on: push
permissions:
  contents: read
jobs:
  deployer:
    runs-on: ubuntu-24.04
    permissions:
      pages: write
    steps:
      - run: echo"

# Contrôles 6 et 7 : déclencheurs dangereux et secrets. Comme pour les droits, un refus doit venir du bon
# contrôle (motif cherché dans le message), sinon un autre contrôle pourrait refuser « par accident ».
# Arguments : nom du cas, code attendu, motif du message (vide si accepté), contenu complet du workflow.
attendre_motif() {
    local nom="$1" attendu="$2" motif="$3" contenu="$4" code=0 dossier="$temporaire/$1" sortie
    mkdir -p "$dossier"
    printf '%s\n' "$contenu" > "$dossier/essai.yml"
    sortie="$("$verifier" "$dossier" 2>&1)" || code=$?
    [ "$code" -eq "$attendu" ] || echec "$nom : code $code au lieu de $attendu
$sortie"
    if [ "$attendu" -ne 0 ]; then
        grep -q -- "$motif" <<< "$sortie" || echec "$nom : refusé, mais pas pour le motif « $motif » :
$sortie"
    fi
    echo "ok : $nom"
}

attendre_motif declencheur-pull-request-target 1 "déclencheur dangereux" "name: p
on:
  pull_request_target:
permissions:
  contents: read
jobs:
  essai:
    runs-on: ubuntu-24.04
    steps:
      - run: echo"

attendre_motif declencheur-workflow-run 1 "déclencheur dangereux" "name: p
on:
  workflow_run:
    workflows: [Paquets]
permissions:
  contents: read
jobs:
  essai:
    runs-on: ubuntu-24.04
    steps:
      - run: echo"

attendre_motif declencheur-en-ligne 1 "déclencheur dangereux" "name: p
on: [push, pull_request_target]
permissions:
  contents: read
jobs:
  essai:
    runs-on: ubuntu-24.04
    steps:
      - run: echo"

attendre_motif secret-dans-env 1 "secret refusé" "name: p
on: push
permissions:
  contents: read
jobs:
  essai:
    runs-on: ubuntu-24.04
    env:
      JETON: \${{ secrets.MON_SECRET }}
    steps:
      - run: echo"

attendre_motif secret-crochets 1 "secret refusé" "name: p
on: push
permissions:
  contents: read
jobs:
  essai:
    runs-on: ubuntu-24.04
    steps:
      - run: echo \${{ secrets['MON_SECRET'] }}"

attendre_motif secrets-inherit 1 "secret refusé" "name: p
on: push
permissions:
  contents: read
jobs:
  essai:
    uses: ./.github/workflows/autre.yml
    secrets: inherit"

attendre_motif secrets-contexte-entier 1 "secret refusé" "name: p
on: push
permissions:
  contents: read
jobs:
  essai:
    runs-on: ubuntu-24.04
    steps:
      - run: echo \${{ toJSON(secrets) }}"

# Témoin : github.token et le mot « secrets » en commentaire ou en texte courant sont acceptés, comme un
# déclencheur ordinaire (pull_request) : les refus ci-dessus ne viennent donc pas d'un motif trop large.
attendre_motif temoin-token-et-commentaire 0 "" "name: p
on:
  pull_request:
  push:
# Aucun secrets.X ici : commentaire, ignoré. pull_request_target non plus.
permissions:
  contents: read
jobs:
  essai:
    runs-on: ubuntu-24.04
    env:
      GH_TOKEN: \${{ github.token }}
    steps:
      - run: echo 'les secretsaires et les mots de passe'"

echo "Tous les refus attendus ont lieu."
