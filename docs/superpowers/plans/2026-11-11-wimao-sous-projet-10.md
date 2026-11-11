# WiMao sous-projet 10 (paquet AUR `wimao-bin`) — plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** publier WiMao sur l'AUR (`wimao-bin`) avec une recette relue, testée en CI, doublement verrouillée
(empreintes + signature), envoyée seulement depuis la machine du propriétaire et surveillée chaque jour.

**Architecture :** un modèle `paquets/aur/PKGBUILD.in` + une liste de dépendances sont transformés par
`paquets/fabriquer-pkgbuild.sh` en `PKGBUILD` + `.SRCINFO` à partir de `SHA256SUMS`. `scripts/publier-aur.sh`
fabrique la recette réelle et ouvre une PR ; `scripts/envoyer-aur.sh` envoie le contenu de `main` sur l'AUR par
SSH. CI : `installation-aur` (paquets.yml, mode local), `aur.yml` (recette réelle, refus), `aur-surveillance.yml`.

**Tech Stack :** bash (`set -euo pipefail`, shellcheck), makepkg/pacman (conteneur Arch figé, podman en local),
GitHub Actions, gpgv (`scripts/lib/publication.sh`), git sur SSH.

**Spec :** `docs/superpowers/specs/2026-11-11-wimao-sous-projet-10-design.md`

## Global Constraints

- Image Arch, partout (scripts et workflows), figée par empreinte :
  `docker.io/library/archlinux:base-devel@sha256:97119fd9e21219b02b3c14ba256c9d3c0d031d07fc26c836c41ed793beeb5b74`.
  Dans les scripts, une seule définition : `IMAGE_ARCH` dans `paquets/aur/commun-aur.sh` (sourcé).
- `makepkg` refuse de tourner en root : toujours via un utilisateur non privilégié (`useradd -m constructeur`,
  `runuser -u constructeur --`), y compris pour `makepkg --printsrcinfo`.
- Empreinte de la clé (validpgpkeys) : lue dans `SECURITY.md` par `empreinte_publiee` (`scripts/lib/publication.sh`),
  jamais recopiée en dur dans un script ; valeur actuelle `3980C3D20A17566A075A65CBCB97DE3FBF41DD46`.
- Adresse des sources : `https://github.com/F1ghtc0re/WiMao/releases/download/v${pkgver}/<fichier>`.
- `pkgname=wimao-bin`, `pkgrel=1`, `arch=(x86_64)`, `provides=(wimao)`, `conflicts=(wimao)`,
  `license=('GPL-3.0-or-later' 'LGPL-2.1-or-later' 'Zlib')`, `options=(!strip !debug)`, pas d'`install=`.
- Ligne de marquage du mode local, exacte : `# ESSAI LOCAL, NE PAS PUBLIER`.
- Clé d'hôte AUR figée (vérifiée le 2026-11-11 contre l'empreinte publiée `SHA256:RFzBCUItH9LZS0cKB5UE6ceAYhBD5C8GeOBip8Z11+4`) :
  `aur.archlinux.org ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEuBKrPzbawxA/k2g6NcyV5jmqwJ2s+zpgZGZ7tpLIcN`.
- Clé SSH d'envoi : `~/.ssh/aur` ; rien n'est écrit dans `~/.ssh/config` ni dans `~/.ssh/known_hosts`.
- Auteur des commits AUR : `F1ghtc0re <162480684+F1ghtc0re@users.noreply.github.com>`.
- Workflows : règles de `paquets/verifier-workflows.sh` (actions figées par commit, `permissions:` racine en lecture,
  images figées, aucun `secrets.`, ni `pull_request_target` ni `workflow_run`).
- Style du dépôt : en-tête de script complet (but, usage, étapes, variables, écrit, codes 0/1/2), commentaires en
  français, messages d'erreur `ERREUR : …` sur la sortie d'erreur, aucun fichier créé dans le dossier personnel
  (dossiers temporaires supprimés par `trap`).

## Review Focus

1. `SHA256SUMS` liste aussi les `.rpm` : seules les trois lignes `.deb` doivent entrer dans la recette (test T1).
2. Premier envoi : le dépôt AUR `wimao-bin.git` existe mais est **vide** (pas de branche) ; `envoyer-aur.sh` doit
   créer la branche `master` au lieu d'échouer (test T4, faux AUR vide).
