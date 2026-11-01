# WiMao — sous-projet 4 : paquets `.deb` et sécurité de la chaîne de fabrication

Date : 2026-11-01 · Branche : `sous-projet-4` · Spec d'ensemble : `2026-10-31-wimao-design.md` (§ 1, 2, 3.1)

## 1. Objectif

Livrer WiMao sous forme de paquets `.deb` installables sur Ubuntu et Debian, fabriqués et testés
automatiquement, **sans qu'aucune version ne puisse sortir sans l'accord du propriétaire du dépôt**,
et sans qu'une modification malveillante (code, dépendance, outil de fabrication) puisse passer
inaperçue.

Critère de réussite (spec d'ensemble § 1) : sur une Ubuntu neuve, un utilisateur installe les
paquets, ajoute l'installeur d'un plugin, et le plugin apparaît et s'ouvre dans son logiciel de
musique, sans jamais ouvrir de terminal.

## 2. Périmètre

Dans ce sous-projet :

- les trois paquets `wimao-wine`, `wimao-bridge`, `wimao` (§ 3) ;
- les retouches du code nécessaires à une installation système (§ 4) ;
- la fabrication automatique dans la CI et les tests d'installation dans des systèmes vierges (§ 5, 6) ;
- les protections de la chaîne de fabrication et de publication (§ 7) ;
- la procédure de version signée, écrite et outillée, mais **exécutée seulement quand le dépôt
  sera public et sur décision du propriétaire** (§ 8).

Hors périmètre (plus tard) : AUR, Flatpak/Flathub, Snap, PPA Launchpad, retrait des données
personnelles et passage du dépôt en public (fait juste avant la première publication, avec accord).

Cibles : **Ubuntu 24.04 et plus récent, Debian 13 et plus récent**, processeurs **x86_64
(`amd64`) uniquement**. Raison : l'interface exige libadwaita ≥ 1.5 (Ubuntu 24.04 a 1.5,
Debian 13 a 1.7).

## 3. Les trois paquets

| Paquet | Version | Contenu | Emplacement |
|---|---|---|---|
| `wimao-wine` | `11.20+wimao1` | Wine 11.20 + correctif `patches-v2/` (annulation de la garde `wm_state_serial`), mode WoW64 | `/opt/wimao/wine` |
| `wimao-bridge` | `5.1.1+dxvk3.1.1-1` | yabridge 5.1.1 (binaires officiels) et DXVK 3.1.1 (DLL officielles x64 et x32) | `/opt/wimao/yabridge`, `/opt/wimao/dxvk` |
| `wimao` | version du workspace (`0.4.0` à la sortie de ce sous-projet) | `wimao` (interface), `wimao-cli`, recettes, traductions `.mo`, icônes, lanceur `.desktop`, fiche AppStream | `/usr/bin`, `/usr/share/wimao/recettes`, `/usr/share/locale`, `/usr/share/icons/hicolor`, `/usr/share/applications`, `/usr/share/metainfo` |

Les emplacements `/opt/wimao/wine`, `/opt/wimao/yabridge`, `/opt/wimao/dxvk`,
`/usr/share/wimao/recettes` et `/usr/share/locale` sont **déjà** ceux que le code cherche en
premier (`chemins.rs`, `dxvk.rs`, `catalogue.rs`, `i18n.rs`) ; le chemin de développement
(`/opt/wine-mao-v2`) reste utilisé quand le paquet est absent.

Dépendances :

