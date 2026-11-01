# WiMao, sous-projet 2 : gestionnaires d'éditeurs, pilote et liens de connexion

Date : 2026-10-31 · Statut : à valider

Ce document détaille le sous-projet 2 annoncé dans la conception d'ensemble
(`docs/superpowers/specs/2026-10-31-wimao-design.md`, section 2). Il en reprend l'architecture
(sections 3 à 8) et **remplace l'exemple de recette de sa section 4** : les clés `lancement`,
`services` et `pilote` changent de place (section 3 ci-dessous). Aucune recette réelle n'utilisait
encore ces clés, qui étaient « reconnues mais pas encore prises en charge ».

## 1. Objectif

Installer et utiliser les gestionnaires d'éditeurs depuis WiMao, sans terminal : Native Access
(Native Instruments), Arturia Software Center, inMusic Software Center (AIR) et Plugin Alliance
Installation Manager. Il s'agit d'automatiser tout ce qui a été fait à la main et noté dans
`NOTES.md` (« Étape 3 »). Fournir aussi les recettes de deux installeurs autonomes qui servent de
référence : Serum 2 (Xfer) et Valhalla Supermassive.

**Critère de réussite.** Dans un environnement WiMao neuf :
- chaque gestionnaire s'installe en un clic, s'ouvre, et la connexion au compte aboutit (pour
  Native Access, par le retour du lien `native-access://`) ;
- les produits que le gestionnaire installe sont pontés pendant la session, puis testés à sa
  fin ;
- l'utilisateur n'a rien à faire pour les installeurs avec interface que l'inMusic Software Center
  lance : le pilote les relance en silencieux.

## 2. Périmètre

**Inclus :**
- nouvelles méthodes et clés de recette (section 3) ;
- téléchargement des gestionnaires publics ;
- session de gestionnaire : lancement, services, pilote, pontage au fil de l'eau, fermeture, tests
  de fin (section 4) ;
- relais des liens de connexion (section 5) ;
- interface : section « Gestionnaires » dans la fenêtre existante (section 6) ;
- `wimao-cli` : commandes équivalentes (section 7) ;
- six recettes fournies (section 8) ;
- deux défauts reportés du sous-projet 1 qui touchent ce sous-projet
  (`docs/ameliorations-reportees.md`, § 1) : « Réparer » ne doit plus fermer un gestionnaire sans
  prévenir, et le message « aucun nouveau plugin » doit donner le code de sortie de l'installeur.

**Exclu :**
- mise à jour des recettes depuis GitHub (le dépôt est encore privé) ;
- assistant pas à pas, mode d'emploi, historique (sous-projet 3) ;
- désinstallation d'un gestionnaire ;
- import de `~/.wine-mao` (sous-projet 3) ;
- montage des `.iso` de Native Access (« Previews », impossible sous Wine, voir `NOTES.md`).

## 3. Recettes : clés nouvelles et modifiées

Le format reste du TOML qui refuse les clés inconnues (`docs/recettes.md`). Les ajouts sont
décrits ci-dessous ; `docs/recettes.md` et `crates/wimao-recettes/tests/recettes/valide.toml`
seront mis à jour en conséquence.

### 3.1 Variables

Elles sont utilisables dans tous les chemins Windows et les `donnee` de registre :