3. Un utilisateur Arch sans la clé importée : `makepkg` échoue sur la signature ; le message du README doit dire quoi
   faire (T6) et la CI doit prouver que la recette **exige** la signature (refus `SHA256SUMS.asc` altéré, T5).
4. Noms de fichiers avec `+` (`wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb`) dans les adresses : téléchargement réel
   prouvé par `aur.yml` (T5) et par `publier-aur.sh` (T3).
5. `.SRCINFO` périmé par rapport au `PKGBUILD` (édition à la main) : refusé par `envoyer-aur.sh` (T4) et `aur.yml` (T5).

---

### Task 1 : modèle, dépendances et `fabriquer-pkgbuild.sh`

**Files :**
- Create : `paquets/aur/commun-aur.sh` (`IMAGE_ARCH`, fonction `srcinfo <dossier>`)
- Create : `paquets/aur/PKGBUILD.in`, `paquets/aur/dependances`
- Create : `paquets/fabriquer-pkgbuild.sh`
- Test : `paquets/tests/tester-fabriquer-pkgbuild.sh` (nouvelle étape de `.github/workflows/ci.yml`, à côté de « Test de la publication dans le dépôt dnf » ; podman est présent sur le runner)

**Interfaces :**
- Produces : `paquets/fabriquer-pkgbuild.sh <SHA256SUMS> <sortie/> [--local] [--cle <empreinte>]` → écrit
  `<sortie>/PKGBUILD` et `<sortie>/.SRCINFO` ; codes 0/1/2. `--cle` remplace l'empreinte de `SECURITY.md` (tests).
  En `--local`, `source=` contient les trois noms de `.deb` seuls (pas de `SHA256SUMS*`, pas de `validpgpkeys`).
- Produces : `srcinfo <dossier>` (dans `commun-aur.sh`) : écrit `<dossier>/.SRCINFO` avec `makepkg --printsrcinfo`
  — directement si `makepkg` est là (conteneur Arch de la CI), sinon via `podman run "$IMAGE_ARCH"` ; toujours en
  utilisateur non root.

- [ ] **Step 1 : écrire `paquets/aur/dependances`** — format de la spec § 3.2 (une dépendance par ligne, `#`
  commentaire, `opt:<nom>: <raison>` pour les facultatives), exactement les listes du tableau § 3.2.
- [ ] **Step 2 : écrire le test** `tester-fabriquer-pkgbuild.sh`, avec un `SHA256SUMS` d'essai qui liste 3 `.deb`
  (`wimao_0.7.0-1_amd64.deb`, `wimao-bridge_5.1.1+dxvk3.1.1-1_amd64.deb`, `wimao-wine_11.20+wimao1_amd64.deb`) et 3
  `.rpm`, la version de `Cargo.toml` étant lue dans le dépôt. Cas :
  - sortie : `pkgver=` = version de `Cargo.toml` ; `source=` contient les 5 adresses de la spec § 3.3 dans cet ordre
    (wimao, bridge, wine, SHA256SUMS, SHA256SUMS.asc) ; `sha256sums=` donne les empreintes du fichier pour les
    `.deb` et celle de `SHA256SUMS` lui-même, `SKIP` pour le `.asc` ; `validpgpkeys=` = `--cle` ; aucun `.rpm` ;
  - deux exécutions → `PKGBUILD` et `.SRCINFO` identiques (`cmp`) ;
  - `.SRCINFO` contient `pkgbase = wimao-bin` et `depends = gtk4`, `optdepends = carla: …` ;
  - refus (code 1) : `SHA256SUMS` sans le `.deb` de wine ; avec un 4e `.deb` ; version wimao ≠ `Cargo.toml` ;
    version mal formée (`wimao_0.7-1_amd64.deb`) ;
  - `--local` : première ligne `# ESSAI LOCAL, NE PAS PUBLIER`, `source=` = 3 noms nus, pas de `validpgpkeys` ;
  - usage (code 2) : argument manquant, option inconnue.
