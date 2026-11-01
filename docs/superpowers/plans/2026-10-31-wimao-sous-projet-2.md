# WiMao sous-projet 2 — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** installer, ouvrir et piloter les gestionnaires d'éditeurs (Native Access, Arturia
Software Center, inMusic Software Center, Plugin Alliance) depuis WiMao, avec relais des liens de
connexion, pontage au fil de l'eau et six recettes fournies.

**Architecture :** le moteur gagne des briques génériques pilotées par les recettes TOML :
- dans `wimao-core` : `processus.rs` (lecture de `/proc`) et `liens.rs` (déclaration de liens) ;
- dans `wimao-recettes` : `chemins_windows.rs`, `obtention.rs`, `extraction.rs`, `pilote.rs` et
  `gestionnaire.rs`.

La CLI et l'interface GTK ne font qu'appeler ces briques. Aucun code propre à un éditeur : tout
ce qui est particulier à un éditeur est dans `recettes/*.toml`.

**Tech Stack :** Rust (workspace existant), GTK4 4.22 / libadwaita 1.9, `7z`, Wine (WoW64),
yabridge 5.1.1, gettext (`.po`).

**Spec :** `docs/superpowers/specs/2026-10-31-wimao-sous-projet-2-design.md` (et la conception
d'ensemble `docs/superpowers/specs/2026-10-31-wimao-design.md`).

## Global Constraints

- Code et commentaires en **français**, détaillés et utiles, pour que n'importe quel développeur
  s'y retrouve. Les identifiants sont en français, comme dans le code existant.
- Textes affichés : `msgid` **anglais** via `tr`/`trf`/`trn` (`wimao_core::i18n`), traduits en
  `fr` et `es` dans `po/`. Les fichiers sources sont ajoutés à `po/POTFILES`, et
  `scripts/verifier-traductions.sh` doit passer. Aucun texte libre en français dans les erreurs :
  ajouter des variantes traduites à `Erreur`.
- Les tests n'écrivent **jamais** hors d'un dossier temporaire (`Chemins::pour_test`, avec
  HOME/XDG de test). Ne jamais toucher à :
  - `~/.wine-mao`, `~/.config/yabridgectl`, `~/.vst3/yabridge` et `~/.vst/yabridge` ;
  - `/opt/wine-mao-v2` ;
  - `~/.local/share/applications` et `~/.config/mimeapps.list` réels.
- Les tests qui exigent Wine portent `#[ignore = "Wine requis"]` (lancés en CI par `--ignored`).
- Avant chaque commit, ces trois commandes doivent passer :
  - `cargo fmt --check` ;
  - `cargo clippy --workspace --all-targets -- -D warnings` ;
  - `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`.
- Pas de nouvelle crate externe : `libc`, `serde`, `toml`, `ureq`, `glob` et les autres crates
  déjà présentes suffisent. Une exception doit être justifiée dans le rapport de la tâche.
- Commits terminés par la ligne `Co-Authored-By:` du modèle qui les écrit.
- Mettre à jour `ARCHITECTURE.md`, et `docs/recettes.md` ou `CONTRIBUTING.md` si concernés, dans
  la tâche qui change le comportement décrit.
- Sous-projet 1, Ruling 9 : on refuse de créer un point de restauration si un plugin tourne
  (`Erreur::PluginsEnCours`).

## Review Focus

1. **La recette ne correspond plus au gestionnaire installé** (l'éditeur a changé le nom d'un
   dossier) : `programme` introuvable → erreur traduite qui cite le motif, ni panique ni session
   ouverte à vide. Tests : tâche 3 `motif_sans_correspondance_est_introuvable` et tâche 9
   `ouvrir_un_gestionnaire_absent_est_introuvable`.
2. **Le pilote ne doit jamais toucher un processus d'un autre environnement Wine**, par exemple
   l'inMusic Software Center ouvert dans `~/.wine-mao`. Test : tâche 1
   `processus_d_un_autre_prefixe_sont_ignores`.
3. **Un gestionnaire de lien `native-access://` existe déjà** (installation actuelle) : on ne le
   remplace jamais sans accord. Test : tâche 6
   `lien_gere_par_un_autre_programme_est_signale_sans_etre_remplace`.
4. **Plugin en cours d'écriture** (grosse bibliothèque Kontakt téléchargée pendant plusieurs
   minutes) : il n'est pas ponté tant que sa signature change. Test : tâche 9
   `plugin_dont_la_signature_change_n_est_pas_ponte`.
5. **Installeur de type inconnu dans un dossier piloté** : il n'est pas tué. Test : tâche 8
   `installeur_inconnu_n_est_pas_arrete`.

---

### Task 1 : `processus.rs` — processus d'un environnement Wine

**Files :**
- Create : `crates/wimao-core/src/processus.rs`
- Modify : `crates/wimao-core/src/lib.rs`, `crates/wimao-core/src/restauration.rs` (les fonctions
  `processus_sur_prefixe`, `processus_sur`, `plugins_en_cours`, `wineserver_actif` utilisent le
  nouveau module), `po/POTFILES` si des textes sont ajoutés.

**Interfaces :**
- Produces :
  ```rust
  pub struct ProcessusWine { pub pid: u32, pub arguments: Vec<String> } // cmdline découpée sur NUL, UTF-8 avec remplacement
  impl ProcessusWine {
      pub fn ligne(&self) -> String;                 // arguments joints par une espace
      pub fn nom_programme(&self) -> Option<String>; // nom de fichier (après le dernier \ ou /) du 1er argument finissant par .exe (casse ignorée)
  }
  pub fn processus_du_prefixe(c: &Chemins) -> Resultat<Vec<ProcessusWine>>;
  pub(crate) fn processus_du_prefixe_dans(proc: &Path, prefixe: &Path) -> Resultat<Vec<ProcessusWine>>;
  pub fn arreter_processus(pids: &[u32], delai: Duration) -> Resultat<()>; // SIGTERM, attend jusqu'à `delai` (vérifie /proc/<pid> toutes les 100 ms), puis SIGKILL aux survivants ; un pid déjà disparu n'est pas une erreur
  pub fn programmes_ouverts(c: &Chemins) -> Resultat<Vec<String>>; // nom_programme des processus du préfixe, sans doublon, triés, hors processus système de Wine
  pub const PROGRAMMES_SYSTEME_WINE: &[&str] = &["services.exe", "winedevice.exe", "plugplay.exe", "explorer.exe", "rpcss.exe", "svchost.exe", "conhost.exe", "start.exe", "wineboot.exe", "tabtip.exe"];
  ```
  Le filtre sur le préfixe compare `WINEPREFIX` (dans `environ`) à `prefixe`, exactement comme
  `processus_sur` aujourd'hui.

