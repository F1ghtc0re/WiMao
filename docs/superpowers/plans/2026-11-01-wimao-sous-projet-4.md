# WiMao sous-projet 4 — paquets `.deb` et sécurité de la chaîne : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** fabriquer, tester et préparer à la publication signée les paquets `wimao-wine`, `wimao-bridge` et `wimao` pour Ubuntu ≥ 24.04 et Debian ≥ 13 (amd64), avec une chaîne de fabrication où rien n'entre sans vérification et rien ne sort sans le propriétaire.

**Architecture:** des scripts Bash commentés dans `paquets/` fabriquent chaque `.deb` (lançables en local et dans la CI) ; un nouveau workflow `.github/workflows/paquets.yml` les fabrique dans des conteneurs `ubuntu:24.04`, contrôle la reproductibilité, teste l'installation dans `ubuntu:24.04` et `debian:13`, et (sur étiquette de version seulement) crée un brouillon de Release avec attestations. La signature se fait en local avec `scripts/signer-version.sh`.

**Tech Stack:** Bash, `dpkg-deb`, `dpkg-shlibdeps`, `cargo-deb`, GitHub Actions, `lintian`, `appstreamcli`, `desktop-file-validate`, `cargo-deny`, `cargo-audit`, GnuPG, `gh attestation`.

**Spec:** `docs/superpowers/specs/2026-11-01-wimao-sous-projet-4-design.md`

## Global Constraints