- [ ] **Step 3 : lancer le test**, constater l'échec (script absent).
- [ ] **Step 4 : écrire `PKGBUILD.in`, `commun-aur.sh` et `fabriquer-pkgbuild.sh`.** Le `PKGBUILD` :
  `prepare()` (mode réel seulement) fait `sha256sum -c SHA256SUMS --ignore-missing` et vérifie que les 3 `.deb` y
  sont ; `package()` extrait `data.tar.*` de chaque `.deb` (`bsdtar -xOf <deb> 'data.tar.*' | bsdtar -xpf - -C
  "$pkgdir"`), puis retire `"$pkgdir/usr/share/lintian"`. `makedepends` vide (bsdtar vient de `base-devel`). Le
  `PKGBUILD` commence par un commentaire qui dit d'où il vient (« généré par paquets/fabriquer-pkgbuild.sh depuis
  le dépôt WiMao ; ne pas modifier à la main ») et le nom du mainteneur (pseudonyme + adresse noreply).
- [ ] **Step 5 : lancer le test** jusqu'au vert ; `shellcheck` sur les nouveaux scripts.
- [ ] **Step 6 : essai réel local** : `fabriquer-pkgbuild.sh` sur le `SHA256SUMS` de la Release v0.7.0 (avec la
  vraie clé), puis `makepkg` dans `IMAGE_ARCH` (clé importée depuis `docs/cle-publique.asc`) → paquet construit.
  Ne rien committer de cet essai (dossier temporaire).
- [ ] **Step 7 : commit** `AUR : modèle de recette et fabriquer-pkgbuild.sh`.

### Task 2 : job CI `installation-aur`

**Files :**
- Create : `paquets/tester-installation-aur.sh`
- Modify : `.github/workflows/paquets.yml` (nouveau job ; ajouté aux `needs` de `brouillon` et `brouillon-a-blanc`)

**Interfaces :**
- Consumes : `fabriquer-pkgbuild.sh --local` (T1).
- Produces : `paquets/tester-installation-aur.sh <dossier des paquets> <dossier des journaux>` (même contrat que
  `tester-installation-rpm.sh` : contrôle `sha256sum -c` des `.deb`, Dexed téléchargé et vérifié comme là-bas).

- [ ] **Step 1 : écrire le script** sur le modèle de `paquets/tester-installation-rpm.sh` : vérifie les empreintes,
  `fabriquer-pkgbuild.sh --local`, copie les `.deb`, `makepkg` en `constructeur`, `pacman -U --noconfirm`,
  dépendances facultatives de test (`carla`, `xorg-xwininfo`) installées **après**, puis la même suite de contrôles
  (`wimao-cli --version`, `verifier` sur Dexed, journaux), puis `pacman -R` et contrôle que `/opt/wimao` a disparu.
- [ ] **Step 2 : ajouter le job** `installation-aur` (`needs: sommes`, même `if:` que `installation-rpm`, conteneur
  = image Arch figée, `pacman -Syu --noconfirm tar git` avant checkout, artefact `paquets-<sha court>`, journaux
  envoyés `if: always()`).
- [ ] **Step 3 : vérifier** `paquets/verifier-workflows.sh` (code 0) et `shellcheck`.
- [ ] **Step 4 : essai local** du script dans `IMAGE_ARCH` avec les `.deb` de v0.7.0 (podman, dossier temporaire) :
  code 0.
- [ ] **Step 5 : commit** `CI : installation du paquet AUR sur Arch`.

### Task 3 : `scripts/publier-aur.sh`

**Files :**
- Create : `scripts/publier-aur.sh`
- Test : `scripts/tests/tester-publier-aur.sh` (nouvelle étape de `.github/workflows/ci.yml`, après celle de `tester-publier-depot-dnf.sh`)

**Interfaces :**
- Consumes : `fabriquer-pkgbuild.sh` (T1) ; `empreinte_publiee`, `trousseau_cle_publiee`, `verifier_signature`
  (`scripts/lib/publication.sh`).
- Produces : `scripts/publier-aur.sh vX.Y.Z` (réel) et `scripts/publier-aur.sh vX.Y.Z --a-blanc --dossier <d>
  --cle <empreinte> --sortie <d> --racine <d>` ; écrit `paquets/aur/wimao-bin/{PKGBUILD,.SRCINFO}` ; codes 0/1/2.

