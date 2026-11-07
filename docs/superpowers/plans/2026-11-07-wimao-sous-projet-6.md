# WiMao sous-projet 6 — Dépôt APT signé : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** servir WiMao par un dépôt APT signé sur GitHub Pages, la signature restant sur la machine du propriétaire.

**Architecture:** `publier-depot-apt.sh` (machine du propriétaire) vérifie la Release, construit et signe l'index APT et l'envoie par PR dans `depot-apt/` ; le workflow `depot-apt.yml` vérifie la signature, télécharge les `.deb` des Releases, contrôle leurs empreintes, teste une installation et déploie GitHub Pages depuis `main`. Fonctions communes dans `scripts/lib/publication.sh`.

**Tech Stack:** bash (`set -euo pipefail`, shellcheck -x), GnuPG/gpgv, `apt-ftparchive` (apt-utils), `dpkg-deb`, `jq`, `gh`, GitHub Actions et Pages.

**Spec:** `docs/superpowers/specs/2026-11-07-wimao-sous-projet-6-design.md`

## Global Constraints

- Tout en français : messages, commentaires détaillés et utiles, documentation, commits (terminés par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, signés par la configuration locale).
- En-têtes des scripts sur le modèle de `scripts/tests/tester-signer-version.sh` (rôle, cas couverts, Usage, Variables, Écrit, Code).
- `shellcheck -x` sans avertissement ; pas de `grep -q` dans un tuyau sous `pipefail` ; pas de `A && B || C` ; JSON par `jq -n`.
- Les tests n'écrivent que dans un `mktemp -d` supprimé à la fin ; trousseaux GnuPG jetables, jamais `~/.gnupg`.
- Toute vérification de signature passe par `gpgv` avec un trousseau construit depuis `docs/cle-publique.asc`, et exige l'empreinte publiée dans `SECURITY.md` (`--cle` en mode à blanc).
- Adresse du site : `https://f1ghtc0re.github.io/WiMao` ; suite `stable`, composant `main`, architecture `amd64`.
- `Filename` des paquets : `pool/vX.Y.Z/<nom du .deb>` ; étiquette au format `vX.Y.Z` (trois nombres).
- Champs de `Release` : `Origin: WiMao`, `Label: WiMao`, `Suite: stable`, `Codename: stable`, `Architectures: amd64`, `Components: main`, `Description: WiMao — Wine pour la MAO (paquets signés)` ; SHA256 seulement ; pas de `Valid-Until`.
- Images de conteneur : exactement les empreintes `ubuntu:24.04@sha256:…` et `debian:13@sha256:…` déjà présentes dans `.github/workflows/paquets.yml`.
- Tester tout ce qui utilise `jq` aussi avec jq 1.7 (`podman run docker.io/library/ubuntu:24.04`) : la CI tourne sous jq 1.7, la machine sous jq 1.8.

## Review Focus

- `depot-apt/` encore absent (avant la première publication, et sur la PR de ce sous-projet) : `assembler-site-apt.sh` doit le dire et sortir proprement, et le workflow ne doit ni installer ni déployer, sans échouer. Test au Task 3, gestion au Task 4.
- Relancer `publier-depot-apt.sh` pour la même version : index identique, aucune entrée en double. Test au Task 2.
- Une signature valide mais faite par une autre clé que celle publiée (autre clé du trousseau, clé jetable) : refusée partout. Tests aux Tasks 1, 2 et 3.
- Un `Filename` qui sort de `pool/vX.Y.Z/` (`../`, chemin absolu, étiquette mal formée) : refusé avant tout téléchargement. Test au Task 3.
- Un `.deb` servi altéré : `apt` doit refuser l'installation (preuve réelle dans le job `installation`). Task 4.

---

### Task 1 : `scripts/lib/publication.sh` (fonctions communes)