- `wimao` dépend de `wimao-wine (>= 11.20+wimao1)`, `wimao-bridge`, `libgtk-4-1`,
  `libadwaita-1-0 (>= 1.5)`, `carla` (test d'ouverture, `carla-single`), `xvfb`, `7zip` ou
  `p7zip-full`, `cabextract`, `zstd`, `tar` ; les bibliothèques liées sont calculées par
  `dpkg-shlibdeps`.
- `wimao-wine` : dépendances calculées par `dpkg-shlibdeps` sur ses binaires ; les bibliothèques
  que Wine charge à la demande (FreeType, fontconfig, Vulkan, GnuTLS, PulseAudio, X11) en
  `Depends` quand elles sont indispensables, sinon en `Recommends`. La liste exacte est relevée
  pendant la fabrication et figée dans le fichier de contrôle.
- `wimao-bridge` dépend de `wimao-wine`.
- Aucun paquet ne contient de DLL Microsoft (spec d'ensemble § 3.1).

Les licences sont installées dans `/usr/share/doc/<paquet>/copyright` : GPL-3.0-or-later (WiMao),
LGPL-2.1-or-later (Wine, avec le correctif fourni en source dans
`/usr/share/doc/wimao-wine/patches/`), GPL-3.0 (yabridge), zlib (DXVK).

## 4. Retouches du code

- **Icône** : en paquet, le thème trouve l'icône dans `hicolor` ; le chemin de développement
  (`CARGO_MANIFEST_DIR`) n'est ajouté que s'il existe (aujourd'hui il est toujours ajouté). Même
  règle pour tout autre chemin calculé à la compilation et utilisé hors des tests.
- **Fiche AppStream** `fr.wimao.WiMao.metainfo.xml` : nom, résumé et description en fr/en/es,
  captures (les images de `docs/captures/`), licence, adresse du dépôt ; validée par
  `appstreamcli validate --no-net` dans la CI.
- **Lanceur** `fr.wimao.WiMao.desktop` : vérifié par `desktop-file-validate`.
- **Version affichée** : la fenêtre « À propos » et `wimao-cli --version` affichent la version du
  paquet.
- **Recettes en lecture seule** : installées `root:root`, mode 0644 ; le code n'écrit jamais dans
  `/usr/share/wimao`.

## 5. Fabrication (CI)

Nouveau fichier `.github/workflows/paquets.yml`, séparé de `ci.yml` :

- déclenché à chaque PR et sur `main` (fabrication + tests), et manuellement ;
- **`wimao-wine`** compilé à partir de l'archive officielle de Wine (empreinte SHA-256 vérifiée,
  comme `scripts/build-wine-v2.sh`) avec `patches-v2/`, **dans un conteneur `ubuntu:24.04`** (la
  plus ancienne cible : le paquet fonctionne ainsi aussi sur les plus récentes). Le résultat est
  mis en cache par clé `version de Wine + empreinte des correctifs + empreinte du script` : la
  compilation (environ une heure) n'est refaite que si l'un d'eux change ;
- **`wimao-bridge`** construit à partir des archives officielles de yabridge et DXVK, empreintes
  vérifiées (celles de `scripts/install-yabridge.sh` et `scripts/install-dxvk.sh`) ;
- **`wimao`** construit avec `cargo-deb` (`cargo build --release --locked`) ;
- les scripts de fabrication sont dans `paquets/` (`fabriquer-wine.sh`, `fabriquer-bridge.sh`,
  fichiers de contrôle), lançables aussi en local ; très commentés ;
- **fabrication reproductible** : `SOURCE_DATE_EPOCH` fixé à la date du dernier commit, dates et
  propriétaires des fichiers normalisés, ordre des fichiers trié ; un job refabrique `wimao` et
  `wimao-bridge` une seconde fois et compare les empreintes (Wine, trop long, est contrôlé au moment
  d'une version, § 8) ;
- les `.deb` et le fichier `SHA256SUMS` sont conservés comme « artefacts » de la CI (visibles
  seulement des personnes qui ont accès au dépôt).

## 6. Tests d'installation

Dans la CI, pour `ubuntu:24.04` et `debian:13` (conteneurs vierges) :

1. `apt install ./wimao-wine_*.deb ./wimao-bridge_*.deb ./wimao_*.deb` réussit, sans autre dépôt ;
2. `lintian` ne signale aucune erreur (avertissements listés et justifiés dans `paquets/lintian.md`) ;
3. un utilisateur non root : `wimao-cli init`, installation du faux installeur NSIS des tests,
   vérification que le plugin factice est ponté dans `~/.vst3/yabridge` ;
4. l'interface démarre sous Xvfb (fenêtre visible, aucune erreur au journal) ; capture conservée en
   artefact ;
5. `apt remove` retire les fichiers système et laisse les données de l'utilisateur
   (`~/.local/share/wimao`, `~/.vst3/yabridge`) intactes ;
6. aucun fichier du paquet n'est inscriptible par un utilisateur, aucun programme `setuid`, aucun
   script d'installation (`postinst`…) hors des déclencheurs standard (icônes, menus).

Puis vérification réelle par le contrôleur sur la machine de l'utilisateur (avec son accord au
moment de l'installation, qui demande `sudo`) : installation des trois paquets, WiMao utilise
alors `/opt/wimao/wine` au lieu de `/opt/wine-mao-v2`, ouverture d'un vrai plugin dans Bitwig,
puis retour possible au Wine de développement par `apt remove`.

## 7. Sécurité de la chaîne

Principe : **rien n'entre sans être vérifié, rien ne sort sans le propriétaire.**

### 7.1 Le dépôt GitHub

- Double authentification obligatoire sur le compte du propriétaire (action de l'utilisateur,
  guidée).
- Règles sur `main` (« ruleset ») : pas d'envoi direct, PR obligatoire, vérifications CI
  obligatoires (`ci.yml` et `paquets.yml`), **validation du propriétaire obligatoire** (fichier
  `.github/CODEOWNERS` qui le désigne pour tout le dépôt), commits signés exigés, réécriture
  d'historique et suppression interdites.
- Les PR de comptes extérieurs ne lancent aucun workflow sans approbation (« Require approval for
  all outside collaborators »).
- Seuls les collaborateurs invités par le propriétaire peuvent écrire ; une fois public, tout le
  monde peut lire et proposer, personne d'autre ne décide.
- Dependabot : alertes de sécurité et mises à jour des actions GitHub, soumises aux mêmes règles
  (PR + validation).

### 7.2 Ce qui entre

- Toute archive téléchargée par la fabrication a une empreinte SHA-256 écrite dans le dépôt ;
  différence = arrêt de la fabrication.
- Rust : `Cargo.lock` versionné, toutes les commandes en `--locked`. `cargo deny` (fichier
  `deny.toml`) refuse les sources hors crates.io, les licences incompatibles avec la GPL et les
  crates signalées ; `cargo audit` refuse les failles connues. Les deux sont des vérifications
  obligatoires.
- Actions GitHub : chacune **figée par l'empreinte complète de son commit** (pas par une étiquette
  modifiable), avec la version en commentaire ; images Docker figées par empreinte (`@sha256:`).
- Permissions des workflows réduites à `contents: read` ; seul le job de publication (§ 8) obtient
  les droits d'écriture, et seulement sur une étiquette de version.
- Aucun secret n'est nécessaire à la fabrication ni aux tests.

### 7.3 Ce qui sort

- La CI **ne publie jamais seule** : elle fabrique, teste et conserve des artefacts.
- La signature se fait **sur l'ordinateur du propriétaire**, avec une clé OpenPGP créée sur cette
  machine, protégée par une phrase de passe, jamais envoyée sur GitHub ni ailleurs (sauvegarde sur
  support hors ligne conseillée). Une sous-clé de signature est utilisée ; la clé principale peut
  rester hors ligne.
- Chaque version publiée comprend : les trois `.deb`, `SHA256SUMS`, `SHA256SUMS.asc` (signature),
  la clé publique, et l'**attestation de provenance** GitHub de chaque `.deb` (preuve du commit et
  du workflow qui l'ont fabriqué), vérifiable par `gh attestation verify`.
- Plus tard, dépôt APT : son fichier `InRelease` est signé avec la même clé ; APT refuse alors
  tout paquet qui ne correspond pas.
- `SECURITY.md` : comment signaler une faille en privé (signalement privé GitHub), délais, et
  procédure si la clé est perdue ou volée (révocation publiée, nouvelle clé, nouvelle version).

### 7.4 Les paquets

Pas de téléchargement à l'installation, pas de `setuid`, fichiers `root:root` non inscriptibles par
les utilisateurs, pas de script d'installation au-delà des déclencheurs standard. Au moment de
l'exécution, WiMao conserve ses règles actuelles : téléchargements vérifiés par empreinte quand la
recette en fournit une.

## 8. Procédure de version (outillée, exécutée plus tard)

1. Le propriétaire fusionne la PR de version (numéro de version, notes de version).
2. Il crée l'étiquette signée `vX.Y.Z` ; le workflow `paquets.yml` refabrique les trois paquets
   depuis cette étiquette, contrôle la reproductibilité (y compris Wine), produit `SHA256SUMS` et
   les attestations, et crée un brouillon de « Release » GitHub (non publié).
3. Sur sa machine, `scripts/signer-version.sh vX.Y.Z` :
   - télécharge les artefacts du brouillon ;
   - vérifie les attestations et les empreintes ;
   - refabrique `wimao` et `wimao-bridge` en local et compare les empreintes (preuve que la CI n'a
     rien ajouté) ;
   - signe `SHA256SUMS` (la phrase de passe est demandée par GnuPG, jamais par le script) ;
   - joint la signature au brouillon.
4. Le propriétaire relit le brouillon et le publie lui-même.

Les étapes 2 à 4 n'ont lieu qu'après le passage du dépôt en public. Dans ce sous-projet, le script
et le workflow sont écrits et testés en mode « à blanc » : le workflow est lancé à la main avec
l'option `a_blanc`, qui fabrique tout sans créer de brouillon de Release, et le script signe avec
une clé jetable sans rien envoyer.

## 9. Actions de l'utilisateur

Guidées pas à pas, sans jamais transmettre de mot de passe : activer la double authentification
GitHub ; créer la clé de signature (`gpg --full-generate-key`, puis sous-clé) et la sauvegarder ;
activer les règles du dépôt (ou autoriser le contrôleur à les poser avec `gh`, en lui montrant le
résultat) ; valider les PR ; plus tard, lancer `signer-version.sh` et publier.

## 10. Tests du code

- Unitaires : choix des chemins d'icônes selon ce qui existe ; version affichée.
- CI : `appstreamcli validate`, `desktop-file-validate`, `lintian`, `cargo deny`, `cargo audit`,
  reproductibilité, tests d'installation du § 6.
- `scripts/signer-version.sh` : test en mode « à blanc » avec une clé jetable créée dans un
  dossier temporaire (`GNUPGHOME` temporaire), jamais la clé du propriétaire.

## 11. Risques connus

- **Durée de compilation de Wine** dans la CI (environ une heure sur les machines gratuites de
  GitHub) : atténuée par le cache ; la limite de 6 h par job laisse de la marge.
- **Dépendances de Wine chargées à la demande** : une bibliothèque oubliée ne se voit qu'à
  l'exécution ; atténué par le test du § 6.3 (vrai Wine dans un système vierge) et une liste
  relevée sur le Wine de développement.
- **Carla absent ou différent selon la distribution** : vérifié par le test d'installation ; si
  `carla` manquait sur une cible, le test d'ouverture serait désactivé avec un message clair, sans
  bloquer l'installation.
- **Coexistence avec le Wine de développement** sur la machine de l'utilisateur : le paquet est
  prioritaire ; `apt remove` rend la main à `/opt/wine-mao-v2`. L'environnement de l'utilisateur
  (créé avec Wine 11.20 v2) est compatible : même version, même correctif.
- **Sécurité** : aucune mesure n'est absolue. Le point le plus sensible reste le compte GitHub du
  propriétaire et sa clé de signature ; d'où la double authentification, la clé hors de GitHub et
  la publication manuelle.