- [ ] **Step 1 : tests sur un faux `/proc`** (dossiers `<pid>/cmdline` et `<pid>/environ` créés
  dans un `tempfile`) :
  - `processus_du_prefixe_lit_les_arguments` : `cmdline` = `"C:\\X\\Setup.exe\0/S\0"`, environ
    avec `WINEPREFIX=<p>` → un `ProcessusWine` dont les arguments sont
    `["C:\\X\\Setup.exe", "/S"]` et `nom_programme() == Some("Setup.exe")` ;
  - `processus_d_un_autre_prefixe_sont_ignores` : même `cmdline` avec `WINEPREFIX=/autre` → liste
    vide ;
  - `entrees_illisibles_ou_non_numeriques_sont_ignorees` : `/proc/self`, fichier sans `environ`
    → ignorés sans erreur ;
  - `programmes_ouverts_exclut_le_systeme_de_wine` (via une variante `_dans`) : `services.exe` et
    `Native Access.exe` → `["Native Access.exe"]` ;
  - `arreter_processus_termine_un_enfant` : lancer `sleep 30` (`std::process::Command`), l'arrêter
    avec un délai de 2 s → `/proc/<pid>` disparaît (récolter le zombie avec `wait`) ;
  - les tests existants de `restauration.rs` sur `plugins_en_cours` doivent continuer de passer.
- [ ] **Step 2 :** lancer `cargo test -p wimao-core processus` → échec à la compilation.
- [ ] **Step 3 :** implémenter, puis faire passer `restauration.rs` par
  `processus_du_prefixe_dans`. Supprimer le code de lecture de `/proc` en double.
- [ ] **Step 4 :** `cargo test -p wimao-core` → tout passe.
- [ ] **Step 5 :** mettre à jour ARCHITECTURE.md, puis commit « WiMao : processus d'un
  environnement Wine (processus.rs) ».

### Task 2 : format des recettes du sous-projet 2

**Files :**
- Modify : `crates/wimao-recettes/src/recette.rs`, `crates/wimao-recettes/src/installation.rs`
  (adapter seulement ce qui ne compile plus ; le comportement change à la tâche 7) ;
  `crates/wimao-recettes/tests/recettes/valide.toml` et les autres recettes de test ;
  `docs/recettes.md` ; `CONTRIBUTING.md` (paragraphe « pas encore pris en charge »).
- Modify : `crates/wimao-core/src/erreur.rs` si une variante est nécessaire. La validation passe
  de préférence par `Erreur::Recette { message }` traduit.

