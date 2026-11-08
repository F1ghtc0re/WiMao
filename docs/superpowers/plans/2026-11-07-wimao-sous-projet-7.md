# WiMao sous-projet 7 — fiabilité pour l'utilisateur : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal :** supprimer les fausses alertes du test de plugins, montrer l'avertissement yabridge dans l'interface, afficher l'historique dans la langue choisie et livrer sept finitions visibles.

**Architecture :** changements ciblés dans les crates existantes : `wimao-core` (test_plugin, tableau, historique, i18n, outils, diagnostic, environnement), `wimao` (interface GTK), `wimao-cli`, plus `paquets/tester-installation.sh`. Aucun nouveau crate, aucune nouvelle dépendance.

**Tech Stack :** Rust, GTK4/libadwaita, gettext (`.po`), Bash.

**Spec :** `docs/superpowers/specs/2026-11-07-wimao-sous-projet-7-design.md`

## Global Constraints

- Code propre, très commenté en français (commentaires détaillés et utiles) ; n'importe quel développeur doit s'y retrouver.
- Tout texte affiché est un msgid anglais traduit en **fr et es** dans `po/` ; `scripts/verifier-traductions.sh` vert.
- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check` verts.
- WiMao ne modifie jamais le lien `~/.local/share/yabridge`.
- Anciennes entrées de `historique.jsonl` toujours lisibles et affichées comme avant.
- Commits signés (SSH déjà configuré), message « WiMao : … », terminé par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- `scripts/verifier-donnees-perso.sh` : code 0.

## Review Focus

1. Journal de Carla où `Finished initializing` apparaît **après** une ligne de plantage hors Wine (ordre des lignes) : le verdict doit rester `Charge`, pas `Probleme` — test dans la tâche 1.
2. Ligne de plantage `[Wine STDERR] … Unhandled exception` alors que le plugin est chargé : `Probleme` (c'est le plugin) — test dans la tâche 1.
3. `historique.jsonl` contenant un `sujet_neutre` à clé inconnue ou à valeur manquante (`{name}` absent) : affichage du `sujet` brut, jamais de panique ni de `{name}` littéral — test dans la tâche 3.
4. Changement de langue pendant que la page Historique est ouverte : textes neutres retraduits à la reconstruction — test unitaire de `sujet_affiche` en fr puis es dans la tâche 3.
5. 1 048 575 octets et 1 073 741 823 octets : « 1,0 Mio » et « 1,0 Gio » — test dans la tâche 5.

---

### Task 1 : verdicts du test d'ouverture

**Files :**
- Modify : `crates/wimao-core/src/test_plugin.rs` (verdicts, constantes, tests à faux `carla-single`)
- Modify : `crates/wimao-core/src/tableau.rs` (affichage de `Charge`)
- Modify : `paquets/tester-installation.sh` (attentes WiMaoTest)
- Modify : `po/*.po`, `po/wimao.pot`

**Interfaces :**
- Produces : `ResultatTest::Charge` (variante sans donnée) ; `pub const DETAIL_CHARGE: &str = tr_noop("Loads correctly; open it in your music software to check its interface")` dans `tableau.rs` ; `brut_depuis_test(&ResultatTest::Charge) == (Statut::Bon, DetailBrut::Cle(DETAIL_CHARGE))`.

- [ ] **Step 1 : tests** (faux `carla-single` qui écrit un journal donné puis vit ou s'arrête, comme les tests existants) :
  - `charge_puis_carla_arrete_donne_charge` : journal `[Wine STDERR] Finished initializing '…'`, Carla s'arrête → `Charge` ;
  - `plantage_hors_wine_avec_chargement_donne_charge` : `JUCE Assertion…` puis `Segmentation fault`/`aborting` hors `[Wine STDERR]`, plus la ligne de chargement (avant **et** après le plantage, deux cas) → `Charge` ;
  - `plantage_dans_wine_donne_probleme` : `[Wine STDERR] … Unhandled exception` + chargé → `Probleme` contenant la ligne ;
  - `page_fault_dans_wine_donne_probleme` ;
  - `arret_sans_chargement_donne_probleme` → `Probleme("The plugin host stopped")` (texte traduit) ;
  - `delai_sans_fenetre_charge_donne_charge` et `delai_sans_fenetre_non_charge_donne_remarque` (délais courts des tests) → `Remarque(tr("The plugin did not finish loading within 5 minutes"))` ;
  - les tests existants (fenêtre → `Pret`, sans audio → `Remarque`, Xvfb absent…) restent verts ;
  - `tableau.rs` : `charge_donne_bon_avec_le_detail`.
- [ ] **Step 2 : échec attendu** (`cargo test -p wimao-core test_plugin tableau`).
- [ ] **Step 3 : code** selon spec § 3.1-3.2 : constantes `MARQUE_CHARGE = "[Wine STDERR] Finished initializing"`, `MARQUE_WINE = "[Wine STDERR]"`, motifs + `page fault` ; une fonction pure `fn analyser_journal(texte: &str) -> AnalyseJournal { charge: bool, plantage_wine: Option<String>, plantage_hors_wine: Option<String>, sans_audio: bool }` testée seule ; la cause est notée par `journal::noter`. Mettre à jour le commentaire d'en-tête du module.
- [ ] **Step 4 : `paquets/tester-installation.sh`** : WiMaoTest accepté si ❌ « The plugin host stopped » (code 2) **ou** ✅ avec le détail `Loads correctly; open it in your music software to check its interface` (code 0) ; tout le reste refusé ; commentaire mis à jour. `shellcheck` propre.
- [ ] **Step 5 : traductions** fr/es des deux nouveaux msgids ; retirer `Loaded, but the interface could not be checked` ; `scripts/verifier-traductions.sh` vert.
- [ ] **Step 6 : verts + commit.**

### Task 2 : contexte de traduction et libellés d'action en noms

**Files :** `crates/wimao-core/src/i18n.rs`, `crates/wimao-core/src/historique.rs` (`libelle_action`), `scripts/verifier-traductions.sh`, `po/*`

**Interfaces :**
- Produces : `pub fn trc(contexte: &str, msgid: &str) -> String` (pgettext ; repli sur `msgid` si absent).

- [ ] **Step 1 : tests** : `trc_traduit_dans_son_contexte` (fr : `trc("history", "Repair") == "Réparation"` et `tr("Repair")` inchangé, = traduction actuelle du bouton) ; `trc_repli_sur_msgid_si_absent` ; `libelle_action` en fr : `Reparation → "Réparation"`, `Retour → "Retour en arrière"`, `Sauvegarde → "Sauvegarde"`, `Restauration → "Restauration"`, `Installation → "Installation"`, `Gestionnaire → "Session de gestionnaire"`, `Import → "Import"`, `AnnulationImport → "Import annulé"` ; es : `"Reparación"`, `"Vuelta atrás"`, `"Copia de seguridad"`, `"Restauración"`, `"Instalación"`, `"Sesión del gestor"`, `"Importación"`, `"Importación cancelada"`.
- [ ] **Step 2 : échec ; Step 3 : code** (`trc` lit le `.mo` comme `tr`, clé `contexte\u{4}msgid`), `libelle_action` passe par `trc("history", …)`, `--keyword=trc:1c,2` ajouté à xgettext dans `scripts/verifier-traductions.sh`, traductions.
- [ ] **Step 4 : verts + commit.**

### Task 3 : historique neutre, traduit à l'affichage

**Files :** `crates/wimao-core/src/historique.rs` ; appelants : `diagnostic.rs` (~484), `sauvegarde.rs` (~163), `restauration.rs` (~752), `import.rs` (5 appels), `crates/wimao-recettes/src/installation.rs` (~286, ~295), `crates/wimao-recettes/src/gestionnaire.rs` (~486) ; affichage : `crates/wimao-cli/src/commandes/` (historique), `crates/wimao/src/historique_vue.rs` ; `po/*`.

**Interfaces :**
- Consumes : rien des tâches précédentes.
- Produces :
  - `#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)] pub struct TexteNeutre { pub cle: String, #[serde(default)] pub valeurs: BTreeMap<String, String> }` ;
  - `Entree` gagne `#[serde(default, skip_serializing_if = "Option::is_none")] pub sujet_neutre: Option<TexteNeutre>` et `details_neutre` (idem) ;
  - `impl Entree { pub fn sujet_affiche(&self) -> String; pub fn details_affiches(&self) -> String }` ;
  - `const CLES_CONNUES: &[&str]` (msgids `tr_noop` utilisés par les appelants) ;
  - `pub fn ajouter_neutre(c: &Chemins, action: Action, sujet: &TexteNeutre, resultat: ResultatAction, point: Option<&str>, details: Option<&TexteNeutre>, details_libre: &str)` qui écrit aussi `sujet`/`details` traduits dans la langue du moment.

- [ ] **Step 1 : tests** : `ancienne_entree_sans_neutre_affiche_le_brut` ; `entree_neutre_traduite_en_fr_puis_es` (même entrée, deux langues, deux textes) ; `cle_inconnue_retombe_sur_le_brut` ; `valeur_manquante_retombe_sur_le_brut` (aucun `{name}` littéral affiché) ; `aller_retour_json` (sérialisation sans champs neutres absents) ; dans chaque appelant converti, l'entrée écrite porte une clé de `CLES_CONNUES` (test existant adapté ou nouveau).
- [ ] **Step 2 : échec ; Step 3 : code** ; chaque phrase traduite écrite aujourd'hui (repérer les `tr`/`trf` passés à `ajouter`) devient une clé `tr_noop` + valeurs ; les causes d'erreur restent du texte libre (`details_libre`). CLI et page Historique appellent `sujet_affiche` / `details_affiches`.
- [ ] **Step 4 : verts** (dont `scripts/verifier-traductions.sh`) **+ commit.**

### Task 4 : avertissement du lien yabridge dans l'interface

**Files :** `crates/wimao-core/src/diagnostic.rs`, `crates/wimao/src/reparation.rs` (et `operations.rs` si le transport des avertissements y passe), `po/*`

**Interfaces :**
- Produces : `pub fn commande_lien_yabridge(lien: &Path, attendu: &Path, etat: &EtatLienYabridge) -> String` (la commande seule, déjà citée pour le shell, identique à celle contenue dans `message_lien_yabridge`) ; `message_lien_yabridge` s'en sert.

- [ ] **Step 1 : tests** (moteur) : `commande_du_lien_vers_autre_yabridge` (`ln -sfn '<attendu>' '<lien>'`), `…_casse`, `…_vrai_dossier` (`ln -s …`) ; `le_message_contient_la_commande` pour les trois états ; chemin avec espace et apostrophe correctement cité.
- [ ] **Step 2 : échec ; Step 3 : code** : moteur, puis interface : après le diagnostic de « Réparer », si un avertissement de lien existe, `adw::AlertDialog` titre `tr("Information")`, corps = message, réponses « Copier la commande » (presse-papiers `gdk::Display::default().clipboard().set_text(..)`) et « Fermer » ; s'il n'y a rien à réparer, pas de toast « No problem found ». La logique « faut-il un toast, une boîte, les deux » est une fonction pure testée : `fn issue_reparation(reparables: bool, avertissement_lien: bool) -> (bool /*toast*/, bool /*boîte*/)` avec ses 4 cas.
- [ ] **Step 4 : verts + commit.**

### Task 5 : finitions de l'interface

**Files :** `crates/wimao/src/fenetre/actions.rs` / `historique_vue.rs` (Ctrl+H), `crates/wimao-core/src/outils.rs` (`taille_lisible`), `crates/wimao/src/assistant_fenetre.rs` (icônes), `crates/wimao/src/fenetre/mod.rs` et `actions.rs` (`mis_a_jour` reporté au changement de langue), `crates/wimao/src/operations.rs` (`morceaux_du_bilan`), `po/*`

- [ ] **Step 1 : tests** :
  - `taille_lisible` : 1 048 575 → `"1.0 MiB"` / `"1,0 Mio"` / `"1,0 MiB"` (en/fr/es) ; 1 073 741 823 → `"1.0 GiB"` / `"1,0 Gio"` ; cas existants inchangés ;
  - bilan : 2 installés dont 1 ⚠️ et 1 ❌ → en `"…, 1 to check, 1 with a problem"`, fr `"…, 1 à vérifier, 1 avec un problème"` (`trn` pour le pluriel) ; ⚠️ seul → aucun « with a problem » ;
  - Ctrl+H : la décision « pousser la page ? » est une fonction pure testée (`fn doit_ouvrir_historique(page_visible: Option<&str>) -> bool`, faux si `"historique"`).
- [ ] **Step 2 : échec ; Step 3 : code** ; icônes : classes CSS Adwaita `success` (prêt) et `error` (erreur) à la place des couleurs fixes ; `mis_a_jour` : l'ensemble est cloné dans la fenêtre reconstruite par `changer_langue`.
- [ ] **Step 4 : verts + commit.**

### Task 6 : « Créer » sur un environnement à moitié créé

**Files :** `crates/wimao-core/src/environnement.rs` (et l'assistant si nécessaire : `crates/wimao/src/assistant.rs`, `assistant_fenetre.rs`)

- [ ] **Step 1 : comprendre** le cas : lire `creer_environnement` et le parcours assistant « échec → retour → Créer » ; noter dans le rapport ce qui échoue ou se double aujourd'hui.
- [ ] **Step 2 : test** `creer_reprend_un_environnement_a_moitie_cree` (préfixe créé puis interrompu à une étape intermédiaire simulée — par exemple `system.reg` présent mais correctifs absents —, nouvelle création → environnement complet, aucune erreur, pas de second préfixe ; points de restauration non périmés à tort) ; il échoue.
- [ ] **Step 3 : code** minimal pour que la création reprenne (étapes déjà faites sautées ou refaites sans risque) ; commentaire qui explique le cas.
- [ ] **Step 4 : verts + commit.**

### Task 7 : documentation

**Files :** `docs/ameliorations-reportees.md`, mode d'emploi intégré (`crates/wimao/src/aide.rs` / ses textes) si les verdicts y sont décrits, `po/*`

- [ ] Retirer les points traités (fausses alertes du test, lien yabridge invisible, langue de l'historique, double Ctrl+H, tailles, icônes, marque « Mis à jour » perdue au changement de langue, bilan, « Créer ») ; ajouter la limite « les causes d'échec restent dans la langue du moment » ; décrire « Se charge correctement… » dans l'aide si les verdicts y sont expliqués. `scripts/verifier-donnees-perso.sh` code 0 ; **commit**.

### Task 8 : vérification réelle (contrôleur, avec l'utilisateur)

- [ ] Relecture finale (modèle le plus capable) et corrections.
- [ ] Sur la machine du propriétaire, avec le binaire de la branche : `wimao-cli verifier` (DrumSynth ✅, Vocal Doubler ✅ « Se charge correctement… » ou ✅ « s'ouvre ») ; interface : « Réparer » montre la boîte du lien yabridge ; changer de langue avec la page Historique ouverte.
- [ ] PR `sous-projet-7`, CI verte, fusion avec l'accord explicite de l'utilisateur.