- [ ] **Step 1 : écrire le test** avec une clé jetable (modèle `tester-publier-depot-dnf.sh`) : à blanc, sortie =
  `PKGBUILD` dont `validpgpkeys` = clé d'essai ; refus : `SHA256SUMS.asc` absent, signé par une autre clé, version
  mal formée (`v0.7`), `--dossier` sans les `.deb` attendus ; usage (code 2) : option à blanc sans `--a-blanc`.
- [ ] **Step 2 : lancer le test**, constater l'échec.
- [ ] **Step 3 : écrire le script**, étapes de la spec § 4.2 ; en réel, l'essai `makepkg` + `pacman -U` +
  `wimao-cli --version` dans `IMAGE_ARCH` (sources téléchargées depuis la Release, clé importée depuis
  `docs/cle-publique.asc`), puis branche `aur-vX.Y.Z` (refusée si elle existe ici ou sur origin), commit des seuls
  fichiers de `paquets/aur/wimao-bin/`, `gh pr create`. Il ne fusionne jamais ; aucune phrase de passe.
- [ ] **Step 4 : tests au vert**, `shellcheck`.
- [ ] **Step 5 : commit** `AUR : publier-aur.sh prépare la recette et ouvre une PR`.

### Task 4 : `scripts/envoyer-aur.sh`

**Files :**
- Create : `scripts/envoyer-aur.sh`, `paquets/aur/aur.archlinux.org.known_hosts` (la ligne des Global Constraints)
- Test : `scripts/tests/tester-envoyer-aur.sh` (ajouté à la CI au même endroit que T3)

**Interfaces :**
- Consumes : `srcinfo` (T1) ; `paquets/aur/wimao-bin/` (T3).
- Produces : `scripts/envoyer-aur.sh vX.Y.Z` (réel) et `scripts/envoyer-aur.sh vX.Y.Z --a-blanc --aur <dépôt git
  local> --racine <d>` ; codes 0/1/2.

- [ ] **Step 1 : écrire le test** : faux AUR = dépôt git nu local. Cas :
  - faux AUR **vide** → après envoi, sa branche `master` contient exactement `PKGBUILD` et `.SRCINFO` de la racine,
    commit `wimao-bin 0.7.0-1`, auteur pseudonyme/noreply ;
  - deuxième envoi identique → « rien à envoyer », code 0, aucun nouveau commit ;
  - refus (code 1) : `pkgver` ≠ version demandée ; `PKGBUILD` avec `# ESSAI LOCAL, NE PAS PUBLIER` ; `.SRCINFO`
    modifié d'une ligne ; racine pas propre (fichier suivi modifié) ;
  - usage (code 2).
- [ ] **Step 2 : lancer le test**, constater l'échec.
- [ ] **Step 3 : écrire le script**, étapes de la spec § 4.3. En réel : refus si pas sur `main`, pas à jour avec
  `origin/main`, ou si `~/.ssh/aur` manque (message : comment la créer, renvoi à `docs/publier-une-version.md`) ;
  `GIT_SSH_COMMAND` avec `-i ~/.ssh/aur -o IdentitiesOnly=yes -o UserKnownHostsFile=<known_hosts du dépôt>
  -o StrictHostKeyChecking=yes` ; message avant le `git push` : « une fenêtre de phrase de passe de la clé SSH peut
  s'ouvrir » ; après envoi, relecture par `https://aur.archlinux.org/cgit/aur.git/plain/PKGBUILD?h=wimao-bin` et
  `.SRCINFO` → identiques à `main`, sinon code 1.
- [ ] **Step 4 : tests au vert**, `shellcheck`.
- [ ] **Step 5 : commit** `AUR : envoyer-aur.sh envoie la recette de main sur l'AUR`.

### Task 5 : workflows `aur.yml` et `aur-surveillance.yml`

**Files :**
- Create : `.github/workflows/aur.yml`, `.github/workflows/aur-surveillance.yml`
- Create : `paquets/tester-recette-aur.sh` (logique de `aur.yml`, lançable en local)