**Interfaces :**
- Produces (dans `recette.rs`, tous `Deserialize` avec `deny_unknown_fields` et
  `rename_all = "kebab-case"`, comme l'existant) :
  ```rust
  pub enum Methode { Extraire7z, Squirrel, Inno, Msi, Wix, Nsis, Installaware, Auto } // "extraire-7z", "squirrel", …
  pub struct EtapeExtraction { pub archive: String, pub vers: String }
  pub struct Installation { pub methode: Methode, pub lien_connexion: Option<String>, pub extraire: Vec<EtapeExtraction> }
  pub struct Dependance { pub fichier: String, pub methode: Methode }
  pub struct Correctifs { pub registre: Vec<EntreeRegistre>, pub services: Vec<String>, pub dossiers: Vec<String> } // `lancement` retiré
  pub struct Gestionnaire { pub programme: String, pub arguments: Vec<String>, pub pilote: Vec<String>, pub fermer: Vec<String> }
  impl Gestionnaire { pub fn noms_a_fermer(&self) -> Vec<String> } // `fermer`, sinon le nom de fichier de `programme`
  pub struct Recette { /* champs existants */ pub empreinte: Option<String>, pub dependances: Vec<Dependance>, pub gestionnaire: Option<Gestionnaire> } // `apres_installation` retiré
  ```
  Clés TOML : `[[installation.extraire]]`, `[[dependances]]`, `[gestionnaire]`,
  `empreinte = "<sha256>"`.
- Règles de validation dans `charger`, en plus des règles existantes ; chacune donne un message
  traduit :
  1. `methode = "extraire-7z"` exige au moins une étape `extraire` ; `extraire` non vide avec une
     autre méthode est refusé ;
  2. `[gestionnaire]` est obligatoire si `type = "gestionnaire"`, refusée sinon ;
  3. `lien-connexion` exige `[gestionnaire]`, car le relais lance son `programme` ; le schéma ne
     contient que `[a-z0-9+.-]` ;
  4. la méthode d'une dépendance ne peut être ni `extraire-7z` ni `squirrel` ;
  5. `telecharger`, s'il est présent, commence par `https://` ;
  6. `empreinte` fait 64 caractères hexadécimaux (minuscules ou majuscules acceptées, stockée en
     minuscules).
- Les anciennes clés `lancement` et `[apres-installation]` sont refusées par `deny_unknown_fields`.
  Le message existant (« unknown field … ») suffit.

- [ ] **Step 1 : tests** dans `recette.rs` :
  - `recette_complete_du_sous_projet_2` : `valide.toml` réécrite selon la spec, section 3. Elle
    contient : extraction `'$PLUGINSDIR/app-64.7z'` → `'C:\Program Files\Native Instruments\Native Access'`,
    dépendance `installaware`, `dossiers`, `services = ["NTKDaemonService"]`, `[gestionnaire]`
    avec `arguments` et `lien-connexion = "native-access"`. Asserts : tous les champs sont lus, et
    `noms_a_fermer() == ["Native Access.exe"]`.
  - Un test par règle, de 1 à 6, nommés `refus_extraire_7z_sans_etape`,
    `refus_etapes_sans_extraire_7z`, `refus_gestionnaire_sans_section`,
    `refus_section_gestionnaire_hors_gestionnaire`, `refus_lien_sans_gestionnaire`,
    `refus_dependance_squirrel`, `refus_telechargement_http`, `refus_empreinte_invalide`. Chacun
    vérifie que l'erreur est `Erreur::Recette` et que son message nomme la clé.
  - `ancienne_cle_lancement_refusee` : `[correctifs] lancement = []` → `Erreur::Recette`, avec
    « lancement » dans le message.
- [ ] **Step 2 :** `cargo test -p wimao-recettes recette` → échec.
- [ ] **Step 3 :** implémenter. Retirer de `installation.rs` les accès à
  `apres_installation.pilote`, mais garder pour l'instant le refus des gestionnaires et
  d'`extraire-7z`/`squirrel` (`verifier_prise_en_charge`) : la tâche 7 le retire.
- [ ] **Step 4 :** `cargo test --workspace` → tout passe.
- [ ] **Step 5 :** `docs/recettes.md` décrit toutes les clés de la spec, section 3 : tableaux,
  exemple complet, et retrait des encadrés « pas encore pris en charge » pour `extraire-7z`,
  `squirrel`, `lien-connexion` et `pilote`. Les variables de la section 3.1 y figurent aussi, même
  si elles sont implémentées à la tâche 3. Puis commit.

### Task 3 : `chemins_windows.rs` — variables et chemins Windows

**Files :**
- Create : `crates/wimao-recettes/src/chemins_windows.rs`
- Modify : `crates/wimao-recettes/src/recette.rs` (`remplacer_variables` déplacée ici ou
  réexportée), `installation.rs` (utilise `utilisateur_windows` d'ici), `erreur.rs` (variante
  `MotifAmbigu`), `po/`.

**Interfaces :**
- Produces :
  ```rust
  pub fn utilisateur_windows(c: &Chemins) -> String;          // logique actuelle d'installation.rs (USER, sinon nom du dossier personnel, sinon "user")
  pub fn remplacer_variables(texte: &str, utilisateur: &str) -> String; // {utilisateur}, {telechargements}, {appdata-local} (spec 3.1)
  pub fn vers_linux(drive_c: &Path, chemin_windows: &str) -> Resultat<PathBuf>;
  pub fn resoudre_motif(drive_c: &Path, chemin_windows: &str) -> Resultat<PathBuf>;
  pub fn comparer_versions(a: &str, b: &str) -> std::cmp::Ordering;
  // erreur.rs :
  Erreur::MotifAmbigu { motif: String, nombre: usize } // « several files match {pattern} ({count}): the recipe must be corrected »
  ```
- `vers_linux` :
  - accepte `C:\…` ou `c:/…` ; sinon, ou si le chemin contient `..`, renvoie
    `Erreur::NonPrisEnCharge` traduite ;
  - chaque composant existant est retrouvé sans tenir compte des majuscules (Wine est insensible à
    la casse) ; le reste est gardé tel quel.
- `resoudre_motif` :
  - le chemin contient au plus des `*` ;
  - `*` dans le **dernier** composant : il faut exactement une correspondance, sinon
    `Introuvable` (0) ou `MotifAmbigu` (plusieurs) ;
  - `*` dans un **dossier** : on retient la correspondance de version la plus haute, avec
    `comparer_versions` sur la partie couverte par `*` ;
  - aucune correspondance → `Erreur::Introuvable` avec le motif dans le texte.
- `comparer_versions` compare des segments séparés par `.`, `-` ou `_`. Les segments numériques
  se comparent comme des nombres, sinon comme du texte : `"1.10.0" > "1.9.2"`.

- [ ] **Step 1 : tests** :
  - `variables_remplacees` : `"{appdata-local}\\x"` donne
    `"C:\\users\\u\\AppData\\Local\\x"`, `"{telechargements}\\NI"` donne
    `"C:\\users\\u\\Downloads\\NI"`, et `"{utilisateur}"` donne `"u"` ;
  - `vers_linux_ignore_la_casse` : avec `drive_c/Program Files/Foo/` existant,
    `C:\PROGRAM FILES\foo\bar.exe` → `drive_c/Program Files/Foo/bar.exe` ;
  - `vers_linux_refuse_hors_c_et_parent` : `D:\x` et `C:\a\..\b` → erreur ;
  - `motif_dans_le_nom_un_seul` : `NTKDaemon * Setup PC.exe` → le fichier ;
  - `motif_dans_le_nom_ambigu` : deux fichiers correspondants → `MotifAmbigu { nombre: 2 }` ;
  - `motif_sans_correspondance_est_introuvable` ;
  - `motif_de_dossier_prend_la_version_la_plus_haute` : avec `app-1.9.2/` et `app-1.40.0/`,
    `…\app-*\x.exe` → `app-1.40.0/x.exe` ;
  - `comparer_versions_numerique`.
- [ ] **Step 2 :** lancer les tests → échec. **Step 3 :** implémenter. **Step 4 :** les tests
  passent.
- [ ] **Step 5 :** commit.

### Task 4 : `obtention.rs` — télécharger l'installeur d'une recette

**Files :** Create `crates/wimao-recettes/src/obtention.rs` ; Modify `lib.rs`, `po/`.

**Interfaces :**
- Consumes : `wimao_core::telecharger_verifie(urls, sha256, dest)` (fichier `.part` puis
  renommage, déjà en place) et `wimao_core::empreinte`.
- Produces :
  ```rust
  pub fn obtenir_installeur(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<PathBuf>;
  pub(crate) fn nom_du_telechargement(r: &Recette, url: &str) -> String;
  ```
  - Pas de `telecharger` → `Erreur::Introuvable(tr("no download address in the recipe: choose the installer file"))`.
  - URL non HTTPS → `Erreur::Telechargement` traduite. C'est une seconde garde : une recette
    chargée l'a déjà vérifié.
  - La destination est `c.cache/telechargements/<nom>` ; le fichier existant est d'abord supprimé,
    car on télécharge à chaque fois (spec 3.6).
  - On appelle ensuite `telecharger_verifie(&[url], r.empreinte.as_deref(), &dest)`.
  - Enfin, `journal::noter("telechargement", "<url> sha256=<empreinte>")`.
- `nom_du_telechargement` : le dernier segment de l'URL s'il finit par `.exe`, `.msi`, `.zip` ou
  `.7z`. Sinon, le nom de la recette en minuscules, où tout ce qui n'est pas `[a-z0-9]` devient
  `-`, suivi de `.exe`. Exemple : `https://inmusic.to/ISCPC` → `inmusic-software-center.exe`.

- [ ] **Step 1 : tests** :
  - `nom_depuis_l_url` : `…/Native-Access_2.exe` → `Native-Access_2.exe` ;
  - `nom_depuis_la_recette` : avec `nom = "inMusic Software Center"` →
    `inmusic-software-center.exe` ;
  - `sans_adresse_introuvable` ;
  - `adresse_http_refusee`.

  Le téléchargement réel n'est pas testé ici : `telecharger_verifie` a déjà ses tests.
- [ ] **Step 2 à 4 :** test qui échoue, implémentation, test qui passe. **Step 5 :** commit.

### Task 5 : `extraction.rs` — méthodes `extraire-7z` et `squirrel`

**Files :**
- Create : `crates/wimao-recettes/src/extraction.rs`
- Modify : `crates/wimao-recettes/src/archive.rs` (la fonction qui lance `7z x` devient
  `pub(crate)` pour être réutilisée), `lib.rs`, `po/`.

**Interfaces :**
- Consumes : tâche 3 (`vers_linux`, `remplacer_variables`) et `EtapeExtraction` (tâche 2).
- Produces :
  ```rust
  pub fn installer_extraire_7z(c: &Chemins, fichier: &Path, etapes: &[EtapeExtraction], utilisateur: &str) -> Resultat<()>;
  pub fn installer_squirrel(c: &Chemins, fichier: &Path, utilisateur: &str) -> Resultat<PathBuf>; // rend le dossier app-<version>
  pub(crate) fn lire_nuspec(texte: &str) -> Option<(String, String)>; // (id, version) : contenu des balises <id> et <version>
  ```
- Fonctionnement commun :
  - extraction dans `c.cache/extraction-<pid>-<n>/`, supprimé à la fin, même en cas d'erreur
    (garde `Drop`) ;
  - les liens symboliques de l'archive ne sont jamais suivis ;
  - motif d'`archive` (relatif, `*` admis, une seule correspondance) : la règle de
    `resoudre_motif` s'applique au dossier extrait ; le code est commun ou factorisé.
- `extraire-7z` : `7z x` de l'installeur, puis, pour chaque étape, `7z x <archive> -o<vers_linux(vers)>`
  (dossier créé).
- `squirrel` :
  1. `7z x` de l'installeur ;
  2. recherche récursive, sans tenir compte de la casse, de l'unique `*-full.nupkg` (absent ou
     ambigu → erreur traduite), de `Update.exe` et de `RELEASES`, tous deux obligatoires ;
  3. `7z x` du nupkg, lecture de l'unique `*.nuspec` à sa racine ;
  4. destination `{appdata-local}\<id>` :
     - `app-<version>/` reçoit le contenu de `lib/net45/` ;
     - `Update.exe` et `RELEASES` copiés à la racine ;
     - le nupkg copié dans `packages/`.
