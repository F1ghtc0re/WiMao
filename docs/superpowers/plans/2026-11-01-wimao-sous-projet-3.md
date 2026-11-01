# WiMao sous-projet 3 — Plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** une interface complète : assistant de premier lancement, import de `~/.wine-mao`,
historique avec retour en arrière, restauration d'une sauvegarde, mode d'emploi intégré (fr, en,
es), défauts reportés corrigés et finitions visuelles.

**Architecture :**
- Le moteur (`wimao-core`) gagne deux modules :
  - `historique.rs`, un journal JSON Lines ;
  - `import.rs`, qui déplace le dossier puis le remplace par un lien.
- Les défauts reportés sont corrigés là où ils naissent : `restauration`, `inventaire`,
  `pontage`, `correctifs/dll`, `outils`, `environnement`.
- L'interface (`wimao`) gagne quatre fichiers : `assistant.rs`, `aide.rs`, `historique_vue.rs` et
  `restaurer.rs`. La logique de chaque écran est pure et testée sans GTK.
- `wimao-cli` reçoit les commandes équivalentes.

**Tech Stack :** Rust (workspace existant), GTK4 4.22 / libadwaita 1.9, Wine (WoW64),
yabridge 5.1.1, gettext (`.po`), `serde_json` (seule dépendance ajoutée).

**Spec :** `docs/superpowers/specs/2026-11-01-wimao-sous-projet-3-design.md`. La conception
d'ensemble `docs/superpowers/specs/2026-10-31-wimao-design.md` reste la référence.

## Global Constraints

- **Code et commentaires** :
  - en **français**, détaillés et utiles : n'importe quel développeur doit s'y retrouver ;
  - identifiants en français, comme dans le code existant ;
  - chaque nouveau module commence par un en-tête `//!` : rôle, ce qu'il utilise, qui l'utilise,
    choix non évidents.
- **Textes affichés** :
  - `msgid` **anglais** via `tr`/`trf`/`trn` (`wimao_core::i18n`), traduits en `fr` et `es` dans
    `po/`, avec vouvoiement en français ;
  - fichiers sources ajoutés à `po/POTFILES` ; `scripts/verifier-traductions.sh` doit passer ;
  - aucune erreur en texte libre français : ajouter des variantes traduites à `Erreur`.
- **Mode d'emploi** : ce sont des fichiers Markdown par langue, sans gettext (spec 9.2).
- **Isolation des tests** : ils n'écrivent **jamais** hors d'un dossier temporaire
  (`Chemins::pour_test`, avec HOME/XDG de test). Ne jamais toucher à :
  - `~/.wine-mao`, `~/.config/yabridgectl`, `~/.vst3/yabridge`, `~/.vst/yabridge` et
    `~/.clap/yabridge` ;
  - `/opt/wine-mao-v2` ;
  - les vrais `~/.local/share/applications` et `~/.config/mimeapps.list`.
- **Tests avec Wine** : ils portent `#[ignore = "Wine requis"]` et sont lancés en CI par
  `--ignored`.
- **Avant chaque commit**, ces trois commandes passent :
  - `cargo fmt --check` ;
  - `cargo clippy --workspace --all-targets -- -D warnings` ;
  - `RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps`.
- **Dépendances** : la seule crate ajoutée est `serde_json` (tâche 1). Toute autre exception est
  justifiée dans le rapport de la tâche.
- **Commits** : terminés par la ligne `Co-Authored-By:` du modèle qui les écrit.
- **Documentation** : mettre à jour `ARCHITECTURE.md` (et `CONTRIBUTING.md` si concerné) dans la
  tâche qui change le comportement décrit.
- **Plugins en cours** : on refuse d'arrêter Wine si un plugin tourne (`Erreur::PluginsEnCours`).
  C'est le Ruling 9 du sous-projet 1.
- **Écriture de l'historique** : un échec d'écriture est noté au journal technique
  (`journal::noter`) et ne fait **jamais** échouer l'action (spec 6.2).

## Review Focus

1. **Import interrompu entre le déplacement et le lien.** Le dossier doit revenir à sa place et
   `~/.wine-mao` doit être intact. Test : tâche 5,
   `echec_du_lien_remet_le_dossier_en_place`.
2. **Liens cassés d'une autre installation**, par exemple une cible sous `/opt` ou un autre
   préfixe : ils ne sont jamais retirés. Test : tâche 3,
   `lien_casse_hors_du_prefixe_est_garde`.
3. **Dernière ligne de `historique.jsonl` tronquée** par une coupure de courant : les autres
   entrées s'affichent quand même. Test : tâche 1, `ligne_abimee_est_ignoree`.
4. **Retour en arrière vers un point d'avant une restauration de sauvegarde** : il doit être
   refusé avant de toucher à quoi que ce soit. Test : tâche 3,
   `revenir_a_un_point_perime_est_refuse_sans_rien_toucher`.
5. **Annulation de l'import après que l'utilisateur a remplacé `~/.wine-mao`** (par un vrai
   dossier, par exemple) : refus, et rien n'est déplacé. Test : tâche 5,
   `annuler_refuse_si_le_lien_a_ete_remplace`.

---

### Task 1 : `historique.rs` — journal des actions

**Files :**
- Create : `crates/wimao-core/src/historique.rs`
- Modify : `crates/wimao-core/src/lib.rs`, `crates/wimao-core/Cargo.toml` (`serde_json = "1"`),
  `crates/wimao-core/src/outils.rs` (si la date ISO y est ajoutée).

