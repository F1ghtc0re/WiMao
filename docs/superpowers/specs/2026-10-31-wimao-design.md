# WiMao : conception du logiciel

Date : 2026-10-31 · Statut : à valider

## 1. Objectif

WiMao est un logiciel Linux libre (licence GPL) qui fait fonctionner les plugins audio Windows
(VST2, VST3, CLAP) dans tous les logiciels de musique Linux (Bitwig, Reaper, Ardour…), sans
terminal et sans IA. Il automatise tout ce qui a été mis au point à la main dans ce dépôt
(voir `NOTES.md` et `scripts/`).

**Ce que l'utilisateur a demandé :**
- ajouter facilement des plugins, depuis un installeur classique ou depuis un gestionnaire
  d'éditeur (Native Access, Arturia Software Center, inMusic Software Center, Plugin Alliance) ;
- un assistant pas à pas et un mode d'emploi intégré ; un logiciel simple, beau et efficace ;
- les solutions trouvées ensemble automatisées sous forme de « recettes » ;
- interface en **français, anglais et espagnol** ;
- licence libre et gratuite (GPL) ; dépôt rendu public plus tard, après retrait des données
  personnelles ;
- distributions : Ubuntu et Debian d'abord (`.deb`), puis AUR, puis Flatpak ;
- technologie : **Rust + GTK4/libadwaita**.

**Hypothèses :** processeurs x86_64 uniquement ; une seule installation de Wine par utilisateur.

**Critère de réussite :** sur une Ubuntu neuve, un utilisateur installe le `.deb`, ajoute
l'installeur d'un plugin, et le plugin apparaît et s'ouvre dans son logiciel de musique, sans
jamais ouvrir de terminal.

## 2. Découpage en sous-projets

| N° | Sous-projet | Contenu |
|----|-------------|---------|
| 1 | Moteur + interface minimale | environnement Wine, correctifs, pontage, tests de plugins, installeurs génériques, sauvegardes, traductions ; fenêtre simple |
| 2 | Recettes des gestionnaires | Native Access, inMusic, Arturia, Plugin Alliance, Serum, Valhalla ; « pilote » ; relais des liens de connexion |
| 3 | Interface complète | assistant pas à pas, mode d'emploi intégré, historique, finitions visuelles |
| 4 | Packaging | `.deb` (Ubuntu, Debian), puis AUR, puis Flatpak |

Chaque sous-projet a son propre plan d'implémentation. **Ce document fixe l'architecture
d'ensemble et détaille le sous-projet 1.** Les sections 4 à 8 décrivent le comportement final ;
la section 9 précise ce qui entre dans le sous-projet 1.

## 3. Architecture

### 3.1 Paquets

- **`wimao-wine`** : Wine corrigé (actuellement Wine 11.20 + annulation de la garde
  `wm_state_serial`, la « v2 » de ce dépôt), installé dans `/opt/wimao/wine`. Il évolue
  séparément de l'application.
- **`wimao-bridge`** : yabridge et DXVK (logiciels libres, redistribuables), dans `/opt/wimao`.
- **`wimao`** : l'application ; elle dépend des deux paquets précédents.

**Non inclus, pour des raisons juridiques : les DLL de Microsoft** (Visual C++ Redistributable).
Leur licence interdit de les redistribuer hors du paquet officiel. WiMao les obtient ainsi,
dans cet ordre :
1. il réutilise celles qu'un installeur de plugin a déjà déposées dans Wine (`Package Cache`) ;
2. sinon, il télécharge le paquet officiel depuis l'adresse permanente de Microsoft, puis depuis
   une adresse de secours ; il accepte aussi un fichier fourni par l'utilisateur ;
3. il extrait les DLL et règle Wine pour les utiliser (`native,builtin`), comme
   `scripts/install-msvcp140.sh`.

WiMao garde la liste des empreintes (sha256) des versions testées. Si la version obtenue est
inconnue, il l'utilise quand même et le signale. Sans internet, il l'explique et réessaie plus
tard. Les adresses de téléchargement sont dans les recettes, ce qui permet de les corriger sans
publier une nouvelle version de l'application.

### 3.2 Modules de l'application (espace de travail Cargo)

1. **`wimao-core`** (bibliothèque, sans interface) : création et réparation de l'environnement
   Wine, correctifs (DXVK, DLL Microsoft, Wine Mono), pontage yabridge, retrait des doublons
   VST2, test d'ouverture des plugins, points de restauration, sauvegardes, traduction des
   erreurs connues, journal.
2. **`wimao-recettes`** (bibliothèque) : lecture et validation des recettes, reconnaissance des
   types d'installeurs, exécution des installations. Dépend de `wimao-core`.