- Signaler dans le rapport de tâche si un vrai installeur Squirrel range ces fichiers
  différemment : la vérification réelle (tâche 14) le confirmera.

- [ ] **Step 1 : tests** (`7z` requis, comme les tests d'archives existants ; fixtures construites
  dans le test avec `7z a`) :
  - `extraire_7z_en_deux_etapes` : une archive `setup.exe` (au format 7z) contient
    `$PLUGINSDIR/app-64.7z`, qui contient `Faux.exe`. Avec l'étape `('$PLUGINSDIR/app-64.7z', 'C:\Program Files\Faux')`,
    on obtient `drive_c/Program Files/Faux/Faux.exe`, et le dossier d'extraction n'existe plus ;
  - `extraire_7z_etape_absente` : motif sans correspondance → `Erreur::Introuvable` ;
  - `squirrel_installe_comme_a_la_main` : `Setup.exe` (zip) contient `RELEASES`, `Update.exe` et
    `Faux-1.2.3-full.nupkg`. Ce nupkg est un zip qui contient `Faux.nuspec`
    (`<id>faux_app</id><version>1.2.3</version>`) et `lib/net45/Faux.exe`. Résultat attendu :
    - `drive_c/users/u/AppData/Local/faux_app/app-1.2.3/Faux.exe` ;
    - `…/faux_app/Update.exe` et `…/faux_app/RELEASES` ;
    - `…/faux_app/packages/Faux-1.2.3-full.nupkg` ;
    - valeur rendue = le dossier `app-1.2.3` ;
  - `squirrel_sans_nupkg` → erreur ;
  - `lire_nuspec_simple` → `Some(("faux_app", "1.2.3"))` ;
  - `lire_nuspec_sans_version` → `None`.
- [ ] **Step 2 à 4.** **Step 5 :** ARCHITECTURE.md, puis commit.

### Task 6 : `liens.rs` — déclaration des liens de connexion

**Files :**
- Create : `crates/wimao-core/src/liens.rs`
- Modify : `crates/wimao-core/src/lib.rs`, `chemins.rs` (si une méthode XDG est ajoutée), `po/`.

**Interfaces :**
- Produces :
  ```rust
  pub enum EtatLien { Absent, DejaWimao, Autre(String) } // Autre : nom du .desktop qui gère le schéma
  pub fn etat_lien(c: &Chemins, schema: &str) -> Resultat<EtatLien>;   // `xdg-mime query default x-scheme-handler/<schema>`
  pub fn declarer_lien(c: &Chemins, schema: &str, relais: &Path) -> Resultat<()>;
  pub fn fichier_desktop(schema: &str) -> String;                     // "wimao-lien-<schema>.desktop"
  pub fn commande_relais() -> Resultat<PathBuf>; // "wimao-cli" à côté de std::env::current_exe(), sinon trouvé dans PATH, sinon Introuvable
  ```
- `declarer_lien` écrit `<XDG_DATA_HOME>/applications/wimao-lien-<schema>.desktop` :
  ```
  [Desktop Entry]
  Type=Application
  Name=WiMao (<schema>)
  Exec="<relais>" relayer-lien %u
  NoDisplay=true
  MimeType=x-scheme-handler/<schema>;
  ```
  Puis il lance `xdg-mime default <fichier> x-scheme-handler/<schema>` et
  `update-desktop-database <dossier applications>`. Si ce dernier programme est absent, ce n'est
  pas une erreur.

  Toutes les commandes reçoivent l'environnement de `wine::preparer_environnement(&mut cmd, c, &[])` :
  en isolation (`c.isole`), `XDG_DATA_HOME` et `XDG_CONFIG_HOME` sont alors ceux du test. Le
  dossier `<XDG_DATA_HOME>` utilisé pour écrire le `.desktop` est calculé par **la même**
  fonction que celle qui pose `XDG_DATA_HOME` en isolation. Factoriser au besoin.
- `declarer_lien` ne consulte pas l'état : c'est l'appelant qui décide (spec, section 5).

- [ ] **Step 1 : tests.** Pour les tests, un faux `xdg-mime` est un script shell placé en tête de
  `PATH` dans un dossier de test. Il écrit ses arguments dans un fichier et répond à `query` à
  partir d'un fichier. Les tests utilisent `serial_test`, car ils modifient `PATH`.
  - `declarer_ecrit_le_desktop_et_appelle_xdg_mime` : le fichier est dans
    `<racine de test>/…/applications/`, avec `Exec="/x/wimao-cli" relayer-lien %u`, et le faux
    `xdg-mime` a reçu `default wimao-lien-test.desktop x-scheme-handler/test` ;
  - `etat_absent` : le faux renvoie une chaîne vide → `Absent` ;
  - `etat_deja_wimao` ;
  - `lien_gere_par_un_autre_programme_est_signale_sans_etre_remplace` : le faux renvoie
    `wimao-native-access.desktop` → `Autre("wimao-native-access.desktop")`, et le dossier
    `applications` de test ne contient pas de nouveau fichier, car on n'a appelé que `etat_lien`.
- [ ] **Step 2 à 4.** **Step 5 :** ARCHITECTURE.md (tableau des outils externes : `xdg-mime`,
  `update-desktop-database`), puis commit.