**Interfaces :**
- Produces :
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "kebab-case")]
  pub enum Action { Installation, Gestionnaire, Reparation, Retour, Sauvegarde, Restauration, Import, AnnulationImport }
  #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "kebab-case")]
  pub enum ResultatAction { Reussi, Echec, Annule }
  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
  pub struct Entree {
      pub date: String,              // "2026-11-01T14:03:12+01:00" (heure locale, décalage)
      pub action: Action,
      pub sujet: String,
      pub resultat: ResultatAction,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub point: Option<String>,
      #[serde(default, skip_serializing_if = "String::is_empty")] pub details: String,
  }
  pub const FICHIER_HISTORIQUE: &str = "historique.jsonl"; // dans c.donnees
  pub fn ajouter(c: &Chemins, action: Action, sujet: &str, resultat: ResultatAction, point: Option<&str>, details: &str);
  pub fn lire(c: &Chemins) -> Resultat<Vec<Entree>>;   // plus récente d'abord
  pub fn date_locale_iso() -> String;                    // date de l'instant présent au format ci-dessus
  ```
  `ajouter` ne renvoie rien : il écrit une ligne avec `OpenOptions::append`, puis `sync_data`.
  Une erreur est notée par `journal::noter("historique", …)`. `lire` renvoie un vecteur vide si
  le fichier est absent. Une ligne illisible est ignorée et notée au journal.

- [ ] **Step 1 : tests** dans `historique.rs`, avec `Chemins::pour_test` sur un `tempdir` :
  - `ajout_puis_lecture_plus_recente_d_abord` : deux `ajouter`, puis `lire` renvoie 2 entrées,
    `[0].sujet` est le second sujet ;
  - `fichier_absent_donne_une_liste_vide` ;
  - `ligne_abimee_est_ignoree` : écrire une ligne valide, puis `{"date":"2026` sans fin de
    ligne ; `lire` renvoie 1 entrée ;
  - `format_d_une_ligne` : après `ajouter(c, Action::AnnulationImport, "x", ResultatAction::Reussi, None, "")`,
    la ligne contient `"action":"annulation-import"` et `"resultat":"reussi"`, sans clé `point`
    ni `details` ;
  - `date_iso_a_le_bon_format` : correspond à `^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d$`.
- [ ] **Step 2 :** `cargo test -p wimao-core historique` échoue (module absent).
- [ ] **Step 3 :** implémenter. Le décalage vient de `tm_gmtoff` (`outils::heure_locale`).
- [ ] **Step 4 :** `cargo test -p wimao-core historique` passe.
- [ ] **Step 5 :** commit `WiMao : historique des actions (historique.jsonl)`.

### Task 2 : petits défauts du moteur — ctime, DLL atomique, dossier de travail, `wineboot -u`

**Files :**
- Modify :
  - `crates/wimao-core/src/inventaire.rs` (`Signature`) ;
  - `crates/wimao-core/src/correctifs/dll.rs` (`remplacer_fichier`) ;
  - `crates/wimao-core/src/outils.rs` (`erreur_de_lancement`) ;
  - `crates/wimao-core/src/environnement.rs` ;
  - `crates/wimao-recettes/src/installation.rs` (`installer_avec_delai`) ;
  - `crates/wimao-core/src/erreur.rs` si une variante est nécessaire ;
  - `ARCHITECTURE.md`.

**Interfaces :**
- Produces :
  - `Signature { modifie, taille, pub change: Option<SystemTime> }` : la plus grande ctime des
    fichiers du binaire (`MetadataExt::ctime` et `ctime_nsec`).
  - `pub fn mettre_a_jour(c: &Chemins, p: &dyn Progression) -> Resultat<()>` dans
    `environnement.rs` : lance `wineboot -u` (même délai que la création), puis
    `remplacer_dossiers_relies(c)`.
- Comportements :
  - `remplacer_fichier(source, dest)` :
    1. supprime `<dest>.part` s'il existe ;
    2. copie vers `<dest>.part`, puis `sync_all` ;
    3. renomme vers `dest`. `rename` remplace un lien sans suivre sa cible.
  - `erreur_de_lancement` : si la commande a un dossier de travail (`Command::get_current_dir`)
    qui n'existe pas, renvoie `Erreur::Introuvable(trf("the working folder {folder}", …))` au
    lieu d'accuser le programme.
  - `installer_avec_delai` : appelle `environnement::mettre_a_jour` juste **après** la création du
    point de restauration, avant le relevé `avant`, avec l'étape
    `tr("Updating the Wine environment…")`.

- [ ] **Step 1 : tests.**
  - `inventaire` : `reecriture_meme_taille_meme_date_est_une_modification`. Écrire une DLL VST2
    factice et relever. Attendre 10 ms, réécrire le même nombre d'octets et remettre l'ancienne
    `mtime` (`File::set_modified`). Relever de nouveau : `apres.modifies(&avant)` contient le
    plugin.
  - `correctifs/dll` :
    - `remplacer_un_lien_ne_touche_pas_sa_cible` : `dest` est un lien vers un fichier `ailleurs`.
      Après l'appel, `ailleurs` est inchangé et `dest` est un fichier ordinaire au contenu de
      `source` ;
    - `part_residuel_est_supprime` : un `<dest>.part` préexistant n'existe plus après l'appel ;
    - `source_absente_laisse_la_cible_intacte` : erreur, et `dest` garde son contenu.
  - `outils` : `dossier_de_travail_absent_est_nomme`. `lancer` avec `current_dir` inexistant donne
    une `Erreur::Introuvable` dont le texte contient le chemin du dossier.
  - `environnement` : `mettre_a_jour_retire_les_dossiers_relies`, avec
    `#[ignore = "Wine requis"]`. Créer l'environnement, relier `Documents` vers un dossier du HOME
    de test, appeler `mettre_a_jour`. Ensuite, `dossiers_relies(c)` est vide.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test --workspace` passe, et `cargo test -p wimao-core -- --ignored mettre_a_jour` passe sur la machine (Wine présent).
- [ ] **Step 5 :** commit `WiMao : ctime dans la signature, DLL écrites atomiquement, dossier de travail nommé, wineboot avant l'installeur`.

### Task 3 : restauration — files séparées, points périmés, liens cassés

**Files :**
- Modify :
  - `crates/wimao-core/src/restauration.rs` ;
  - `crates/wimao-core/src/sauvegarde.rs` (`restaurer`) ;
  - `crates/wimao-core/src/pontage.rs` ;
  - `crates/wimao-core/src/erreur.rs` ;
  - les appelants de `revenir` dans `crates/wimao-cli/src/commandes/restauration.rs` et
    `crates/wimao/src/` ;
  - `ARCHITECTURE.md`.

**Interfaces :**
- Produces :
  ```rust
  // restauration.rs
  const POINTS_CONSERVES: usize = 10;            // file « installation » (installations et sessions)
  const POINTS_REPARATION_CONSERVES: usize = 3;  // file « repair »
  const FICHIER_PERIME: &str = "perime";
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum FilePoints { Installation, Reparation }
  impl PointRestauration { pub fn file(&self) -> FilePoints } // Reparation si motif == MOTIF_REPARATION
  pub struct ListePoints { pub points, pub incomplets, pub perimes: Vec<PointRestauration> }
  pub fn marquer_perimes(c: &Chemins) -> Resultat<usize>;
  pub struct BilanRetour { pub fichiers_supprimes: usize, pub liens_retires: usize }
  pub fn revenir(c: &Chemins, point: &PointRestauration, confirmes: &[PathBuf]) -> Resultat<BilanRetour>;
  // pontage.rs
  pub fn racines_du_prefixe(c: &Chemins) -> Vec<PathBuf>; // [c.prefixe] ; la tâche 5 y ajoute l'ancien chemin d'import
  pub fn retirer_liens_casses(c: &Chemins) -> Resultat<usize>;
  // erreur.rs
  Erreur::PointPerime(String) // id du point ; message traduit « this restore point predates the restored backup »
  ```
- Règles :
  - `elaguer` garde les 10 plus récents de la file Installation et les 3 plus récents de la file
    Reparation. Pour chaque file, il compte les points complets **et** les périmés, triés par id.
  - `lister` range dans `perimes` un point complet qui contient un fichier `perime`.
  - `revenir` sur un point périmé renvoie `Erreur::PointPerime` avant toute autre opération.
  - `etat_selon_liens` utilise `racines_du_prefixe` au lieu de `c.prefixe`.
  - `retirer_liens_casses` parcourt `<home>/.vst3/yabridge`, `<home>/.vst/yabridge` et
    `<home>/.clap/yabridge` sans suivre les liens. Un lien est retiré s'il est cassé **et** si sa
    cible commence par une des `racines_du_prefixe`. On retire alors aussi son pendant ponté,
    selon la « Disposition constatée » en tête de `pontage.rs` :
    - **VST3** : le bundle `…/<X>.vst3`, trois niveaux au-dessus du lien, seulement s'il ne
      contient rien d'autre que `Contents/x86_64-linux/*.so`, le lien et des dossiers ;
    - **VST2** : `<rel>.so` ;
    - **CLAP** : le chemin sans le suffixe `-win`.

    Les dossiers vides laissés sous ces trois racines sont supprimés, mais jamais les racines
    elles-mêmes.
  - `revenir` appelle `retirer_liens_casses` après la suppression des fichiers. Une erreur à cette
    étape est notée au journal et donne `liens_retires = 0`.
  - `sauvegarde::restaurer` appelle `marquer_perimes` juste après le renommage réussi.
  - CLI `restauration lister` : la colonne « file » affiche installation ou réparation, et un
    point périmé porte la mention traduite `tr("outdated")`. `revenir` sur un point périmé
    affiche l'erreur.
  - Interface : le texte de fin de retour cite `liens_retires` s'il est non nul (`trn`).

- [ ] **Step 1 : tests** dans `restauration.rs` et `pontage.rs`. Préfixe factice : les trois
  `.reg` et `drive_c/`, comme les tests existants.
  - `onze_points_reparation_ne_chassent_aucun_point_installation` : 2 points installation, puis
    11 `repair`. `lister` donne 2 installation et 3 repair.
  - `points_perimes_sont_listes_a_part` : 2 points, `marquer_perimes` renvoie 2, puis `lister` a
    `points` vide et 2 `perimes`.
  - `revenir_a_un_point_perime_est_refuse_sans_rien_toucher` : créer un fichier après le point,
    marquer, puis `revenir`. L'erreur est `Erreur::PointPerime(_)` et le fichier existe toujours.
  - `lien_casse_dans_le_prefixe_est_retire_avec_son_bundle` : bundle VST3 factice
    `<home>/.vst3/yabridge/Ed/X.vst3/Contents/x86_64-linux/X.so` et lien
    `…/Contents/x86_64-win/X.vst3` vers `<prefixe>/drive_c/…/X.vst3`, qui n'existe pas. Le résultat
    vaut 1 et le bundle n'existe plus.
  - `lien_casse_hors_du_prefixe_est_garde` : même chose avec une cible `/opt/ailleurs/X.vst3`.
    Le résultat vaut 0 et le lien existe toujours.
  - `lien_vst2_casse_retire_le_so` : `<home>/.vst/yabridge/P/X_x64.dll` est un lien cassé vers
    le préfixe et `X_x64.so` existe. Après l'appel, les deux n'existent plus.
  - `bundle_avec_autre_contenu_est_garde` : bundle VST3 avec un fichier en plus. Le lien est
    retiré, le bundle reste.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter, puis adapter la CLI et l'interface à `BilanRetour`.
- [ ] **Step 4 :** `cargo test --workspace` passe.
- [ ] **Step 5 :** commit `WiMao : files de points séparées, points périmés après restauration, liens cassés retirés`.

### Task 4 : historique écrit par le moteur et `wimao-cli historique`

**Files :**
- Modify :
  - `crates/wimao-recettes/src/installation.rs` (`installer_avec_delai`) ;
  - `crates/wimao-recettes/src/gestionnaire.rs` (`Session::fermer`) ;
  - `crates/wimao-core/src/diagnostic.rs` (`reparer`) ;
  - `crates/wimao-core/src/restauration.rs` (`revenir`) ;
  - `crates/wimao-core/src/sauvegarde.rs` (`sauvegarder`, `restaurer`) ;
  - `crates/wimao-cli/src/cli.rs`, `crates/wimao-cli/src/commandes/` (nouveau fichier
    `historique.rs`) ;
  - `crates/wimao-cli/tests/cli.rs`.

**Interfaces :**
- Consumes : `historique::{ajouter, lire, Action, ResultatAction, Entree}` (tâche 1).
- Contenu écrit (sujet, point, details) :

  | Appel | Action | Sujet | Point | Résultat |
  |---|---|---|---|---|
  | `installer_avec_delai`, une fois le point créé | `Installation` | nom de la recette, sinon nom du fichier | id du point | `Reussi` ; `Echec` si une étape suivante renvoie une erreur ; `details` = `trn` du nombre de plugins traités |
  | `Session::fermer` | `Gestionnaire` | nom de la recette | id du point de session | `Reussi` ; `Echec` si erreur |
  | `diagnostic::reparer` | `Reparation` | `tr("Repair")` | id du point `repair` | |
  | `restauration::revenir` | `Retour` | `libelle_motif(&point.motif)` | aucun | `details` = `trn` du nombre de fichiers |
  | `sauvegarde::sauvegarder` | `Sauvegarde` | nom du fichier `.sha256` | aucun | `Reussi` ou `Echec` |
  | `sauvegarde::restaurer` | `Restauration` | nom du fichier `.sha256` | aucun | `Reussi` ou `Echec` |

  Une erreur **avant** la création du point (verrou pris, plugin en cours) n'écrit rien.
- CLI : `wimao-cli historique [--tout]` affiche les 20 dernières entrées (toutes avec `--tout`)
  sur une ligne chacune : `date  résultat  action  sujet  [point]`. Les libellés d'action et de
  résultat sont traduits ; la fonction `libelle_action(Action) -> String` est dans
  `historique.rs` (moteur), pour être partagée avec l'interface.

- [ ] **Step 1 : tests.**
  - `restauration` : `revenir_ajoute_une_entree_retour`.
  - `sauvegarde` : `sauvegarde_reussie_ajoute_une_entree` (le test de sauvegarde existant sur un
    préfixe factice est étendu).
  - CLI (`tests/cli.rs`) : `historique_vide_puis_rempli`. Sans fichier, la commande sort avec le
    code 0 et le texte traduit `tr("No action yet")`. Avec deux lignes écrites à la main dans
    `<donnees>/historique.jsonl`, les deux sujets apparaissent, le plus récent en premier.
  - Intégration Wine (`crates/wimao-recettes/tests/`, existant, `#[ignore = "Wine requis"]`) :
    `installation_ajoute_une_entree_avec_point`. Après l'installation de l'installeur factice,
    `lire` contient une entrée `Installation` `Reussi`, et son `point` est l'id du dernier point
    de `lister`.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test --workspace` passe, et le test `--ignored` ci-dessus passe localement.
- [ ] **Step 5 :** commit `WiMao : le moteur remplit l'historique ; commande historique`.

### Task 5 : `import.rs` — importer et annuler l'import de `~/.wine-mao`

**Files :**
- Create : `crates/wimao-core/src/import.rs`
- Modify :
  - `crates/wimao-core/src/lib.rs`, `crates/wimao-core/src/erreur.rs` ;
  - `crates/wimao-core/src/pontage.rs` (`racines_du_prefixe`) ;
  - `crates/wimao-core/src/processus.rs` (rendre public `programmes_ouverts_sur`) ;
  - `ARCHITECTURE.md`.

**Interfaces :**
- Produces :
  ```rust
  pub const DOSSIER_IMPORT: &str = "import";            // <donnees>/import
  pub const FICHIER_TRACE: &str = "import.toml";        // ancien = "...", date = "...", sauvegarde = "..."
  pub struct EnvironnementExistant { pub chemin: PathBuf, pub taille: u64, pub plugins: usize }
  pub struct Importe { pub ancien: PathBuf, pub sauvegarde: PathBuf }
  pub fn detecter(c: &Chemins) -> Option<EnvironnementExistant>;
  pub fn programmes_ouverts(c: &Chemins) -> Resultat<Vec<String>>;   // programmes Windows sur <home>/.wine-mao
  pub fn importer(c: &Chemins, p: &dyn Progression) -> Resultat<Importe>;
  pub fn ancien_emplacement(c: &Chemins) -> Option<PathBuf>;          // lu dans import.toml
  pub fn annulation_possible(c: &Chemins) -> bool;
  pub fn annuler(c: &Chemins, p: &dyn Progression) -> Resultat<()>;
  // processus.rs
  pub fn programmes_ouverts_sur(prefixe: &Path) -> Resultat<Vec<String>>;
  // erreur.rs (messages traduits)
  Erreur::DejaPresent(PathBuf)                                 // l'environnement WiMao existe déjà
  Erreur::AutreDisque { source: PathBuf, destination: PathBuf } // explique qu'aucune copie n'est faite
  Erreur::PasNotreLien(PathBuf)                                // annulation refusée
  ```
- Déroulé de `importer`. Le cœur est `importer_dans(c, p, meme_disque, creer_lien)`, avec les
  deux fonctions injectées pour les tests.
  1. Vérifications, dans cet ordre :
     - `detecter` (sinon `Introuvable`) ;
     - `c.prefixe` absent (sinon `DejaPresent`) ;
     - verrou d'installation ;
     - plugins en cours **sur l'ancien préfixe** (cloner `Chemins` et y mettre
       `prefixe = <home>/.wine-mao`) ;
     - `meme_disque(ancien, parent de c.prefixe)` (`st_dev` de `MetadataExt::dev`, sinon
       `AutreDisque`).
  2. `Wine::nouveau(&ancien_chemins).arreter()`.
  3. Petite sauvegarde dans `<donnees>/import/sauvegarde-<identifiant local>/` : les trois `.reg`
     et, s'il existe, `<home>/.config/yabridgectl/config.toml` (en **lecture seule**).
  4. `create_dir_all(parent de c.prefixe)`, puis `rename(ancien, c.prefixe)`.
  5. `creer_lien(c.prefixe, ancien)`. En cas d'échec : `rename(c.prefixe, ancien)`, puis
     remonter l'erreur.
  6. Écrire `import.toml` (`ecrire_atomique`), puis `historique::ajouter(Import, "~/.wine-mao",
     Reussi, None, …)`. Une erreur à cette étape est notée au journal sans faire échouer l'import.