- Code, scripts et commentaires en français, commentaires détaillés et utiles (un développeur qui découvre le dépôt doit s'y retrouver) ; chaque script commence par un en-tête : rôle, usage, variables, ce qu'il écrit et où.
- Scripts : `#!/usr/bin/env bash`, `set -euo pipefail`, `shellcheck` sans avertissement.
- Toute archive téléchargée a son empreinte SHA-256 écrite dans le dépôt et vérifiée avant usage ; différence = arrêt.
- Versions : `wimao-wine` = `11.20+wimao1` ; `wimao-bridge` = `5.1.1+dxvk3.1.1-1` ; `wimao` = version du workspace, passée à `0.4.0`.
- Emplacements : `/opt/wimao/wine`, `/opt/wimao/yabridge`, `/opt/wimao/dxvk`, `/usr/bin/{wimao,wimao-cli}`, `/usr/share/wimao/recettes`, `/usr/share/locale/<l>/LC_MESSAGES/wimao.mo`, `/usr/share/icons/hicolor/...`, `/usr/share/applications/fr.wimao.WiMao.desktop`, `/usr/share/metainfo/fr.wimao.WiMao.metainfo.xml`, `/usr/share/doc/<paquet>/copyright`.
- Architecture `amd64` seulement ; cibles Ubuntu 24.04+ et Debian 13+.
- Aucune DLL Microsoft dans un paquet.
- Fichiers des paquets `root:root`, dossiers 0755, fichiers 0644 (exécutables 0755), aucun `setuid`, aucun `postinst`/`preinst`/`prerm`/`postrm` écrit à la main (déclencheurs `dpkg` standard seulement).
- Reproductibilité : `SOURCE_DATE_EPOCH` = date du dernier commit (`git log -1 --format=%ct`), `dpkg-deb --root-owner-group`, dates normalisées (`touch -d @$SOURCE_DATE_EPOCH`), compression `xz` fixée.
- Workflows : `permissions: contents: read` en tête ; chaque action figée par l'empreinte complète de son commit avec la version en commentaire (`uses: actions/checkout@<sha40> # v4.x.y`) ; images figées par `@sha256:` ; aucun secret.
- Jamais toucher l'installation réelle de l'utilisateur (`~/.local/share/wimao`, `~/.wine-mao`, `~/.vst3`, `~/.config/yabridgectl`, `/opt/wine-mao-v2`) ; les tests utilisent des dossiers temporaires ; aucune installation de paquet sur la machine sauf à la tâche 11 (contrôleur, avec accord).
- Les implémenteurs ne poussent jamais : le contrôleur pousse la branche pour faire tourner la CI et transmet les journaux.
- Avant chaque commit touchant du Rust : `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`, `cargo test --workspace`, `scripts/verifier-traductions.sh`.

## Review Focus

1. Une bibliothèque que Wine charge à la demande manque dans `Depends` → Wine démarre mais un plugin ne s'affiche pas : test d'installation (tâche 7) lance un vrai `wineboot` + installeur NSIS factice + `wimao-cli verifier` dans le conteneur vierge.
2. Un fichier du paquet inscriptible par l'utilisateur ou un chemin de développement figé dans un binaire installé → test de permissions et `strings` sur les binaires à la recherche de `/home/` et `CARGO_MANIFEST_DIR` (tâche 7).
3. `apt remove` qui touche aux données de l'utilisateur → test explicite (tâche 7).
4. Une action GitHub non figée ou un workflow avec des droits d'écriture superflus → vérification automatique `paquets/verifier-workflows.sh` (tâche 2).
5. Une version signée dont le contenu diffère de ce que le code produit → `signer-version.sh` refabrique et compare avant de signer (tâche 9).

---

### Task 1 : retouches du code pour l'installation système

**Files:** Modify `crates/wimao/src/main.rs`, `Cargo.toml` (version `0.4.0`), `Cargo.lock` ; Create `crates/wimao/data/fr.wimao.WiMao.metainfo.xml` ; Modify `crates/wimao/data/fr.wimao.WiMao.desktop` si `desktop-file-validate` le demande ; Test : dans `main.rs` (`#[cfg(test)]`).

**Interfaces — Produces:** `fn dossier_icones_developpement(existe: impl Fn(&Path) -> bool) -> Option<&'static Path>` (retourne `Some(ICONES_DEVELOPPEMENT)` seulement s'il existe) ; `ajouter_icones_developpement` ne l'ajoute que dans ce cas.

- [ ] Test `icones_de_developpement_seulement_si_le_dossier_existe` : `existe = |_| false` → `None` ; `existe = |p| p == Path::new(ICONES_DEVELOPPEMENT)` → `Some`.
- [ ] Vérifier qu'il échoue, implémenter, vérifier qu'il passe.
- [ ] Chercher tout autre `env!("CARGO_MANIFEST_DIR")` utilisé hors `#[cfg(test)]` (`grep`) : chacun doit avoir un repli quand le chemin n'existe pas (aujourd'hui `catalogue.rs` a déjà `/usr/share/wimao/recettes` en premier) ; documenter dans le rapport.
- [ ] Fiche AppStream : `id` `fr.wimao.WiMao`, `metadata_license` `CC0-1.0`, `project_license` `GPL-3.0-or-later`, nom, résumé et description en en (par défaut) + `xml:lang="fr"` + `xml:lang="es"`, `launchable` du `.desktop`, `url homepage` = dépôt, `screenshots` pointant vers les images `docs/captures/*.png` du dépôt (adresse `https://raw.githubusercontent.com/F1ghtc0re/WiMao/main/docs/captures/…`), `content_rating oars-1.1`, `releases` avec `0.4.0`. `appstreamcli validate --no-net` sans erreur ; `desktop-file-validate` sans erreur.
- [ ] Version du workspace `0.4.0` (le dialogue « À propos » et `wimao-cli --version` la lisent déjà via `CARGO_PKG_VERSION`).
- [ ] Vérifications globales, commit `WiMao : icônes de développement facultatives, fiche AppStream, version 0.4.0`.

### Task 2 : base de sécurité du dépôt

**Files:** Create `deny.toml`, `.github/dependabot.yml`, `.github/CODEOWNERS`, `SECURITY.md`, `paquets/verifier-workflows.sh` ; Modify `.github/workflows/ci.yml`.

- [ ] `.github/CODEOWNERS` : `* @F1ghtc0re`.
- [ ] `deny.toml` : sources autorisées crates.io seulement (`unknown-registry = "deny"`, `unknown-git = "deny"`) ; licences compatibles GPL-3.0-or-later (liste explicite : MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Unicode-DFS-2016, Zlib, MPL-2.0, GPL-3.0-or-later, LGPL-2.1-or-later — retirer celles qui ne servent pas) ; `advisories` : `yanked = "deny"`. `cargo deny check` passe.
- [ ] `ci.yml` : `permissions: contents: read` en tête ; nouveau job `chaine` : `cargo deny check` et `cargo audit` (outils installés à version figée avec `cargo install --locked cargo-deny@<version> cargo-audit@<version>`) ; toutes les actions figées par empreinte de commit (relever l'empreinte de l'étiquette utilisée aujourd'hui avec `git ls-remote https://github.com/<action> refs/tags/<tag>`, en déréférençant les étiquettes annotées) ; toutes les commandes cargo en `--locked`.
- [ ] `.github/dependabot.yml` : écosystèmes `cargo` et `github-actions`, hebdomadaire.
- [ ] `paquets/verifier-workflows.sh` : échoue si un `uses:` de `.github/workflows/*.yml` n'est pas suivi d'une empreinte de 40 caractères hexadécimaux, si un workflow n'a pas de bloc `permissions:` au niveau racine, ou si une image `container:`/`image:` n'est pas figée par `@sha256:`. Ajouté comme étape du job `chaine`. Tester : il passe sur le dépôt, échoue sur une copie temporaire contenant `uses: actions/checkout@v4`.
- [ ] `SECURITY.md` (fr, avec un court résumé en anglais) : signaler une faille en privé (« Report a vulnerability » de GitHub), délai de réponse visé (7 jours), versions suivies (la dernière), procédure clé perdue ou volée (révocation publiée dans le dépôt et la Release suivante, nouvelle clé, nouvelle version), comment vérifier une version (`gpg --verify SHA256SUMS.asc`, `sha256sum -c`, `gh attestation verify`).
- [ ] Commit `WiMao : base de sécurité (cargo deny, audit, actions figées, CODEOWNERS, SECURITY.md)`.

### Task 3 : paquet `wimao-bridge`

**Files:** Create `paquets/commun.sh` (fonctions partagées), `paquets/fabriquer-bridge.sh`, `paquets/bridge/control.in`, `paquets/bridge/copyright`.

**Interfaces — Produces:** `paquets/commun.sh` définit `verifier_empreinte <fichier> <sha256>`, `normaliser_arborescence <dossier>` (dates `SOURCE_DATE_EPOCH`, droits 0755/0644, exécutables conservés), `construire_deb <dossier> <sortie.deb>` (`dpkg-deb --root-owner-group -Zxz --build`), `date_source` (exporte `SOURCE_DATE_EPOCH` depuis git si absent). Chaque script : `paquets/fabriquer-X.sh <dossier de sortie>`, cache des téléchargements dans `${WIMAO_CACHE_PAQUETS:-<sortie>/cache}`.

- [ ] Reprendre versions, adresses et empreintes de `scripts/install-yabridge.sh` (yabridge 5.1.1, `d11f0307412b5566b09a75ed4aa191266db56d22f4de4d20e4fbde6296cd1a47`) et `scripts/install-dxvk.sh` (DXVK 3.1.1, `SHA256_ARCHIVE` de `crates/wimao-core/src/correctifs/dxvk.rs`) ; téléchargement par `curl -fsSL` depuis les adresses des Releases GitHub officielles (pas `gh`).
- [ ] Contenu : `/opt/wimao/yabridge/` (contenu de l'archive yabridge), `/opt/wimao/dxvk/x64/*.dll` et `/opt/wimao/dxvk/x32/*.dll` (les 4 DLL `d3d11`, `dxgi`, `d3d10core`, `d3d9` seulement, disposition attendue par `dxvk.rs`), `/usr/share/doc/wimao-bridge/copyright` (GPL-3.0 yabridge, zlib DXVK, format DEP-5).
- [ ] `control` : `Package: wimao-bridge`, `Version: 5.1.1+dxvk3.1.1-1`, `Architecture: amd64`, `Depends: wimao-wine (>= 11.20+wimao1)` + bibliothèques relevées par `dpkg-shlibdeps` sur les `.so` Linux de yabridge, `Maintainer`, `Description` (fr court + en).
- [ ] Vérifier en local : deux fabrications successives donnent le même `sha256sum` ; `dpkg-deb -c` montre `root/root` partout ; `lintian` (si installé) sans erreur. Si `lintian`/`dpkg-shlibdeps` manquent, le contrôleur installe `lintian` (apt autorisé) — demander via le rapport, ne pas contourner.
- [ ] Commit `WiMao : paquet wimao-bridge (yabridge 5.1.1, DXVK 3.1.1)`.

### Task 4 : paquet `wimao-wine`

**Files:** Create `paquets/fabriquer-wine.sh`, `paquets/wine/control.in`, `paquets/wine/copyright`, `paquets/wine/dependances.md`.

- [ ] Reprendre `scripts/build-wine-v2.sh` (archive 11.20, empreinte `eac8ca448f9c0087f963026b4dc276d1e2b7fbb64056edc093e59bbb7e56fe28`, `patches-v2/*.patch`, contrôle du correctif, mêmes options `configure` : `--enable-archs=i386,x86_64 --disable-tests --without-ffmpeg --without-gstreamer`, `CFLAGS="-O2 -g0"`), mais `--prefix=/opt/wimao/wine` et `make install DESTDIR=<racine du paquet>`. Le script liste en tête les paquets de compilation à installer (`gcc-mingw-w64`, `flex`, `bison`, en-têtes X11/FreeType/Vulkan/GnuTLS/PulseAudio…) dans une variable `PAQUETS_COMPILATION` utilisée par la CI.
- [ ] Copier le correctif dans `/usr/share/doc/wimao-wine/patches/` ; `copyright` LGPL-2.1-or-later (DEP-5).
- [ ] `Depends` : sortie de `dpkg-shlibdeps` sur les ELF de `lib/wine/x86_64-unix/` et `bin/` ; plus, en `Depends`, les bibliothèques chargées à la demande indispensables (`libfreetype6`, `libfontconfig1`, `libx11-6`, `libxext6`, `libxrender1`, `libxrandr2`, `libxi6`, `libxcursor1`, `libxcomposite1`, `libxfixes3`, `libvulkan1`, `libgnutls30`, `libpulse0`) et en `Recommends` le reste (`libgl1`, `libxinerama1`, `libxxf86vm1`, `libcups2`, `libsdl2-2.0-0`) ; justifier chaque choix dans `paquets/wine/dependances.md` (relevé : `ldd` et `strings` des `.so` du Wine de développement, liste des `dlopen` de Wine). Noms de paquets valables à la fois sur Ubuntu 24.04 et Debian 13 (vérifier les renommages `t64` : utiliser l'alternative `libfoo | libfoot64` quand le nom diffère).
- [ ] Le script ne compile **pas** en local sur la machine de l'utilisateur dans cette tâche (Ubuntu 26.04 n'est pas la cible) : tester seulement l'empaquetage avec `WIMAO_WINE_DEJA_COMPILE=<dossier>` qui saute la compilation et empaquette un Wine déjà installé dans un `DESTDIR` (essai avec une copie de `/opt/wine-mao-v2` dans un dossier temporaire). La vraie compilation est faite par la CI (tâche 6).
- [ ] Commit `WiMao : paquet wimao-wine (Wine 11.20 + correctif wm_state_serial)`.

### Task 5 : paquet `wimao`

**Files:** Modify `crates/wimao/Cargo.toml` (`[package.metadata.deb]`), Create `paquets/fabriquer-wimao.sh`, `paquets/wimao/copyright`.

- [ ] `paquets/fabriquer-wimao.sh <sortie>` : `cargo build --release --locked -p wimao -p wimao-cli` ; compile les `.mo` (`msgfmt po/fr.po`, `po/es.po`) dans un dossier de travail ; lance `cargo deb -p wimao --no-build --locked` (version de `cargo-deb` figée dans le script, installée par `cargo install --locked cargo-deb@<version>` si absente) ; normalise et vérifie la reproductibilité comme les autres.
- [ ] `[package.metadata.deb]` : `name = "wimao"`, `depends = "$auto, wimao-wine (>= 11.20+wimao1), wimao-bridge (>= 5.1.1+dxvk3.1.1-1), libadwaita-1-0 (>= 1.5), carla, xvfb, 7zip | p7zip-full, cabextract, zstd, tar"`, `section = "sound"`, `assets` : les deux binaires vers `usr/bin/`, `recettes/*.toml` vers `usr/share/wimao/recettes/`, les `.mo` vers `usr/share/locale/{fr,es}/LC_MESSAGES/wimao.mo`, les icônes `data/icons/hicolor/**` vers `usr/share/icons/hicolor/`, le `.desktop` et la fiche AppStream, `copyright` (GPL-3.0-or-later). Vérifier le nom réel du domaine gettext et des icônes dans `i18n.rs` et `data/icons`.
- [ ] Vérifier en local : `dpkg-deb -c` liste exactement ces chemins ; `strings` sur les binaires du paquet ne contient ni `/home/` ni le chemin du dépôt (sinon corriger : le seul chemin de développement autorisé est `ICONES_DEVELOPPEMENT` et le repli des recettes, qui ne servent que s'ils existent — les noter dans le rapport) ; reproductibilité.
- [ ] Commit `WiMao : paquet wimao (application, CLI, recettes, traductions, icônes)`.

### Task 6 : workflow `paquets.yml` (fabrication)

**Files:** Create `.github/workflows/paquets.yml`.

- [ ] Déclencheurs : `pull_request`, `push` sur `main`, `push` d'étiquettes `v*`, `workflow_dispatch` avec entrée booléenne `a_blanc` (défaut `true`).
- [ ] `permissions: contents: read` à la racine.
- [ ] Job `wine` : `container: ubuntu:24.04@sha256:<empreinte>` ; installe `PAQUETS_COMPILATION` ; `actions/cache` (clé : `wine-11.20-` + `hashFiles('patches-v2/**', 'paquets/fabriquer-wine.sh', 'paquets/wine/**', 'paquets/commun.sh')`) sur le `DESTDIR` compilé ; lance `paquets/fabriquer-wine.sh` ; `timeout-minutes: 300` ; artefact `wimao-wine`.
- [ ] Job `bridge` et job `wimao` : même conteneur ; `wimao` installe Rust à version figée (`rust-toolchain.toml` ajouté au dépôt, version = celle de la CI actuelle) et les paquets de compilation GTK ; artefacts.
- [ ] Job `reproductible` : refabrique `wimao-bridge` et `wimao` dans un second conteneur et compare `sha256sum` avec les artefacts ; échoue si différent. Sur une étiquette `v*`, refabrique aussi `wimao-wine` sans cache et compare.
- [ ] Job `sommes` : télécharge les trois artefacts, produit `SHA256SUMS`, artefact `paquets-<sha court>` (les trois `.deb` + `SHA256SUMS`).
- [ ] `paquets/verifier-workflows.sh` passe sur ce workflow.
- [ ] Commit `WiMao : fabrication des paquets dans la CI`. Le contrôleur pousse la branche et rapporte le résultat des jobs ; corriger jusqu'à ce que tout soit vert (une correction = un commit).

### Task 7 : tests d'installation

**Files:** Create `paquets/tester-installation.sh`, `paquets/lintian.md` ; Modify `.github/workflows/paquets.yml` (job `installation`, matrice).

- [ ] `paquets/tester-installation.sh <dossier des .deb> <dossier des fixtures>` (lancé en root dans un conteneur vierge) :
  1. `apt-get install -y ./wimao-wine_*.deb ./wimao-bridge_*.deb ./wimao_*.deb` (seuls les dépôts de la distribution) ;
  2. `lintian` sur les trois paquets : aucune erreur (avertissements acceptés listés et justifiés dans `paquets/lintian.md`, passés avec `--suppress-tags-from-file`) ;
  3. permissions : `dpkg -L` de chaque paquet → aucun fichier inscriptible par groupe/autres, aucun `setuid`/`setgid` ; aucun script de mainteneur hors déclencheurs (`/var/lib/dpkg/info/wimao*.{preinst,postinst,prerm,postrm}` absents) ;
  4. `strings /usr/bin/wimao /usr/bin/wimao-cli | grep -E '/home/|/github/workspace'` ne renvoie rien d'autre que les chemins documentés au rapport de la tâche 5 ;
  5. utilisateur `essai` créé ; sous lui (HOME neuf) : `wimao-cli init`, `wimao-cli installer <fixtures>/test-nsis.exe --oui`, présence de `~/.vst3/yabridge/WiMaoTest.vst3`, `wimao-cli diagnostiquer` sans problème bloquant ;
  6. interface : `xvfb-run -a -s '-screen 0 1280x900x24'` lance `wimao` sous `dbus-run-session`, attend 10 s, vérifie que le processus vit et qu'une fenêtre « WiMao » existe (`xdotool search --name WiMao`), capture `import -window root` en artefact, puis arrêt propre ;
  7. `apt-get remove -y wimao wimao-bridge wimao-wine` : `/opt/wimao` et `/usr/bin/wimao*` disparus, `~essai/.local/share/wimao` et `~essai/.vst3/yabridge` intacts.
- [ ] Job `installation` : matrice `ubuntu:24.04@sha256:…` et `debian:13@sha256:…`, dépend de `sommes` ; fixtures fabriquées par `tests/fabriquer-fixtures.sh` (NSIS seulement suffit : ajouter si besoin une option `--seulement-nsis` au script, testée) avec le Wine du paquet (`WIMAO_WINE_DIR=/opt/wimao/wine`) ; artefacts : captures et journaux.
- [ ] Commit `WiMao : tests d'installation des paquets (Ubuntu 24.04, Debian 13)`. Le contrôleur pousse et rapporte ; itérer jusqu'au vert.

### Task 8 : brouillon de Release sur étiquette

**Files:** Modify `.github/workflows/paquets.yml` (job `brouillon`).

- [ ] Job `brouillon` : seulement si `github.ref` commence par `refs/tags/v` **ou** `workflow_dispatch` avec `a_blanc == false` ; dépend de `installation` et `reproductible` ; `permissions: contents: write, id-token: write, attestations: write` **sur ce job seulement** ; `actions/attest-build-provenance` sur chaque `.deb` ; crée une Release **brouillon** (`gh release create --draft --verify-tag`) avec les `.deb`, `SHA256SUMS` et les fichiers d'attestation. En mode `a_blanc` (défaut), le job affiche ce qu'il ferait et ne crée rien.
- [ ] Vérifier par `workflow_dispatch` avec `a_blanc=true` (le contrôleur le lance) : aucun brouillon créé, journal explicite.
- [ ] Commit `WiMao : brouillon de Release signé par le propriétaire (étape CI)`.

### Task 9 : `scripts/signer-version.sh`

**Files:** Create `scripts/signer-version.sh`, `scripts/tests/tester-signer-version.sh`.

- [ ] Usage : `scripts/signer-version.sh vX.Y.Z [--a-blanc] [--dossier <dossier des artefacts>]`. Étapes : télécharger les fichiers du brouillon (`gh release download`) — ou prendre `--dossier` ; `sha256sum -c SHA256SUMS` ; `gh attestation verify <deb> --repo F1ghtc0re/WiMao` pour chaque `.deb` (sauté en `--a-blanc` avec un message) ; refabriquer `wimao` et `wimao-bridge` depuis l'étiquette dans un dossier temporaire (`git worktree add`) et comparer les empreintes ; signer : `gpg --armor --detach-sign --output SHA256SUMS.asc SHA256SUMS` (la phrase de passe est demandée par GnuPG, jamais lue par le script) ; joindre (`gh release upload`) sauf en `--a-blanc`. Toute différence = arrêt avec un message clair en français.
- [ ] `scripts/tests/tester-signer-version.sh` : crée un `GNUPGHOME` temporaire et une clé jetable sans phrase de passe, des faux artefacts dont la refabrication est simulée (variable `SIGNER_REFABRIQUER=<commande>` pour les tests), et vérifie : signature valide (`gpg --verify`) ; arrêt si `SHA256SUMS` ne correspond pas ; arrêt si l'empreinte refabriquée diffère ; aucun appel réseau en `--a-blanc`. Ajouté au job `chaine` de `ci.yml`.
- [ ] Commit `WiMao : script de signature des versions (mode à blanc testé)`.

### Task 10 : documentation

**Files:** Modify `README.md`, `ARCHITECTURE.md`, `CONTRIBUTING.md`, `docs/ameliorations-reportees.md` ; Create `paquets/README.md`, `docs/publier-une-version.md`.

- [ ] `README.md` : section « Installation » (télécharger les trois `.deb` d'une Release, vérifier `SHA256SUMS.asc` avec la clé publiée et `sha256sum -c`, `sudo apt install ./wimao*.deb`), cibles et limites (amd64, Ubuntu 24.04+, Debian 13+).
- [ ] `paquets/README.md` : rôle de chaque script, fabrication locale, reproductibilité, tests.
- [ ] `docs/publier-une-version.md` : la procédure du § 8 de la spec pas à pas, plus la création de la clé (clé principale + sous-clé de signature, sauvegarde hors ligne, publication de la clé publique dans `docs/cle-publique.asc`), révocation.
- [ ] `ARCHITECTURE.md`, `CONTRIBUTING.md` : nouveaux dossiers et vérifications (`chaine`, `paquets.yml`), règle « PR + validation du propriétaire ».
- [ ] `docs/ameliorations-reportees.md` : section « Après le sous-projet 4 » (points reportés des relectures ; AUR, Flatpak, dépôt APT signé, PPA).
- [ ] Commit `WiMao : documentation des paquets et de la publication`.

### Task 11 : vérification réelle et réglages du dépôt (contrôleur)

- [ ] Télécharger les artefacts de la CI ; avec l'accord de l'utilisateur, `sudo apt install ./wimao*.deb` sur sa machine ; vérifier que WiMao utilise `/opt/wimao/wine` (journal, `wimao-cli diagnostiquer`), ouvrir un vrai plugin dans Bitwig (méthode Xvfb déjà éprouvée) ; montrer que `apt remove` rend la main à `/opt/wine-mao-v2` (sans le faire si l'utilisateur préfère garder les paquets).
- [ ] Guider l'utilisateur pour la double authentification GitHub et la signature de ses commits (clé SSH de signature, `git config gpg.format ssh`, `user.signingkey`, `commit.gpgsign true`).
- [ ] Poser la règle de `main` avec `gh api` (PR obligatoire ; vérifications `ci` et `paquets` obligatoires ; 1 validation + validation des propriétaires de code ; réécriture et suppression interdites ; commits signés exigés **seulement une fois la signature des commits configurée**) avec l'administrateur du dépôt en contournement « pull request seulement » (sinon le propriétaire, seul, ne pourrait jamais valider sa propre PR) ; activer « Require approval for all outside collaborators » et les alertes Dependabot. Montrer le résultat à l'utilisateur.
- [ ] Section « WiMao sous-projet 4 — vérification réelle » dans `NOTES.md`. Commit.