### Task 7 : installation des recettes du sous-projet 2

**Files :** Modify `crates/wimao-recettes/src/installation.rs`, `lib.rs`, `po/`, ARCHITECTURE.md.

**Interfaces :**
- Consumes : tâches 2 à 5.
- Produces :
  ```rust
  pub fn installer_recette(c: &Chemins, r: &Recette, fichier: Option<&Path>, options: OptionsInstallation, p: &dyn Progression) -> Resultat<RapportInstallation>;
  pub fn appliquer_reglages(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<()>; // registre + dossiers (créés via vers_linux)
  pub fn est_installe(c: &Chemins, r: &Recette) -> bool;  // gestionnaire : resoudre_motif(programme) réussit ; autre type : false
  pub fn reparer_gestionnaires(c: &Chemins, catalogue: &Catalogue, p: &dyn Progression) -> Resultat<()>; // appliquer_reglages pour chaque gestionnaire installé
  // RapportInstallation, nouveaux champs :
  pub gestionnaire: bool,              // recette de type gestionnaire : « aucun plugin » n'est pas un échec
  pub lien_a_declarer: Option<String>, // schéma de `lien-connexion`, que l'appelant déclarera après confirmation (tâche 6)
  pub(crate) fn ponter_plugins(c: &Chemins, apres: &Inventaire, a_traiter: Vec<PluginWindows>, p: &dyn Progression) -> Resultat<Vec<(PluginWindows, EtatPontage)>>;
  pub(crate) fn tester_plugins(c: &Chemins, plugins: Vec<(PluginWindows, EtatPontage)>, p: &dyn Progression) -> Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>;
  ```
  Les deux dernières fonctions découpent `ponter_et_tester`, qui les appelle désormais. La
  session (tâche 9) les réutilise.
- Déroulé de `installer_recette` :
  1. `fichier` absent : `obtenir_installeur` (tâche 4) ;
  2. on suit ensuite le parcours de `installer_avec_delai` avec **cette** recette, au lieu de
     `catalogue.trouver`. `installer_avec_delai` reçoit la recette en paramètre, et `installer`
     garde son comportement ;
  3. méthodes : `Extraire7z` → `installer_extraire_7z` et `Squirrel` → `installer_squirrel`, sans
     code d'installeur ;
  4. puis chaque dépendance : `resoudre_motif`, type d'installeur pris dans la méthode, ou détecté
     si `auto`, lancement silencieux avec le délai d'installation. Les codes InstallAware 100 et
     110 sont des réussites, et un autre code non nul est une erreur `Erreur::Commande` ;
  5. puis `appliquer_reglages` ;
  6. pour un gestionnaire, `interactif_necessaire = false` et aucun message « aucun plugin ».
- `verifier_prise_en_charge` disparaît, avec son test `recette_pilote_est_refusee…`.

- [ ] **Step 1 : tests** (même banc que les tests actuels d'`installation.rs` ; un test qui exige
  Wine est `#[ignore]`) :
  - `gestionnaire_extrait_sans_message_aucun_plugin` : recette gestionnaire `extraire-7z` (fixture
    de la tâche 5). Le rapport donne `gestionnaire == true`, `interactif_necessaire == false`,
    `lien_a_declarer == Some("wimao-test")`, et l'exécutable est en place ;
  - `reglages_creent_les_dossiers` : `dossiers = ['{telechargements}\NI']` donne
    `drive_c/users/<u>/Downloads/NI` existant ;
  - `est_installe_suit_le_programme` : `false`, puis `true` une fois le fichier `programme` créé ;
  - `dependance_ambigue_refusee` : deux fichiers correspondent → `MotifAmbigu`, et l'erreur
    remonte.
- [ ] **Step 2 à 4.** **Step 5 :** commit.

### Task 8 : `pilote.rs` — relancer en silencieux les installeurs d'un gestionnaire

**Files :** Create `crates/wimao-recettes/src/pilote.rs` ; Modify `lib.rs`, `po/`.

**Interfaces :**
- Consumes : `ProcessusWine`, `arreter_processus` (tâche 1) ; `detecter`,
  `arguments_silencieux` ; `vers_linux`, `remplacer_variables` (tâche 3).
- Produces :
  ```rust
  pub struct InstalleurPilote { pub pid: u32, pub fichier_windows: String, pub nom: String } // nom = nom de fichier
  pub const ARGUMENTS_SILENCIEUX: &[&str] = &["/verysilent", "/silent", "-quiet", "/quiet", "/s", "/qn"]; // comparaison sans casse, argument entier
  pub fn installeurs_a_piloter(processus: &[ProcessusWine], dossiers_windows: &[String]) -> Vec<InstalleurPilote>;
  pub enum IssueRelance {
      Relance { nom: String, type_: TypeInstalleur, code: Option<i32> }, // code None = délai dépassé
      Refermee { nom: String },   // déjà relancé dans cette session : fermé sans relance
      Inconnu { nom: String },    // type inconnu : laissé tel quel
  }
  pub struct Pilote { /* noms déjà traités, sans casse */ }
  impl Pilote {
      pub fn new() -> Pilote;
      pub fn agir(&mut self, c: &Chemins, processus: &[ProcessusWine], dossiers_windows: &[String], delai: Duration) -> Resultat<Vec<IssueRelance>>;
  }
  ```