| Variable | Valeur |
|---|---|
| `{utilisateur}` | nom du profil Windows (le nom d'utilisateur Linux, comme Wine) |
| `{telechargements}` | `C:\users\{utilisateur}\Downloads` (existe déjà) |
| `{appdata-local}` | `C:\users\{utilisateur}\AppData\Local` |

### 3.2 `[installation]`

| Clé | Nouveauté |
|---|---|
| `methode = "extraire-7z"` | **Prise en charge.** L'installeur est décompressé avec `7z` dans un dossier temporaire du cache, puis chaque étape `[[installation.extraire]]` s'applique. |
| `[[installation.extraire]]` | Nouvelle liste. Chaque étape a deux clés : `archive`, le chemin relatif au dossier décompressé (motifs `*` admis, une seule correspondance exigée, par exemple `'$PLUGINSDIR/app-64.7z'`), et `vers`, le dossier Windows de destination. L'archive est décompressée dans `vers`. Avec `extraire-7z`, au moins une étape est obligatoire (vérifié à la validation) ; ailleurs, la liste est refusée. |
| `methode = "squirrel"` | **Nouvelle.** Pour les applications « Squirrel » (inMusic Software Center). Elle décompresse l'installeur et trouve l'unique `*-full.nupkg`, dont elle lit l'identifiant et la version dans le `.nuspec`. Elle installe ensuite dans `{appdata-local}\<identifiant>` : `app-<version>\` (contenu de `lib\net45`), `Update.exe`, `RELEASES` et `packages\<nupkg>`. C'est la reprise exacte de l'installation manuelle de `NOTES.md`. |
| `lien-connexion` | **Prise en charge** (section 5). |

Les archives qui contiennent un installeur (Plugin Alliance, Valhalla) sont déjà gérées par
`archive.rs` (`methode = "auto"`) : aucune clé nouvelle.

### 3.3 `[[dependances]]` (nouvelle section)

Il s'agit d'installeurs à lancer après l'installation principale, qui se trouvent déjà dans
l'environnement. Exemple : NTKDaemon, déposé par Native Access.

```toml
[[dependances]]
fichier = 'C:\Program Files\Native Instruments\Native Access\resources\daemon\win\NTKDaemon * Setup PC.exe'
methode = "installaware"   # mêmes valeurs que [installation].methode, sauf extraire-7z et squirrel
```

`fichier` admet `*` dans le nom, pas dans les dossiers, et doit désigner un seul fichier ; sinon,
l'erreur est claire et traduite. Les codes de sortie d'InstallAware 100 et 110 comptent comme une
réussite (`NOTES.md`).

### 3.4 `[correctifs]`

| Clé | Changement |
|---|---|
| `registre` | Inchangé (déjà appliqué à l'installation). |
| `dossiers` | **Nouvelle.** Liste des dossiers Windows à créer, s'ils manquent, à l'installation et à chaque ouverture. Exemple : `dossiers = ['{telechargements}\NI']`. |
| `services` | **Prise en charge.** Ce sont les noms de services Windows, démarrés par `wine net start <nom>` à chaque ouverture du gestionnaire. Un service déjà démarré ne compte pas comme une erreur. |
| `lancement` | **Retirée**, remplacée par `[gestionnaire].arguments`. |

### 3.5 `[gestionnaire]` (nouvelle section, obligatoire si `type = "gestionnaire"`)

| Clé | Type | Défaut | Rôle |
|---|---|---|---|
| `programme` | chemin Windows | **obligatoire** | Exécutable à lancer. `*` admis dans un dossier (`app-*` des applications Squirrel) ; si plusieurs dossiers correspondent, on prend la version la plus haute (comparaison numérique des parties). |
| `arguments` | liste | `[]` | Arguments ajoutés, par exemple `["--no-sandbox", "--disable-gpu"]`. |
| `pilote` | liste de dossiers Windows | `[]` | Dossiers où le gestionnaire dépose les installeurs qu'il lance. Si la liste n'est pas vide, le pilote est actif (section 4.3). |
| `fermer` | liste de noms d'exécutables | nom de `programme` | Processus à arrêter quand la session se ferme. |

La section `[apres-installation]` est **retirée** : `pilote` passe dans `[gestionnaire]`.
`[gestionnaire]` est refusée si `type` n'est pas `"gestionnaire"`.

### 3.6 Téléchargement : clé `empreinte` (nouvelle, facultative)

Si `telecharger` est présent et que l'utilisateur ne fournit pas de fichier, WiMao télécharge
l'installeur (HTTPS obligatoire) dans `<cache>/telechargements/`. Si `empreinte` (sha256) est
présente, elle est vérifiée avant toute exécution, comme `telecharger_verifie`. Sans empreinte,
car les éditeurs publient sous une adresse fixe des versions qui changent, l'empreinte obtenue est
écrite dans le journal. Un téléchargement interrompu ne laisse qu'un fichier `.part`. Chaque
installation télécharge à nouveau : on ne garde pas de vieille version en cache.

## 4. Session de gestionnaire

Le nouveau module `wimao-recettes/src/gestionnaire.rs` s'appuie sur `wimao-core` pour les
processus, le pontage et les tests.

### 4.1 Installation

Elle suit le parcours existant (`installation::installer`) :
1. recette ;
2. point de restauration ;
3. environnement ;
4. méthode ;
5. dépendances ;
6. registre et dossiers ;
7. déclaration du lien de connexion.

Une recette de gestionnaire n'est plus refusée : le refus actuel (`recette_pilote_est_refusee…`)
disparaît. L'installation d'un gestionnaire n'attend pas de plugin : « aucun nouveau plugin »
n'est pas une erreur ici. Le résultat est « gestionnaire installé ».

### 4.2 Ouverture

1. WiMao prend le verrou d'opération de WiMao, le même que pour une installation. Pendant la
   session, « Ajouter un installeur », « Réparer », « Sauvegarder » et « Revenir » sont
   indisponibles, avec la raison affichée.
2. Il crée un point de restauration (« session <gestionnaire> ») et refuse si un plugin tourne,
   comme le décide le Ruling 9 du sous-projet 1.
3. Il crée les dossiers de la recette et démarre les services.
4. Il fait un relevé des plugins (inventaire).
5. Il lance `programme` avec `arguments`, depuis le dossier du programme : l'Arturia Software
   Center l'exige. Les sorties vont dans le journal, l'entrée est vide.
6. Il affiche la consigne `connexion` de la recette, s'il y en a une.

### 4.3 Pendant la session

**Pilote** (si `pilote` n'est pas vide). Toutes les 2 secondes, WiMao parcourt `/proc` et ne
retient que les processus **de son environnement** : `WINEPREFIX` égal au sien dans
`/proc/<pid>/environ`. Un processus est un installeur à piloter si sa ligne de commande désigne un
`.exe` situé dans un dossier `pilote` (comparaison sans tenir compte des majuscules, chemin
Windows) et ne contient pas déjà d'argument silencieux connu (`/VERYSILENT`, `-quiet`, `/S`,
`/qn`, `/s`). Pour chacun :
1. arrêter l'installeur interactif et ses enfants : processus de l'environnement dont la ligne de
   commande contient le nom du fichier sans extension (le `.tmp` d'Inno en fait partie) ;
2. s'il a déjà été relancé pendant cette session, s'arrêter là : c'est l'inMusic Software Center
   qui le relance ;
3. sinon, reconnaître son type (`detection::detecter`) et le relancer avec
   `arguments_silencieux`, depuis son dossier, avec le délai de 30 minutes des installations non
   interactives et un journal d'installeur (`/LOG=` pour Inno, `-log` pour WiX Burn) ;
4. type inconnu : le laisser tourner normalement et l'écrire dans le journal.

Chaque relance est notée dans le journal et affichée dans la session (« Jura installé en
silencieux »).

**Pontage au fil de l'eau.** Toutes les 60 secondes, un relevé est comparé au relevé
d'ouverture. Un plugin nouveau ou modifié est ponté dès que sa signature (date et taille) n'a pas
changé entre deux relevés : il n'est plus en cours d'écriture. Le doublon VST2 est exclu comme
dans le sous-projet 1. Les tests attendent la fin de la session. Ce rythme couvre aussi le daemon
de Native Access, qui continue d'installer après la fermeture de la fenêtre.

### 4.4 Fin de session

La session prend fin quand l'utilisateur clique sur « J'ai fini », ou quand plus aucun processus
`fermer` ne tourne et qu'aucun installeur piloté n'est en cours.

1. Si des processus de téléchargement ou d'installation tournent encore (installeur piloté, ou
   installeur lancé par un service : processus de l'environnement dont le nom contient `setup`,
   `install` ou `msiexec`, sans tenir compte des majuscules), WiMao l'affiche et attend. On peut
   alors choisir « Arrêter quand même ».
2. Il arrête les processus `fermer` : **tous** ceux de l'environnement dont la ligne de commande
   contient ce nom. Pour l'inMusic Software Center, une instance survivante rendait la fenêtre
   noire au lancement suivant (`NOTES.md`). Les services ne sont pas arrêtés.
3. Il fait un dernier relevé et ponte ce qui reste, puis teste chaque plugin nouveau ou modifié de
   la session (`tester_plugin`, sous Xvfb).
4. Il affiche le bilan, comme pour une installation : ✅/⚠️/❌, plugins mis à jour, relances du
   pilote. Ce bilan est écrit dans le journal.
5. Il libère le verrou.

Si WiMao est fermé pendant une session, il le signale et demande confirmation. À la confirmation,
le gestionnaire reste ouvert, mais le pilote s'arrête. Au prochain démarrage, WiMao fait un
relevé et ponte ce qui manque (pontage habituel du rafraîchissement).

### 4.5 « Réparer » et un gestionnaire ouvert (défaut reporté)

La réparation « Wine bloqué » arrête le wineserver, donc aussi un gestionnaire ouvert hors session
(lancé à la main ou par un lien). Avant d'agir, `reparer` regarde les processus de
l'environnement. S'il y en a, il renvoie une demande de confirmation qui les nomme, et ne les
arrête qu'après accord. La CLI demande `o/N` ou accepte `--oui`.

## 5. Relais des liens de connexion

Certains gestionnaires (Native Access) reçoivent le retour de connexion par un lien
`<schéma>://…` ouvert par le navigateur Linux.

- **Déclaration**, à l'installation d'une recette qui a `lien-connexion` : WiMao écrit
  `$XDG_DATA_HOME/applications/wimao-lien-<schéma>.desktop` :
  - `NoDisplay=true`,
  - `MimeType=x-scheme-handler/<schéma>;`,
  - `Exec=<chemin de wimao-cli> relayer-lien %u`.

  Puis il lance `xdg-mime default wimao-lien-<schéma>.desktop x-scheme-handler/<schéma>` et
  `update-desktop-database`. **Si un autre programme gère déjà ce schéma**, par exemple
  `wimao-native-access.desktop` de l'installation actuelle, WiMao ne le remplace pas sans le dire :
  l'interface demande confirmation, et la CLI demande `o/N` ou accepte `--oui`.
- **Relais** : `wimao-cli relayer-lien <url>` cherche la recette dont `lien-connexion` correspond
  au schéma, puis lance `programme` avec `arguments` et l'adresse. Les sorties vont dans un journal
  (`<données>/journaux/liens.log`) et l'entrée est vide : sans sortie valide, la seconde instance
  d'Electron plante (`EBADF`, voir `NOTES.md`). Le gestionnaire déjà ouvert reçoit l'adresse de
  cette seconde instance, à sa manière.
- Le relais n'écrit pas dans le journal de WiMao, il n'utilise que le sien : il ne doit pas
  échouer si WiMao est en cours d'opération. Il ne prend pas le verrou.

Le retour de connexion de l'inMusic Software Center passe par `http://localhost` : aucun relais.

## 6. Interface

C'est l'extension de la fenêtre existante. Le soin visuel reste celui du sous-projet 1 :
libadwaita, propre, moderne, décoré.

- **Section « Gestionnaires »** au-dessus de la liste des plugins : un `boxed-list` avec une
  ligne par recette `type = "gestionnaire"`. Chaque ligne affiche le nom, l'éditeur et l'état
  (« Non installé » / « Installé »). L'état vient de la présence de `programme` dans
  l'environnement. Bouton **Installer** ou **Ouvrir**.
- **Pendant une session** : une bannière `adw::Banner` en haut de la fenêtre, « Native Access est
  ouvert — WiMao ponte les nouveaux plugins », avec le bouton **J'ai fini**. La consigne de
  connexion s'affiche une fois, dans un dialogue. Les relances du pilote arrivent en toasts. Les
  plugins pontés apparaissent dans la liste au fil de l'eau, marqués « à tester ».
- **« Ajouter un installeur »** : si le fichier correspond à une recette `installeur`, par exemple
  `Install_Xfer_Serum2_*.exe`, c'est cette recette qui s'applique : elle était déjà prise en
  charge. Sa consigne d'activation s'affiche à la fin.
- Tous les textes nouveaux sont en français, anglais et espagnol (`tr`/`trn`). Le test des
  traductions s'applique.

## 7. `wimao-cli`

| Commande | Effet |
|---|---|
| `recettes` | Liste des recettes : nom, type, installé ou non. |
| `gestionnaire installer <nom> [--fichier <installeur>]` | Section 4.1 ; télécharge si `--fichier` est absent. |
| `gestionnaire ouvrir <nom>` | Sections 4.2 à 4.4 ; la session dure jusqu'à la fermeture du gestionnaire ou jusqu'à Ctrl+C, qui vaut « J'ai fini ». |
| `relayer-lien <url>` | Section 5. |

Codes de sortie : ceux du sous-projet 1 (0 réussi, 1 erreur, 2 au moins un plugin ❌).

## 8. Recettes fournies (`recettes/`)

Les valeurs viennent des installations à la main (`NOTES.md`) et des chemins relevés en lecture
seule dans `~/.wine-mao`. Les adresses de téléchargement sont vérifiées pendant la vérification
réelle (section 10).

| Fichier | Type | Points clés |
|---|---|---|
| `native-access.toml` | gestionnaire | `telecharger` : `https://assets.native-instruments.com/downloads/Native-Access_2.exe` ; `extraire-7z`, étape `'$PLUGINSDIR/app-64.7z'` vers `C:\Program Files\Native Instruments\Native Access` ; dépendance NTKDaemon (InstallAware) ; registre `HKCU\Software\Native Instruments\Native Access` `DownloadLocation` = `{telechargements}\NI` ; `dossiers` correspondant ; `services = ["NTKDaemonService"]` ; `programme` `…\Native Access\Native Access.exe` ; `arguments` `--no-sandbox --disable-gpu` ; `lien-connexion = "native-access"` |
| `arturia-software-center.toml` | gestionnaire | `telecharger` : adresse publique `downloads.arturia.net/…/Arturia_Software_Center__<version>.exe` (Inno) ; `programme` `C:\Program Files (x86)\Arturia\Arturia Software Center\Arturia Software Center.exe` |
| `inmusic-software-center.toml` | gestionnaire | `telecharger` : `https://inmusic.to/ISCPC` ; `squirrel` ; `programme` `{appdata-local}\inmusic_software_center\app-*\inMusic Software Center.exe` ; `arguments` `--no-sandbox --disable-gpu` ; `pilote = ['{telechargements}\inMusic']` ; `fermer = ["inMusic Software Center.exe"]` |
| `plugin-alliance.toml` | gestionnaire | `telecharger` : archive publique de l'Installation Manager (zip contenant un MSI) ; `programme` `C:\Program Files\Plugin Alliance\Installation Manager\PA-InstallationManager.exe` ; consigne : le gestionnaire peut se figer à 98 % sur le dernier produit alors que les fichiers sont complets, il faut alors le fermer (`NOTES.md`) |
| `serum-2.toml` | installeur | `reconnaitre = ["Install_Xfer_Serum2*.exe"]` ; NSIS ; pas de `telecharger` (compte Xfer) ; consigne `activation` : ouvrir l'adresse d'autorisation dans le navigateur Linux si le bouton ne réagit pas |
| `valhalla-supermassive.toml` | installeur | `telecharger` : zip officiel ; `empreinte` sha256 de `NOTES.md` (vérifiée ou mise à jour pendant la vérification réelle) ; archive contenant un installeur Inno |

Chaque recette a ses consignes en `fr`, `en` et `es`.

## 9. Erreurs

- Les nouveaux messages sont des variantes traduites d'`Erreur` (pas de texte libre en français,
  voir le sous-projet 1, I3). Les cas couverts :
  - programme du gestionnaire introuvable : la recette ne correspond plus à la version installée ;
  - plusieurs fichiers correspondent à un motif, ou aucun ;
  - nupkg absent ou ambigu ;
  - téléchargement refusé (pas en HTTPS) ;
  - empreinte fausse ;
  - lien déjà géré par un autre programme.
- Nouvelles règles d'erreurs dans `regles.toml` :
  - `ERROR_CODE_DOWNLOAD_DIR_MISSING` → dossier de téléchargement de Native Access : réparation
    « registre et dossiers de la recette » ;
  - `Error: open EBADF` → relais du lien.
- « Réparer » réapplique le registre, les dossiers et les services des gestionnaires installés.

## 10. Tests

1. **Unitaires** (sans Wine) :
   - validation des clés nouvelles et retirées (`lancement`, `[apres-installation]` refusées avec
     un message clair) ;
   - variables ;
   - choix du dossier `app-*` le plus récent ;
   - lecture du `.nuspec` ;
   - parcours de `/proc` et choix des processus à piloter, sur une fausse arborescence `/proc`
     fournie au module (pas le vrai `/proc`) ;
   - écriture du `.desktop` et refus de remplacer un autre gestionnaire de lien (`XDG_*` de test,
     `xdg-mime` remplacé par un faux script) ;
   - signature stable entre deux relevés.
2. **Intégration avec Wine** (CI) :
   - **faux gestionnaire** `tests/fixtures/gestionnaire/FauxGestionnaire.c` (mingw, comme le faux
     plugin) : il lance `test-nsis.exe` **sans** `/S`, depuis un dossier `pilote` ;
   - le test ouvre une session : le pilote doit arrêter l'installeur interactif, le relancer avec
     `/S`, et le plugin factice doit être ponté puis testé à la fin ;
   - un faux installeur `extraire-7z` (archive 7z dans une archive 7z) et un faux `.nupkg`
     vérifient les deux nouvelles méthodes ;
   - le relais est testé avec une fausse adresse : le faux gestionnaire écrit l'argument reçu
     dans un fichier.
3. **Vérification réelle** (sur l'ordinateur de l'utilisateur, par le contrôleur) :
   - toujours dans un HOME, des `XDG_*` et un `WIMAO_DONNEES` de test, **jamais** dans
     `~/.wine-mao`, ni dans `~/.config/yabridgectl`, ni dans les vrais menus et associations ;
   - chaque gestionnaire est installé, ouvert jusqu'à l'écran de connexion, puis fermé
     proprement ;
   - le relais est vérifié en ouvrant une fausse adresse `native-access://` dans l'environnement
     de test ;
   - **aucun produit payant n'est installé**, pour ne pas consommer d'activations : seuls des
     produits gratuits le sont, si l'utilisateur se connecte. Valhalla Supermassive (gratuit) est
     installé de bout en bout.
4. **Traductions** : inchangé, le test échoue s'il manque un texte.

## 11. Risques connus

- Les adresses et la structure des installeurs changent avec les versions des éditeurs. Les
  recettes le prévoient : un motif `*`, une erreur claire si rien ne correspond, une correction
  sans nouvelle version de l'application.
- Le relais remplace, après confirmation, le gestionnaire de lien `native-access://` de
  l'installation actuelle (`~/.wine-mao`). L'utilisateur doit choisir lequel des deux
  environnements reçoit la connexion.
- L'inMusic Software Center réaffiche « Install » après une relance silencieuse : il revérifie la
  présence au clic ou à son redémarrage (`NOTES.md`). C'est documenté dans sa consigne.