- `annuler` :
  1. Vérifications :
     - `import.toml` existe ;
     - `<home>/.wine-mao` est un lien dont la cible égale `c.prefixe` (sinon `PasNotreLien`) ;
     - aucun plugin en cours ;
     - verrou ;
     - `Wine::nouveau(c).arreter()`.
  2. Retirer le lien, puis `rename(c.prefixe, ancien)`, et renommer `import.toml` en
     `import.annule-<identifiant>.toml`.
  3. Lancer `yabridgectl sync` avec la configuration de l'utilisateur, sans `XDG_CONFIG_HOME`
     isolé et **sans écrire** `~/.config/yabridgectl`, si ce fichier existe. Sinon, renvoyer un
     avertissement via `p.etape` qui donne la commande à lancer.
  4. Entrée d'historique `AnnulationImport`.
- `racines_du_prefixe(c)` = `[c.prefixe]`, plus `ancien_emplacement(c)` s'il existe.

- [ ] **Step 1 : tests** dans `import.rs`. HOME de test, faux `<home>/.wine-mao` avec les `.reg`,
  `drive_c/Program Files/Common Files/VST3/A.vst3/`, et faux
  `<home>/.config/yabridgectl/config.toml`.
  - `detecte_un_vrai_dossier` : `plugins == 1` et `taille > 0`.
  - `ignore_un_lien_ou_un_dossier_incomplet` : deux cas, `None`.
  - `ignore_si_le_prefixe_wimao_existe`.
  - `import_complet` : après `importer`, `c.prefixe/system.reg` existe, `<home>/.wine-mao` est un
    lien vers `c.prefixe`, la sauvegarde contient les 3 `.reg` et `config.toml`, et
    `<home>/.config/yabridgectl/config.toml` garde le même contenu et la même `mtime`.
    `historique::lire` contient une entrée `Import`.
  - `autre_disque_est_refuse_sans_rien_toucher` : avec `meme_disque` qui renvoie `false`, on
    obtient `AutreDisque`, `<home>/.wine-mao` reste un dossier et `c.prefixe` est absent.
  - `echec_du_lien_remet_le_dossier_en_place` : `creer_lien` renvoie une erreur ; elle est
    remontée, `<home>/.wine-mao` est un dossier avec `system.reg` et `c.prefixe` est absent.
  - `annuler_remet_tout_en_place` : après `importer` puis `annuler`, `<home>/.wine-mao` est un
    dossier, `c.prefixe` est absent, `import.annule-*.toml` existe et `annulation_possible` est
    faux.
  - `annuler_refuse_si_le_lien_a_ete_remplace` : après l'import, remplacer le lien par un dossier
    vide. On obtient `PasNotreLien`, et `c.prefixe` existe toujours.
  - Dans `pontage.rs` : `lien_vers_l_ancien_emplacement_est_le_notre`. Après un import, un lien de
    `~/.vst3/yabridge` vers `<home>/.wine-mao/drive_c/…` donne `EtatPontage::Ponte` et pas
    `DejaPonteAilleurs`.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter. Pour l'arrêt de Wine sur un faux préfixe sans Wine, suivre la façon
  dont les tests existants de `restauration::creer` s'en accommodent.