- `installeurs_a_piloter` : un argument quelconque, comparé sans tenir compte de la casse avec
  `\` et `/` équivalents, commence par un des dossiers suivi d'un séparateur et finit par `.exe`.
  Aucun argument n'est dans `ARGUMENTS_SILENCIEUX`. Un même fichier présent dans plusieurs
  processus ne compte qu'une fois.
- `agir`, pour chaque installeur :
  1. **déjà dans l'ensemble** : arrêter les processus dont la ligne contient le nom sans
     extension (comparaison sans casse) → `Refermee` ;
  2. **sinon** : détecter le type du fichier (`vers_linux`). S'il est `Inconnu` → `Inconnu`, sans
     rien arrêter, mais on l'ajoute à l'ensemble pour ne pas le signaler à chaque passage ;
  3. **sinon** :
     - arrêter les processus correspondants (`arreter_processus`, délai de 10 s) ;
     - relancer `wine <fichier> <arguments_silencieux>` depuis son dossier, avec `delai`, en
       ajoutant le journal : `/LOG=C:\wimao-pilote-<nom sans extension>.log` pour Inno, ou
       `-log C:\wimao-pilote-<…>.log` pour WiX Burn ;
     - noter dans le journal → `Relance`.

- [ ] **Step 1 : tests** (sans Wine pour la sélection) :
  - `installeur_interactif_selectionne` : argument
    `C:\users\u\Downloads\inMusic\Hype\Install-Hype.exe` et dossier
    `C:\users\u\Downloads\inMusic` → un `InstalleurPilote` dont le nom est `Install-Hype.exe` ;
  - `installeur_deja_silencieux_ignore` (`/VERYSILENT`) ;
  - `exe_hors_dossier_ignore` ;
  - `dossier_compare_sans_casse_et_separateurs` ;
  - `installeur_inconnu_n_est_pas_arrete` : fichier sans marqueur connu, lancé comme `sleep 30` par
    un vrai processus. On construit à la main le `ProcessusWine` correspondant. `agir` renvoie
    `Inconnu`, et le processus est toujours vivant. On l'arrête à la fin du test.
- [ ] **Step 2 à 4.** **Step 5 :** commit. La relance réelle est testée à la tâche 11.

### Task 9 : `gestionnaire.rs` — session et relais

**Files :** Create `crates/wimao-recettes/src/gestionnaire.rs` ; Modify `lib.rs`, `po/`, ARCHITECTURE.md.

**Interfaces :**
- Consumes : tâches 1, 3, 7 et 8 ; `poser_verrou_installation`, `restauration::creer`,
  `Inventaire`, `Wine`.
- Produces :
  ```rust
  pub const INTERVALLE_PILOTE: Duration = Duration::from_secs(2);
  pub const INTERVALLE_RELEVE: Duration = Duration::from_secs(60);
  pub enum EvenementSession { Relance(IssueRelance), Ponte(PluginWindows) }
  pub struct BilanSession {
      pub point: PointRestauration,
      pub plugins: Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>,
      pub mis_a_jour: BTreeSet<PathBuf>,
      pub relances: Vec<IssueRelance>,
  }
  pub struct Session { /* possède : Chemins (clone), Recette (clone), verrou, point, relevé d'ouverture, Pilote, enfant, signatures du relevé précédent, plugins pontés, relances, instant du dernier relevé */ }
  pub fn ouvrir(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<Session>;
  impl Session {
      pub fn tick(&mut self, maintenant: Instant, p: &dyn Progression) -> Resultat<Vec<EvenementSession>>;
      pub fn gestionnaire_ferme(&mut self) -> Resultat<bool>;
      pub fn installeurs_en_cours(&self) -> Resultat<Vec<String>>;
      pub fn fermer(self, tester: bool, p: &dyn Progression) -> Resultat<BilanSession>;
  }
  pub(crate) fn plugins_stables(precedent: &BTreeMap<PathBuf, Signature>, candidats: &[PluginWindows]) -> Vec<PluginWindows>;
  pub(crate) fn noms_d_installeurs(processus: &[ProcessusWine], exclus: &[String]) -> Vec<String>;
  pub fn relayer_lien(c: &Chemins, catalogue: &Catalogue, url: &str) -> Resultat<()>;
  ```
  `Session` n'emprunte rien, pour pouvoir passer dans un fil de l'interface.
- `ouvrir` suit la spec 4.2 :
  1. `est_installe`, sinon `Erreur::Introuvable` qui cite `programme` ;
  2. verrou, environnement créé si absent, point de restauration `session <nom>` ;
  3. `appliquer_reglages`, services (`wine net start <nom>` ; une sortie qui contient « already »
     n'est pas une erreur), relevé ;
  4. lancement de `programme` résolu, avec les `arguments` (variables remplacées) :
     - dossier de travail = celui du programme ;
     - sorties ajoutées à `<données>/journaux/<nom de la recette en minuscules, tirets>.log` ;
     - entrée vide ;
     - `spawn`, sans attendre.
- `tick` :
  - toutes les `INTERVALLE_PILOTE`, le pilote s'il y a des dossiers `pilote` ;
  - toutes les `INTERVALLE_RELEVE` :
    1. relevé ;
    2. candidats = nouveaux + modifiés par rapport au relevé d'ouverture, sauf ceux déjà pontés
       avec cette signature ;
    3. `plugins_stables` (signature égale au relevé précédent) ;
    4. `ponter_plugins` → `Ponte` ;
    5. signatures mémorisées.
- `gestionnaire_ferme` : l'enfant est terminé (`try_wait`) **et** aucun processus du préfixe n'a
  un `nom_programme` dans `noms_a_fermer()` (comparaison sans casse).
- `installeurs_en_cours` : les `nom_programme` qui contiennent `setup`, `install` ou `msiexec`
  (sans casse), **sauf** les `noms_a_fermer()`, car `PA-InstallationManager.exe` contient
  « install ».
- `fermer` suit la spec 4.4, étapes 2 à 5 :
  1. arrêter les processus `noms_a_fermer` (délai de 10 s) ;
  2. dernier relevé, et pontage de tous les candidats restants, stables ou non ;
  3. si `tester`, `tester_plugins` sur tous les plugins de la session ;
  4. bilan dans le journal ; le verrou est libéré par `Drop`.
- `relayer_lien` :
  1. lire le schéma (avant `://`, sans casse) ; trouver la recette dont `lien_connexion` est égal
     à ce schéma, sinon `Introuvable` ;
  2. lancer le `programme` avec `arguments` + `url` ;
  3. sorties ajoutées à `<données>/journaux/liens.log`, entrée vide, `spawn` sans attendre ;
  4. ni verrou, ni `journal::initialiser`.

- [ ] **Step 1 : tests** (sans Wine) :
  - `plugin_dont_la_signature_change_n_est_pas_ponte` : signature précédente de taille 10 et
    actuelle de taille 20 → vide ; deux signatures égales → le plugin ;
  - `plugin_nouveau_sans_releve_precedent_attend` : absent du relevé précédent → pas encore
    stable ;
  - `installeurs_en_cours_exclut_le_gestionnaire` : `PA-InstallationManager.exe`,
    `Setup PC.exe` et `msiexec.exe` avec l'exclu `PA-InstallationManager.exe` →
    `["Setup PC.exe", "msiexec.exe"]` ;
  - `ouvrir_un_gestionnaire_absent_est_introuvable` (`Chemins::pour_test`, programme absent) ;
  - `relayer_un_schema_inconnu_est_introuvable`.
- [ ] **Step 2 à 4.** **Step 5 :** commit. Le parcours complet avec Wine est testé à la tâche 11.

### Task 10 : `wimao-cli` — recettes, gestionnaires, relais, réparation avec confirmation

**Files :**
- Modify : `crates/wimao-cli/src/cli.rs`, `commandes/mod.rs`, `commandes/entretien.rs` ;
  `crates/wimao-cli/Cargo.toml` (`libc` du workspace si absent) ; `tests/cli.rs` ; `po/`.
- Create : `crates/wimao-cli/src/commandes/gestionnaires.rs`.

**Interfaces :**
- Commandes (spec, section 7) :
  - `recettes` ;
  - `gestionnaire installer <nom> [--fichier <f>] [--oui]` ;
  - `gestionnaire ouvrir <nom> [--sans-test]` ;
  - `relayer-lien <url>`.

  `<nom>` est comparé au `nom` des recettes sans tenir compte de la casse ; on accepte aussi le nom
  du fichier de recette sans `.toml`. Une recette absente donne le code 1 avec un message traduit.
- `gestionnaire installer` :
  - lance `installer_recette`, puis, si `lien_a_declarer`, fait `etat_lien` ;
  - `Absent` ou `DejaWimao` → `declarer_lien(commande_relais()?)` ;
  - `Autre(x)` → question `o/N` qui nomme `x`, sauf avec `--oui`. Un refus n'est pas une erreur ;
    on affiche que le lien n'est pas relayé.
- `gestionnaire ouvrir` :
  - `ouvrir` ; boucle qui appelle `tick` toutes les `INTERVALLE_PILOTE` et affiche chaque
    événement sur une ligne ;
  - la boucle s'arrête quand `gestionnaire_ferme()` est vrai et qu'`installeurs_en_cours()` est
    vide, ou au premier Ctrl+C ;
  - après Ctrl+C, si des installeurs tournent, on les affiche et on attend qu'ils finissent. Un
    second Ctrl+C vaut « Arrêter quand même ». Gestion de SIGINT par `libc::signal` et un
    `AtomicUsize` (nombre de Ctrl+C) ;
  - ensuite `fermer(!sans_test)` et affichage du bilan avec le tableau existant (`tableau.rs`).

    Codes : 0, ou 2 si un plugin est ❌.
- `relayer-lien` appelle `relayer_lien`. Aucune sortie sur stdout : seul le code de sortie compte.
- `reparer` (spec 4.5) : si les problèmes contiennent `WineBloque` et que
  `programmes_ouverts(c)` n'est pas vide, une question `o/N` nomme ces programmes, sauf avec
  `--oui`. En cas de refus, `WineBloque` est retiré des problèmes à réparer. Après `reparer`, on
  appelle `reparer_gestionnaires`.

- [ ] **Step 1 : tests** (`tests/cli.rs`, sans Wine, HOME de test) :
  - `recettes_liste_les_recettes_du_dossier` (avec `WIMAO_RECETTES` vers
    `crates/wimao-recettes/tests/recettes`) ;
  - `gestionnaire_inconnu_code_1` ;
  - `relayer_lien_schema_inconnu_code_1` ;
  - `aide_de_gestionnaire_liste_installer_et_ouvrir`.
- [ ] **Step 2 à 4.** **Step 5 :** ARCHITECTURE.md, puis commit.

### Task 11 : faux gestionnaire, test d'intégration avec Wine, CI

**Files :**
- Create :
  - `tests/fixtures/gestionnaire/FauxGestionnaire.c` ;
  - `tests/fixtures/gestionnaire/FauxGestionnaire.exe` (binaire versionné, comme
    `WiMaoTest.vst3`, avec sa commande de compilation en tête du `.c`) ;
  - `crates/wimao-recettes/tests/recettes-gestionnaire/faux-gestionnaire.toml` ;
  - `crates/wimao-recettes/tests/gestionnaire.rs`.
- Modify :
  - `tests/fabriquer-fixtures.sh` : fabrique `faux-gestionnaire-setup.exe`, une archive 7z qui
    contient `$PLUGINSDIR/app-64.7z`, qui contient `FauxGestionnaire.exe` ;
  - `.github/workflows/ci.yml` si besoin ;
  - `CONTRIBUTING.md`.

**Interfaces :**
- `FauxGestionnaire.exe` (C, mingw `x86_64-w64-mingw32-gcc`, sans dépendance) :
  - si un argument contient `://`, il l'ajoute à `C:\faux-gestionnaire-liens.txt` et quitte
    (relais) ;
  - sinon, il lance avec `CreateProcess` le fichier passé en 1er argument, **sans** argument
    silencieux. Il attend ensuite au plus 120 s que ce fichier soit arrêté et relancé par le
    pilote, puis attend 5 s et quitte.
- Recette de test :
  - `type = "gestionnaire"`, `methode = "extraire-7z"` ;
  - étape `'$PLUGINSDIR/app-64.7z'` → `'C:\Program Files\FauxEditeur'` ;
  - `programme = 'C:\Program Files\FauxEditeur\FauxGestionnaire.exe'` ;
  - `arguments = ['{telechargements}\FauxEditeur\test-nsis.exe']` ;
  - `pilote = ['{telechargements}\FauxEditeur']` ;
  - `lien-connexion = "wimao-test"` ;
  - `reconnaitre = ["faux-gestionnaire-setup.exe"]`.

- [ ] **Step 1 :** test `#[ignore = "Wine requis"]` `session_pilote_et_ponte`, avec `WIMAO_FIXTURES`
  comme le test d'intégration existant :
  1. `installer_recette` ;
  2. copier `test-nsis.exe` dans `drive_c/users/<u>/Downloads/FauxEditeur/` ;
  3. `ouvrir` ;
  4. boucle de `tick` (2 s, au plus 5 min) jusqu'à `gestionnaire_ferme() && installeurs_en_cours().is_empty()` ;
  5. `fermer(true)`.

  Asserts :
  - `relances` contient `Relance { type_: Nsis, code: Some(0), .. }` ;
  - le bilan contient le plugin `WiMaoTest.vst3`, ponté ;
  - aucun processus du préfixe ne reste (`processus_du_prefixe` vide après `wineserver -w`).
- [ ] **Step 2 :** test `#[ignore = "Wine requis"]` `relais_transmet_l_adresse` :
  `relayer_lien(c, cat, "wimao-test://ok?x=1")`, puis attente de 60 s au plus jusqu'à ce que
  `drive_c/faux-gestionnaire-liens.txt` contienne l'adresse.
- [ ] **Step 3 :** fabriquer les fixtures, puis lancer
  `cargo test -p wimao-recettes --test gestionnaire -- --ignored` en local, avec `/opt/wine-mao-v2`
  et Xvfb (`xvfb-run -a`) → les tests passent.
- [ ] **Step 4 :** la CI lance ces tests dans le job `integration`, sans changement s'ils sont dans
  le même `--ignored`. Vérifier que `fabriquer-fixtures.sh` fabrique le faux gestionnaire **avant**
  l'arrêt de `WIMAO_FIXTURES_SANS_INNO`.
- [ ] **Step 5 :** CONTRIBUTING.md, puis commit.

### Task 12 : interface — gestionnaires, session, confirmations

**Files :**
- Create : `crates/wimao/src/gestionnaires.rs` (lignes de la section) et
  `crates/wimao/src/session.rs` (fil de session et bannière).
- Modify : `fenetre.rs`, `dialogues.rs`, `operations.rs`, `etat.rs` si besoin, `data/style.css`
  si besoin, `po/`, ARCHITECTURE.md ; `docs/ameliorations-reportees.md` (retirer les deux points
  corrigés).

**Interfaces :**
- Consumes : tâches 6, 7, 9 et 10. La logique des confirmations est la même que dans la CLI.
- Section « Gestionnaires » (spec, section 6) :
  - un `adw::PreferencesGroup` (ou `boxed-list`) au-dessus de la liste des plugins, aussi visible
    sur la page vide ;
  - une `adw::ActionRow` par recette gestionnaire : titre = nom, sous-titre =
    « éditeur · Installé / Non installé », icône ;
  - bouton `pill` « Install » (`suggested-action`) ou « Open ».
- « Install » :
  - travail en fond (`travail.rs`), puis dialogue de lien si `EtatLien::Autre` ;
  - toast « <nom> installé » ; la liste est rafraîchie.
- « Open » :
  - `ouvrir` en fond ;
  - un fil appelle `tick` toutes les 2 s et envoie les événements au fil principal ;
  - `adw::Banner` « <nom> is open — WiMao bridges new plugins », bouton « I'm done » ;
  - la consigne `connexion` s'affiche une fois dans un `adw::AlertDialog` ;
  - les relances arrivent en toasts, et les plugins pontés s'ajoutent à la liste ;
  - fin automatique (gestionnaire fermé), ou « I'm done » → dialogue d'attente si des
    installeurs tournent (« Stop anyway ») → `fermer(true)` → toast de bilan
    (`bilan_installation` adapté au `BilanSession`).
- Pendant une session, les actions Ajouter, Réparer, Sauvegarder et Revenir sont désactivées, et
  leur infobulle donne la raison. Fermer la fenêtre demande confirmation : le gestionnaire reste
  ouvert, et le pilote s'arrête.
- « Réparer » : même confirmation que la CLI (`programmes_ouverts`), puis `reparer_gestionnaires`.
- Défaut reporté : le dialogue « silent installation did not add a plugin » cite le code de
  l'installeur (`code_installeur`) quand il est non nul.

- [ ] **Step 1 : tests** dans `operations.rs`, sans fenêtre :
  - `bilan_de_session_avec_relances` (en et fr) ;
  - `lignes_gestionnaires_etat_installe` ;
  - `message_aucun_plugin_cite_le_code`.
- [ ] **Step 2 :** implémenter.
- [ ] **Step 3 :** captures sous Xvfb (sous-projet 1, Ruling 14) :
  - section gestionnaires ;
  - bannière de session (avec une session simulée par un faux `Session` si nécessaire, sinon la
    capture de la bannière seule) ;
  - dialogue de lien.

  Les déposer dans le dossier de travail de la tâche, pas dans le dépôt, et les relire pour la
  propreté visuelle.
- [ ] **Step 4 :** tests, traductions, clippy.
- [ ] **Step 5 :** commit.

### Task 13 : recettes fournies, règles d'erreurs, documentation

**Files :**
- Create : `recettes/native-access.toml`, `recettes/arturia-software-center.toml`,
  `recettes/inmusic-software-center.toml`, `recettes/plugin-alliance.toml`,
  `recettes/serum-2.toml`, `recettes/valhalla-supermassive.toml`.
- Modify : `crates/wimao-core/ressources/regles.toml`, `crates/wimao-core/src/regles.rs`
  (`tr_noop` des nouveaux messages), `po/`, `README.md`, ARCHITECTURE.md.
- Test : `crates/wimao-recettes/tests/recettes_fournies.rs`.

**Interfaces :**
- Les valeurs sont celles de la spec, section 8, mot pour mot. Consignes en `fr`, `en` et `es` :
  - `connexion` pour chaque gestionnaire ;
  - pour inMusic : il réaffiche « Install » après une relance silencieuse, il faut cliquer à
    nouveau ou le redémarrer ;
  - pour Plugin Alliance : blocage à 98 % ;
  - `activation` pour Serum 2.
- Adresse Plugin Alliance : l'archive publique de l'Installation Manager, notée dans `NOTES.md`
  (`plugin-alliance.com/pages/installation-manager`). Si l'adresse directe du zip n'est pas connue
  sans navigateur, ne pas mettre `telecharger` : la consigne demande de choisir le fichier. Le
  signaler dans le rapport.
- `regles.toml` :
  - `ERROR_CODE_DOWNLOAD_DIR_MISSING` → message « Native Access has no download folder: use
    Repair » (sans `reparation` : « Réparer » appelle toujours `reparer_gestionnaires`) ;
  - `Error: open EBADF` → « A sign-in link could not reach the manager: open it again from the
    browser ».

- [ ] **Step 1 : tests** :
  - `recettes_fournies_valides` : `Catalogue::charger(&[<dépôt>/recettes])` ne donne aucune
    erreur, et contient exactement 4 recettes de type gestionnaire et 2 installeurs ;
  - `serum_reconnu` : `trouver("Install_Xfer_Serum2_2.1.5.exe")` → « Serum 2 » ;
  - deux tests dans `regles.rs` pour les nouveaux motifs.
- [ ] **Step 2 à 4.** **Step 5 :** README (gestionnaires pris en charge), ARCHITECTURE, puis
  commit.

### Task 14 : vérification réelle (par le contrôleur)

Pas de code, sauf les corrections que la vérification rend nécessaires.

- [ ] Préparer un environnement de test : `HOME`, `XDG_*` et `WIMAO_DONNEES` dans un dossier
  temporaire, `WIMAO_WINE_DIR=/opt/wine-mao-v2`, Xvfb pour les fenêtres. **Jamais** `~/.wine-mao`,
  ni les vrais menus ou associations.
- [ ] Pour chacun des 4 gestionnaires :
  - `wimao-cli gestionnaire installer <nom>` (téléchargement réel) ;
  - `gestionnaire ouvrir` jusqu'à l'écran de connexion (capture sous Xvfb) ;
  - fermeture propre ; aucun processus restant.
- [ ] Relais : `xdg-open 'native-access://test'`, dans l'environnement de test → la ligne apparaît
  dans `liens.log`.
- [ ] Valhalla Supermassive (gratuit) de bout en bout : téléchargement, installation, pontage,
  test ✅. Empreinte de `NOTES.md` vérifiée, ou mise à jour dans la recette.
- [ ] Aucun produit payant n'est installé. Résultats dans `NOTES.md`, section « WiMao sous-projet 2
  — vérification réelle », sans nom d'utilisateur.
