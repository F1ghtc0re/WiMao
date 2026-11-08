# WiMao sous-projet 8 : RPM et dépôt dnf — plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Réemballer les trois `.deb` en RPM reproductibles et publier un dépôt dnf signé, avec la même chaîne de confiance que le dépôt apt.

**Architecture:** Un script extrait chaque `.deb` et le passe à `rpmbuild` dans un conteneur Fedora figé. La CI ajoute les RPM à `SHA256SUMS`, à l'attestation et au brouillon de Release. Un index `depot-dnf/`, signé en local, pointe vers les RPM des Releases et rejoint le site GitHub Pages.

**Tech Stack:** bash, rpmbuild, createrepo_c, dnf5, podman, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-11-08-wimao-sous-projet-8-design.md`

## Global Constraints

- Tout script : `set -euo pipefail`, en-tête détaillé en français (rôle, usage, variables, ce qu'il écrit), `shellcheck` propre avec la version 0.9 (pas de `A && B || C`).
- Image Fedora 45 : `registry.fedoraproject.org/fedora:45@sha256:4a8a63ec2df9f26a96c35ff380237ad578cf42e326b979d9b5801c31c6daf2e8`. Elle sert à fabriquer et indexer.
- Image Fedora 44 : `registry.fedoraproject.org/fedora:44@sha256:4b39e947a92d0ac2df1db9ebbf7672256d828a73e6e0219f8a6e774f84f34d64`.
- Les workflows restent conformes à `paquets/verifier-workflows.sh` (actions et images figées, aucun nouveau droit d'écriture, aucun secret).
- Jobs en conteneur : `defaults: run: shell: bash`. jq compatible 1.7.
- Aucune clé en CI. Toute signature se fait sur la machine du propriétaire. Empreinte principale `3980C3D20A17566A075A65CBCB97DE3FBF41DD46`, sous-clé `5F1D4F6002B80C81`.
- Commits `WiMao : …`, terminés par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. `scripts/verifier-donnees-perso.sh` doit rendre 0. Le chemin de la clé SSH de signature peut demander une phrase de passe : si un commit reste bloqué, rendre la main au contrôleur.
- Rien n'est poussé, fusionné ou publié par les tâches.

## Review Focus

1. Un fichier installé par le RPM diffère de celui du `.deb` (droits, liens symboliques, bit exécutable, chemin avec espace ou `%`) : l'arborescence installée doit être identique ; test T1.
2. Un index dnf signé mais dont le `location` sort de `https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/` : refus à l'assemblage ; test T4.
3. `.repo` inchangé après une nouvelle version : `dnf upgrade` doit trouver la version suivante alors que le cache porte l'ancienne (`metadata_expire` court, fixé à `6h`) ; test T4 et T7.
4. Conflit avec le Wine de Fedora ou Carla absent : `wimao-wine` s'installe à côté du paquet `wine` système sans conflit de fichiers ; test T3.
5. Moins de six paquets dans le brouillon ou dans `SHA256SUMS` : `signer-version.sh` et la CI refusent ; test T2 et T5.

---

### Task 1: Fabrication reproductible des RPM

**Files:**
- Create: `paquets/fabriquer-rpm.sh`, `paquets/rpm/wimao.spec.in`, `paquets/rpm/{wimao,wimao-wine,wimao-bridge}.deps`, `paquets/rpm/{wimao,wimao-wine,wimao-bridge}.recommends`, `paquets/rpm/{wimao,wimao-wine,wimao-bridge}.resume`
- Test: `paquets/tests/tester-fabriquer-rpm.sh`

**Interfaces:**
- Produces: `paquets/fabriquer-rpm.sh <paquet.deb> <dossier-de-sortie>`, à lancer DANS Fedora 45 (paquets : `rpm-build`, `dpkg`, `zstd`). Écrit `<nom>-<version>-1.x86_64.rpm`. Lit `SOURCE_DATE_EPOCH` (obligatoire : refus si absent). Le nom et la version viennent de `dpkg-deb -f`. Pour `wimao`, la version Debian `0.5.0-1` donne Version `0.5.0` ; pour les deux autres, la version complète moins un éventuel suffixe `-1` : `5.1.1+dxvk3.1.1-1` donne `5.1.1+dxvk3.1.1`, `11.20+wimao1` reste tel quel.

- [ ] **Step 1: Écrire le test** `tester-fabriquer-rpm.sh`, lancé dans Fedora, sur un faux `.deb` construit par le test contenant : un exécutable, un fichier 0644, un lien symbolique, un chemin avec espace, un fichier contenant `%`. Assertions : (a) `rpm -qlp` liste tous les chemins ; (b) `rpm -qp --qf '%{VERSION}-%{RELEASE}'` conforme à la règle ci-dessus ; (c) après `rpm2cpio | cpio -id`, droits, liens et contenus égaux à ceux du `.deb` extrait (`diff -r` + `stat`) ; (d) deux fabrications : `cmp` identique ; (e) `SOURCE_DATE_EPOCH` absent : code non nul ; (f) `.deb` inconnu (sans fichier `.deps`) : code non nul ; (g) `rpm -qp --scripts` vide ; (h) `rpm -qp --requires` ne contient que les lignes de `.deps` et les `rpmlib(...)`.
- [ ] **Step 2: Lancer** : `podman run --rm -v "$PWD:/d:ro,Z" <image 45> bash /d/paquets/tests/tester-fabriquer-rpm.sh`. Attendu : échec (script absent).
- [ ] **Step 3: Implémenter.** Dépendances exactes : tableau du § 3.1 de la spec. Le `.spec` est écrit à partir de `wimao.spec.in` ; `%files` est généré depuis l'arborescence avec chaque chemin entre guillemets et `%` doublé ; les dossiers possédés se limitent à ceux propres au paquet (`/opt/wimao/...`), jamais `/usr/bin` ou `/usr/share`. Réglages : `AutoReqProv: no`, `%global debug_package %{nil}`, `%global __brp_* ` neutralisés par `%global __os_install_post %{nil}`, `%use_source_date_epoch_as_buildtime 1`, `%clamp_mtime_to_source_date_epoch 1`, `%_buildhost wimao.invalid`, `%_binary_payload w19T1.zstdio` avec un seul fil, `%define _build_id_links none`.
- [ ] **Step 4: Relancer le test.** Attendu : réussite. `shellcheck` propre.
- [ ] **Step 5: Essai réel** sur les vrais `.deb` 0.5.0 (`gh release download v0.5.0 -p '*.deb'` dans un dossier temporaire) : trois RPM, `cmp` identique sur deux fabrications.
- [ ] **Step 6: Commit.**

### Task 2: Les RPM dans la CI des paquets

**Files:**
- Modify: `.github/workflows/paquets.yml`, `paquets/ecrire-notes-release.sh`, `paquets/tester-verifier-workflows.sh` si une règle l'exige

**Interfaces:**
- Consumes: `paquets/fabriquer-rpm.sh` (T1).
- Produces: artefact `wimao-rpm` (trois `.rpm`) ; `SHA256SUMS` couvre six paquets.

- [ ] **Step 1:** Job `rpm` (`needs: [wine, bridge, wimao]`, conteneur image 45) : télécharge les `.deb`, installe `rpm-build dpkg zstd tar`, fabrique deux fois, compare par `cmp`, envoie l'artefact. `SOURCE_DATE_EPOCH` calculé comme les `.deb`.
- [ ] **Step 2:** `sommes` : `needs` gagne `rpm` avec condition de succès ; contrôle exact `3` `.deb` et `3` `.rpm`, sinon arrêt ; `sha256sum -- *.deb *.rpm` ; l'artefact contient aussi `paquets/*.rpm`.
- [ ] **Step 3:** `brouillon` et `brouillon-a-blanc` : attestation et Release couvrent les `.rpm` ; notes de Release listant les six paquets.
- [ ] **Step 4:** `paquets/verifier-workflows.sh` et `paquets/tester-verifier-workflows.sh` rendent 0. Attendu : aucune règle à assouplir.
- [ ] **Step 5: Commit.**

### Task 3: Test d'installation sur Fedora

**Files:**
- Create: `paquets/tester-installation-rpm.sh`
- Modify: `.github/workflows/paquets.yml`

**Interfaces:**
- Consumes: artefact `paquets-<sha court>` (T2) ; plugin Dexed et serveur JACK factice exactement comme `paquets/tester-installation.sh`.
- Produces: `paquets/tester-installation-rpm.sh <dossier-rpm> <dossier-journaux>`.

- [ ] **Step 1:** Écrire le script. Il `dnf install -y` les trois RPM locaux, puis vérifie : `wimao-cli --version` conforme ; `rpm -V` sans écart sur les trois paquets ; `wine --version` de `/opt/wimao/wine/bin/wine` rend `wine-11.20` ; Wine et yabridge lancés via `wimao-cli init` ; `wimao-cli verifier` donne `Dexed | VST3 | ✅ | The plugin opens` ; `wimao-cli diagnostiquer` rend `No problem found` ; retrait par `dnf remove` sans fichier restant sous `/opt/wimao`. Ajout : installer aussi le paquet `wine` de Fedora avant les RPM, et vérifier l'absence de conflit (Review Focus 4). Réutiliser la logique de Dexed en la factorisant dans une fonction de `paquets/commun.sh` si elle se partage sans changer le comportement Debian ; sinon la reproduire avec un commentaire.
- [ ] **Step 2:** Essai local dans `podman` image 45 avec les RPM de T1. Attendu : code 0. Si une bibliothèque manque au lancement, corriger `paquets/rpm/*.deps`.
- [ ] **Step 3:** Job `installation-rpm` avec matrice Fedora 44 et 45 (`fail-fast: false`), `needs: sommes`, blocs lisibles. `brouillon` et `brouillon-a-blanc` en dépendent. `verifier-workflows.sh` rend 0.
- [ ] **Step 4: Commit.**

### Task 4: Index dnf : construction, publication, assemblage du site

**Files:**
- Create: `scripts/publier-depot-dnf.sh`, `scripts/tests/tester-publier-depot-dnf.sh`, `depot-dnf/wimao.repo`
- Modify: `scripts/assembler-site-apt.sh`, `scripts/lib/publication.sh` si un contrôle se partage, `.github/workflows/depot-apt.yml`

**Interfaces:**
- Produces: `scripts/publier-depot-dnf.sh vX.Y.Z` ; dossier `depot-dnf/rpm/repodata/` (avec `repomd.xml.asc`) et `depot-dnf/wimao.repo`. Option `--a-blanc` : clé jetable, aucun appel `gh`, aucune PR, comme `signer-version.sh`.

- [ ] **Step 1: Écrire `tester-publier-depot-dnf.sh`.** Cas exigés, chacun avec code et message : réussite à blanc ; version mal formée ; dépôt non propre ; Release encore brouillon ; `SHA256SUMS.asc` absent ou invalide ; RPM à l'empreinte fausse ; nombre de RPM différent de trois.
- [ ] **Step 2: Implémenter** selon § 4.3, en reprenant la structure de `publier-depot-apt.sh`. `createrepo_c --location-prefix https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/ --revision <SOURCE_DATE_EPOCH> --set-timestamp-to-revision` dans le conteneur 45. Signature : `gpg -u 5F1D4F6002B80C81! --detach-sign --armor repodata/repomd.xml`, avec avertissement préalable sur la fenêtre de phrase de passe, et vérification par un trousseau jetable. `wimao.repo` : valeurs du § 4.2, plus `metadata_expire=6h`, `name=WiMao`, `enabled=1`.
- [ ] **Step 3: Assemblage.** `assembler-site-apt.sh` : si `depot-dnf/` existe, vérifier `repomd.xml.asc` avec `docs/cle-publique.asc`, contrôler que CHAQUE `location` de `primary.xml` commence par `https://github.com/F1ghtc0re/WiMao/releases/download/v` et se termine par `.rpm`, télécharger chaque RPM et contrôler taille et SHA-256, puis copier `rpm/` et `wimao.repo` dans `site/`, les RPM n'y étant pas copiés. Sans `depot-dnf/`, comportement inchangé. Test : ajouter à `scripts/tests/` un cas où un `location` pointe ailleurs, signature valide : arrêt (Review Focus 2).
- [ ] **Step 4: `depot-apt.yml`.** Filtres `paths` étendus à `depot-dnf/**`. Nouveau job `installation-dnf`, matrice Fedora 44 et 45 : installe `wimao` via le site assemblé servi par `python3 -m http.server`, le `.repo` étant réécrit vers cette adresse, puis prouve qu'un `repomd.xml` modifié d'un octet rend `wimao` introuvable. `deployer` en dépend, sauté si `depot-dnf/` est absent sans bloquer.
- [ ] **Step 5:** tous les tests, `shellcheck` et `verifier-workflows.sh` rendent 0.
- [ ] **Step 6: Commit.**

### Task 5: Signature locale

**Files:**
- Modify: `scripts/signer-version.sh`, `scripts/tests/tester-signer-version.sh`

**Interfaces:**
- Consumes: `paquets/fabriquer-rpm.sh` (T1), image 45.

- [ ] **Step 1: Test.** Cas ajoutés : RPM absent du brouillon, refus ; RPM dont le contenu diffère de la refabrication, refus ; réussite avec six paquets.
- [ ] **Step 2: Implémenter.** Contrôle exact 3 `.deb` + 3 `.rpm` ; attestation de chaque RPM ; refabrication des RPM à partir des `.deb` vérifiés, dans le conteneur 45 lu dans `paquets.yml` (job `rpm`, clé `image:`) ; comparaison octet pour octet. Ajouter en début de script, en mode réel, un déverrouillage de la sous-clé (signature d'un texte jetable) pour que la fenêtre s'ouvre tout de suite, avant la refabrication.
- [ ] **Step 3:** `scripts/tests/tester-signer-version.sh` et `shellcheck` : verts.
- [ ] **Step 4: Commit.**

### Task 6: Documentation

**Files:**
- Modify: `README.md`, `paquets/README.md`, `ARCHITECTURE.md`, `docs/publier-une-version.md`, `SECURITY.md`, `docs/ameliorations-reportees.md`, `crates/wimao/aide/{fr,en,es}/*.md` (seulement là où Ubuntu/Debian sont cités)

- [ ] **Step 1:** Rédiger selon le § 6 de la spec : commandes exactes du § 1, phrase expliquant « skipped OpenPGP checks », retrait du dépôt (`sudo rm /etc/yum.repos.d/wimao.repo`), limites du § 8.
- [ ] **Step 2:** `cargo test --workspace` (l'aide est embarquée) et `scripts/verifier-donnees-perso.sh` rendent 0.
- [ ] **Step 3: Commit.**

### Task 7: Essai de bout en bout (contrôleur)

- [ ] **Step 1:** Dans un conteneur Fedora 45 : dépôt de test à deux versions successives (0.5.0 réemballée, puis une 0.5.1 factice) ; `dnf install wimao`, puis `dnf upgrade --refresh` installe la suivante ; `dnf upgrade` sans `--refresh`, après `metadata_expire`, la voit aussi.
- [ ] **Step 2:** Vérifier la logique sur la vraie adresse GitHub (redirection) : `dnf download` d'un RPM via `--location-prefix` sur une Release publiée, dès qu'une Release contient des RPM ; sinon consigner que ce contrôle se fera à la première publication.
- [ ] **Step 3:** Pousser la branche, ouvrir la PR, CI verte. **Fusion uniquement avec l'accord explicite du propriétaire.**
