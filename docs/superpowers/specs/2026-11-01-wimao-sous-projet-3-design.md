# WiMao, sous-projet 3 : interface complète

Date : 2026-11-01 · Statut : à valider

Ce document détaille le sous-projet 3 annoncé dans la conception d'ensemble
(`docs/superpowers/specs/2026-10-31-wimao-design.md`, section 2 : « assistant pas à pas, mode
d'emploi intégré, historique, finitions visuelles »). Il en reprend l'architecture (sections 3 à 8)
et précise ses sections 5.8 (historique), 6 (revenir en arrière), 7 (traductions) et 9 (import de
l'environnement existant).

## 1. Objectif

Rendre WiMao utilisable de bout en bout sans terminal ni connaissance de Wine : un premier
lancement guidé, un mode d'emploi dans la fenêtre, un historique d'où l'on peut revenir en arrière,
la restauration d'une sauvegarde et l'import de l'environnement `~/.wine-mao` existant. Corriger au
passage les défauts reportés qui touchent l'interface.

**Critère de réussite.** Dans un `HOME` neuf :
- au premier lancement, l'assistant crée l'environnement (ou importe un `~/.wine-mao` factice)
  puis propose un premier installeur ou un gestionnaire ;
- chaque installation, session de gestionnaire, réparation, sauvegarde, restauration, import et
  retour en arrière apparaît dans l'historique ; « Revenir à avant ceci » fonctionne depuis la
  fenêtre ;
- une sauvegarde faite depuis la fenêtre se restaure depuis la fenêtre ;
- le mode d'emploi s'ouvre par F1, dans la langue choisie (fr, en, es) ;
- aucun point de la section 1 de `docs/ameliorations-reportees.md` ni la première priorité de sa
  section « Après le sous-projet 2 » ne reste ouvert (section 8 ci-dessous).

## 2. Périmètre

**Inclus :** assistant de premier lancement (section 4), import (section 5), historique et retour
en arrière (section 6), restauration d'une sauvegarde (section 7), mode d'emploi (section 9),
défauts reportés (section 8), finitions visuelles (section 10), commandes `wimao-cli`
équivalentes (section 11).

**Exclu :**
- paquet `.deb`, installation de l'icône dans `hicolor` (sous-projet 4) ;
- mise à jour des recettes depuis GitHub ;
- vérification de bout en bout de Valhalla (dépend du fichier fourni par l'utilisateur) ;
- copie de l'environnement vers un autre disque lors de l'import (section 5.4) ;
- restauration des fichiers **modifiés** lors d'un retour en arrière : seul le registre est remis et
  seuls les fichiers apparus sont supprimés, comme aujourd'hui (`restauration.rs`).

**Règle de sécurité du sous-projet.** L'import ne touche le vrai `~/.wine-mao` que lorsque
l'utilisateur le lance lui-même. Les tests et la vérification réelle se font dans un `HOME`, des
`XDG_*` et un `WIMAO_DONNEES` temporaires, avec un faux `~/.wine-mao`.

## 3. Organisation du code

| Crate | Fichier | Rôle |
|---|---|---|
| `wimao-core` | `historique.rs` (nouveau) | journal des actions, lecture, rattachement aux points |
| `wimao-core` | `import.rs` (nouveau) | détection, import et annulation de l'import de `~/.wine-mao` |
| `wimao-core` | `restauration.rs` | files séparées, points périmés, liens cassés |
| `wimao-core` | `inventaire.rs` | `Signature` gagne la date de changement (ctime) |
| `wimao-core` | `correctifs/dll.rs` | écriture atomique |
| `wimao-core` | `pontage.rs` | liens vers l'ancien emplacement reconnus comme les nôtres |
| `wimao-recettes` | `installation.rs`, `gestionnaire.rs` | question avant d'arrêter un programme ouvert, historique |
| `wimao` | `assistant.rs` (nouveau) | assistant de premier lancement |
| `wimao` | `aide.rs` + `aide/{fr,en,es}/*.md` (nouveaux) | mode d'emploi et son convertisseur Markdown |
| `wimao` | `historique_vue.rs` (nouveau) | page Historique |
| `wimao` | `restaurer.rs` (nouveau) | écran « Restaurer une sauvegarde » |
| `wimao` | `fenetre.rs` | découpé : la barre de titre et le menu, puis les actions, vont dans `fenetre/entete.rs` et `fenetre/actions.rs` |
| `wimao-cli` | `commandes/` | `historique`, `importer`, `importer --annuler` |

Chaque module garde l'en-tête `//!` habituel : rôle, ce qu'il utilise, qui l'utilise, et les
choix non évidents. La logique (étapes de l'assistant, convertisseur Markdown, regroupement de
l'historique par jour) est séparée du code GTK pour être testée sans affichage.

## 4. Assistant de premier lancement

Il s'affiche à la place de la fenêtre principale quand `Chemins::prefixe` n'existe pas. Il se
compose de pages dans un `AdwNavigationView`. Chaque page a un bouton « Plus tard » ou
« Retour ». Le fermer ne casse rien : il revient au lancement suivant tant que l'environnement
n'existe pas.

1. **Bienvenue.** Une illustration, une phrase sur ce que fait WiMao, puis le bouton
   « Commencer ».
2. **Votre environnement.**
   - Si `import::detecter` trouve un `~/.wine-mao` valide (section 5.1), la page affiche sa taille
     et le nombre de plugins, puis propose deux choix : « Importer cet environnement
     (recommandé) » et « Créer un nouvel environnement ». Le second laisse `~/.wine-mao` intact.
   - Sinon, la page ne propose que « Créer l'environnement ».
   - L'opération choisie s'affiche avec une barre de progression et les messages de
     `Progression`. En cas d'échec : le message, « Copier le rapport » et « Réessayer ».
3. **Premier pas.** Trois cartes : « Ajouter un installeur », qui ouvre le même sélecteur que
   Ctrl+O, « Ouvrir un gestionnaire d'éditeur », qui affiche la section Gestionnaires, et « Plus
   tard ». La fenêtre principale prend ensuite la place de l'assistant.

La logique (quelle page suit, quels choix sont proposés) est un automate pur dans
`assistant.rs`, testé sans GTK.

## 5. Import de `~/.wine-mao`

### 5.1 Détection

`import::detecter(c: &Chemins) -> Option<EnvironnementExistant>` reconnaît `<home>/.wine-mao`
comme un environnement Wine s'il remplit trois conditions :
- c'est un vrai dossier, pas un lien ;
- il contient `system.reg`, `user.reg` et `drive_c/` ;
- `Chemins::prefixe` n'existe pas encore.

`EnvironnementExistant` contient le chemin, la taille apparente (parcours sans suivre les liens)
et le nombre de plugins (`Inventaire::relever`).

### 5.2 Étapes de l'import (`import::importer`)

Il tient le verrou d'installation pendant toute l'opération.

1. **Programmes ouverts.** Si un plugin tourne (`plugins_en_cours`), l'import est refusé avec
   `Erreur::PluginsEnCours`. Si un programme Windows tourne dans `~/.wine-mao`, l'interface le
   nomme et demande l'accord avant d'arrêter son `wineserver`, comme « Réparer »
   (`programmes_a_nommer_avant_reparer`).
2. **Même disque.** Si `~/.wine-mao` et le dossier parent de `Chemins::prefixe` ne sont pas sur le
   même système de fichiers (`st_dev`), l'import est refusé avec un message qui explique pourquoi
   (section 5.4). Rien n'est modifié.
3. **Petite sauvegarde.** WiMao copie `system.reg`, `user.reg`, `userdef.reg` et, s'il existe,
   `~/.config/yabridgectl/config.toml` dans `<donnees>/import/sauvegarde-<date>/`, avec la méthode
   atomique habituelle (`ecrire_atomique`).
4. **Déplacement.** `rename(~/.wine-mao, <donnees>/prefixe)` : l'opération est instantanée et
   atomique sur un même disque.
5. **Lien.** WiMao crée le lien symbolique `~/.wine-mao → <donnees>/prefixe`. Les ponts existants
   dans `~/.vst3/yabridge`, `~/.vst/yabridge` et `~/.clap/yabridge` pointent vers des chemins sous
   `~/.wine-mao` : ils continuent de fonctionner sans aucune modification. Bitwig ne voit aucune
   différence.
6. **Trace.** WiMao écrit `<donnees>/import/import.toml`, qui contient l'ancien chemin, la date et
   le dossier de la petite sauvegarde, puis ajoute une entrée à l'historique.

En cas d'échec à l'étape 5, le dossier est remis à sa place (renommage inverse) et l'erreur est
remontée. À l'étape 6, l'import est réussi et une erreur d'écriture est seulement notée au journal.

**`~/.config/yabridgectl` n'est jamais modifié.** WiMao garde sa propre configuration isolée
(`pontage.rs`, « Isolation »). Si l'utilisateur relance un jour `yabridgectl sync` à la main avec
l'ancienne configuration, celle-ci reste valide grâce au lien.

### 5.3 Reconnaissance des ponts existants

`pontage::etat_selon_liens` compare aujourd'hui la cible d'un lien à `c.prefixe` avec
`starts_with`. Une cible sous `~/.wine-mao` serait donc classée `DejaPonteAilleurs` et laissée de
côté. Après l'import, une cible est reconnue comme la nôtre si elle est sous `c.prefixe` **ou** sous
l'ancien chemin lu dans `import.toml`. Le prochain `ponter` réécrit alors ces liens vers les
nouveaux chemins. On ne fait pas de `canonicalize` sur chaque lien, ce qui coûterait un accès disque
par plugin et suivrait des liens hors de notre contrôle.

### 5.4 Pas de copie vers un autre disque

L'environnement réel pèse plus de 100 Go. Une copie doublerait la place occupée et durerait
longtemps. Si les dossiers sont sur deux disques différents, WiMao l'explique et propose de
laisser `~/.wine-mao` intact. L'utilisateur pourra déplacer le dossier lui-même ou attendre une
version ultérieure.

### 5.5 Annuler l'import (`import::annuler`)

Il n'est possible que si `import.toml` existe, que `~/.wine-mao` est le lien créé à l'étape 5 et
que la condition de l'étape 1 est remplie. WiMao retire le lien, fait `rename(<donnees>/prefixe,
~/.wine-mao)`, renomme `import.toml` en `import.annule-<date>.toml` (rien n'est supprimé) et ajoute
une entrée à l'historique. Les liens réécrits vers `<donnees>/prefixe` entre-temps deviennent
cassés. L'annulation lance donc `yabridgectl sync` avec la configuration de l'utilisateur
(`~/.config/yabridgectl`, sans la modifier). Si elle n'existe pas, WiMao affiche un avertissement
qui donne la commande à lancer.

L'interface propose « Annuler l'import » dans le menu tant que l'annulation est possible.

## 6. Historique et retour en arrière

### 6.1 Fichier

Le journal est rangé dans `<donnees>/historique.jsonl`, avec une entrée JSON par ligne. On ne fait
qu'ajouter des lignes, avec `O_APPEND` et `sync_data`. Une ligne illisible, par exemple la
dernière après une coupure de courant, est ignorée à la lecture et notée au journal technique.
Elle ne fait pas échouer la lecture.

```json
{"date":"2026-11-01T14:03:12+01:00","action":"installation","sujet":"Serum 2","resultat":"reussi","point":"20261101-140301","details":"2 plugins ajoutés"}
```

- `action` : `installation`, `gestionnaire`, `reparation`, `retour`, `sauvegarde`, `restauration`,
  `import` ou `annulation-import`.
- `resultat` : `reussi`, `echec` ou `annule`.
- `point` : identifiant du point de restauration créé **avant** l'action. Ce champ est absent si
  l'action n'en crée pas.
- `details` : texte court, déjà traduit au moment de l'écriture. L'historique garde la langue du
  moment, sans retraduction.

La lecture est faite par `historique::lire(c) -> Resultat<Vec<Entree>>`, de la plus récente à la
plus ancienne. Il n'y a pas de rotation : une ligne fait environ 200 octets, soit 2 Mo pour 10 000
actions.

### 6.2 Qui écrit

Les appels vers `historique::ajouter` sont placés dans le moteur, au même niveau que la création
des points de restauration, pour que la CLI et l'interface écrivent la même chose :
- `installation::installer` ;
- la fin de session de gestionnaire ;
- `diagnostic::reparer` ;
- `restauration::revenir` ;
- `sauvegarde::sauvegarder` ;
- `sauvegarde::restaurer` ;
- `import::importer` et `import::annuler`.

Si `ajouter` échoue, l'erreur est notée au journal technique mais ne fait jamais échouer l'action
elle-même.

### 6.3 Page Historique

On l'ouvre avec un bouton à icône dans la barre de titre (`document-open-recent-symbolic`) et le
raccourci Ctrl+H. C'est une page de l'`AdwNavigationView` de la fenêtre : une liste groupée par jour
(« Aujourd'hui », « Hier », puis la date). Chaque ligne affiche l'icône de l'action, le sujet,
l'heure et une marque de résultat.

Une ligne dont le point est encore disponible et non périmé (section 7.2) propose un bouton
« Revenir à avant ceci ». La confirmation affiche le motif et la date du point, le nombre de
fichiers qui seront supprimés, puis lance le retour en arrière existant
(`restauration::revenir`). Le retour devient lui-même une entrée de l'historique. Le bouton est
grisé, avec une infobulle explicative, pendant qu'une autre opération tourne.

Quand il n'y a encore aucune entrée, la page affiche « Rien pour l'instant ».

## 7. Restaurer une sauvegarde

### 7.1 Écran

L'entrée de menu « Restaurer une sauvegarde… » ouvre un sélecteur de fichier filtré sur `*.sha256`.
Le toast de fin de sauvegarde indique déjà ce fichier et propose désormais « Afficher », qui ouvre
le dossier. WiMao affiche ensuite une confirmation : date de l'archive, taille, et rappel que
l'environnement actuel est conservé sous `prefixe.ancien-<date>`. La progression utilise la page
d'opération existante. À la fin, un toast confirme la restauration et l'inventaire est relu.

### 7.2 Points de restauration périmés

`sauvegarde::restaurer` écrit, après le renommage, un fichier `perime` dans chaque point existant
(`<donnees>/restauration/<id>/perime`). `lister` range ces points dans une liste `perimes`, distincte
de `incomplets`, et `revenir` les refuse. Ils ne sont jamais supprimés automatiquement, mais la
rotation normale les fait sortir de la file. La CLI les affiche avec la mention « périmé ».

## 8. Défauts reportés corrigés

| Défaut (`ameliorations-reportees.md`) | Correction |
|---|---|
| Pas de « Restaurer » | section 7 |
| Points proposés après restauration | section 7.2 |
| Liens cassés après « revenir » | après `revenir`, WiMao parcourt `~/.vst3/yabridge`, `~/.vst/yabridge` et `~/.clap/yabridge` et supprime **uniquement** les liens cassés dont la cible est sous `c.prefixe` (ou sous l'ancien chemin d'import), puis les bundles `.so` qui n'ont plus de lien. Le nombre est donné dans le bilan. |
| Mise à jour non détectée | `Signature` gagne `change: Option<SystemTime>` (ctime, `MetadataExt::ctime`). La ctime change à chaque réécriture, même si l'installeur conserve la date du fichier. La liste des plugins affiche une marque « Mis à jour » jusqu'à la fermeture de la fenêtre. |
| `wineboot -u` recrée les liens Documents | avant chaque installeur, WiMao lance `wineboot -u` lui-même, puis la réparation `DossiersRelies`. L'installeur ne trouve donc jamais ces liens. |
| `NotFound` attribué au programme | le dossier de travail est vérifié avant le lancement. S'il manque, une erreur distincte le nomme. |
| « Quitter quand même » pendant une réparation | `remplacer_fichier` écrit dans `<nom>.part`, fait `sync_all`, puis renomme. Un `.part` resté d'une coupure est supprimé au début de l'essai suivant. |
| File des points de restauration | deux files : 10 points pour les installations et les sessions, 3 pour `repair`. La rotation de l'une ne touche jamais l'autre. |
| Gestionnaire ouvert arrêté sans question | avant « Ajouter un installeur », l'ouverture d'un autre gestionnaire, une sauvegarde ou un import, l'interface nomme les programmes ouverts et demande l'accord (même dialogue que « Réparer »). La CLI demande aussi, ou accepte `--oui`. |
| Dialogues de fermeture empilés | un seul dialogue à la fois : un nouveau clic sur la croix ramène le dialogue ouvert au premier plan. |
| Page « chargement » bloquée | si le premier relevé échoue, une page d'état d'erreur s'affiche avec « Réessayer » et « Copier le rapport ». |
| `reglages.toml` abîmé ignoré | le fichier est renommé en `reglages.toml.abime-<date>`, les réglages par défaut sont utilisés et un toast le signale. |
| Commentaires inexacts (`etat.rs`), `clone` en double (`main.rs`), ARCHITECTURE incomplète | corrigés au passage dans les fichiers touchés. |

## 9. Mode d'emploi

### 9.1 Fenêtre

F1 l'ouvre dans une fenêtre secondaire `AdwNavigationSplitView`, avec la liste des chapitres à
gauche et le texte à droite. Un bouton « Mode d'emploi » s'ajoute au menu, et « À propos » y
reste. L'aide est aussi accessible depuis l'assistant (lien « Comment ça marche ? ») et depuis les
dialogues d'erreur (lien vers « Problèmes fréquents »).

Chapitres :
1. Premiers pas
2. Ajouter un plugin
3. Gestionnaires d'éditeurs
4. Utiliser les plugins dans Bitwig
5. Réparer
6. Historique et retour en arrière
7. Sauvegarder et restaurer
8. Importer un environnement existant
9. Problèmes fréquents

### 9.2 Textes

Il y a un fichier par chapitre et par langue : `crates/wimao/aide/<langue>/<NN>-<nom>.md`. Les
fichiers sont intégrés au programme avec `include_str!`, et la liste des chapitres est générée
par une table unique. La langue suit le réglage « Langue ». Si un chapitre manque dans une langue,
c'est la version française qui s'affiche, et un test vérifie que les trois langues ont les mêmes
chapitres.

**Markdown accepté** (et seulement celui-ci) :
- titres `#` et `##` ;
- paragraphes ;
- listes `-` et `1.` ;
- `**gras**` et `` `code` `` ;
- liens `[texte](https://…)`, ouverts dans le navigateur, et `[texte](aide:chapitre)`, qui mènent
  vers un autre chapitre.

Le convertisseur `aide::convertir(&str) -> Vec<Bloc>` est pur et testé. L'affichage transforme
chaque `Bloc` en `GtkLabel` (balisage Pango, texte échappé) ou en liste. Tout autre élément
Markdown est affiché comme texte brut : rien n'est perdu et aucune dépendance n'est ajoutée.

## 10. Finitions visuelles

- **Pages d'état** (`AdwStatusPage`) avec icône et texte pour : liste de plugins vide (« Ajoutez
  votre premier installeur »), historique vide, erreur de relevé, environnement absent.
- **Marques** (classes CSS libadwaita `success`, `warning`, `error`, `accent`) : « Nouveau », « Mis à
  jour », « Échec du test », « Doublon ».
- **Transitions** : `AdwNavigationView` pour Historique et assistant ; `GtkRevealer` pour les
  bandeaux.
- **Espacements** : marges de 12 et 24 px, `AdwClamp` de 640 px pour le contenu, `boxed-list`
  partout.
- **Mode sombre** : uniquement des couleurs nommées de libadwaita, aucune couleur codée en dur. La
  vérification se fait avec des captures dans les deux modes.
- Une feuille `style.css` unique, intégrée et commentée, regroupe les quelques règles propres à
  WiMao.

## 11. `wimao-cli`

- `wimao-cli historique [--tout]` : les 20 dernières entrées, ou toutes avec `--tout`.
- `wimao-cli importer [--oui]` : même parcours que l'interface. La commande affiche ce qui sera
  fait, puis demande confirmation, sauf avec `--oui`.
- `wimao-cli importer --annuler [--oui]`.
- `restauration lister` affiche la mention « périmé » et la file de chaque point.

## 12. Traductions

Tous les nouveaux messages passent par `i18n` (`tr`, `trf`), en fr, en et es, comme dans les
sous-projets précédents. Le vouvoiement est la règle en français. Le test qui compare les clés des
trois langues couvre les nouvelles clés.

## 13. Erreurs

| Situation | Comportement |
|---|---|
| Import : autre disque | refus expliqué, rien n'est modifié |
| Import : `prefixe` existe déjà | refus : l'assistant ne propose l'import que si l'environnement est absent |
| Import : échec du lien après le déplacement | renommage inverse, erreur remontée |
| Annulation : `~/.wine-mao` n'est plus notre lien | refus expliqué, rien n'est modifié |
| `historique.jsonl` illisible ou ligne abîmée | lignes ignorées, entrée au journal technique, page affichée avec le reste |
| Écriture de l'historique impossible | journal technique uniquement ; l'action continue |
| Restauration : empreinte fausse | refus avant toute extraction (comportement existant), message dans la fenêtre |
| Point périmé | « Revenir » absent dans la fenêtre ; refus explicite dans la CLI |
| Chapitre d'aide manquant dans une langue | version française |

## 14. Tests

**Moteur (unitaires, dossiers temporaires) :**
- `historique` : ajout et relecture, ordre, ligne abîmée ignorée, fichier absent = liste vide ;
- `import` :
  - détection (vrai dossier, lien refusé, fichiers manquants) ;
  - import complet sur un faux `~/.wine-mao` ;
  - refus si `prefixe` existe ;
  - refus sur un autre disque (fonction de comparaison `st_dev` injectable) ;
  - remise en place si la création du lien échoue ;
  - annulation, puis refus si le lien a été remplacé ;
- `pontage` : un lien vers l'ancien chemin d'import est reconnu comme le nôtre ;
- `restauration` :
  - files séparées (11 points « repair » ne chassent aucun point d'installation) ;
  - points périmés listés à part et refusés ;
  - liens cassés : seuls ceux qui pointent dans le préfixe sont retirés, un lien cassé vers ailleurs
    reste ;
- `inventaire` : un fichier réécrit avec la même taille et la même date est vu comme modifié
  (ctime) ;
- `correctifs/dll` : un `.part` résiduel est supprimé, et la cible n'est jamais tronquée.

**Interface (sans GTK) :** automate de l'assistant, convertisseur Markdown (chaque élément accepté,
échappement Pango, élément inconnu en texte brut), regroupement de l'historique par jour,
chapitres présents dans les trois langues.

**Intégration avec Wine (CI) :** l'historique est rempli après une installation de l'installeur
factice, puis un retour en arrière depuis l'historique est fait.

**Vérification réelle**, dans un `HOME` temporaire, avec des captures en clair et en sombre :
- assistant de bout en bout avec un faux `~/.wine-mao` ;
- installation ;
- historique, puis retour en arrière ;
- sauvegarde, puis restauration depuis la fenêtre ;
- mode d'emploi dans les trois langues.

L'import réel de `~/.wine-mao` est fait par l'utilisateur lui-même, après la fusion.

## 15. Risques connus

- **ctime** : elle change aussi lors d'un `chmod` ou d'un changement de propriétaire. Un plugin
  peut donc être signalé « Mis à jour » à tort. C'est rare dans le préfixe et sans conséquence
  grave : le plugin est seulement re-testé.
- **Environnement importé ancien** : le premier lancement de Wine sur `~/.wine-mao` importé peut
  lancer une mise à jour du préfixe (`wineboot`). Elle est couverte par le point de restauration de
  la première action et par la petite sauvegarde du registre.
- **Lien `~/.wine-mao`** : un outil tiers qui supprime ce lien casse les ponts anciens jusqu'au
  prochain `ponter`, qui les réécrit vers `<donnees>/prefixe`.
