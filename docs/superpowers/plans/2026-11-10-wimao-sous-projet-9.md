# WiMao sous-projet 9 — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** ouvrir depuis WiMao les lanceurs d'éditeurs inconnus, faire charger les plugins pontés par les logiciels de musique en Flatpak, et raccourcir le README avec des crédits.

**Architecture:** côté `wimao-core`, un module `flatpak.rs` pose le lien yabridge dans `~/.var/app/<id>/data` (généralisation de `pontage::assurer_lien_yabridge`) et le diagnostic signale les permissions manquantes. Côté `wimao-recettes`, un module `applications.rs` lit les raccourcis `.lnk` du menu Démarrer de Wine, retient les nouveaux programmes après une installation sans plugin, les enregistre dans `applications.toml` et les ouvre comme un gestionnaire (recette construite en mémoire, `gestionnaire::ouvrir` inchangé). L'interface et la CLI les affichent ; la doc suit.

**Tech Stack:** Rust (workspace existant), GTK4/libadwaita, gettext (`po/*.po`), serde/toml, shellcheck.

**Spec:** `docs/superpowers/specs/2026-11-10-wimao-sous-projet-9-design.md`

## Global Constraints

- Code propre, **très commenté en français**, en-tête de module « Utilise / Utilisé par » comme les modules existants ; un développeur inconnu doit s'y retrouver.
- Toute chaîne visible passe par `tr`/`trf` avec un `msgid` anglais, traduite en fr et es dans `po/*.po` (voir `CONTRIBUTING.md`).
- Aucune nouvelle dépendance de crate (le lecteur `.lnk` est écrit à la main).
- WiMao ne remplace **jamais** un lien existant et ne lance **jamais** `flatpak override` lui-même.
- Identifiants Flatpak connus, exactement : `com.bitwig.BitwigStudio`, `fm.reaper.Reaper`, `org.ardour.Ardour`, `studio.kx.carla`, `io.lmms.LMMS`, `org.zrythm.Zrythm`, `org.rncbc.qtractor`, `com.renoise.Renoise`.
- Fichier des applications : `<Chemins::donnees>/applications.toml`, écrit de façon atomique.
- Les tests qui lancent Wine sont `#[ignore = "Wine requis"]`, comme l'existant.
- Vérifications finales de chaque tâche : `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (workspace), `RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps`.

## Review Focus

1. Un `.lnk` corrompu, tronqué ou malveillant dans le préfixe ne doit jamais faire paniquer WiMao ni lire hors du tampon → tests de fuzz simples dans la tâche 3.
2. L'installeur d'un **plugin** qui crée aussi des raccourcis (versions autonomes, désinstalleur) ne doit créer aucune application → test dans la tâche 4.
3. `~/.var/app/<id>/data/yabridge` déjà présent (lien vers un autre yabridge, vrai dossier) doit rester intact → test dans la tâche 1.
4. Une application dont le programme a été désinstallé ou retiré par un retour en arrière ne doit plus être affichée, sans perdre la ligne du fichier → test dans la tâche 3.
5. `flatpak` absent (Ubuntu sans Flatpak) ou en échec ne doit produire ni erreur ni problème de diagnostic → tests dans les tâches 1 et 2.

---

### Task 1: Liens yabridge pour les logiciels de musique en Flatpak

**Files:**
- Modify: `crates/wimao-core/src/pontage.rs` (lien généralisé, appel dans `assurer_lien_et_noter`)
- Create: `crates/wimao-core/src/flatpak.rs`
- Modify: `crates/wimao-core/src/lib.rs` (module + doc d'en-tête)

**Interfaces:**
- Produces:
  - `pontage::etat_lien_yabridge_a(c: &Chemins, lien: &Path) -> Resultat<EtatLienYabridge>` et `pontage::assurer_lien_yabridge_a(c: &Chemins, lien: &Path) -> Resultat<LienYabridge>` ; `etat_lien_yabridge(c)` / `assurer_lien_yabridge(c)` deviennent des appels avec `emplacement_lien_yabridge(c)` (comportement inchangé).
  - `flatpak::LOGICIELS_FLATPAK: [&str; 8]` (les identifiants des Global Constraints).
  - `flatpak::logiciels_installes() -> Vec<String>` : lance `flatpak list --app --columns=application` (trouvé dans le `PATH`), garde ceux de la liste ; tout échec → `vec![]`.
  - `flatpak::emplacement_lien_flatpak(c: &Chemins, id: &str) -> PathBuf` = `c.home/.var/app/<id>/data/yabridge`.
  - `flatpak::assurer_liens_flatpak(c: &Chemins, ids: &[String]) -> Vec<(String, Resultat<LienYabridge>)>`.
  - `flatpak::rafraichir_dates_plugins(c: &Chemins) -> Resultat<usize>` : met à maintenant la date de modification des `*.so` sous `~/.vst3/yabridge`, `~/.vst/yabridge`, `~/.clap/yabridge` (récursif, liens non suivis) ; rend le nombre de fichiers touchés.
  - `pontage::assurer_lien_et_noter` appelle aussi `assurer_liens_flatpak(c, &logiciels_installes())`, note au journal les cas `AutreLien`/`Dossier`/erreur comme pour le lien principal, et appelle `rafraichir_dates_plugins` **seulement** si au moins un lien est `Cree`.

- [ ] **Step 1: Tests** dans `flatpak.rs` (faux `flatpak` = script shell écrit dans un dossier temporaire mis en tête du `PATH`, tests `#[serial_test::serial]`) :
  - `logiciels_installes_garde_les_connus` : le faux `flatpak` affiche `com.bitwig.BitwigStudio\norg.gimp.GIMP\nfm.reaper.Reaper\n` → `["com.bitwig.BitwigStudio", "fm.reaper.Reaper"]`.
  - `logiciels_installes_sans_flatpak_est_vide` : `PATH` sans `flatpak` → `[]` ; faux `flatpak` qui sort en code 1 → `[]`.
  - `lien_flatpak_cree_puis_deja_bon` : `Cree` puis `DejaBon`, la cible lue est `c.yabridge_dir`.
  - `lien_flatpak_existant_laisse_intact` : lien vers un autre dossier → `AutreLien(cible)` et lien inchangé ; vrai dossier → `Dossier`, contenu inchangé.
  - `dates_rafraichies_seulement_les_so` : un `.so` et un `.txt` datés de 2001 sous `~/.vst3/yabridge/X.vst3/Contents/x86_64-linux/` → seul le `.so` a une date récente, retour `1`.
  - Dans `pontage.rs` : les tests existants de `assurer_lien_yabridge` passent sans changement.