- [ ] **Step 4 :** `cargo test --workspace` passe.
- [ ] **Step 5 :** commit `WiMao : import de ~/.wine-mao (déplacement + lien) et annulation`.

### Task 6 : `wimao-cli` — import et question avant d'arrêter un programme ouvert

**Files :**
- Modify : `crates/wimao-cli/src/cli.rs`, `crates/wimao-cli/src/commandes/` (nouveau
  `import.rs`, plus `installation.rs`, `gestionnaires.rs`, `sauvegarde.rs`),
  `crates/wimao-cli/src/confirmation.rs`, `crates/wimao-cli/tests/cli.rs`, `README.md` (section CLI).

**Interfaces :**
- Consumes : `import::*` (tâche 5), `processus::programmes_ouverts(c)` (existant).
- Commandes :
  - `wimao-cli importer [--oui]` : affiche ce qui sera fait (chemin, taille lisible, nombre de
    plugins, destination, « un lien sera laissé »). Ensuite, il nomme les programmes ouverts
    (`import::programmes_ouverts`) et demande confirmation, sauf avec `--oui`.
  - `wimao-cli importer --annuler [--oui]`.
- Question avant d'arrêter un programme ouvert : `installer`, `gestionnaire ouvrir` et
  `sauvegarder` gagnent `--oui`. Si `processus::programmes_ouverts(c)` n'est pas vide, la commande
  les nomme et demande l'accord, avec le même texte et le même mécanisme que `reparer`
  (`confirmation.rs`). Un refus termine la commande avec le code 0 et le message traduit
  `tr("Cancelled: nothing was changed.")`.

