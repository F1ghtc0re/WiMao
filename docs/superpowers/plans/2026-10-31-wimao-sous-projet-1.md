# WiMao, sous-projet 1 (moteur + interface minimale) : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** construire le moteur de WiMao (environnement Wine, correctifs, installation des installeurs
classiques, pontage, test des plugins, restauration, réparation, sauvegarde, trois langues), utilisable
par une commande `wimao-cli` et par une fenêtre GTK4 minimale.

**Architecture :** espace de travail Cargo à quatre crates. `wimao-core` (moteur, sans interface),
`wimao-recettes` (recettes et installeurs, dépend de core), `wimao-cli` et `wimao` (GTK4/libadwaita),
qui ne font qu'appeler les deux bibliothèques. Les outils externes (Wine, yabridgectl, carla-single,
Xvfb, tar, zstd, cabextract, 7z) sont appelés par `std::process::Command`.

**Tech Stack :** Rust stable (cargo 1.99), `thiserror`, `serde` + `toml`, `sha2`, `ureq`, `regex`,
`glob`, `gettext-rs`, `tempfile` (tests), `gtk4` + `libadwaita` (crates compatibles avec les
bibliothèques d'Ubuntu 26.04), gettext 0.23 (`xgettext`, `msgfmt`).

**Spec :** `docs/superpowers/specs/2026-10-31-wimao-design.md`

## Global Constraints

- **Qualité du code (exigence de l'utilisateur) : code propre, commenté, facile à corriger et à
  maintenir, commentaires détaillés et utiles.** Concrètement :
  - chaque fichier commence par un commentaire `//!` qui dit son rôle, ce qu'il utilise et qui l'utilise ;
  - chaque élément public a un commentaire `///` : ce qu'il fait, ses paramètres, les erreurs possibles,
    et un exemple quand l'usage n'est pas évident ;
  - les commentaires expliquent le **pourquoi** : chaque contournement de Wine, chaque valeur fixe
    (délai, option silencieuse, empreinte) cite son origine (`NOTES.md`, bug Wine, documentation) ;
  - fonctions courtes (moins de 60 lignes environ), une seule responsabilité, noms explicites en
    français, aucune valeur « magique » sans constante nommée ;
  - pas de `unwrap()` ni de `expect()` hors des tests : toute erreur remonte dans `Erreur` avec un
    message qui aide à corriger ;
  - `cargo fmt`, `cargo clippy -- -D warnings` et `cargo doc --no-deps` sans avertissement à chaque
    tâche ; `#![warn(missing_docs)]` dans les bibliothèques.
- **Lisibilité pour n'importe quel développeur (exigence de l'utilisateur)** : quelqu'un qui découvre
  le dépôt doit pouvoir compiler, tester, trouver le bon fichier et corriger un défaut sans aide.
  Documents tenus à jour **dans la même tâche** que le code qu'ils décrivent :
  - `README.md` : ce qu'est WiMao, état du projet, compilation et lancement en 5 commandes ;
  - `ARCHITECTURE.md` : schéma des crates, rôle de chaque fichier (une ligne chacun), parcours
    d'une installation fichier par fichier, emplacements sur disque, outils externes appelés et pourquoi ;
  - `CONTRIBUTING.md` : préparer sa machine, lancer chaque niveau de tests, conventions (français,
    commentaires, messages de commit), et trois guides pas à pas : **ajouter une recette**, **ajouter
    une langue**, **ajouter une règle d'erreur** ;
  - `docs/recettes.md` : référence complète du format des recettes, chaque clé avec un exemple ;
  - chaque crate a un `//!` dans `lib.rs`/`main.rs` qui résume le module et renvoie à `ARCHITECTURE.md`.
  Une tâche qui ajoute ou renomme un fichier met à jour `ARCHITECTURE.md` ; le relecteur le vérifie.
- Licence GPL-3.0-or-later dans chaque `Cargo.toml` et un fichier `LICENSE` à la racine.
- Identifiants, commentaires et messages de commit en **français**, comme le reste du dépôt.
  Chaînes d'interface : `msgid` en anglais, traduites dans `po/fr.po` et `po/es.po` ; aucune chaîne
  visible par l'utilisateur hors de `tr("…")`.
- Langues : français, anglais, espagnol. Langue du système par défaut, réglable.
- Données : `~/.local/share/wimao/` (surchargeable par `WIMAO_DONNEES`), environnement Wine dans
  `<donnees>/prefixe`, réglages `~/.config/wimao/`, cache `~/.cache/wimao/`.
- Wine : `WIMAO_WINE_DIR`, sinon `/opt/wimao/wine`, sinon `/opt/wine-mao-v2`.
  yabridge : `WIMAO_YABRIDGE_DIR`, sinon `/opt/wimao/yabridge`, sinon `/opt/wine-mao-v2/yabridge`.
- **Ne jamais toucher** `~/.wine-mao` ni `~/.config/yabridgectl` (installation actuelle de
  l'utilisateur). yabridgectl est toujours lancé avec `XDG_CONFIG_HOME=<donnees>/yabridgectl`, et
  jamais avec `--prune`.
- Pendant le développement sur la machine de l'utilisateur, toute commande qui écrit (init,
  installer, ponter) tourne avec `HOME` et `WIMAO_DONNEES` pointant vers un dossier temporaire du
  scratchpad. Seule exception : `wimao-cli verifier --prefixe ~/.wine-mao`, qui ne fait que lire.
- Wine Mono 11.3.0 : `https://dl.winehq.org/wine/wine-mono/11.3.0/wine-mono-11.3.0-x86.msi`,
  sha256 `df2dfc1665c2511882e7cabd56eafd0c0a3d94e5a7e86f969277f6c189d418d3`.
- DXVK 3.1.1 : `https://github.com/doitsujin/dxvk/releases/download/v3.1.1/dxvk-3.1.1.tar.gz`,
  sha256 `40565b4a724aadc4433fa4e010b4b23916d9b1f1baeee64e17186db94f54e608` ; DLL `d3d11 dxgi
  d3d10core d3d9` (x64 → system32, x32 → syswow64), override `native,builtin`.
- DLL Microsoft : `msvcp140` seulement. Source 1 : `ProgramData/Package Cache/**/vcRuntimeMinimum_amd64/cab1.cab`
  et `…_x86/cab1.cab` (fichiers `msvcp140.dll_amd64` et `msvcp140.dll`). Source 2 : installer en
  silencieux `https://aka.ms/vs/17/release/vc_redist.x64.exe` et `…/vc_redist.x86.exe`
  (`/install /quiet /norestart`), puis source 1. Empreintes connues (testées) :
  x64 `0f885b509a685d2bbfa652fed26b5fb31d88fbdab0a978c641d1c7b8aa460aa9`,
  x86 `83f935454ae8e450b6f042509ecf28cceff95edb2495c63a782b9d45c2eaf1c0`.
- Initialisation de Wine : `wineboot -u` avec `WINEDLLOVERRIDES="mscoree,mshtml="`, `WINEDEBUG=-all`.
- Installation silencieuse : Inno `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-` ; MSI
  `msiexec /i <f> /qn /norestart` ; WiX Burn `-quiet -norestart` ; NSIS `/S` ; InstallAware `/s`.
  Délai maximal : 30 min.
- Sauvegarde : morceaux de 3900 MiB sur FAT32 (`vfat`, `msdos`, `fat`), nom
  `wimao-AAAA-MM-JJ-HHhMM.tar.zst`, fichier `.sha256`, refus d'écraser ; exclusions
  `drive_c/users/*/AppData/Local/Temp`, `drive_c/users/*/Downloads`, `drive_c/users/*/AppData/Local/dxvk`.
- Points de restauration : 10 au maximum.

## Review Focus

1. **Plugin du même nom déjà ponté depuis `~/.wine-mao`** : WiMao ne l'écrase pas, le marque
   ⚠️ « déjà ponté depuis un autre environnement » (test dans la tâche 7).
2. **Chemins avec espaces, `!` ou accents** (`Xpand!2`, `Program Files`, `Électrique`) : installation,
   inventaire, pontage et sauvegarde fonctionnent (tests dans les tâches 6, 7 et 14).
3. **Installeur silencieux qui ne rend jamais la main** ou qui renvoie 0 sans rien installer :
   délai respecté, puis proposition d'installation normale (test dans la tâche 11).
4. **Disque de sauvegarde plein ou débranché pendant l'écriture** : erreur claire, aucun fichier
   `.sha256` écrit pour une archive incomplète (test dans la tâche 14).
5. **Plugin dont l'interface ne s'affiche pas sur l'écran virtuel (Vulkan absent sous Xvfb)** : le
   plugin chargé mais sans interface vue est ⚠️ « interface non vérifiée », pas ❌ (test dans la
   tâche 8).

---

## Structure des fichiers

```
Cargo.toml                         espace de travail
LICENSE
crates/wimao-core/src/
  lib.rs            ré-exports
  erreur.rs         Erreur (thiserror), Resultat<T>
  chemins.rs        Chemins : tous les emplacements (section Global Constraints)
  i18n.rs           init_langue(), tr()
  progression.rs    trait Progression
  outils.rs         lancer() : exécution d'une commande avec délai et journal
  telechargement.rs telecharger_verifie()
  wine.rs           Wine : commandes wine/wineboot/reg/wineserver
  environnement.rs  creer_environnement()
  correctifs/{mod,mono,dxvk,msvcp}.rs
  inventaire.rs     Inventaire, PluginWindows, Format
  pontage.rs        ponter()
  test_plugin.rs    tester_plugin(), ResultatTest
  restauration.rs   PointRestauration
  diagnostic.rs     Probleme, diagnostiquer(), reparer()
  regles.rs         RegleErreur, expliquer()  + ressources/regles.toml
  rapport.rs        anonymiser(), rapport()
  sauvegarde.rs     sauvegarder(), restaurer()
crates/wimao-recettes/src/
  lib.rs
  recette.rs        Recette (serde), charger(), valider
  catalogue.rs      Catalogue : trouver la recette d'un fichier
  detection.rs      TypeInstalleur, detecter()
  archive.rs        installer_archive()
  installation.rs   installer() : le parcours complet de la section 5 de la spec
crates/wimao-cli/src/main.rs
crates/wimao/src/{main,fenetre,travail}.rs
po/{POTFILES,wimao.pot,fr.po,es.po}
scripts/verifier-traductions.sh
tests/fixtures/{nsis,msi,inno}/…   sources des installeurs factices
tests/fabriquer-fixtures.sh
.github/workflows/ci.yml
```

---

### Task 1 : espace de travail, chemins, erreurs, intégration continue

**Files :**
- Create : `Cargo.toml`, `LICENSE`, `README.md` (remplace l'actuel s'il existe), `ARCHITECTURE.md`, `CONTRIBUTING.md`, `docs/recettes.md` (squelette complété à la tâche 9), `crates/wimao-core/{Cargo.toml,src/lib.rs,src/erreur.rs,src/chemins.rs,src/progression.rs,src/outils.rs}`, `crates/wimao-recettes/{Cargo.toml,src/lib.rs}`, `crates/wimao-cli/{Cargo.toml,src/main.rs}`, `.github/workflows/ci.yml`
- Modify : `.gitignore` (ajouter `target/`)

**Interfaces :**
- Produces :
  - `enum Erreur { Commande { programme: String, code: Option<i32>, sortie: String }, Delai { programme: String }, Telechargement(String), Empreinte { attendue: String, obtenue: String }, Recette { fichier: PathBuf, ligne: Option<usize>, message: String }, Introuvable(String), Io(#[from] std::io::Error), NonPrisEnCharge(String) }` ; `type Resultat<T> = Result<T, Erreur>`
  - `struct Chemins { donnees, prefixe, config, cache, home, wine_dir, yabridge_dir: PathBuf }` ; `Chemins::depuis_env() -> Resultat<Chemins>` ; `Chemins::pour_test(racine: &Path) -> Chemins` (tout sous `racine`, wine/yabridge résolus normalement) ; `fn wine_bin(&self) -> PathBuf` (`wine_dir/bin`) ; `fn drive_c(&self) -> PathBuf`
  - `trait Progression: Send + Sync { fn etape(&self, message: &str); }` ; `struct ProgressionMuette`
  - `fn lancer(cmd: &mut Command, delai: Duration) -> Resultat<Sortie>` avec `struct Sortie { code: i32, texte: String }` (stdout+stderr mélangés, conservés pour les règles d'erreurs). Délai dépassé : processus tué (groupe entier, `setsid`), `Erreur::Delai`.

- [ ] **Step 1 : tests de `Chemins`** dans `chemins.rs` : `depuis_env` avec `WIMAO_DONNEES=/x` donne `prefixe == /x/prefixe` ; avec `WIMAO_WINE_DIR=/w` donne `wine_bin() == /w/bin` ; sans variable et sans `/opt/wimao/wine`, `wine_dir == /opt/wine-mao-v2`. Tests de `lancer` : `sh -c 'echo a; echo b >&2; exit 3'` → `code 3`, texte contient `a` et `b` ; `sleep 30` avec délai 1 s → `Erreur::Delai` en moins de 3 s, et aucun `sleep` enfant restant (`sh -c 'sleep 30 & sleep 30'`).
- [ ] **Step 2 : lancer** `cargo test -p wimao-core` → échec de compilation.
- [ ] **Step 3 : implémenter** les types ci-dessus. `lancer` : `process_group(0)`, lecture des sorties dans un fil séparé, `libc::kill(-pgid, SIGKILL)` au délai.
- [ ] **Step 4 : lancer** `cargo test --workspace` → PASS ; `cargo clippy --workspace -- -D warnings` → aucun avertissement.
- [ ] **Step 5 : CI** : `.github/workflows/ci.yml`, job `unitaires` sur `ubuntu-24.04` : `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`. La crate `wimao` (GTK) est exclue de ce job tant qu'elle n'existe pas (`--exclude wimao` ajouté à la tâche 17).
- [ ] **Step 6 : documents de départ** : `README.md`, `ARCHITECTURE.md` (crates et fichiers créés jusqu'ici), `CONTRIBUTING.md` (préparer sa machine, `cargo test`, conventions). Vérification : en suivant seulement `README.md` dans un clone neuf (`git clone` dans le scratchpad), `cargo build` et `cargo test` réussissent.
- [ ] **Step 7 : commit** `git commit -m "WiMao : espace de travail Cargo, chemins, erreurs, CI, documentation de départ"`

### Task 2 : trois langues

**Files :**
- Create : `crates/wimao-core/src/i18n.rs`, `po/POTFILES`, `po/wimao.pot`, `po/fr.po`, `po/es.po`, `crates/wimao-core/build.rs`, `scripts/verifier-traductions.sh`
- Modify : `.github/workflows/ci.yml`

**Interfaces :**
- Produces : `fn init_langue(langue: Option<&str>)` (None = langue du système ; `"fr"`, `"en"`, `"es"`) ; `fn tr(msgid: &str) -> String` ; `fn trf(msgid: &str, args: &[(&str, &str)]) -> String` qui remplace `{nom}` par la valeur. Domaine gettext `wimao` ; dossier des traductions : `WIMAO_LOCALEDIR`, sinon `/usr/share/locale`, sinon `<OUT_DIR>/locale` compilé par `build.rs` (`msgfmt po/<l>.po -o <OUT_DIR>/locale/<l>/LC_MESSAGES/wimao.mo`).

- [ ] **Step 1 : test** `init_langue(Some("fr"))` puis `tr("Ready")` == `"Prêt"` ; `Some("es")` → `"Listo"` ; `Some("en")` → `"Ready"` ; `trf("{n} plugins found", &[("n","3")])` en fr == `"3 plugins trouvés"`. Les tests qui changent la langue tournent en série (`serial_test`).
- [ ] **Step 2 : lancer** `cargo test -p wimao-core i18n` → FAIL.
- [ ] **Step 3 : implémenter** `i18n.rs` (`gettext-rs` : `setlocale`, `bindtextdomain`, `textdomain` ; pour forcer la langue, variable `LANGUAGE`) et `build.rs`. Créer `po/fr.po` et `po/es.po` avec les deux chaînes du test.
- [ ] **Step 4 : `scripts/verifier-traductions.sh`** : régénère `po/wimao.pot` par `xgettext --language=C --keyword=tr --keyword=trf --from-code=UTF-8 -f po/POTFILES`, fait `msgmerge --update` sur fr et es, puis échoue (code 1, liste des chaînes) si `msgfmt --statistics` signale une chaîne non traduite ou approximative (fuzzy). `po/POTFILES` liste les `.rs` de toutes les crates (régénéré par le script avec `git ls-files 'crates/*.rs'`).
- [ ] **Step 5 : vérifier** `scripts/verifier-traductions.sh` → code 0 ; retirer la traduction espagnole de « Ready » → code 1 avec « Ready » dans la sortie ; remettre.
- [ ] **Step 6 : CI** : ajouter l'étape `scripts/verifier-traductions.sh` (paquet `gettext`).
- [ ] **Step 7 : commit** `"WiMao : traductions français, anglais, espagnol"`

> **À partir d'ici, chaque tâche qui ajoute une chaîne visible ajoute ses traductions fr/es dans le même commit** ; `scripts/verifier-traductions.sh` fait partie de l'étape « tests passent ».

### Task 3 : Wine et téléchargements vérifiés

**Files :**
- Create : `crates/wimao-core/src/wine.rs`, `crates/wimao-core/src/telechargement.rs`

**Interfaces :**
- Consumes : `Chemins`, `lancer`, `Resultat`.
- Produces :
  - `struct Wine<'a> { chemins: &'a Chemins }` ; `fn commande(&self, programme: &str) -> Command` (variables `WINEPREFIX`, `WINEDEBUG=-all`, `PATH` avec `wine_bin()` en tête, `HOME`) ; `fn executer(&self, args: &[&OsStr], delai: Duration) -> Resultat<Sortie>` ; `fn reg_ajouter(&self, cle: &str, valeur: &str, donnee: &str) -> Resultat<()>` (`REG_SZ`) ; `fn reg_lire(&self, cle: &str, valeur: &str) -> Resultat<Option<String>>` ; `fn reg_supprimer(&self, cle: &str, valeur: &str) -> Resultat<()>` ; `fn arreter(&self) -> Resultat<()>` (`wineserver -k`, puis `wineserver -w`).
  - `fn telecharger_verifie(urls: &[&str], sha256: Option<&str>, dest: &Path) -> Resultat<PathBuf>` : si `dest` existe avec la bonne empreinte, aucun téléchargement ; sinon essaie chaque adresse dans l'ordre, écrit dans `dest.part`, vérifie, renomme. Empreinte fausse : fichier supprimé, `Erreur::Empreinte`. `sha256: None` : aucune vérification (fichiers Microsoft, vérifiés après extraction).
  - `fn empreinte(fichier: &Path) -> Resultat<String>` (sha256 en hexadécimal minuscule).

- [ ] **Step 1 : tests** de `telecharger_verifie` avec un petit serveur HTTP local (`tiny_http` en dépendance de test) : première adresse en 404 puis seconde OK → fichier présent ; mauvaise empreinte → `Erreur::Empreinte` et ni `dest` ni `dest.part` ; second appel sans serveur avec le fichier déjà là → OK. Test de `Wine` (marqué `#[ignore = "Wine requis"]`, lancé par `cargo test -- --ignored` en local et en CI d'intégration) : dans un préfixe temporaire initialisé par `wineboot -u`, `reg_ajouter` puis `reg_lire` sur `HKCU\Software\WiMaoTest` rend la valeur ; `reg_supprimer` puis `reg_lire` → `None`.
- [ ] **Step 2 : lancer** les tests → FAIL.
- [ ] **Step 3 : implémenter.** `reg_lire` analyse la sortie de `wine reg query` (ligne `    <valeur>    REG_SZ    <donnee>`).
- [ ] **Step 4 : lancer** `cargo test -p wimao-core` puis `cargo test -p wimao-core -- --ignored wine` → PASS.
- [ ] **Step 5 : commit** `"WiMao : commandes Wine et téléchargements vérifiés"`

### Task 4 : création de l'environnement Wine et Wine Mono

**Files :**
- Create : `crates/wimao-core/src/environnement.rs`, `crates/wimao-core/src/correctifs/{mod.rs,mono.rs}`

**Interfaces :**
- Consumes : `Wine`, `telecharger_verifie`, `Progression`.
- Produces :
  - `trait Correctif { fn nom(&self) -> &'static str; fn est_applique(&self, c: &Chemins) -> Resultat<bool>; fn appliquer(&self, c: &Chemins, p: &dyn Progression) -> Resultat<()>; }`
  - `fn correctifs() -> Vec<Box<dyn Correctif>>` : ordre `Mono`, `Dxvk`, `Msvcp140` (les deux derniers ajoutés à la tâche 5).
  - `struct Mono` : appliqué si `HKLM\Software\Microsoft\NET Framework Setup\NDP\v4\Full` contient `Version` **ou** si `drive_c/windows/mono/mono-2.0` existe ; application : téléchargement dans `<cache>/wine-mono-11.3.0-x86.msi` puis `msiexec /i … /qn`.
  - `fn creer_environnement(c: &Chemins, p: &dyn Progression) -> Resultat<()>` : `wineboot -u` si `prefixe/system.reg` est absent, puis chaque correctif non appliqué. Idempotent.

- [ ] **Step 1 : tests** (`#[ignore = "Wine requis"]`) : `creer_environnement` sur `Chemins::pour_test(tmp)` crée `prefixe/system.reg` et `Mono.est_applique == true` ; un second appel ne relance ni `wineboot` ni `msiexec` (le compteur d'étapes d'une `Progression` de test ne reçoit que des messages « déjà fait »).
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.** Messages de progression : `tr("Creating the Wine environment…")`, `tr("Installing Wine Mono…")`, `tr("Already done: {name}")`.
- [ ] **Step 4 : lancer** `cargo test -p wimao-core -- --ignored environnement` → PASS (peut prendre quelques minutes).
- [ ] **Step 5 : commit** `"WiMao : création de l'environnement Wine et Wine Mono"`

### Task 5 : DXVK et DLL Microsoft

**Files :**
- Create : `crates/wimao-core/src/correctifs/{dxvk.rs,msvcp.rs}`
- Modify : `crates/wimao-core/src/correctifs/mod.rs`

**Interfaces :**
- Consumes : `Correctif`, `Wine`, `telecharger_verifie`, `empreinte`.
- Produces :
  - `struct Dxvk` : source `/opt/wimao/dxvk/{x64,x32}` si présent, sinon archive téléchargée dans `<cache>` et extraite dans `<cache>/dxvk-3.1.1`. Avant de remplacer, copie les DLL de Wine dans `<donnees>/originaux/{system32,syswow64}/` (sans écraser une copie existante). `est_applique` : les 4 overrides valent `native,builtin` et les 8 fichiers ont l'empreinte des fichiers DXVK.
  - `struct Msvcp140` : sources dans l'ordre de Global Constraints (cabextract). `est_applique` : override `native,builtin` et fichiers différents de la version Wine (comparaison avec `wine_dir/lib/wine/{x86_64,i386}-windows/msvcp140.dll`). Empreinte inconnue : appliqué quand même, message `tr("Unknown Microsoft library version, used anyway: {hash}")` via `Progression`.
  - `fn retirer(&self, c: &Chemins) -> Resultat<()>` sur les deux (remet les originaux et supprime les overrides) : utilisé par « Réparer » si le correctif est abîmé.

- [ ] **Step 1 : tests unitaires sans Wine** : `Msvcp140::trouver_cabs(drive_c)` sur une arborescence factice (`Package Cache/{A}v14.38/packages/vcRuntimeMinimum_amd64/cab1.cab` et une version `v14.40`) retient la plus récente (tri par version, comme `sort -V`). Tests `#[ignore = "Wine requis"]` : `Dxvk.appliquer` puis `est_applique == true`, puis `retirer` puis `est_applique == false` et fichiers identiques aux originaux.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** tests unitaires et `-- --ignored dxvk` → PASS. Le test `Msvcp140` avec Wine passe par la source 2 (téléchargement Microsoft) : `-- --ignored msvcp` → PASS.
- [ ] **Step 5 : commit** `"WiMao : correctifs DXVK et DLL Microsoft"`

### Task 6 : inventaire des plugins

**Files :**
- Create : `crates/wimao-core/src/inventaire.rs`

**Interfaces :**
- Produces :
  - `enum Format { Vst2, Vst3, Clap }` ; `struct PluginWindows { chemin: PathBuf, format: Format, nom: String }` (`nom` = nom de fichier sans extension).
  - `const DOSSIERS_PLUGINS: &[&str] = &["Program Files/Common Files/VST3", "Program Files/Common Files/CLAP", "Program Files/Common Files/VST2", "Program Files/Steinberg/VstPlugins", "Program Files/VstPlugins"]` (relatifs à `drive_c`) — mêmes dossiers que la configuration yabridgectl actuelle.
  - `struct Inventaire(BTreeSet<PluginWindows>)` ; `Inventaire::relever(drive_c: &Path) -> Resultat<Inventaire>` ; `fn nouveaux(&self, avant: &Inventaire) -> Vec<PluginWindows>`.
  - Règles : un dossier ou fichier `*.vst3` est un VST3 (ne pas descendre dans un bundle) ; `*.clap` un CLAP ; `*.dll` dans les dossiers VST2 est un VST2 **seulement s'il contient la chaîne d'octets `VSTPluginMain`** (ou `main` exportée — on se limite à `VSTPluginMain`).

- [ ] **Step 1 : tests** sur une arborescence temporaire : `Common Files/VST3/Serum2.vst3/Contents/x86_64-win/Serum2.vst3` compte pour **un** VST3 ; `VstPlugins/Xpand!2_x64.dll` (contenant `VSTPluginMain`) est un VST2 nommé `Xpand!2_x64` ; `VstPlugins/libfoo.dll` sans la chaîne est ignoré ; `Common Files/VST3/Électrique Bass.vst3` est trouvé ; `nouveaux` entre deux relevés rend exactement le plugin ajouté.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter** (`walkdir`).
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : inventaire des plugins Windows"`

### Task 7 : pontage yabridge

**Files :**
- Create : `crates/wimao-core/src/pontage.rs`

**Interfaces :**
- Consumes : `Chemins`, `Inventaire`, `PluginWindows`, `lancer`.
- Produces :
  - `enum EtatPontage { Ponte, Doublon, DejaPonteAilleurs(PathBuf) }`
  - `fn ponter(c: &Chemins, plugins: &Inventaire) -> Resultat<BTreeMap<PathBuf, EtatPontage>>` :
    1. écrit `<donnees>/yabridgectl/yabridgectl/config.toml` : `plugin_dirs` = les `DOSSIERS_PLUGINS` existants du préfixe WiMao, `vst2_location = 'centralized'`, `blacklist` = doublons + plugins déjà pontés ailleurs ;
    2. **doublon** : un VST2 dont le nom, après retrait du suffixe `_x64`, est égal (sans casse) au nom d'un VST3 de l'inventaire ;
    3. **déjà ponté ailleurs** : `~/.vst3/yabridge/<rel>.vst3/Contents/x86_64-win/<nom>.vst3` (ou l'équivalent `~/.vst/yabridge/<nom>.dll`, `~/.clap/yabridge/<nom>.clap`) est un lien vers un chemin **hors** du préfixe WiMao ;
    4. lance `yabridgectl sync` (stdin fermé, `XDG_CONFIG_HOME=<donnees>/yabridgectl`, `PATH` avec `wine_bin()`), délai 5 min.

- [ ] **Step 1 : tests** sans yabridgectl (la fonction de calcul de la configuration est testée seule : `fn configuration(c, plugins) -> (ConfigYabridge, BTreeMap<…>)`) : `Hype_x64.dll` + `Hype.vst3` → `Hype_x64.dll` en `Doublon` et dans `blacklist` ; un lien `~/.vst3/yabridge/Serum2.vst3/Contents/x86_64-win/Serum2.vst3 → /autre/drive_c/…` → `DejaPonteAilleurs("/autre/…")` ; `Xpand!2_x64.dll` sans VST3 → `Ponte`. Test `#[ignore = "Wine requis"]` : après `ponter`, `~/.config/yabridgectl` (du HOME de test) n'existe pas et `<home>/.vst3/yabridge/<nom>.vst3` existe.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter** (`serde` + `toml` pour la configuration).
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : pontage yabridge isolé, doublons VST2, conflits"`

### Task 8 : test d'ouverture des plugins

**Files :**
- Create : `crates/wimao-core/src/test_plugin.rs`

**Interfaces :**
- Consumes : `Chemins`, `PluginWindows`, `lancer`.
- Produces :
  - `enum ResultatTest { Pret, Remarque(String), Probleme(String) }` (textes déjà traduits).
  - `fn tester_plugin(c: &Chemins, bundle_linux: &Path, format: Format) -> ResultatTest` où `bundle_linux` est le plugin ponté (`~/.vst3/yabridge/…`).
  - Déroulé : démarre `Xvfb :<n> -screen 0 1920x1080x24` (premier `n` libre à partir de 90 ; si `Xvfb` est absent, utilise `DISPLAY` courant) ; lance `carla-single vst3|vst <bundle>` en nouveau groupe de processus avec `DISPLAY=:<n>`, journal dans un fichier temporaire ; toutes les 3 s pendant 300 s : fenêtre dont le titre se termine par `(GUI)` dans `xwininfo -root -tree` → attend 10 s → `Pret` ; journal contenant `aborting|stack overflow|Unhandled exception` → `Probleme(<première ligne>)` ; processus terminé → `Probleme(tr("The plugin host stopped"))`. Délai atteint et processus vivant → `Remarque(tr("Loaded, but the interface could not be checked"))`. Toujours : groupe tué, Xvfb arrêté.
  - CLAP : `Remarque(tr("CLAP plugins are not tested yet"))`, sans lancer Carla.
  - `carla-single` absent : `Remarque(tr("Install Carla to test plugins"))`.

- [ ] **Step 1 : tests** avec un faux `carla-single` (script shell placé en tête de `PATH` par le test) et un faux `xwininfo` : script qui écrit `Unhandled exception` puis attend → `Probleme` contenant `Unhandled exception` ; script qui se termine aussitôt → `Probleme` « hôte arrêté » ; script vivant et `xwininfo` sans fenêtre, délai réduit à 2 s par `tester_plugin_avec_delai` (fonction interne `pub(crate)`) → `Remarque` « interface non vérifiée » ; `xwininfo` qui renvoie une ligne `0x1 "Serum 2 (GUI)": ()` → `Pret`. Après chaque test, plus aucun processus du faux Carla (`pgrep -f faux-carla` vide — motif lancé dans un processus séparé).
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** → PASS. Installer les outils de l'écran virtuel : `sudo apt-get install -y xvfb` (autorisé sans mot de passe), puis vérification réelle avec Serum 2 de l'installation actuelle (lecture seule, préfixe `~/.wine-mao`, plugin `~/.vst3/yabridge/Serum2.vst3`) : résultat `Pret` ou `Remarque` « interface non vérifiée » ; noter lequel dans `NOTES.md`.
- [ ] **Step 5 : commit** `"WiMao : test d'ouverture des plugins sur écran virtuel"`

### Task 9 : format des recettes et catalogue

**Files :**
- Create : `crates/wimao-recettes/src/{recette.rs,catalogue.rs}`, `crates/wimao-recettes/tests/recettes/{valide.toml,cle-inconnue.toml,methode-invalide.toml}`

**Interfaces :**
- Produces :
  - `struct Recette { nom, editeur: String, telecharger: Option<String>, type_: TypeRecette (renommé "type"), reconnaitre: Vec<String>, installation: Installation, correctifs: Correctifs, apres_installation: ApresInstallation, consignes: BTreeMap<String, BTreeMap<String, String>> }`, toutes les structures en `#[serde(deny_unknown_fields, rename_all = "kebab-case")]`, champs facultatifs par défaut.
  - `enum TypeRecette { Gestionnaire, Installeur, Archive }` ; `enum Methode { Extraire7z, Inno, Msi, Wix, Nsis, Installaware, Auto }` (défaut `Auto`) ; `Installation { methode, lien_connexion: Option<String> }` ; `Correctifs { lancement: Vec<String>, registre: Vec<EntreeRegistre { cle, valeur, donnee }>, services: Vec<String> }` ; `ApresInstallation { pilote: bool }`.
  - `fn charger(fichier: &Path) -> Resultat<Recette>` : erreur `Erreur::Recette { fichier, ligne, message }` avec la ligne tirée de l'erreur `toml` ; validation supplémentaire : `reconnaitre` non vide, chaque consigne a `fr`, `en` et `es`.
  - `struct Catalogue(Vec<Recette>)` ; `Catalogue::charger(dossiers: &[PathBuf]) -> (Catalogue, Vec<Erreur>)` (une recette invalide n'empêche pas les autres ; le premier dossier l'emporte à nom égal) ; `fn trouver(&self, fichier: &Path) -> Option<&Recette>` (motifs `glob` sans casse sur le nom du fichier). Dossiers : `<donnees>/recettes`, puis `WIMAO_RECETTES` ou `/usr/share/wimao/recettes`, sinon `recettes/` du dépôt (développement).
  - Variables dans `donnee` : `{telechargements}` → `C:\users\<user>\Downloads`.

- [ ] **Step 1 : tests** : `valide.toml` = exemple de la section 4 de la spec, chargé sans erreur, `reconnaitre == ["Native Access*.exe"]` ; `cle-inconnue.toml` (clé `telechargerr`) → `Erreur::Recette` avec `ligne == Some(3)` et le nom de la clé dans le message ; `methode-invalide.toml` → erreur ; consigne sans `es` → erreur ; `trouver("native access installer.exe")` trouve la recette ; dossier contenant une recette valide et une invalide → 1 recette, 1 erreur.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : format des recettes et catalogue"`

### Task 10 : reconnaissance des installeurs et archives

**Files :**
- Create : `crates/wimao-recettes/src/{detection.rs,archive.rs}`, `crates/wimao-recettes/tests/echantillons/` (débuts de fichiers réels, voir Step 1)

**Interfaces :**
- Produces :
  - `enum TypeInstalleur { Inno, Msi, WixBurn, Nsis, InstallAware, Archive, Inconnu }` ; `fn detecter(fichier: &Path) -> Resultat<TypeInstalleur>`.
  - Règles, dans l'ordre : en-tête OLE `D0 CF 11 E0 A1 B1 1A E1` → `Msi` ; en-tête `PK\x03\x04` ou extension `.zip`/`.7z`/`.rar` → `Archive` ; puis recherche d'octets dans le fichier (lecture en flux, sans tout charger) : `Inno Setup` → `Inno` ; `.wixburn` → `WixBurn` ; `Nullsoft` → `Nsis` ; `InstallAware` → `InstallAware` ; sinon `Inconnu`.
  - `fn arguments_silencieux(t: TypeInstalleur, fichier: &Path) -> Option<Vec<OsString>>` (valeurs de Global Constraints ; `Msi` rend `["msiexec", "/i", fichier, "/qn", "/norestart"]`).
  - `fn installer_archive(c: &Chemins, archive: &Path, p: &dyn Progression) -> Resultat<ContenuArchive>` : extrait avec `7z x` dans un dossier temporaire ; `enum ContenuArchive { Installeur(PathBuf), PluginsCopies(Vec<PathBuf>), Vide }` : un seul `.exe`/`.msi` reconnu → `Installeur` (l'appelant l'installe) ; sinon copie les `*.vst3` dans `Common Files/VST3`, `*.clap` dans `Common Files/CLAP`, les `*.dll` contenant `VSTPluginMain` dans `Common Files/VST2`.

- [ ] **Step 1 : échantillons** : copier les 4 premiers Mo de vrais installeurs déjà présents dans `~/.wine-mao/drive_c/users/<utilisateur>/Downloads/` (un Inno de `inMusic/`, un WiX Burn `MiniGrand_1.4.0_Setup.exe`) avec `head -c 4M` ; vérifier avant commit qu'ils ne contiennent ni nom d'utilisateur ni e-mail (`grep -a -c <utilisateur>` → 0). Pour MSI et NSIS, les échantillons sont produits à la tâche 16 ; ici, fichiers synthétiques (en-tête OLE ; octets `Nullsoft`).
- [ ] **Step 2 : tests** `detecter` sur chaque échantillon ; un `.zip` contenant `Foo.vst3/` et `Bar.dll` (avec `VSTPluginMain`) et `readme.txt` → `PluginsCopies` avec 2 chemins aux bons endroits ; un `.zip` contenant `Setup.exe` (échantillon Inno) → `Installeur`.
- [ ] **Step 3 : lancer** → FAIL.
- [ ] **Step 4 : implémenter.**
- [ ] **Step 5 : lancer** → PASS.
- [ ] **Step 6 : commit** `"WiMao : reconnaissance des installeurs et archives"`

### Task 11 : points de restauration

**Files :**
- Create : `crates/wimao-core/src/restauration.rs`

**Interfaces :**
- Produces :
  - `struct PointRestauration { id: String /* AAAA-MM-JJ-HHhMMmSS */, motif: String, dossier: PathBuf }`
  - `fn creer(c: &Chemins, motif: &str) -> Resultat<PointRestauration>` : `Wine::arreter`, copie de `system.reg`, `user.reg`, `userdef.reg`, et `fichiers.txt` = chemins relatifs de tous les fichiers de `prefixe/` (triés, un par ligne, octets bruts du nom) ; puis supprime les points au-delà des 10 plus récents.
  - `fn lister(c: &Chemins) -> Resultat<Vec<PointRestauration>>` (plus récent d'abord) ; `fn revenir(c: &Chemins, point: &PointRestauration) -> Resultat<usize>` : `Wine::arreter`, remet les `.reg`, supprime les fichiers absents de `fichiers.txt` puis les dossiers devenus vides ; rend le nombre de fichiers supprimés.

- [ ] **Step 1 : tests** sur un faux préfixe (fichiers `.reg` texte, `wineserver` remplacé par un script vide via `WIMAO_WINE_DIR` de test) : créer un point, ajouter `drive_c/Program Files/X !/a.dll`, modifier `user.reg`, `revenir` → `a.dll` et `X !/` supprimés, `user.reg` identique à l'original, rend `1` ; créer 12 points → `lister` en rend 10.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : points de restauration"`

### Task 12 : parcours d'installation

**Files :**
- Create : `crates/wimao-recettes/src/installation.rs`

**Interfaces :**
- Consumes : `Catalogue::trouver`, `detecter`, `arguments_silencieux`, `installer_archive`, `restauration::creer`, `Inventaire`, `ponter`, `tester_plugin`, `Wine`, `creer_environnement`.
- Produces :
  - `struct RapportInstallation { point: PointRestauration, recette: Option<String>, methode: TypeInstalleur, plugins: Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>, interactif_necessaire: bool }`
  - `fn installer(c: &Chemins, fichier: &Path, catalogue: &Catalogue, options: OptionsInstallation, p: &dyn Progression) -> Resultat<RapportInstallation>` avec `struct OptionsInstallation { interactif: bool, tester: bool }`.
  - Déroulé (section 5 de la spec) : `creer_environnement` **seulement si `prefixe/system.reg` est absent** (un environnement existant mais incomplet relève de « Réparer ») ; recette (si trouvée : sa `methode`, sauf `Auto`) sinon `detecter` ; `restauration::creer(c, "install <nom du fichier>")` ; `Inventaire::relever` ; installation (silencieuse, ou sans arguments si `options.interactif`) avec délai 30 min ; correctifs de la recette (`registre`) ; nouveau relevé ; si **aucun plugin nouveau** après une installation silencieuse (code 0 ou non) → `interactif_necessaire = true` et pas d'erreur (l'interface propose « Ouvrir l'installeur normalement ») ; `Erreur::Delai` → même chose ; puis `ponter` sur l'inventaire complet et `tester_plugin` pour chaque nouveau plugin ponté (si `options.tester`).
  - Recette avec `pilote = true` ou `lien-connexion` : `Erreur::NonPrisEnCharge(tr("Publisher managers arrive in a later version"))` (sous-projet 2). `methode = Extraire7z` : idem dans ce sous-projet.
  - Installeur `Inconnu` : lancé sans arguments (mode interactif).

- [ ] **Step 1 : tests** avec un faux `wine` (script dans un `WIMAO_WINE_DIR` de test qui, selon les arguments reçus, crée un `.vst3` dans le préfixe, ou ne fait rien, ou dort 10 s) et un délai réduit (`installer_avec_delai`, `pub(crate)`), `tester: false`, faux `yabridgectl` : installeur Inno factice → le faux `wine` a reçu `/VERYSILENT` et le rapport contient 1 plugin `Ponte` ; faux `wine` qui ne crée rien → `interactif_necessaire == true` ; faux `wine` qui dort au-delà du délai → `interactif_necessaire == true` et plus aucun processus du faux `wine` ; recette avec `pilote = true` → `Erreur::NonPrisEnCharge` **avant** toute création de point de restauration.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : parcours complet d'installation d'un plugin"`

### Task 13 : règles d'erreurs, diagnostic et réparation

**Files :**
- Create : `crates/wimao-core/src/{regles.rs,diagnostic.rs}`, `crates/wimao-core/ressources/regles.toml`

**Interfaces :**
- Produces :
  - `regles.toml` : `[[regle]] motif = "<regex>"`, `message = "<msgid anglais>"`, `reparation = "<Probleme>"` (facultatif). Embarqué par `include_str!`. Pour que `xgettext` trouve les messages, chacun est aussi écrit dans `regles.rs` avec la macro `tr_noop!("…")` (rend la chaîne telle quelle ; ajoutée comme mot-clé `--keyword=tr_noop` dans `scripts/verifier-traductions.sh`), et un test vérifie que chaque `message` de `regles.toml` y figure.
  - Règles initiales (depuis `NOTES.md`) : `(?i)(msvcp140|vcruntime140|msvcp140_1)\.dll.*not found` → `Msvcp140Absent` ; `(?i)mscoree|\.NET Framework` → `MonoAbsent` ; `(?i)d3d11.*(failed|error)|DXGI_ERROR` → `DxvkAbsent` ; `0x80070643` → message « installeur WiX en échec, souvent un composant .NET manquant » → `MonoAbsent`.
  - `fn expliquer(texte: &str) -> Option<(String /* traduit */, Option<Probleme>)>`
  - `enum Probleme { EnvironnementAbsent, MonoAbsent, DxvkAbsent, Msvcp140Absent, PontageIncomplet(usize), WineBloque }`
  - `fn diagnostiquer(c: &Chemins) -> Resultat<Vec<Probleme>>` : correctifs non appliqués ; `PontageIncomplet(n)` = plugins de l'inventaire ni pontés ni en `blacklist` ; `WineBloque` = `wineserver` du préfixe actif alors qu'aucun `yabridge-host.exe` ni installation WiMao (fichier verrou `<donnees>/installation.lock`) n'est en cours.
  - `fn reparer(c: &Chemins, problemes: &[Probleme], p: &dyn Progression) -> Resultat<()>` : point de restauration (motif « réparation »), puis pour chaque problème : correctif `retirer` + `appliquer`, `ponter`, ou `Wine::arreter`.
  - `fn message_probleme(p: &Probleme) -> String` (traduit, pour l'interface).

- [ ] **Step 1 : tests** : `expliquer("err:module:import_dll Library MSVCP140.dll (needed by …) not found")` → message traduit + `Msvcp140Absent` ; texte sans motif → `None` ; chaque `message` de `regles.toml` est présent dans `regles.rs` ; `diagnostiquer` sur un faux préfixe où un VST3 n'a pas de bundle ponté → contient `PontageIncomplet(1)`.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.** Le verrou `installation.lock` est posé et retiré par `installer` (modifier `installation.rs`).
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : messages d'erreur compréhensibles, diagnostic et réparation"`

### Task 14 : journal, rapport anonymisé, sauvegarde et restauration

**Files :**
- Create : `crates/wimao-core/src/{rapport.rs,sauvegarde.rs}`

**Interfaces :**
- Produces :
  - Journal : `<donnees>/journal.log`, ajouté par `lancer` (commande + code + 200 dernières lignes de sortie), limité à 5 Mo (au-delà, renommé `journal.1.log`).
  - `fn anonymiser(texte: &str, utilisateur: &str, home: &Path, machine: &str) -> String` : `home` → `~` ; nom d'utilisateur (mot entier, sans casse) → `<user>` ; e-mails (`[\w.+-]+@[\w-]+\.[\w.]+`) → `<e-mail>` ; nom de la machine → `<machine>`.
  - `fn rapport(c: &Chemins) -> Resultat<String>` : version WiMao, version Wine (`wine --version`), distribution (`/etc/os-release` : `PRETTY_NAME`), `diagnostiquer`, 200 dernières lignes du journal ; le tout passé par `anonymiser`.
  - `fn sauvegarder(c: &Chemins, destination: &Path, p: &dyn Progression) -> Resultat<PathBuf>` : comportement de `scripts/sauvegarde-prefixe.sh` (Global Constraints), sauf : refus si un processus `yabridge-host.exe` tourne (au lieu de tester Bitwig seul), dossier `WiMao-sauvegarde`, nom `wimao-AAAA-MM-JJ-HHhMM.tar.zst`, archive de `prefixe/` sous le nom `prefixe`. Pipeline : `tar` → `zstd -T0 -3` → découpage et empreintes **en Rust** (lecture de la sortie de `zstd`, écriture des morceaux `.part000…`, `sha2` calculé à la volée). Le fichier `.sha256` (format `sha256sum`) est écrit **en dernier**, uniquement si tout a réussi ; en cas d'erreur, les morceaux déjà écrits sont supprimés.
  - `fn restaurer(c: &Chemins, archive_sha256: &Path, p: &dyn Progression) -> Resultat<()>` : vérifie les empreintes ; extrait dans `<donnees>/prefixe.restauration` ; puis renomme `prefixe` en `prefixe.ancien-<date>` et `prefixe.restauration` en `prefixe`.

- [ ] **Step 1 : tests** : `anonymiser("/home/<utilisateur>/x <utilisateur>@a.fr <utilisateur> <utilisateur>x", "<utilisateur>", "/home/<utilisateur>", "pc")` == `"~/x <e-mail> <user> <utilisateur>x"` ; sauvegarde d'un faux préfixe contenant `drive_c/Program Files/Xpand!2/é.dll` et `drive_c/users/u/Downloads/gros.bin` vers un dossier temporaire avec une taille de morceau réduite à 1 Mo (`sauvegarder_avec_taille`, `pub(crate)`, forcée) → plusieurs `.part`, `sha256sum -c` → OK, `gros.bin` absent de l'archive ; second appel dans la même minute → erreur « existe déjà » ; restauration → contenu identique (`é.dll` présent) et `prefixe.ancien-*` présent ; disque plein ou débranché : `sauvegarder_avec_taille` reçoit aussi une fabrique de fichiers de sortie (`pub(crate)`, paramètre `ouvrir: impl FnMut(&Path) -> io::Result<Box<dyn Write>>`) ; la fabrique de test rend une erreur `StorageFull` à l'ouverture du deuxième morceau → `sauvegarder` rend une erreur, aucun `.sha256`, aucun `.part` restant, plus aucun processus `tar`/`zstd`.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.**
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : journal, rapport anonymisé, sauvegarde et restauration"`

### Task 15 : commande `wimao-cli`

**Files :**
- Modify : `crates/wimao-cli/src/main.rs`

**Interfaces :**
- Consumes : toutes les fonctions publiques ci-dessus.
- Produces : sous-commandes (`clap`, textes d'aide traduits) :
  `init` · `installer <fichier> [--interactif] [--sans-test]` · `detecter <fichier>` · `verifier [--prefixe <dossier>]` (teste chaque plugin déjà ponté de ce préfixe, n'écrit rien, imprime un tableau `nom | format | ✅/⚠️/❌ | détail`) · `diagnostiquer` · `reparer` · `restauration lister|revenir <id>` · `sauvegarder <destination>` · `restaurer <fichier.sha256>` · `rapport` · option globale `--langue fr|en|es`. Code de sortie : 0 succès, 1 erreur (message traduit et, si une règle s'applique, son explication), 2 au moins un plugin ❌ (`verifier`, `installer`).

- [ ] **Step 1 : tests** (`assert_cmd`) : `wimao-cli --langue fr detecter ../wimao-recettes/tests/echantillons/inno.bin` affiche `Inno Setup` ; `wimao-cli --langue es rapport` avec `WIMAO_DONNEES` temporaire ne contient pas le nom de l'utilisateur courant ; `installer fichier-absent.exe` → code 1 et message traduit.
- [ ] **Step 2 : lancer** → FAIL.
- [ ] **Step 3 : implémenter.** `verifier --prefixe` construit un `Chemins` dont seul `prefixe` change ; il retrouve les bundles pontés en suivant les liens de `~/.vst3/yabridge` et `~/.vst/yabridge` qui pointent dans ce préfixe.
- [ ] **Step 4 : lancer** → PASS.
- [ ] **Step 5 : commit** `"WiMao : commande wimao-cli"`

### Task 16 : installeurs factices et tests d'intégration avec Wine

**Files :**
- Create : `tests/fixtures/plugin/WiMaoTest.vst3/Contents/x86_64-win/WiMaoTest.vst3` (fichier factice), `tests/fixtures/nsis/test.nsi`, `tests/fixtures/msi/test.wxs`, `tests/fixtures/inno/test.iss`, `tests/fabriquer-fixtures.sh`, `crates/wimao-recettes/tests/integration.rs`
- Modify : `.github/workflows/ci.yml`

**Interfaces :**
- `tests/fabriquer-fixtures.sh <sortie>` produit `test-nsis.exe` (`makensis`), `test.msi` (`wixl`, paquet `msitools`), `test-inno.exe` (Inno Setup 6.4.3 installé en silencieux dans un préfixe Wine temporaire depuis `https://files.jrsoftware.org/is/6/innosetup-6.4.3.exe`, puis `ISCC.exe`) ; chacun installe `WiMaoTest.vst3` dans `C:\Program Files\Common Files\VST3`.

- [ ] **Step 1 : installer les outils** : `sudo apt-get install -y nsis msitools` (autorisé sans mot de passe) ; lancer `tests/fabriquer-fixtures.sh $SCRATCH/fixtures` → trois fichiers ; `wimao-cli detecter` sur chacun → `Nsis`, `Msi`, `Inno`. Remplacer les échantillons synthétiques MSI/NSIS de la tâche 10 par les 64 premiers Ko de ces fichiers.
- [ ] **Step 2 : test d'intégration** `#[ignore = "Wine requis"]` `installer_les_trois_types` : pour chaque fixture, `installer(…, OptionsInstallation { interactif: false, tester: false })` dans un `Chemins::pour_test` neuf → 1 plugin `WiMaoTest` `Ponte`, `<home>/.vst3/yabridge/WiMaoTest.vst3` existe ; puis `restauration::revenir` → plus de `WiMaoTest.vst3` dans le préfixe.
- [ ] **Step 3 : lancer** `WIMAO_FIXTURES=$SCRATCH/fixtures cargo test -p wimao-recettes -- --ignored integration` → PASS (Wine v2 local).
- [ ] **Step 4 : CI** : job `integration` sur `ubuntu-24.04` : `apt-get install wine64 wine32:i386 nsis msitools xvfb` (dépôt i386 activé), yabridge 5.1.1 téléchargé depuis sa publication GitHub, `WIMAO_WINE_DIR=/usr` (Wine d'Ubuntu, adapté par un lien `bin/`), fixtures, puis `cargo test --workspace -- --ignored`.
- [ ] **Step 5 : vérifier** le job sur GitHub : `gh run watch` → succès des deux jobs.
- [ ] **Step 6 : commit** `"WiMao : installeurs factices et tests d'intégration"`

### Task 17 : interface minimale GTK4

**Files :**
- Create : `crates/wimao/{Cargo.toml,src/main.rs,src/fenetre.rs,src/travail.rs}`, `crates/wimao/data/fr.wimao.WiMao.desktop`
- Modify : `Cargo.toml`, `.github/workflows/ci.yml`

**Interfaces :**
- Consumes : `Chemins::depuis_env`, `init_langue`, `Inventaire`, `installer`, `diagnostiquer`, `reparer`, `sauvegarder`, `expliquer`, `rapport`.
- Produces :
  - Identifiant d'application `fr.wimao.WiMao`.
  - `travail.rs` : `fn en_arriere_plan<T: Send + 'static>(f: impl FnOnce(&dyn Progression) -> T + Send + 'static, fin: impl FnOnce(T) + 'static)` : exécute `f` dans un fil, transmet les messages de progression et le résultat au fil GTK par `async-channel` + `glib::spawn_future_local`.
  - `fenetre.rs` : `adw::ApplicationWindow` 900×640 ; `adw::HeaderBar` avec les boutons `tr("Add an installer")` (sélecteur de fichier `.exe .msi .zip .7z`), `tr("Repair")`, `tr("Back up")` (sélecteur de dossier), menu « Copier le rapport » et « Langue » (Système, Français, English, Español ; choix enregistré dans `~/.config/wimao/reglages.toml`, clé `langue`, appliqué au redémarrage de la fenêtre) ; contenu : `adw::StatusPage` « Aucun plugin » si l'inventaire est vide, sinon `gtk::ListBox` de `adw::ActionRow` (titre = nom, sous-titre = format, suffixe = ✅/⚠️/❌ et détail) ; bandeau `adw::Banner` affichant le message de progression pendant un travail, boutons désactivés pendant ce temps.
  - Fin d'installation avec `interactif_necessaire` : `adw::AlertDialog` `tr("The silent installation did not add a plugin. Open the installer normally?")` → relance avec `interactif: true`. Erreur : `adw::AlertDialog` avec l'explication de `expliquer` et, si un `Probleme` est associé, un bouton `tr("Repair")`.
  - Les résultats de test sont gardés dans `<donnees>/etat.toml` (`[plugins."<chemin>"] resultat = "pret|remarque|probleme"`, `detail`) pour l'affichage au démarrage.

- [ ] **Step 1 : dépendances système** : `sudo apt-get install -y libgtk-4-dev libadwaita-1-dev` (autorisé sans mot de passe) ; `pkg-config --modversion gtk4 libadwaita-1` affiche deux versions.
- [ ] **Step 2 : test** de `travail.rs` (`gtk::test_synced` ou boucle `glib::MainContext` sans fenêtre) : une tâche qui envoie 3 messages de progression puis rend 42 → la fermeture `fin` reçoit 42 et les 3 messages arrivent dans l'ordre. Test de `etat.toml` : écriture puis relecture identiques.
- [ ] **Step 3 : lancer** → FAIL.
- [ ] **Step 4 : implémenter.**
- [ ] **Step 5 : lancer** `cargo test -p wimao` → PASS ; puis démarrage réel avec `HOME` et `WIMAO_DONNEES` de test : `cargo run -p wimao` → capture de la fenêtre (`gnome-screenshot -w` ou `xwd` si XWayland) relue par l'agent : barre, trois boutons et page « Aucun plugin » visibles, en français ; idem `LANGUAGE=es` (textes en espagnol). Installer `test-inno.exe` par la fenêtre n'est pas automatisable sous Wayland : le faire par `wimao-cli installer` dans le même `WIMAO_DONNEES`, relancer la fenêtre, et vérifier sur la capture la ligne `WiMaoTest  VST3`.
- [ ] **Step 6 : CI** : ajouter `libgtk-4-dev libadwaita-1-dev` au job `unitaires` et retirer `--exclude wimao`.
- [ ] **Step 7 : commit** `"WiMao : interface minimale GTK4"`

### Task 18 : vérification réelle sur la machine de l'utilisateur (niveau 3)

**Files :**
- Modify : `NOTES.md`

- [ ] **Step 1 : lancer** `wimao-cli --langue fr verifier --prefixe ~/.wine-mao > $SCRATCH/verification.txt` (lecture seule ; Bitwig fermé, vérifier avec `pgrep -x BitwigStudio` lancé seul).
- [ ] **Step 2 : comparer** avec les résultats connus de `NOTES.md` : tous les plugins listés comme OK sont ✅ ou ⚠️ « interface non vérifiée » ; DrumSynth est ❌ (plantage de Carla, connu). Tout autre ❌ est un défaut de WiMao à corriger avant de clore la tâche (`superpowers:systematic-debugging`).
- [ ] **Step 3 : tester un vrai installeur WiX Burn** dans un environnement de test : `HOME=$SCRATCH/h WIMAO_DONNEES=$SCRATCH/d wimao-cli installer ~/.wine-mao/drive_c/users/<utilisateur>/Downloads/<…>/MiniGrand_1.4.0_Setup.exe --sans-test` → l'installeur est reconnu `WixBurn` et lancé avec `-quiet` (le résultat peut être ⚠️ si le contenu exige l'inMusic Software Center : le noter).
- [ ] **Step 4 : consigner** le tableau et les constats dans `NOTES.md` (section « WiMao sous-projet 1 »), sans e-mail ni nom d'utilisateur.
- [ ] **Step 5 : commit** `"WiMao : vérification réelle du sous-projet 1"`