- [ ] **Step 2:** `cargo test -p wimao-core flatpak` → échec (module absent).
- [ ] **Step 3:** implémenter (pour les dates : `std::fs::File::options().write(true).open(p)?.set_modified(SystemTime::now())`).
- [ ] **Step 4: Test réel** `#[ignore = "Wine requis"]` `plugin_ponte_se_charge_depuis_un_flatpak_simule` : prend le premier `~/.vst3/yabridge/**/x86_64-linux/*.so` de la machine (test sauté avec message s'il n'y en a aucun), pose le lien dans un dossier temporaire, lance `python3 -I` sur un petit script `ctypes` (dans `crates/wimao-core/tests/donnees/charger_vst3.py`) avec `env_clear`, `HOME`, `PATH=/usr/bin:/bin`, `XDG_DATA_HOME=<temp>`, `XDG_RUNTIME_DIR`, `DISPLAY` ; attend `ModuleEntry -> True` dans la sortie. Lancer : `cargo test -p wimao-core -- --ignored plugin_ponte_se_charge_depuis_un_flatpak_simule` → PASS.
- [ ] **Step 5:** vérifications globales, puis commit `Flatpak : lien yabridge dans ~/.var/app/<id>/data`.

### Task 2: Diagnostic des permissions Flatpak