- [ ] **Step 1 : tests CLI** (HOME, XDG et `WIMAO_DONNEES` temporaires) :
  - `importer_sans_environnement_existant` : code non nul, message `Introuvable` traduit ;
  - `importer_oui_puis_annuler_oui` : faux `<home>/.wine-mao`. Après `importer --oui`, le lien
    existe ; après `importer --annuler --oui`, c'est de nouveau un dossier ;
  - `importer_refus_ne_change_rien` : stdin `n\n`. Le code vaut 0 et `<home>/.wine-mao` reste un
    dossier.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test -p wimao-cli` passe.
- [ ] **Step 5 :** commit `WiMao CLI : importer, annuler l'import, question avant d'arrêter un programme ouvert`.

### Task 7 : interface — découpage de `fenetre.rs`, navigation, petits défauts

**Files :**
- Create : `crates/wimao/src/fenetre/entete.rs` (barre de titre, menu), `crates/wimao/src/fenetre/actions.rs` (`creer_actions`, `action`, `action_nommee`, `action_langue`, `changer_langue`).
- Move : `crates/wimao/src/fenetre.rs` → `crates/wimao/src/fenetre/mod.rs`
- Modify : `crates/wimao/src/main.rs`, `crates/wimao/src/dialogues.rs`, `crates/wimao/src/reglages.rs`, `crates/wimao/src/etat.rs` (commentaire `//!`), `crates/wimao/src/fenetre/mod.rs`, `ARCHITECTURE.md`.

**Interfaces :**
- Produces :
  - `Fenetre` gagne `pub(crate) navigation: adw::NavigationView`. La page racine, d'étiquette
    `"principale"`, contient le `ToolbarView` actuel. Le contenu de la fenêtre devient
    `ToastOverlay(NavigationView)`. Les tâches 9 et 10 y poussent leurs pages avec
    `pub(crate) fn pousser(&self, page: &adw::NavigationPage)`.
  - `pub(crate) fn avant_d_arreter_wine(&self, puis: impl FnOnce() + 'static)` : si
    `processus::programmes_ouverts` n'est pas vide, appelle `dialogues::confirmer_fermeture_programmes`
    (existant) puis `puis` seulement en cas d'accord. Sinon, appelle `puis` tout de suite.
    « Ajouter un installeur » (sélecteur, dépôt), l'ouverture d'un gestionnaire et
    « Sauvegarder » passent désormais par là.
  - `Reglages::charger(config) -> (Reglages, Option<String>)`. Un fichier abîmé est renommé en
    `reglages.toml.abime-<identifiant>` et l'avertissement traduit est rendu. `main.rs` le garde,
    puis la fenêtre l'affiche en toast à l'ouverture.
- Corrections :
  - un seul dialogue de fermeture à la fois : un champ `dialogue_fermeture: Rc<RefCell<Option<adw::AlertDialog>>>` ; un nouveau clic sur la croix appelle `present` sur le dialogue ouvert ;
  - premier relevé en échec : page `PAGE_ERREUR` (`AdwStatusPage`, icône `dialog-error-symbolic`) avec « Réessayer » (relance `rafraichir`) et « Copier le rapport » ;
  - `clone` en double de `main.rs` retiré ; commentaire `//!` d'`etat.rs` corrigé.
- Le comportement visible reste inchangé, en dehors des points ci-dessus.

- [ ] **Step 1 : tests.**
  - `reglages` : `fichier_abime_est_mis_de_cote_avec_un_avertissement`. Écrire `langue = [` ;
    `charger` renvoie les réglages par défaut, `Some(_)`, et un fichier `reglages.toml.abime-*`
    existe.
- [ ] **Step 2 :** le test échoue.
- [ ] **Step 3 :** découper, puis implémenter. `cargo build -p wimao` après chaque déplacement.
- [ ] **Step 4 :** `cargo test --workspace` passe. Lancer `cargo run -p wimao` avec un HOME,
  des XDG et un `WIMAO_DONNEES` temporaires, et vérifier à l'œil que la fenêtre s'ouvre comme
  avant.
- [ ] **Step 5 :** commit `WiMao : fenetre.rs découpé, navigation, question avant d'arrêter Wine, petits défauts de l'interface`.

### Task 8 : mode d'emploi — convertisseur et fenêtre

