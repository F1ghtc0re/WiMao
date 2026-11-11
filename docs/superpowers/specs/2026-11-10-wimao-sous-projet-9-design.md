# WiMao — sous-projet 9 : lanceurs inconnus, logiciels de musique en Flatpak, README court

Date : 2026-11-10 · Statut : validé par le propriétaire (« c'est parti fais tout ») · Branche : `sous-projet-9`

Origine : premier retour d'un testeur Fedora (nicod_, Fedora 45, Bitwig 6.1.4 en Flatpak, WiMao 0.6.0).

## 1. But

1. Un lanceur d'éditeur que WiMao ne connaît pas (exemple : BLEASS Plugins Installer), installé par
   « Ajouter un installeur », peut ensuite être **ouvert depuis WiMao**, avec le même pontage automatique qu'un
   gestionnaire connu.
2. Un logiciel de musique installé en **Flatpak** (Bitwig de Flathub en premier) charge les plugins pontés
   **sans réglage manuel** ; s'il ne peut pas voir les dossiers nécessaires, « Diagnostiquer » le dit et donne
   la commande.
3. Le README redevient une porte d'entrée courte, avec des **crédits** visibles (yabridge, Wine, DXVK, Carla).

**Critères de réussite :**
- après l'installation d'un faux lanceur (installeur de test qui crée un raccourci du menu Démarrer et aucun
  plugin), la section « Gestionnaires » montre une ligne pour lui avec **Ouvrir** ; l'ouvrir lance son
  programme dans une session ; « Retirer de la liste » l'enlève sans rien désinstaller ;
- avec un logiciel de musique Flatpak connu installé, WiMao crée `~/.var/app/<id>/data/yabridge` →
  yabridge de WiMao, et un plugin ponté se charge (`ModuleEntry` vrai) avec `XDG_DATA_HOME` pointé sur
  `~/.var/app/<id>/data` et un `PATH` sans Wine ni yabridge ;
- le README tient sur environ un écran et demi avant les sections de référence.

**Hors périmètre :** recette dédiée à BLEASS (le mécanisme générique la couvre), plugins Linux absents du
runtime Flatpak (PaulXStretch / `libfftw3f`), lancement automatique de `flatpak override`, publication 0.7.0.

## 2. Constat

### 2.1 Lanceurs inconnus

- La section « Gestionnaires » ne liste que les recettes de type `gestionnaire` (4 aujourd'hui).
- Un lanceur installé par « Ajouter un installeur » n'ajoute aucun plugin : en silencieux, WiMao conclut
  `interactif_necessaire = true` et propose « Ouvrir l'installeur ». Une fois le lanceur installé, rien ne
  permet de le lancer.
- Les installeurs Windows créent presque toujours un raccourci `.lnk` dans le menu Démarrer de Wine :
  `drive_c/ProgramData/Microsoft/Windows/Start Menu/Programs/**` (tous les utilisateurs) ou
  `drive_c/users/<utilisateur>/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/**`. Exemple relevé :
  `Plugin Alliance Installation Manager.lnk`, à côté de `Uninstall Plugin Alliance Installation Manager.lnk`.
- Une [`Session`](../../../crates/wimao-recettes/src/gestionnaire.rs) ne dépend de la recette que par sa section
  `[gestionnaire]` (`programme`, `arguments`, `pilote`, `fermer`) et ses consignes : une recette construite en
  mémoire suffit.

### 2.2 Flatpak (essai du 2026-11-10 sur la machine du propriétaire)

- Les « chainloaders » de yabridge cherchent `libyabridge-*.so` à côté d'eux, puis dans
  `$XDG_DATA_HOME/yabridge`, puis dans le `PATH`. Dans un Flatpak, `XDG_DATA_HOME` vaut
  `~/.var/app/<id>/data` et le `PATH` vaut `/app/bin:/usr/bin` : le lien `~/.local/share/yabridge` posé par
  WiMao n'est pas consulté, d'où `ModuleEntry() returned false` chez le testeur.
- Essai : lien `<dossier>/yabridge` → `/opt/wimao/yabridge`, puis chargement d'un plugin ponté réel
  (Mini Grand) avec `env -i PATH=/usr/bin:/bin XDG_DATA_HOME=<dossier>` : `ModuleEntry -> True`,
  `GetPluginFactory` non nul. Le `wimao-bridge` lance Wine par son chemin fixe (`WINELOADER` par défaut
  `/opt/wimao/wine/bin/wine`) : **le lien suffit**, aucun `PATH` à modifier.
- Le Flatpak doit en outre **voir** `/opt/wimao`, le préfixe (`~/.local/share/wimao`) et les dossiers de
  plugins. Bitwig de Flathub a `filesystems=host` : c'est le cas.
- Bitwig garde en cache l'échec d'un plugin et ne le relit que si la date du fichier change (constat du
  testeur).

## 3. Applications installées (lanceurs inconnus)

### 3.1 Détection

Nouveau module `wimao-recettes/src/applications.rs`.

- `releve_raccourcis(prefixe) -> BTreeSet<PathBuf>` : tous les `.lnk` sous les deux menus Démarrer du
  préfixe (tous les utilisateurs et l'utilisateur Windows de WiMao), récursivement.
- `cible_raccourci(octets) -> Option<String>` : chemin Windows de la cible d'un `.lnk` (format
  « Shell Link » de Microsoft, MS-SHLLINK : en-tête de 76 octets, `LinkTargetIDList` sauté si présent,
  `LinkInfo` → `LocalBasePath`, en ANSI ou en Unicode selon les champs présents). Lecteur écrit à la main,
  borné (aucune lecture hors du tampon, fichier de plus de 1 Mo refusé) : pas de nouvelle dépendance.
- Pendant une installation (`installation.rs`), WiMao relève les raccourcis avant et après. Un raccourci
  **nouveau** devient une application si :
  - l'installation n'a ajouté ni mis à jour **aucun plugin** (un installeur de plugin crée aussi des
    raccourcis vers ses versions autonomes : on ne les veut pas) ;
  - il n'est pas dans un dossier `StartUp` ;
  - son nom ne commence pas par `Uninstall`/`Désinstaller` (insensible à la casse) et sa cible n'est pas un
    désinstalleur (`unins*.exe`, `uninstall*.exe`) ;
  - sa cible est un `.exe` qui existe dans le préfixe ;
  - ce n'est pas le programme d'une recette de gestionnaire déjà connue.
- Si au moins une application est trouvée, l'installation est **réussie** : `interactif_necessaire = false`,
  `RapportInstallation::applications` donne les noms, et le message de fin dit
  « BLEASS Plugins Installer installé : ouvrez-le depuis la section Gestionnaires ».

### 3.2 Liste enregistrée

- Fichier `<données>/applications.toml` (`~/.local/share/wimao/applications.toml`), une table par
  application : `nom` (nom du raccourci sans `.lnk`), `programme` (chemin Windows de la cible), `raccourci`
  (chemin du `.lnk` relatif au préfixe), `ajoutee` (date). Écrit de façon atomique (fichier temporaire puis
  renommage), comme les autres fichiers de WiMao.
- Lecture tolérante : un fichier absent = liste vide ; un fichier illisible est journalisé et traité comme vide
  (jamais d'erreur bloquante pour l'affichage), sans être écrasé.
- Une application dont le programme a disparu du préfixe (désinstallée, retour en arrière) n'est pas
  affichée ; elle reste dans le fichier et réapparaît si le programme revient.
- Les sauvegardes et retours en arrière ne touchent pas ce fichier (il est hors du préfixe) ; la règle
  précédente suffit à garder l'affichage juste.

### 3.3 Ouverture

- `Application::vers_recette() -> Recette` : recette en mémoire de type `gestionnaire`, `programme` = la
  cible, pas d'arguments, pas de pilote, `fermer` = nom de l'exécutable, aucune consigne.
- L'ouverture passe par `gestionnaire::ouvrir` sans changement : point de restauration, pontage au fil de
  l'eau, « J'ai fini », bilan, historique (action `Gestionnaire` avec le nom de l'application).
- Interface : les applications s'affichent dans la section **Gestionnaires**, après les recettes, avec la même
  ligne (sous-titre « Application ajoutée · Installé »), un bouton **Ouvrir** (action
  `win.ouvrir-application`, paramètre : le nom) et un menu avec **Retirer de la liste** (confirmation ;
  n'efface que la ligne du fichier).
- CLI : `wimao-cli application lister | ouvrir <nom> | retirer <nom>`.

## 4. Logiciels de musique en Flatpak

### 4.1 Liens

Nouveau module `wimao-core/src/flatpak.rs`.

- Liste fixe, commentée, des identifiants connus : `com.bitwig.BitwigStudio`, `fm.reaper.Reaper`,
  `org.ardour.Ardour`, `studio.kx.carla`, `io.lmms.LMMS`, `org.zrythm.Zrythm`, `org.rncbc.qtractor`,
  `com.renoise.Renoise`.
- `logiciels_installes() -> Vec<String>` : identifiants connus présents dans `flatpak list --app
  --columns=application` (installations système et utilisateur). `flatpak` absent ou en échec → liste vide,
  sans erreur.
- `assurer_liens_flatpak(c) -> Vec<(String, LienYabridge)>` : pour chacun, applique la règle de
  `assurer_lien_yabridge` à `~/.var/app/<id>/data/yabridge` (créé s'il est absent, dossiers parents compris ;
  jamais remplacé s'il existe). La fonction existante est généralisée par un paramètre « emplacement du lien »,
  sans changer son comportement pour `~/.local/share/yabridge`.
- Quand au moins un lien vient d'être **créé**, WiMao met à la date du jour les fichiers `.so` des plugins
  pontés (`~/.vst3/yabridge`, `~/.vst/yabridge`, `~/.clap/yabridge`) : le logiciel de musique les relit au
  prochain démarrage. Rien d'autre n'est modifié.
- Appelé par `pontage::assurer_liens` : au pontage (`ponter`), au démarrage de l'interface (`operations::plugins_non_pontes`, en fond) et au début de `diagnostic::reparer` (« Réparer » et `wimao-cli reparer`), avant le retour « rien à réparer ».

### 4.2 Diagnostic

- Pour chaque logiciel Flatpak installé, `flatpak info --show-permissions <id>` donne ses `filesystems`.
  Fonction pure `chemins_invisibles(filesystems, home, chemins_requis) -> Vec<PathBuf>` : `host` couvre tout ;
  `home` couvre ce qui est sous le dossier personnel ; une entrée explicite couvre son dossier et ses
  sous-dossiers (`~/` développé, suffixes `:ro`/`:rw`/`:create` ignorés) ; `xdg-data`, `xdg-config` et
  `xdg-cache` (avec un éventuel sous-dossier) couvrent `~/.local/share`, `~/.config` et `~/.cache`. Chemins requis : `/opt/wimao`, les
  données de WiMao, `~/.vst3`, `~/.vst`, `~/.clap`.
- Nouveau `Probleme::FlatpakSansAcces { id, invisibles, commande }`, avertissement comme
  `LienYabridge` (montré, jamais réparé ni compté dans un point de restauration), avec la commande :
  `flatpak override --user --filesystem=/opt/wimao:ro --filesystem=~/.local/share/wimao …  <id>`
  (seulement les chemins invisibles). WiMao ne la lance jamais : modifier les permissions d'une autre
  application est la décision de l'utilisateur.
- Un lien Flatpak qui vise un autre yabridge est signalé comme le lien principal (`Probleme::LienYabridge`,
  emplacement Flatpak).

## 5. Documentation

- `README.md` réécrit court : ce que fait WiMao (3 lignes), capture, **Crédits** (yabridge — Robbert van der
  Helm, Wine, DXVK, Carla, avec liens et licences), Installation Ubuntu/Debian et Fedora (commandes seules),
  Premiers pas (5 lignes), liens vers la suite. Le détail actuel (paquets, vérification manuelle, sécurité,
  Debian 13, données…) part dans `docs/installation-detaillee.md`, lié depuis le README ; rien n'est perdu.
- Mode d'emploi (fr, en, es) :
  - `03-gestionnaires.md` : « Autres applications » (lanceur ajouté par un installeur, Ouvrir, Retirer) ;
  - `04-logiciel-de-musique.md` : « Logiciel de musique en Flatpak » (automatique, rôle de « Diagnostiquer »,
    Bitwig qui ne relit pas un plugin en échec).
- `docs/ameliorations-reportees.md` : limites du § 7.

## 6. Tests

- `cible_raccourci` : `.lnk` réels de Wine (fichiers de test dans le dépôt, créés par Wine, sans donnée
  personnelle) en ANSI et Unicode, plus fichiers tronqués, vides, trop gros, sans `LinkInfo` → `None`, jamais de
  panique.
- Filtrage des raccourcis (désinstalleur, `StartUp`, cible absente, recette connue, installation avec
  plugins) ; lecture/écriture de `applications.toml` (absent, illisible, aller-retour).
- Installation d'un faux lanceur (installeur de test qui crée un `.lnk` et un `.exe`) : application
  enregistrée, `interactif_necessaire` faux ; session ouverte et fermée sur ce programme.
- `chemins_invisibles` : `host`, `home`, entrée explicite, `~/`, suffixes, rien.
- `assurer_liens_flatpak` avec un faux `flatpak` dans le `PATH` : création, lien existant laissé, autre lien
  signalé, `flatpak` absent ; mise à jour des dates seulement après une création.
- Chargement réel d'un plugin ponté avec `XDG_DATA_HOME` d'un Flatpak simulé (essai du § 2.2), automatisé
  dans les tests ignorés par défaut qui demandent Wine (`--ignored`), comme les autres tests réels.

## 7. Limites connues

- Un lanceur sans raccourci du menu Démarrer n'est pas détecté.
- Le chargement dans un **vrai** Flatpak n'est prouvé que par le testeur (Bitwig) ; les autres logiciels de la
  liste ne sont pas essayés. Le runtime d'un Flatpak doit fournir les bibliothèques dont Wine a besoin (c'est le
  cas du runtime freedesktop de Bitwig).
- Un logiciel de musique Flatpak absent de la liste n'est pas pris en charge automatiquement ; l'aide donne le
  lien à créer.