**Files:**
- Modify: `crates/wimao-core/src/flatpak.rs`, `crates/wimao-core/src/diagnostic.rs`, `po/fr.po`, `po/es.po` (et le `.pot` s'il existe)
- Modify: l'affichage des problèmes dans `crates/wimao/src/` et `crates/wimao-cli/src/` seulement si un `match` exhaustif l'exige (même présentation que `LienYabridge` : texte + commande copiable).

**Interfaces:**
- Consumes: tâche 1 (`logiciels_installes`, `emplacement_lien_flatpak`, `etat_lien_yabridge_a`).
- Produces:
  - `flatpak::chemins_invisibles(filesystems: &[String], home: &Path, requis: &[PathBuf]) -> Vec<PathBuf>` (pure).
  - `flatpak::systemes_de_fichiers(id: &str) -> Option<Vec<String>>` : lit la ligne `filesystems=` de `flatpak info --show-permissions <id>` (séparateur `;`, entrées vides retirées) ; échec → `None` (pas de problème signalé).
  - `flatpak::commande_override(id: &str, invisibles: &[PathBuf], home: &Path) -> String` : `flatpak override --user --filesystem=<chemin>[:ro] … <id>` ; `:ro` pour `/opt/wimao`, `~/.vst3`, `~/.vst`, `~/.clap` ; données de WiMao sans suffixe ; chemins sous `home` écrits `~/…` ; chaque argument cité pour le shell comme `commande_lien_yabridge`.
  - `Probleme::FlatpakSansAcces { id: String, invisibles: Vec<PathBuf>, commande: String }` : avertissement (exclu de la réparation et du point de restauration comme `LienYabridge` ; même rang d'affichage que lui).
  - `Probleme::LienYabridge` est aussi produit pour un lien Flatpak `Ailleurs`/`Casse`/`Dossier` (son champ d'emplacement porte `emplacement_lien_flatpak`).

- [ ] **Step 1: Tests** de `chemins_invisibles` (home = `/home/u`, requis = `/opt/wimao`, `/home/u/.local/share/wimao`, `/home/u/.vst3`) :
  - `["host"]` → `[]` ; `["home"]` → `[/opt/wimao]` ; `["/opt/wimao:ro", "~/.local/share/wimao", "xdg-run/pipewire-0"]` → `[/home/u/.vst3]` ; `["/opt"]` couvre `/opt/wimao` ; `["/opt/wim"]` ne couvre pas `/opt/wimao` ; `[]` → les trois.
  - `commande_override_ne_cite_que_les_invisibles` : invisibles `[/opt/wimao, /home/u/.local/share/wimao]` → `flatpak override --user --filesystem=/opt/wimao:ro --filesystem='~/.local/share/wimao' com.bitwig.BitwigStudio` (forme exacte de citation à aligner sur l'existant, mais `~` doit rester non développé par le shell : le citer en `--filesystem=~/…` sans guillemets autour du `~` est attendu ; choisir et tester une forme que `sh -c` passe telle quelle à flatpak).
  - Diagnostic avec faux `flatpak` (liste Bitwig, permissions `filesystems=home;`) → un `FlatpakSansAcces` pour Bitwig avec `/opt/wimao` ; avec `filesystems=host;` → aucun ; sans `flatpak` → aucun.
  - Texte traduit (fr) du problème : contient le nom de l'identifiant et la commande.
- [ ] **Step 2:** tests en échec, **Step 3:** implémenter + traductions fr/es, **Step 4:** vérifications globales, **Step 5:** commit `Diagnostic : permissions des logiciels de musique Flatpak`.

### Task 3: Raccourcis `.lnk` et liste des applications

**Files:**
- Create: `crates/wimao-recettes/src/applications.rs`
- Create: `crates/wimao-recettes/tests/raccourcis/` (au moins deux `.lnk` réels faits par Wine, sans donnée personnelle : vérifier avec `strings`)
- Modify: `crates/wimao-recettes/src/lib.rs`

**Interfaces:**
- Produces:
  - `cible_raccourci(octets: &[u8]) -> Option<String>` : MS-SHLLINK ; en-tête 76 octets (`HeaderSize` = 0x4C et CLSID `00021401-0000-0000-C000-000000000046` vérifiés), `LinkFlags` bit 0 `HasLinkTargetIDList` → sauter `2 + IDListSize`, bit 1 `HasLinkInfo` sinon `None` ; dans `LinkInfo` : si `LinkInfoHeaderSize >= 0x24`, `LocalBasePathOffsetUnicode` (UTF-16LE terminé par 0) prioritaire, sinon `LocalBasePathOffset` (ANSI terminé par 0, décodé en Windows-1252 → on accepte l'ASCII et remplace le reste par `char::REPLACEMENT_CHARACTER`) ; tout dépassement → `None`. Taille max lue : `TAILLE_MAX_RACCOURCI = 1 << 20`.
  - `releve_raccourcis(c: &Chemins) -> BTreeSet<PathBuf>` : `.lnk` (extension insensible à la casse) sous `drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs` et `drive_c/users/<utilisateur_windows>/AppData/Roaming/Microsoft/Windows/Start Menu/Programs` ; chemins **relatifs au préfixe** ; dossiers absents ignorés.
  - `struct Application { nom: String, programme: String, raccourci: PathBuf, ajoutee: String }` (serde, `deny_unknown_fields`) ; fichier : `[[application]]`.
  - `nouvelles_applications(c: &Chemins, avant: &BTreeSet<PathBuf>, apres: &BTreeSet<PathBuf>, programmes_connus: &[String]) -> Vec<Application>` : filtres de la spec § 3.1 (hors la condition « aucun plugin », vérifiée par l'appelant) ; `programmes_connus` = chemins Windows des programmes des recettes de gestionnaire (comparaison insensible à la casse, `/` et `\` équivalents). Vérification d'existence par conversion du chemin Windows en chemin du préfixe (réutiliser `chemins_windows`).
  - `lire_applications(c: &Chemins) -> Vec<Application>` (absent → vide ; illisible → journal + vide) ; `enregistrer_applications(c: &Chemins, liste: &[Application]) -> Resultat<()>` (atomique) ; `ajouter_applications(c, nouvelles) -> Resultat<()>` (pas de doublon de `programme`) ; `retirer_application(c, nom) -> Resultat<bool>`.
  - `applications_presentes(c: &Chemins) -> Vec<Application>` : seulement celles dont le programme existe, triées par nom (insensible à la casse).
  - `Application::vers_recette(&self) -> Recette` : `type_ = Gestionnaire`, `nom`, `editeur` vide, section gestionnaire `{ programme, arguments: [], pilote: [], fermer: [<nom de l'exe>] }`, le reste par défaut. Si `Recette`/`Gestionnaire` n'ont pas de constructeur, en ajouter un `pub(crate)` minimal plutôt que de rendre les champs privés publics.

- [ ] **Step 1: Tests** :
  - `cible_des_raccourcis_de_wine` : chaque `.lnk` de test → chemin attendu (`C:\…\*.exe`, relevé avec `strings`).
  - `cible_unicode_prioritaire` et `cible_ansi` : tampons construits dans le test.
  - `raccourci_abime_rend_none` : vide, 10 octets, CLSID faux, chaque troncature d'un vrai `.lnk` (toutes les longueurs de 0 à n), offsets pointant au-delà du tampon, `> 1 Mo` → `None`, aucune panique.
  - `nouvelles_ignore_desinstalleurs_demarrage_absents_connus` : six raccourcis nouveaux (`Uninstall X.lnk`, `Désinstaller Y.lnk`, cible `unins000.exe`, `StartUp/Z.lnk`, cible inexistante, cible = programme connu) + un bon → seul le bon ; un raccourci déjà dans `avant` → ignoré.
  - `applications_fichier_aller_retour`, `fichier_absent_vide`, `fichier_illisible_vide_et_conserve` (le contenu illisible n'est pas écrasé par la lecture), `pas_de_doublon`, `retirer`.
  - `programme_disparu_masque_mais_garde` : programme supprimé → absent de `applications_presentes`, présent dans `lire_applications`.
  - `vers_recette_ferme_le_bon_exe` : `noms_a_fermer()` = `["BLEASS Plugins Installer.exe"]`.
- [ ] **Step 2:** échec ; **Step 3:** implémenter ; **Step 4:** vérifications globales ; **Step 5:** commit `Applications : lecture des raccourcis et liste enregistrée`.

### Task 4: Détection des applications pendant une installation

**Files:**
- Modify: `crates/wimao-recettes/src/installation.rs` (relevé avant/après, champ du rapport), éventuellement `apres_installation.rs`
- Test: dans `installation.rs` ou `crates/wimao-recettes/tests/`, sur le modèle des tests d'installation existants (faux Wine/faux installeur).

**Interfaces:**
- Consumes: tâche 3.
- Produces: `RapportInstallation::applications: Vec<String>` (noms enregistrés). Règle : raccourcis relevés juste avant de lancer l'installeur et juste après ; si `plugins` est vide **et** `nouvelles_applications` non vide, elles sont ajoutées au fichier, `applications` est rempli et `interactif_necessaire = false`. Si des plugins ont été ajoutés, aucun relevé n'est retenu. Recette de type gestionnaire : rien (le gestionnaire est déjà listé). Échec d'écriture du fichier → journal, installation pas en échec.

- [ ] **Step 1: Tests** :
  - `lanceur_sans_plugin_devient_application` : faux installeur qui crée `ProgramData/.../Programs/BLEASS/BLEASS Plugins Installer.lnk` (copie d'un `.lnk` de test réécrit vers une cible créée par le faux installeur, ou tampon construit) et l'exe cible → `applications == ["BLEASS Plugins Installer"]`, `interactif_necessaire == false`, `applications.toml` contient la ligne.
  - `installeur_de_plugin_ne_cree_pas_d_application` : faux installeur qui crée un plugin VST3 **et** un raccourci autonome + un désinstalleur → `applications` vide, fichier absent.
  - `sans_raccourci_reste_interactif_necessaire` : comportement actuel inchangé.
- [ ] **Step 2–4:** échec, implémenter, vérifications globales ; **Step 5:** commit `Installation : un lanceur sans plugin devient une application`.

### Task 5: CLI `wimao-cli application`

**Files:**
- Modify: `crates/wimao-cli/src/cli.rs`, le fichier qui exécute les commandes (`main.rs` ou équivalent), `po/*.po`, et la page de manuel / complétions si elles sont générées depuis `cli.rs` (suivre `gestionnaire`).

**Interfaces:**
- Consumes: tâche 3 (`applications_presentes`, `retirer_application`, `vers_recette`) et `gestionnaire::ouvrir` comme `gestionnaire ouvrir`.
- Produces: `wimao-cli application lister` (une ligne par application présente : `nom — programme`, ou « No application added » traduit), `application ouvrir <NAME>` (même déroulé et mêmes messages que `gestionnaire ouvrir`, nom introuvable → erreur `Introuvable` et code de sortie non nul), `application retirer <NAME>` (message « removed from the list; nothing was uninstalled » traduit).

- [ ] **Step 1: Tests** sur le modèle des tests CLI existants : `lister` vide et avec une application ; `retirer` d'un nom inconnu → code non nul ; analyse des arguments (`application ouvrir "BLEASS Plugins Installer"`).
- [ ] **Step 2–4**, **Step 5:** commit `CLI : commande application`.

### Task 6: Interface — applications dans la section Gestionnaires

**Files:**
- Modify: `crates/wimao/src/gestionnaires.rs`, `crates/wimao/src/operations.rs`, `crates/wimao/src/fenetre/mod.rs`, `crates/wimao/src/session.rs` (ouverture par recette en mémoire), message de fin d'installation (là où `interactif_necessaire` est lu), `po/*.po`.

**Interfaces:**
- Consumes: tâches 3 et 4.
- Produces:
  - `LigneGestionnaire` gagne `application: bool` ; `operations::lignes_gestionnaires` ajoute après les recettes (elles-mêmes triées) une ligne par `applications_presentes` : `editeur` vide, `installe = true`, `telechargeable = false`, `application = true`. Sous-titre : « Added application · Installed » (traduit).
  - Action `win.ouvrir-application` (paramètre : nom), constante `ACTION_OUVRIR_APPLICATION = "ouvrir-application"` ; elle ouvre la même session que `ouvrir-gestionnaire` avec `Application::vers_recette()`. Coupée pendant un travail comme les autres actions.
  - Action `win.retirer-application` + bouton menu (icône `view-more-symbolic`) sur la ligne d'une application : dialogue de confirmation « Remove “{name}” from the list? The program stays installed. » (Annuler / Retirer, style destructif), puis rafraîchit la section.
  - Fin d'installation avec `rapport.applications` non vide : toast/message « {name} installed: open it from the Managers section » (une ligne par nom), au lieu de la proposition « Ouvrir l'installeur ».
- [ ] **Step 1: Tests** (données pures, sans écran, comme `lignes_gestionnaires_etat_installe`) : `lignes_avec_applications_apres_les_recettes` ; `application_absente_non_listee` ; `sous_titre_application`.
- [ ] **Step 2–4:** échec, implémenter, vérifications globales ; lancer `cargo run -p wimao` avec un `applications.toml` de test pointant vers un exe existant du préfixe de développement et vérifier visuellement la ligne et le menu (capture dans le rapport de tâche).
- [ ] **Step 5:** commit `Interface : applications ajoutées dans la section Gestionnaires`.

### Task 7: Documentation

**Files:**
- Modify: `README.md` (réécrit), `crates/wimao/aide/{fr,en,es}/03-gestionnaires.md`, `crates/wimao/aide/{fr,en,es}/04-logiciel-de-musique.md`, `docs/ameliorations-reportees.md`, `ARCHITECTURE.md` (deux nouveaux modules)
- Create: `docs/installation-detaillee.md`

**Interfaces:** aucune.

- [ ] **Step 1:** déplacer **tel quel** dans `docs/installation-detaillee.md` tout le contenu du README actuel qui n'est pas repris (paquets, vérification manuelle `sha256sum -c --ignore-missing`, signatures, Debian 13/Carla, données, retrait des dépôts, Fedora « skipped OpenPGP checks », compilation) ; vérifier avec un `diff` de mots que rien n'est perdu.
- [ ] **Step 2:** README court, dans cet ordre : titre + 3 lignes (ce que fait WiMao, bêta) ; logo/capture existants ; **Crédits** (yabridge — Robbert van der Helm, GPL-3.0, https://github.com/robbert-vdh/yabridge ; Wine — LGPL-2.1, https://www.winehq.org ; DXVK — zlib, https://github.com/doitsujin/dxvk ; Carla — falkTX, GPL-2.0, https://github.com/falkTX/Carla ; vérifier chaque licence dans `paquets/*/copyright`) ; Installation Ubuntu/Debian et Fedora (commandes seules, reprises à l'identique du README actuel) ; Premiers pas (≤ 5 lignes, dont « dans votre logiciel de musique, ajoutez `~/.vst3`, `~/.clap`, `~/.vst/yabridge` ») ; liens (installation détaillée, mode d'emploi, SECURITY.md, CONTRIBUTING.md, licence). Cible : ≤ 90 lignes.
- [ ] **Step 3:** mode d'emploi fr/en/es : section « Autres applications » (gestionnaires) et « Logiciel de musique en Flatpak » (lien automatique, ce que dit « Diagnostiquer », Bitwig qui garde un échec en cache : redémarrer Bitwig ; logiciel absent de la liste : `ln -s /opt/wimao/yabridge ~/.var/app/<id>/data/yabridge`). Les tests existants de l'aide (liens `aide:` valides, mêmes sections dans les trois langues) passent.
- [ ] **Step 4:** `docs/ameliorations-reportees.md` : limites du § 7 de la spec.
- [ ] **Step 5:** vérifications globales ; commit `Docs : README court avec crédits, aide Flatpak et applications`.