3. **`wimao`** (application GTK4/libadwaita) : interface uniquement. Elle appelle les deux
   bibliothèques et ne contient aucune logique d'installation.

Une petite commande `wimao-cli` (sans interface) expose les mêmes fonctions. Elle sert aux tests
et au dépannage ; l'utilisateur n'en a jamais besoin.

### 3.3 Emplacements chez l'utilisateur

- Environnement Wine : `~/.local/share/wimao/prefixe`.
- Données de WiMao (historique, points de restauration, recettes mises à jour, journaux) :
  `~/.local/share/wimao/`.
- Réglages : `~/.config/wimao/`.
- Plugins pontés : `~/.vst3`, `~/.clap` (et `~/.vst` pour les VST2), lus par tous les logiciels
  de musique.
- yabridgectl est lancé avec sa propre configuration (dans `~/.local/share/wimao/`) : WiMao ne
  modifie jamais `~/.config/yabridgectl`. Si un plugin du même nom est déjà ponté depuis un autre
  environnement Wine, WiMao ne l'écrase pas et le signale.
- Liens de connexion des gestionnaires (`native-access://`, etc.) : déclarés auprès du bureau
  par des fichiers `.desktop` et relayés vers Wine, comme `scripts/native-access-lien.sh`.

## 4. Recettes

Une recette est un fichier TOML qui décrit l'installation d'un produit. Il n'y a pas de code à
écrire. Exemple :

```toml
nom = "Native Access 2"
editeur = "Native Instruments"
telecharger = "https://www.native-instruments.com/…/Native-Access-Installer.exe"
type = "gestionnaire"            # gestionnaire | installeur | archive
reconnaitre = ["Native Access*.exe"]  # noms de fichiers d'installeur auxquels la recette s'applique

[installation]
methode = "extraire-7z"          # ou : inno, msi, wix, nsis, installaware
lien-connexion = "native-access" # relais du lien native-access:// vers Wine

[correctifs]
lancement = ["--no-sandbox", "--disable-gpu"]
registre = [{ cle = 'HKCU\Software\Native Instruments\Native Access', valeur = "DownloadLocation", donnee = "{telechargements}" }]
services = ["NTKDaemon"]

[apres-installation]
pilote = true                    # relance en silencieux les installeurs lancés par le gestionnaire

[consignes]                      # textes affichés à l'utilisateur, dans les trois langues
connexion.fr = "Connecte-toi à ton compte Native Instruments."
connexion.en = "Sign in to your Native Instruments account."
connexion.es = "Inicia sesión en tu cuenta de Native Instruments."
```

- WiMao choisit la recette dont un motif `reconnaitre` correspond au nom du fichier choisi.
- Les recettes sont validées à la lecture : une clé inconnue ou une valeur invalide produit un
  message clair qui indique la ligne en cause.
- **Installeur sans recette** : WiMao reconnaît le type (Inno Setup, MSI, WiX/Burn, NSIS,
  InstallAware) et l'installe en silencieux (`/VERYSILENT`, `/qn`, `-quiet`, `/S`…). En cas
  d'échec ou de type inconnu, il ouvre l'installeur normalement.
- Les recettes fournies sont installées avec l'application. Une mise à jour est possible depuis
  le dépôt GitHub : les fichiers sont téléchargés, puis validés, avant d'être utilisés.
- **Le pilote** : pendant qu'un gestionnaire tourne, WiMao surveille les installeurs qu'il lance.
  Il ferme chaque installeur interactif et le relance en silencieux (méthode de
  `pilote-inmusic.sh`). C'est une fonction du moteur, activée par la recette.

## 5. Parcours d'ajout d'un plugin

1. L'utilisateur choisit un fichier (`.exe`, `.msi`, `.zip`) ou un gestionnaire dans la liste.
2. WiMao cherche une recette ; sinon, il reconnaît le type d'installeur.
3. Il crée un point de restauration (section 6).
4. Il relève le contenu des dossiers de plugins de Wine (VST3, VST2, CLAP), lance l'installation,
   puis relève à nouveau. La différence donne les plugins ajoutés.
5. Il ponte les nouveaux plugins avec yabridge et met en liste d'exclusion les VST2 qui ont un
   équivalent VST3.
6. Il teste chaque nouveau plugin en arrière-plan : chargement, puis ouverture de l'interface,
   avec une limite de temps, dans un processus séparé.
7. Il affiche le résultat de chaque plugin : ✅ prêt, ⚠️ fonctionne avec une remarque (par
   exemple, une activation est nécessaire), ❌ problème, avec l'explication et la réparation
   proposée.
8. Il enregistre dans l'historique ce qui a été installé, avec quelle recette et quel résultat.

**Activation et connexion à un compte** : WiMao ouvre la fenêtre de l'éditeur et affiche à côté
la consigne de la recette. C'est le seul moment où l'utilisateur agit lui-même.