**Files :**
- Create : `crates/wimao/src/aide.rs` (logique pure), `crates/wimao/src/aide_fenetre.rs` (GTK), `crates/wimao/aide/{fr,en,es}/01-premiers-pas.md` … `09-problemes-frequents.md` (vides sauf un titre ; les textes arrivent à la tâche 9)
- Modify : `crates/wimao/src/fenetre/actions.rs` (F1, action `win.aide`), `crates/wimao/src/fenetre/entete.rs` (menu « Mode d'emploi »), `crates/wimao/src/dialogues.rs` (lien « Problèmes fréquents » dans `afficher_erreur`).

**Interfaces :**
- Produces (`aide.rs`) :
  ```rust
  pub struct Chapitre { pub cle: &'static str, pub titre_msgid: &'static str, pub textes: [(&'static str, &'static str); 3] } // (langue, include_str!)
  pub const CHAPITRES: [Chapitre; 9]; // clés : premiers-pas, ajouter-un-plugin, gestionnaires, bitwig, reparer, historique, sauvegarde, import, problemes
  pub fn texte(cle: &str, langue: &str) -> Option<&'static str>; // repli sur "fr" si vide ou langue inconnue
  pub enum Bloc { Titre(u8, String), Paragraphe(String), Liste { numerotee: bool, elements: Vec<String> } }
  pub fn convertir(markdown: &str) -> Vec<Bloc>; // Paragraphe et éléments déjà en balisage Pango
  pub enum Lien { Web(String), Chapitre(String) }
  pub fn lien(uri: &str) -> Option<Lien>;        // "https://…" ou "aide:<clé>" ; autre schéma : None
  ```
- Règles de `convertir` (spec 9.2) :
  - lignes `# ` et `## ` → `Titre(1|2)` ;
  - lignes `- ` → éléments d'une liste non numérotée ; `N. ` → liste numérotée ;
  - lignes consécutives → un `Paragraphe`, séparé par une ligne vide ;
  - `**x**` → `<b>x</b>`, `` `x` `` → `<tt>x</tt>`, `[t](u)` → `<a href="u">t</a>` ;
  - **tout** autre texte est échappé (`glib::markup_escape_text`, ou une fonction pure équivalente
    testée) ;
  - tout autre élément Markdown est rendu en texte brut, échappé.
- Fenêtre : `adw::Window` avec un `AdwNavigationSplitView`. La barre latérale est une
  `GtkListBox` des titres traduits ; le contenu est un `ScrolledWindow > AdwClamp(720)` de
  `GtkLabel` (`wrap`, `use_markup`, `selectable`), avec des classes `title-1`, `title-2` et
  `body`. `activate-link` : `Lien::Chapitre` sélectionne le chapitre, `Lien::Web` est ouvert par
  `gtk4::UriLauncher`. Une seule fenêtre d'aide à la fois : un nouveau F1 la présente. F1 ouvre
  l'aide, et « À propos » n'a plus de raccourci.
- `pub fn ouvrir_aide(parent: &impl IsA<gtk4::Window>, chapitre: Option<&str>)` dans
  `aide_fenetre.rs`.

- [ ] **Step 1 : tests** dans `aide.rs` :
  - `titres_listes_et_paragraphes` : un texte avec `# A`, un paragraphe de deux lignes, `- x`,
    `- y`, `1. z`. On obtient `[Titre(1,"A"), Paragraphe(..), Liste{false,[x,y]}, Liste{true,[z]}]` ;
  - `gras_code_et_liens` : `**a** `b` [t](aide:import)` donne
    `<b>a</b> <tt>b</tt> <a href="aide:import">t</a>` ;
  - `texte_echappe` : `a < b & c` donne `a &lt; b &amp; c` ;
  - `element_inconnu_en_texte_brut` : `> citation` et `| a | b |` restent, échappés ;
  - `lien_web_chapitre_et_inconnu` : `https://x`, `aide:import` et `file:///etc` donnent
    respectivement `Web`, `Chapitre` et `None` ;
  - `repli_sur_le_francais` : `texte("import","de")` égale `texte("import","fr")` ;
  - `chaque_chapitre_existe_dans_les_trois_langues` : pour chaque `CHAPITRES`, les trois textes
    commencent par `# `.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test -p wimao aide` passe. Vérifier à l'œil que F1 ouvre la fenêtre et
  que la navigation entre chapitres marche (HOME temporaire).
- [ ] **Step 5 :** commit `WiMao : mode d'emploi intégré (convertisseur Markdown, fenêtre, F1)`.

### Task 9 : textes du mode d'emploi (fr, en, es)

**Files :**
- Modify : les 27 fichiers `crates/wimao/aide/{fr,en,es}/*.md`.

Le contenu suit la spec 9.1 et ce que fait réellement le programme : relire `README.md`,
`NOTES.md` (vérification réelle des sous-projets 2 et 3 quand elle existe), la spec et les textes de
l'interface (`po/fr.po`). Les noms de boutons cités sont exactement ceux de l'interface traduite.

- **Ton** : phrases courtes, vouvoiement, pas de jargon. « Environnement Wine » est expliqué une
  fois, dans « Premiers pas ».
- **Liens entre chapitres** : `aide:<clé>`.
- **« Utiliser les plugins dans Bitwig »** : dossiers `~/.vst3/yabridge`, `~/.vst/yabridge` et
  `~/.clap/yabridge` à ajouter dans les réglages de Bitwig, puis « Rescan ».
- **« Problèmes fréquents »**, au moins :
  - un plugin « en échec » ;
  - « un plugin tourne » ;
  - un gestionnaire qui ne s'ouvre pas ;
  - le navigateur qui ne revient pas après la connexion ;
  - un disque plein pendant une sauvegarde ;
  - l'import refusé pour cause d'autre disque.
- **Longueur** : de 150 à 500 mots par chapitre en français. Les versions en et es sont des
  traductions fidèles.

- [ ] **Step 1 :** écrire les 9 chapitres en français.
- [ ] **Step 2 :** traduire en anglais et en espagnol.
- [ ] **Step 3 :** le test `chaque_chapitre_existe_dans_les_trois_langues` passe. Ajouter
  `liens_internes_valides` dans `aide.rs` : tout `aide:<clé>` des 27 textes désigne une clé de
  `CHAPITRES`.
- [ ] **Step 4 :** relire chaque chapitre dans la fenêtre d'aide, en fr, puis survoler en et es.
- [ ] **Step 5 :** commit `WiMao : textes du mode d'emploi en français, anglais et espagnol`.

### Task 10 : interface — page Historique et retour en arrière

**Files :**
- Create : `crates/wimao/src/historique_vue.rs`
- Modify : `crates/wimao/src/fenetre/entete.rs` (bouton `document-open-recent-symbolic`), `crates/wimao/src/fenetre/actions.rs` (`win.historique`, Ctrl+H), `crates/wimao/src/operations.rs`.

**Interfaces :**
- Consumes : `historique::{lire, Entree, Action, ResultatAction, libelle_action}`, `restauration::{lister, apercu_retour, revenir, BilanRetour}`, `Fenetre::pousser` (tâche 7).
- Produces (partie pure, testée) :
  ```rust
  pub struct Jour { pub titre: String, pub entrees: Vec<Entree> }
  pub fn grouper_par_jour(entrees: &[Entree], aujourd_hui: &str, hier: &str) -> Vec<Jour>; // "AAAA-MM-JJ" ; titres tr("Today"), tr("Yesterday"), sinon la date
  pub fn point_disponible(entree: &Entree, liste: &ListePoints) -> Option<PointRestauration>; // dans liste.points (jamais perimes)
  ```
- Page : `adw::NavigationPage` titrée `tr("History")`, avec sa propre `HeaderBar` (bouton retour
  automatique). Contenu :
  - un `AdwPreferencesGroup` par jour ;
  - un `AdwActionRow` par entrée : icône selon l'action, titre = sujet, sous-titre = heure ·
    libellé de l'action · `details`, et une marque de résultat (même style que `liste::creer_badge`) ;
  - un bouton suffixe « Revenir à avant ceci » si `point_disponible` ;
  - aucune entrée : `AdwStatusPage` « Rien pour l'instant » (`tr("Nothing yet")`).
- « Revenir à avant ceci » :
  1. `apercu_retour` dans un travail d'arrière-plan (`travail::en_arriere_plan`) ;
  2. un `AdwAlertDialog` affiche le libellé du motif, la date et
     `trn("{n} file will be removed", …)` ;
  3. `revenir` ;
  4. un toast de bilan (fichiers et liens), puis rechargement de la page et de la liste des plugins.

  Pendant un travail ou une session (`occupe`), les boutons sont grisés, avec l'infobulle
  `tr("Wait until the current task is finished")`.

- [ ] **Step 1 : tests** dans `historique_vue.rs` :
  - `groupes_aujourd_hui_hier_puis_date` : 3 entrées sur 3 jours donnent 3 groupes, avec les
    titres « Today », « Yesterday » et `2026-10-30` ;
  - `point_perime_ou_absent_n_est_pas_propose`.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test -p wimao` passe. Vérification à l'œil dans un HOME temporaire :
  installer l'installeur factice des fixtures, ouvrir l'historique, revenir en arrière ; le plugin
  disparaît de la liste.
- [ ] **Step 5 :** commit `WiMao : page Historique avec retour en arrière`.

### Task 11 : interface — restaurer une sauvegarde, annuler l'import

**Files :**
- Create : `crates/wimao/src/restaurer.rs`
- Modify : `crates/wimao/src/fenetre/entete.rs` (menu « Restaurer une sauvegarde… », « Annuler l'import »), `crates/wimao/src/fenetre/actions.rs`, `crates/wimao/src/fenetre/mod.rs` (toast de fin de sauvegarde avec « Afficher »), `crates/wimao/src/dialogues.rs` (sélecteur `*.sha256`).

**Interfaces :**
- Consumes : `sauvegarde::restaurer`, `import::{annulation_possible, annuler}`.
- Produces : `pub fn resume_archive(sha256: &Path) -> Resultat<ResumeArchive>` (pure, testée).
  `ResumeArchive { pub nom: String, pub date: Option<String>, pub taille: u64 }`. La date vient du
  nom `wimao-AAAA-MM-JJ-HHhMM` ; la taille est la somme des morceaux listés dans le `.sha256`.
- Parcours « Restaurer » :
  1. le sélecteur (filtre `*.sha256`) ;
  2. un `AdwAlertDialog` affiche le nom, la date, la taille et
     `tr("Your current environment will be kept as a copy.")`, avec un bouton « Restaurer » de
     style `destructive` ;
  3. la restauration tourne en arrière-plan avec la barre et le bandeau existants ;
  4. un toast `tr("Backup restored")`, puis `rafraichir`.

  Passe par `avant_d_arreter_wine` (tâche 7).
- Toast de fin de sauvegarde : le texte existant, plus le bouton `tr("Show")`, qui ouvre le dossier
  avec `gtk4::FileLauncher::open_containing_folder`.
- « Annuler l'import » est visible dans le menu seulement si `annulation_possible`. Le parcours
  est le suivant :
  1. une confirmation, qui explique que le dossier retourne à `~/.wine-mao` ;
  2. `avant_d_arreter_wine` ;
  3. l'annulation en arrière-plan ;
  4. la fenêtre se ferme et `app.activate()` est appelé. Le choix de la fenêtre à l'activation
     (tâche 12) ouvre alors l'assistant, puisque l'environnement est absent. Avant la tâche 12,
     c'est la fenêtre principale qui se rouvre, ce qui est acceptable entre les deux tâches.

- [ ] **Step 1 : tests** dans `restaurer.rs` :
  - `resume_d_une_archive_en_un_morceau` ;
  - `resume_d_une_archive_en_morceaux` : deux `.part00N` listés, la taille est la somme ;
  - `nom_inattendu_donne_une_date_absente`.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test -p wimao` passe. Vérifier à l'œil dans un HOME temporaire que la
  sauvegarde puis la restauration se font depuis la fenêtre, et que l'historique les montre.
- [ ] **Step 5 :** commit `WiMao : restaurer une sauvegarde et annuler l'import depuis la fenêtre`.

### Task 12 : interface — assistant de premier lancement

**Files :**
- Create : `crates/wimao/src/assistant.rs` (automate pur), `crates/wimao/src/assistant_fenetre.rs` (GTK)
- Modify : `crates/wimao/src/main.rs` (`demarrer`), `crates/wimao/src/operations.rs`.

**Interfaces :**
- Consumes : `import::{detecter, programmes_ouverts, importer, EnvironnementExistant}`, `creer_environnement`, `ouvrir_aide` (tâche 8), `Fenetre::nouvelle`, `Fenetre::demander_installeur`.
- Produces (`assistant.rs`) :
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum PageAssistant { Bienvenue, Environnement, Operation, Echec, PremierPas }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ChoixEnvironnement { Importer, Creer }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Evenement { Commencer, Choisir(ChoixEnvironnement), Reussite, Echec, Reessayer, Retour }
  pub struct Assistant { /* page, existant: bool, choix: Option<ChoixEnvironnement> */ }
  impl Assistant {
      pub fn nouveau(existant: bool) -> Assistant;          // page Bienvenue
      pub fn page(&self) -> PageAssistant;
      pub fn choix_proposes(&self) -> Vec<ChoixEnvironnement>; // [Importer, Creer] si existant, sinon [Creer]
      pub fn choix(&self) -> Option<ChoixEnvironnement>;
      pub fn suivant(&mut self, e: Evenement);              // transitions ci-dessous ; un événement inattendu ne change rien
  }
  ```
  Transitions :
  - `Bienvenue` + `Commencer` → `Environnement` ;
  - `Environnement` + `Choisir(x)` → `Operation`, avec `choix = x` si `x` est proposé ;
  - `Environnement` + `Retour` → `Bienvenue` ;
  - `Operation` + `Reussite` → `PremierPas` ;
  - `Operation` + `Echec` → `Echec` ;
  - `Echec` + `Reessayer` → `Operation`, avec le même choix ;
  - `Echec` + `Retour` → `Environnement` ;
  - pendant `Operation`, `Retour` est ignoré.
- `main.rs` : à l'activation, si `!chemins.prefixe.join("system.reg").exists()`, WiMao ouvre
  `assistant_fenetre::ouvrir(app, chemins, reglages)` au lieu de `Fenetre::nouvelle`. À la fin de
  l'assistant (`PremierPas`, choix fait), sa fenêtre se ferme et `Fenetre::nouvelle` s'ouvre ; si
  le choix est « Ajouter un installeur », il appelle ensuite `demander_installeur`. Fermer
  l'assistant avant la fin quitte l'application sans rien écrire. Pendant `Operation`, la fermeture
  demande confirmation (`dialogues::confirmer_sortie`).
- Pages (une `adw::NavigationPage` par page, dans un `adw::NavigationView`, fenêtre de
  640 × 560) :
  - **Bienvenue** : `AdwStatusPage` avec l'icône de l'application, le titre `tr("Welcome to WiMao")`,
    une description en une phrase, le bouton `pill suggested-action` « Commencer » et le lien
    « Comment ça marche ? » (`ouvrir_aide(…, Some("premiers-pas"))`).
  - **Environnement** : si un environnement existe, une `boxed-list` avec deux `AdwActionRow`
    activables : « Importer cet environnement », sous-titré avec la taille lisible, le nombre de
    plugins et la marque « Recommandé », puis « Créer un nouvel environnement ». Sinon, un seul
    bouton « Créer l'environnement ».
  - **Operation** : spinner, message d'étape (`Progression`) et barre pulsée. Avant l'import,
    `avant_d_arreter_wine` est appliqué à `import::programmes_ouverts` : on réutilise
    `dialogues::confirmer_fermeture_programmes`.
  - **Échec** : `AdwStatusPage` d'erreur avec le message, « Réessayer » et « Copier le rapport ».
  - **PremierPas** : trois `AdwActionRow` en cartes : « Ajouter un installeur », « Ouvrir un
    gestionnaire d'éditeur » (ouvre la fenêtre principale, dont la page vide montre la section
    Gestionnaires) et « Plus tard ».

- [ ] **Step 1 : tests** dans `assistant.rs` :
  - `sans_environnement_existant_seul_creer_est_propose` ;
  - `parcours_import_reussi` : Commencer, Choisir(Importer), Reussite donnent `PremierPas` et
    `choix() == Some(Importer)` ;
  - `echec_puis_reessayer_garde_le_choix` ;
  - `retour_ignore_pendant_l_operation` ;
  - `choix_non_propose_est_ignore` : `nouveau(false)` puis `Choisir(Importer)` laisse la page sur
    `Environnement`.
- [ ] **Step 2 :** les tests échouent.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test -p wimao` passe. Vérifier à l'œil dans un HOME temporaire :
  - sans `~/.wine-mao`, l'environnement est créé ;
  - avec un faux `~/.wine-mao` (petit dossier factice), l'import se fait, puis la fenêtre
    principale s'ouvre.
- [ ] **Step 5 :** commit `WiMao : assistant de premier lancement (création ou import)`.

### Task 13 : finitions visuelles et marque « Mis à jour »

**Files :**
- Modify : `crates/wimao/data/style.css`, `crates/wimao/src/liste.rs`, `crates/wimao/src/fenetre/mod.rs`, `crates/wimao/src/gestionnaires.rs`, `crates/wimao/src/historique_vue.rs`, `crates/wimao/src/assistant_fenetre.rs`.

**Interfaces :**
- Consumes : `RapportInstallation::mis_a_jour`, `BilanSession` (plugins mis à jour), `liste::creer_badge`.
- Produces :
  - `Fenetre` gagne `mis_a_jour: Rc<RefCell<BTreeSet<PathBuf>>>`. Il est rempli après une
    installation ou une session et vidé à la fermeture de la fenêtre.
  - `LignePlugin` gagne `pub mis_a_jour: bool`, et `depuis_inventaire` reçoit
    `&BTreeSet<PathBuf>`.
  - Une marque `tr("Updated")` de classe `accent` s'ajoute à la rangée.
- Finitions (spec 10) :
  - **marques** : « Nouveau » (`success`), « Mis à jour » (`accent`), « Échec du test » (`error`)
    et « Doublon » (`warning`), avec des classes libadwaita uniquement ;
  - **`style.css`** : une seule feuille, commentée règle par règle. Aucune couleur codée en dur :
    `var(--accent-color)` etc., ou les couleurs nommées `@accent_color` ;
  - **marges** : 12 et 24 px, `AdwClamp` de 640 px pour les pages de contenu (assistant,
    historique). La liste des plugins garde `LARGEUR_LISTE` ;
  - **bandeaux** dans un `GtkRevealer` (transition `slide-down`) ;
  - **pages d'état** : liste vide (déjà là, texte relu), historique vide, erreur de relevé
    (tâche 7) et assistant. Chacune a une icône symbolique et un texte d'action.

- [ ] **Step 1 : test** dans `liste.rs` : `plugin_mis_a_jour_porte_la_marque`. Avec un ensemble
  qui contient le chemin, on obtient `mis_a_jour == true`, sinon `false`.
- [ ] **Step 2 :** le test échoue.
- [ ] **Step 3 :** implémenter.
- [ ] **Step 4 :** `cargo test --workspace` passe. Faire des captures en clair et en sombre
  (`ADW_DEBUG_COLOR_SCHEME=prefer-dark`), dans un HOME temporaire, de : la page vide, la liste,
  l'historique, l'assistant et l'aide. Les ranger dans le dossier de la tâche, sous
  `.superpowers/sdd/`.
- [ ] **Step 5 :** commit `WiMao : finitions visuelles et marque « Mis à jour »`.

### Task 14 : intégration continue et documentation

**Files :**
- Modify : `.github/workflows/ci.yml` (si de nouveaux tests `--ignored` doivent tourner), `crates/wimao-recettes/tests/` (test d'intégration), `ARCHITECTURE.md`, `README.md`, `CONTRIBUTING.md`, `docs/ameliorations-reportees.md`.

- Test d'intégration avec Wine, `retour_depuis_l_historique` : installer l'installeur factice,
  lire l'historique, prendre le `point` de l'entrée, puis `restauration::revenir` sur ce point. Le
  plugin factice n'existe plus et l'historique contient une entrée `Retour`.
- `docs/ameliorations-reportees.md` :
  - retirer les points corrigés de la section 1 et la première priorité de « Après le
    sous-projet 2 » ;
  - ajouter une section « Après le sous-projet 3 » avec ce que les relectures auront reporté ;
  - Valhalla reste listé.
- `README.md` :
  - captures (assistant, liste, historique) ;
  - commandes `historique` et `importer` ;
  - mode d'emploi ;
  - phrase sur l'import (« vous le lancez vous-même ; un lien reste à l'ancien emplacement ;
    annulable »).
- `ARCHITECTURE.md` : les nouveaux modules et le flux de l'historique.

- [ ] **Step 1 :** écrire le test d'intégration et vérifier qu'il passe localement avec `--ignored`.
- [ ] **Step 2 :** mettre la documentation à jour.
- [ ] **Step 3 :** `cargo test --workspace`, les trois vérifications globales et
  `scripts/verifier-traductions.sh` passent.
- [ ] **Step 4 :** commit `WiMao : test d'intégration du retour depuis l'historique, documentation du sous-projet 3`.

### Task 15 : vérification réelle (par le contrôleur)

Elle se fait dans un HOME, des XDG et un `WIMAO_DONNEES` temporaires, avec le vrai Wine
(`/opt/wine-mao-v2`), **jamais** sur `~/.wine-mao`.

1. Assistant :
   - sans `~/.wine-mao` factice, l'environnement est créé ;
   - avec un `~/.wine-mao` factice créé par `wineboot` dans le HOME temporaire, l'import se fait.
     Ensuite, le lien est présent, l'historique contient « Import » et « Annuler l'import » remet
     tout en place.
2. Installation de Serum 2 si l'installeur est disponible dans le cache, sinon de l'installeur
   factice. Ensuite : historique, « Revenir à avant ceci », plugin retiré, liens cassés retirés
   de `<home temporaire>/.vst3/yabridge`.
3. Sauvegarde puis « Restaurer une sauvegarde » depuis la fenêtre ; les anciens points ne sont plus
   proposés.
4. Mode d'emploi en fr, en et es (changer la langue dans le menu).
5. Captures en clair et en sombre.
6. Noter le résultat dans `NOTES.md`, section « WiMao sous-projet 3 — vérification réelle ».
   Corriger ce qui est trouvé, avec un commit par correction.