**Interfaces :**
- Consumes : `srcinfo` (T1), `paquets/aur/wimao-bin/` (T3).
- Produces : `paquets/tester-recette-aur.sh <dossier de la recette> <journaux>` ; codes 0/1, et 0 avec la note
  « aucune recette publiée » si le dossier n'a pas de `PKGBUILD`.

- [ ] **Step 1 : écrire `tester-recette-aur.sh`** : `.SRCINFO` régénéré = celui du dossier ; clé importée depuis
  `docs/cle-publique.asc` pour `constructeur` ; `makepkg` réel + `pacman -U` + `wimao-cli --version` +
  `verifier` Dexed (fonctions partagées avec `tester-installation-aur.sh` si possible, sinon appel de ce script) ;
  puis trois refus attendus, chacun dans une copie neuve de la recette : (a) le `.deb` wimao, modifié d'un octet,
  posé à côté du `PKGBUILD` avant `makepkg` ; (b) une empreinte `sha256sums` changée ; (c) un `SHA256SUMS.asc`
  modifié d'un caractère. Chaque refus doit échouer **à la vérification** (message de makepkg contrôlé), pas pour une
  autre raison.
- [ ] **Step 2 : `aur.yml`** : `on: pull_request` (chemins `paquets/aur/**`, `paquets/fabriquer-pkgbuild.sh`,
  `paquets/tester-recette-aur.sh`) et `workflow_dispatch` ; conteneur Arch figé ; `pacman -Syu` (Arch vivant).
- [ ] **Step 3 : `aur-surveillance.yml`** : `schedule` (cron `17 6 * * *`) + `workflow_dispatch` ; clone public
  `https://aur.archlinux.org/wimao-bin.git` ; si le dépôt est vide ou absent → note, code 0 ; sinon `cmp` de
  `PKGBUILD` et `.SRCINFO` avec `paquets/aur/wimao-bin/` → échec si différent (message : que faire, renvoi à
  `SECURITY.md`). `runs-on: ubuntu-24.04`, sans conteneur.
- [ ] **Step 4 : vérifier** `paquets/verifier-workflows.sh` (code 0), `shellcheck`, et lancer
  `tester-recette-aur.sh` en local sur une recette v0.7.0 fabriquée en T1 (podman) : code 0 et trois refus constatés.
- [ ] **Step 5 : commit** `CI : recette AUR éprouvée et surveillance quotidienne de l'AUR`.

### Task 6 : documentation

**Files :** `README.md`, `docs/publier-une-version.md`, `SECURITY.md`, `paquets/README.md`, `ARCHITECTURE.md`,
`docs/installation-detaillee.md`, `crates/wimao/aide/{fr,en,es}/` (là où Ubuntu et Fedora sont cités),
`docs/ameliorations-reportees.md`.

- [ ] **Step 1 :** README : section courte « Arch Linux » (import de la clé + empreinte à comparer, `yay -S
  wimao-bin` ou `git clone https://aur.archlinux.org/wimao-bin.git && cd wimao-bin && makepkg -si`, lire la recette
  avant, désinstallation) ; détails dans `docs/installation-detaillee.md`. Le README doit rester court.
- [ ] **Step 2 :** `publier-une-version.md` : schéma et étape 7 (« Publier sur l'AUR » : `publier-aur.sh`, fusion,
  `envoyer-aur.sh` juste après, surveillance) ; « Avant la toute première version » : compte AUR et clé SSH (spec
  § 4.5) ; « Ce qu'il faut sur votre machine ».
- [ ] **Step 3 :** `SECURITY.md` : risque du compte AUR, protections, conduite si la surveillance échoue (spec § 6).
- [ ] **Step 4 :** `paquets/README.md`, `ARCHITECTURE.md`, aide fr/en/es (traductions cohérentes ;
  `scripts/verifier-traductions.sh` au vert), `ameliorations-reportees.md` (limites spec § 8).
- [ ] **Step 5 :** `cargo test -q --workspace` (l'aide est testée), `scripts/verifier-donnees-perso.sh` (code 0).
- [ ] **Step 6 : commit** `Docs : AUR (installation, publication, sécurité)`.