## 6. Gestion des erreurs

- **Point de restauration** avant chaque installation, mise à jour ou correctif : copie des
  fichiers de registre de Wine (`system.reg`, `user.reg`, `userdef.reg`) et liste des fichiers
  présents. « Revenir en arrière » restaure le registre et supprime les fichiers ajoutés depuis.
  *Limite :* un fichier existant qu'un installeur a écrasé n'est pas restauré. Seule une
  sauvegarde complète couvre ce cas. WiMao conserve les 10 derniers points de restauration.
- **Messages compréhensibles** : une table de règles associe un motif d'erreur (par exemple
  `MSVCP140.dll not found`) à une explication dans les trois langues et à une réparation. Les
  problèmes décrits dans `NOTES.md` deviennent des règles.
- **Réparer** : vérifie et corrige les problèmes connus (DLL Microsoft, DXVK, pontage
  désynchronisé, service arrêté, processus Wine bloqués) sans tout réinstaller.
- **Isolation** : les tests de plugins tournent dans un processus séparé, tué à la fin du délai ;
  un plugin qui plante est marqué ❌ avec la raison.
- **Rapport de problème** : « Copier le rapport » rassemble la version de WiMao, la recette, la
  configuration et la fin du journal, et remplace le nom d'utilisateur, les adresses e-mail et
  les chemins personnels par des valeurs neutres.
- **Sauvegardes** : reprise de `scripts/sauvegarde-prefixe.sh` (fermeture de Wine, contrôle de
  la place libre, découpage en morceaux de 3,9 Go sur FAT32, empreintes sha256, refus d'écraser
  une sauvegarde existante) et ajout de la restauration.

## 7. Langues

- Français, anglais et espagnol dès la première version, avec `gettext` (fichiers `.po`).
- La langue suit celle du système et peut être changée dans les préférences.
- Les messages d'erreur, le mode d'emploi et les consignes des recettes sont traduits.
- Ajouter une langue ne demande aucune modification du code.

## 8. Tests

1. **Tests unitaires** (sans Wine, à chaque envoi sur GitHub) : lecture et validation des
   recettes, reconnaissance des installeurs, détection des plugins ajoutés, règles d'erreurs,
   anonymisation des rapports.
2. **Tests d'intégration avec Wine** (GitHub) : petits installeurs factices Inno, MSI et NSIS,
   fabriqués sous Linux, qui installent un plugin factice ; WiMao doit les reconnaître, les
   installer et les ponter. Les installeurs WiX (Burn) ne peuvent pas être fabriqués sous Linux :
   leur reconnaissance est testée sur un échantillon, et leur installation au niveau 3.
   L'ouverture d'un vrai plugin est vérifiée au niveau 3.
3. **Recettes réelles** (sur une machine avec les comptes, hors GitHub) : `wimao-cli verifier`
   teste tous les plugins installés et produit un tableau ✅/⚠️/❌.
4. **Traductions** : un test échoue si un texte manque dans l'une des trois langues.
5. **Paquet `.deb`** (sous-projet 4) : installation dans des conteneurs Ubuntu et Debian vierges,
   puis vérification du démarrage et de la création de l'environnement Wine.

## 9. Périmètre du sous-projet 1

**Inclus :**
- espace de travail Cargo avec `wimao-core`, `wimao-recettes`, `wimao`, `wimao-cli` ;
- création de l'environnement Wine (avec le Wine de `/opt/wimao/wine`, ou celui de
  `/opt/wine-mao-v2` pendant le développement), Wine Mono, DXVK, DLL Microsoft ;
- installation des installeurs sans recette (Inno, MSI, WiX, NSIS, InstallAware) et format des
  recettes (lecture, validation, exécution des méthodes d'installation et des correctifs) ;
- détection des plugins ajoutés, pontage yabridge, exclusion des doublons VST2 ;
- test d'ouverture des plugins ;
- points de restauration, « Réparer », règles d'erreurs, rapport anonymisé ;
- sauvegarde et restauration ;
- traductions FR/EN/ES ;
- **interface minimale** : une fenêtre avec la liste des plugins et leur état, et les boutons
  « Ajouter un installeur », « Réparer », « Sauvegarder » ;
- tests des niveaux 1, 2, 3 et 4.

**Exclu (sous-projets suivants) :** recettes des gestionnaires, pilote, relais des liens de
connexion, assistant, mode d'emploi, historique avec retour en arrière, paquets.

**Migration de l'installation actuelle :** WiMao ne touche pas à `~/.wine-mao`. Une fonction
« Importer un environnement existant », qui le déplacera vers `~/.local/share/wimao/prefixe`
après une sauvegarde, est prévue dans le sous-projet 3.