**Files:**
- Create: `scripts/lib/publication.sh` (sourcé, sans bit d'exécution, sans `set -e` propre : il hérite de l'appelant)
- Modify: `scripts/signer-version.sh` (supprime sa copie de `empreinte_publiee`, source la bibliothèque)
- Create: `scripts/tests/tester-publication.sh`
- Modify: `.github/workflows/ci.yml` (job `chaine`)

**Interfaces:**
- Produces:
  - `empreinte_publiee <racine du dépôt>` → imprime 40 hexadécimaux majuscules ; code 1 et message si la ligne manque ou est invalide (comportement identique à l'actuel).
  - `trousseau_cle_publiee <racine> <empreinte> <fichier trousseau>` → crée le trousseau `gpgv` depuis `<racine>/docs/cle-publique.asc` ; code 1 si la clé principale du fichier n'a pas `<empreinte>`.
  - `verifier_signature <trousseau> <empreinte> <signature> [<fichier signé>]` → code 0 seulement si `gpgv --status-fd` rapporte `VALIDSIG` dont l'empreinte de clé principale (dernier champ de `VALIDSIG`) vaut `<empreinte>` ; sinon code 1 avec un message.

- [ ] **Step 1 : test** `tester-publication.sh` : deux clés jetables A et B (A avec sous-clé de signature), faux dépôt contenant `SECURITY.md` et `docs/cle-publique.asc` de A. Cas : `empreinte_publiee` lit l'empreinte avec espaces ; ligne absente → 1 ; `trousseau_cle_publiee` avec l'empreinte de A → 0, avec celle de B → 1 ; signature détachée par la sous-clé de A → 0 ; clearsign par A → 0 ; signature par B (B présente dans le trousseau via un second fichier importé) → 1 ; fichier modifié après signature → 1.
- [ ] **Step 2 : échec attendu** (bibliothèque absente).
- [ ] **Step 3 : écrire la bibliothèque** ; `signer-version.sh` la source par un chemin relatif à `BASH_SOURCE`.
- [ ] **Step 4 : verts** : `tester-publication.sh` et `scripts/tests/tester-signer-version.sh` (inchangé) ; shellcheck.
- [ ] **Step 5 : CI** (étape après « Test de la signature des versions ») ; **commit**.

### Task 2 : `scripts/publier-depot-apt.sh`

**Files:**
- Create: `scripts/publier-depot-apt.sh`, `scripts/tests/tester-publier-depot-apt.sh`
- Modify: `.github/workflows/ci.yml` (job `chaine` : installer `apt-utils` si `apt-ftparchive` manque, puis lancer le test)

**Interfaces:**
- Consumes: Task 1 (`empreinte_publiee`, `trousseau_cle_publiee`, `verifier_signature`).
- Produces: `scripts/publier-depot-apt.sh vX.Y.Z [--a-blanc --dossier <artefacts> --cle <empreinte> --sortie <dossier depot-apt>] [--aide]` ; codes 0/1/2 ; en mode réel écrit `depot-apt/dists/stable/{Release,InRelease,Release.gpg,main/binary-amd64/Packages,Packages.gz}` et ouvre une PR `depot-apt-vX.Y.Z`.

- [ ] **Step 1 : test** (clé jetable avec sous-clé de signature ; trois faux `.deb` fabriqués par `dpkg-deb --build` nommés comme les vrais mais en version `9.9.9` ; `SHA256SUMS` et `SHA256SUMS.asc` signés par la clé ; faux `gh` et faux `git` en tête du PATH qui consignent et échouent). Cas de la spec § 10 : trois entrées `pool/v9.9.9/` avec `SHA256` et `Size` justes ; `gpgv` accepte `InRelease` et `Release.gpg` ; `Release` ne contient ni `MD5Sum` ni `SHA1` mais les champs exacts des Global Constraints et l'empreinte SHA256 de `Packages` et `Packages.gz` ; même version relancée → fichiers `Packages` identiques octet pour octet ; `v9.9.10` ajoutée → six entrées, triées par nom puis version ; `SHA256SUMS.asc` d'une autre clé → 1 ; `.deb` modifié → 1 ; `.deb` en trop → 1 ; aucun appel aux faux `gh`/`git` ; option inconnue → 2 ; étiquette `v1.2` → 2.
- [ ] **Step 2 : échec attendu.**
- [ ] **Step 3 : écrire le script** selon la spec § 4 (étapes 1 à 8). Fusion de `Packages` : découper en paragraphes, retirer ceux dont `Filename` commence par `pool/vX.Y.Z/`, ajouter les nouveaux, trier par `Package` puis `Version` avec `dpkg --compare-versions` (pas de `sort -V`). Configuration de `apt-ftparchive release` passée par `-o APT::FTPArchive::Release::<Champ>=…` et `-o APT::FTPArchive::Release::MD5=false`, `…::SHA1=false`, `…::SHA512=false`. Message avant la signature : « La fenêtre de la phrase de passe va s'ouvrir ; elle attend environ une minute. »
- [ ] **Step 4 : verts** (test, shellcheck, et le test aussi dans `podman run ubuntu:24.04` avec `apt-utils gnupg jq dpkg-dev` installés).
- [ ] **Step 5 : CI, commit.**

### Task 3 : `scripts/assembler-site-apt.sh`

**Files:**
- Create: `scripts/assembler-site-apt.sh`, `scripts/tests/tester-assembler-site-apt.sh`
- Modify: `.github/workflows/ci.yml` (job `chaine`)

**Interfaces:**
- Consumes: Task 1 ; le format de `depot-apt/` produit par Task 2.
- Produces: `scripts/assembler-site-apt.sh <dossier du site> [--cle <empreinte>]` ; code 0 site complet ; **code 3 « dépôt APT vide »** si `depot-apt/dists/stable/InRelease` est absent (rien écrit) ; 1 contrôle échoué ; 2 usage. Variable `WIMAO_SOURCE_RELEASES` (tests seulement) : dossier local `<étiquette>/<nom>` remplaçant `https://github.com/F1ghtc0re/WiMao/releases/download`.

- [ ] **Step 1 : test** : un `depot-apt/` produit par `publier-depot-apt.sh --a-blanc` (clé jetable, faux `.deb`), faux téléchargements dans `WIMAO_SOURCE_RELEASES`. Cas : site complet (`dists/…`, `pool/v9.9.9/*.deb`, `wimao.asc` identique à `docs/cle-publique.asc`, `index.html` contenant l'adresse du dépôt et l'empreinte) ; `InRelease` altéré → 1 ; `Packages` altéré (empreinte de `Release` fausse) → 1 ; `.deb` téléchargé de taille ou d'empreinte différente → 1 ; `Filename: pool/../x.deb`, `Filename: /etc/x.deb`, `Filename: pool/v1.2/x.deb` → 1 sans aucun téléchargement ; `depot-apt/` absent → 3.
- [ ] **Step 2 : échec attendu. Step 3 : script** (téléchargement par `curl -fsSL --proto '=https'` en mode réel ; `index.html` en HTML simple, autonome, sans ressource externe, avec les quatre commandes de la spec § 1 et l'empreinte). **Step 4 : verts** (aussi sous jq 1.7 si `jq` est utilisé). **Step 5 : CI, commit.**

### Task 4 : workflow `depot-apt.yml` et contrôle des workflows

**Files:**
- Create: `.github/workflows/depot-apt.yml`
- Modify: `paquets/verifier-workflows.sh` (règle 5), `paquets/tester-verifier-workflows.sh`

**Interfaces:**
- Consumes: Task 3 (codes 0/1/3).

- [ ] **Step 1 : test du contrôle** : `pages: write` et `id-token: write` admis dans le job `deployer` de `depot-apt.yml` ; refusés dans un autre job de ce fichier et dans tout autre fichier ; les autres droits d'écriture restent refusés dans `deployer`.
- [ ] **Step 2 : échec attendu ; Step 3 : règle 5 étendue** (commentaire d'en-tête mis à jour).
- [ ] **Step 4 : workflow** selon la spec § 5. Job `assembler` : sortie `vide` (`true` si code 3) ; `installation` et `deployer` ont `if: needs.assembler.outputs.vide == 'false'`. `deployer` : `if` supplémentaire `github.ref == 'refs/heads/main' && github.event_name != 'pull_request'`, `environment: { name: github-pages, url: … }` écrit en bloc (le contrôle refuse les formes en ligne), `concurrency` du groupe `pages` sans annulation. Actions `actions/upload-pages-artifact` et `actions/deploy-pages` figées par empreinte du commit de leur dernière étiquette (vérifiée par `gh api repos/actions/<nom>/git/ref/tags/<étiquette>`, en déréférençant une étiquette annotée), commentaire `# vX.Y.Z`. Second essai d'installation avec un octet modifié dans une copie du `.deb` de `wimao` servi : `apt-get install --reinstall` doit échouer.
- [ ] **Step 5 : verts** : `paquets/verifier-workflows.sh`, son test, shellcheck ; **commit**.

### Task 5 : réglages GitHub Pages dans `regler-depot-github.sh`

**Files:**
- Modify: `scripts/regler-depot-github.sh`, `scripts/tests/tester-regler-depot-github.sh`

- [ ] **Step 1 : tests** (spec § 7) : en `--a-blanc`, `POST repos/<d>/pages` avec `{"build_type":"workflow"}` si Pages absent (faux `gh` : 404 sur `GET …/pages`), `PUT …/pages` avec `{"build_type":"workflow","https_enforced":true}` ; `PUT …/environments/github-pages` avec `deployment_branch_policy` `{protected_branches:false, custom_branch_policies:true}` ; `POST …/deployment-branch-policies` `{"name":"main","type":"branch"}` seulement si absente ; vérification ❌ si `build_type` ≠ `workflow`, si HTTPS n'est pas imposé, ou si une autre politique que `main` existe.
- [ ] **Step 2 : échec ; Step 3 : code** (mêmes fonctions `poser`/`lire`/`controler`) ; **Step 4 : verts** sous jq 1.8 et jq 1.7 ; **Step 5 : commit.**

### Task 6 : documentation

**Files:**
- Modify: `README.md`, `docs/publier-une-version.md`, `SECURITY.md`, `paquets/README.md`, `docs/ameliorations-reportees.md`

- [ ] Contenu de la spec § 9 ; commandes copiées exactement de la spec § 1 ; `scripts/verifier-donnees-perso.sh` : code 0 ; **commit**.

### Task 7 : mise en service (contrôleur, avec l'utilisateur)

- [ ] Relecture finale de la branche (modèle le plus capable), corrections ; `cargo test --workspace` et tous les tests de scripts verts.
- [ ] PR `sous-projet-6` ; CI verte (le workflow `depot-apt.yml` y constate « dépôt APT vide ») ; **fusion avec l'accord explicite de l'utilisateur**.
- [ ] Expliquer puis lancer `sudo apt install -y apt-utils`.
- [ ] `scripts/regler-depot-github.sh --appliquer` : tout ✅ (Pages activé).
- [ ] `scripts/publier-depot-apt.sh v0.4.0` (l'utilisateur surveille la fenêtre de la phrase de passe) → PR `depot-apt-v0.4.0` ; CI : `assembler` et `installation` verts sur Ubuntu 24.04 et Debian 13 ; **fusion avec accord** ; `deployer` vert.
- [ ] Contrôle public : `curl` de `https://f1ghtc0re.github.io/WiMao/dists/stable/InRelease` vérifié par `gpgv` ; page d'accueil lisible.
- [ ] Machine : clé et source posées (`sudo install -m 644`), `sudo apt update`, `apt policy wimao wimao-bridge wimao-wine` montre le dépôt.
- [ ] Mémoire (`etat-wimao.md`) mise à jour.
