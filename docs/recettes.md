# Format des recettes

Une recette est un fichier TOML qui décrit l'installation d'un produit (un gestionnaire comme
Native Access, un installeur, une archive). Il n'y a aucun code à écrire. WiMao choisit la
recette dont un motif `reconnaitre` correspond au nom du fichier choisi par l'utilisateur.

Le code qui lit les recettes est dans `crates/wimao-recettes` (`recette.rs` pour le format,
`catalogue.rs` pour la recherche). Les fichiers refusent toute clé inconnue : une faute de frappe
donne une erreur avec le numéro de ligne, jamais un réglage ignoré en silence.

## Exemple complet

```toml
nom = "Native Access 2"
editeur = "Native Instruments"
telecharger = "https://www.native-instruments.com/…/Native-Access-Installer.exe"
empreinte = "0f885b509a685d2bbfa652fed26b5fb31d88fbdab0a978c641d1c7b8aa460aa9"
type = "gestionnaire"
reconnaitre = ["Native Access*.exe"]

[installation]
methode = "extraire-7z"
lien-connexion = "native-access"

[[installation.extraire]]
archive = '$PLUGINSDIR/app-64.7z'
vers = 'C:\Program Files\Native Instruments\Native Access'

[[dependances]]
fichier = 'C:\Program Files\Native Instruments\Native Access\resources\daemon\win\NTKDaemon * Setup PC.exe'
methode = "installaware"

[correctifs]
registre = [{ cle = 'HKCU\Software\Native Instruments\Native Access', valeur = "DownloadLocation", donnee = "{telechargements}" }]
dossiers = ['{telechargements}\NI']
services = ["NTKDaemonService"]

[gestionnaire]
programme = 'C:\Program Files\Native Instruments\Native Access\Native Access.exe'
arguments = ["--no-sandbox", "--disable-gpu"]
pilote = ['{telechargements}\NI']

[consignes]
connexion.fr = "Connecte-toi à ton compte Native Instruments."
connexion.en = "Sign in to your Native Instruments account."
connexion.es = "Inicia sesión en tu cuenta de Native Instruments."
```

Le même fichier, commenté, sert de test : `crates/wimao-recettes/tests/recettes/valide.toml`.

## Clés de premier niveau

| Clé | Type | Défaut | Rôle |
|---|---|---|---|
| `nom` | texte | **obligatoire** | Nom du produit. Identifie la recette : si deux dossiers ont une recette de même nom, celle du premier dossier l'emporte. Exemple : `nom = "Serum 2"` |
| `editeur` | texte | `""` | Éditeur du produit. Exemple : `editeur = "Xfer Records"` |
| `telecharger` | texte | absent | Adresse de téléchargement de l'installeur. Doit commencer par `https://` (refusé sinon). Si l'utilisateur ne fournit pas de fichier, WiMao télécharge l'installeur dans `<cache>/telechargements/` ; chaque installation télécharge à nouveau. Exemple : `telecharger = "https://exemple.org/Installeur.exe"` |
| `empreinte` | texte | absent | Empreinte sha256 de l'installeur téléchargé : 64 caractères hexadécimaux (majuscules ou minuscules, conservée en minuscules). Si elle est présente, elle est vérifiée avant toute exécution ; sinon l'empreinte obtenue est écrite dans le journal. Exemple : `empreinte = "0f885b50…aa460aa9"` |
| `type` | `"gestionnaire"`, `"installeur"` ou `"archive"` | `"installeur"` | Nature du produit : application qui installe elle-même des plugins, installeur d'un produit, ou archive à décompresser. Exemple : `type = "gestionnaire"` |
| `reconnaitre` | liste de textes | **obligatoire, au moins un** | Motifs de noms de fichiers. `*` remplace n'importe quels caractères, `?` un seul, `[ab]` un caractère parmi ceux listés. Les majuscules ne comptent pas, et seul le nom du fichier est comparé (pas son dossier). Exemple : `reconnaitre = ["Native Access*.exe"]` |

## `[installation]`

| Clé | Type | Défaut | Rôle |
|---|---|---|---|
| `methode` | `"inno"`, `"msi"`, `"wix"`, `"nsis"`, `"installaware"`, `"extraire-7z"`, `"squirrel"` ou `"auto"` | `"auto"` | Façon d'installer. `auto` laisse WiMao reconnaître l'installeur. `extraire-7z` décompresse l'installeur avec `7z` dans un dossier temporaire du cache, puis applique chaque étape `[[installation.extraire]]`. `squirrel` sert aux applications « Squirrel » (inMusic Software Center) : l'installeur est décompressé, l'unique `*-full.nupkg` est lu (identifiant et version dans le `.nuspec`), puis installé dans `{appdata-local}\<identifiant>` (`app-<version>\` = contenu de `lib\net45`, `Update.exe`, `RELEASES`, `packages\<nupkg>`), comme l'installation manuelle décrite dans `NOTES.md`. Exemple : `methode = "inno"` |
| `lien-connexion` | texte | absent | Schéma de lien à relayer vers Wine (par exemple `native-access` pour les liens `native-access://`), utile pour se connecter à un compte. Le schéma ne contient que `a-z`, `0-9`, `+`, `.` et `-`. **Exige la section `[gestionnaire]`** : son `programme` reçoit le lien. Exemple : `lien-connexion = "native-access"` |

### `[[installation.extraire]]`

Liste d'étapes, **obligatoire (au moins une) avec `methode = "extraire-7z"`, refusée avec toute autre méthode**.

| Clé | Type | Rôle |
|---|---|---|
| `archive` | texte | Chemin de l'archive relatif au dossier décompressé (ni absolu, ni `..`). `*` admis : dans le nom du fichier, une seule correspondance est exigée ; dans un dossier, la version la plus haute est retenue. Exemple : `archive = '$PLUGINSDIR/app-64.7z'` |
| `vers` | chemin Windows | Dossier de destination. L'archive est décompressée à part, dans le cache, puis son contenu y est copié sans aucun lien symbolique. Exemple : `vers = 'C:\Program Files\Native Instruments\Native Access'` |

Les archives qui contiennent un installeur (Plugin Alliance, Valhalla) n'ont besoin d'aucune clé :
`methode = "auto"` les gère.

## `[[dependances]]`

Installeurs à lancer **après** l'installation principale, qui se trouvent déjà dans
l'environnement (par exemple NTKDaemon, déposé par Native Access).

| Clé | Type | Rôle |
|---|---|---|
| `fichier` | chemin Windows | Installeur à lancer. `*` admis partout. Dans le nom du fichier, un seul fichier doit correspondre, sinon l'erreur est claire et traduite. Dans un dossier (`app-*`), la version la plus haute est retenue (comparaison numérique des parties), comme pour `[gestionnaire].programme`. |
| `methode` | comme `[installation].methode` | Façon de l'installer en silencieux. **Ni `extraire-7z` ni `squirrel`** (refusées). Les codes de sortie InstallAware 100 et 110 comptent comme une réussite (`NOTES.md`). |

```toml
[[dependances]]
fichier = 'C:\Program Files\Native Instruments\Native Access\resources\daemon\win\NTKDaemon * Setup PC.exe'
methode = "installaware"
```

## `[correctifs]`

| Clé | Type | Défaut | Rôle |
|---|---|---|---|
| `registre` | liste de tables | `[]` | Valeurs de registre à écrire dans l'environnement Wine, à l'installation, à chaque ouverture du gestionnaire et à chaque « Réparer » (qui arrête alors Wine : les programmes ouverts sont nommés avant). Chaque table a trois clés obligatoires : `cle` (clé de registre), `valeur` (nom de la valeur) et `donnee` (contenu, variables admises). Exemple : `registre = [{ cle = 'HKCU\Software\Éditeur\Produit', valeur = "Dossier", donnee = "{telechargements}" }]` |
| `dossiers` | liste de chemins Windows | `[]` | Dossiers à créer s'ils manquent, à l'installation, à chaque ouverture et à chaque « Réparer ». Exemple : `dossiers = ['{telechargements}\NI']` |
| `services` | liste de textes | `[]` | Noms de services Windows, démarrés par `wine net start <nom>` à chaque ouverture du gestionnaire et à chaque « Réparer » d'un gestionnaire installé (la réparation arrête Wine, donc les services) ; un service déjà démarré n'est pas une erreur. Exemple : `services = ["NTKDaemonService"]` |

L'ancienne clé `lancement` est **retirée** (elle est refusée comme clé inconnue) : utiliser
`[gestionnaire].arguments`. Utiliser des guillemets simples (`'...'`) pour les clés de registre et
les chemins Windows : les `\` y sont littéraux.

### Variables

Utilisables dans tous les chemins Windows et dans les `donnee` du registre.

| Variable | Remplacée par |
|---|---|
| `{utilisateur}` | Nom du profil Windows : le nom d'utilisateur Linux (c'est ainsi que Wine nomme le profil) |
| `{telechargements}` | `C:\users\{utilisateur}\Downloads` |
| `{appdata-local}` | `C:\users\{utilisateur}\AppData\Local` |

## `[gestionnaire]`

**Obligatoire si `type = "gestionnaire"`, refusée sinon.** Décrit comment ouvrir l'application de
l'éditeur. Remplace l'ancienne section `[apres-installation]` (retirée) : `pilote` en fait partie.

| Clé | Type | Défaut | Rôle |
|---|---|---|---|
| `programme` | chemin Windows | **obligatoire** | Exécutable à lancer. `*` admis dans un dossier (`app-*` des applications Squirrel) ; si plusieurs dossiers correspondent, la version la plus haute est retenue (comparaison numérique des parties). Exemple : `programme = 'C:\Program Files\Native Instruments\Native Access\Native Access.exe'` |
| `arguments` | liste de textes | `[]` | Arguments ajoutés au lancement. Exemple : `arguments = ["--no-sandbox", "--disable-gpu"]` |
| `pilote` | liste de dossiers Windows | `[]` | Dossiers où le gestionnaire dépose les installeurs qu'il lance. Si la liste n'est pas vide, le pilote est actif : il ferme chaque installeur interactif lancé par le gestionnaire et le relance en silencieux. Exemple : `pilote = ['{telechargements}\NI']` |
| `fermer` | liste de noms d'exécutables | nom de fichier de `programme` | Processus à arrêter quand la session se ferme. Ces noms, et toujours celui de `programme`, sont aussi **cachés au pilote** : un processus qui en cite un n'est jamais fermé ni relancé par lui. N'y mettez donc pas un installeur à piloter. Exemple : `fermer = ["Native Access.exe"]` |

## `[consignes]`

Textes affichés à l'utilisateur quand il doit agir lui-même (se connecter, activer une licence).
Chaque consigne a un nom, puis un texte **dans les trois langues** `fr`, `en` et `es` : il en
manque une, la recette est refusée. Défaut : aucune consigne. Le texte est affiché dans la langue
de WiMao (en anglais si elle n'en a pas).

WiMao affiche lui-même deux noms de consigne ; un autre nom est accepté mais n'est montré nulle
part :

| Nom | Quand | Où |
|---|---|---|
| `connexion` | à l'ouverture du gestionnaire (`[gestionnaire]`), une fois | dialogue « Connexion à … » de l'interface ; ligne après « … est ouvert » dans `wimao-cli gestionnaire ouvrir` |
| `activation` | après une installation réussie de la recette (licence, numéro de série…) | dialogue « Activation de … » de l'interface, après « Ajouter un installeur » comme après « Installer » un gestionnaire ; dernière ligne de `wimao-cli installer` et de `wimao-cli gestionnaire installer` |

```toml
[consignes]
activation.fr = "Entre ton numéro de série."
activation.en = "Enter your serial number."
activation.es = "Introduce tu número de serie."
```

## Erreurs

Une recette invalide produit un message qui nomme le fichier, la ligne quand elle est connue et
la cause. Sortie réelle pour `tests/recettes/cle-inconnue.toml` (clé `telechargerr`) :

```text
recette invalide "…/tests/recettes/cle-inconnue.toml" (ligne 3) : unknown field `telechargerr`, expected one of `nom`, `editeur`, `telecharger`, `empreinte`, `type`, `reconnaitre`, `installation`, `correctifs`, `dependances`, `gestionnaire`, `consignes`
```

Le cadre (« recette invalide », « ligne ») vient de WiMao ; la cause des erreurs de syntaxe, de
clé ou de valeur est le message technique de la bibliothèque `toml`, qui reste en anglais. Les
contrôles propres à WiMao sont traduits : `reconnaitre` vide, consigne incomplète, motif illisible,
`extraire-7z` sans étape (ou étapes avec une autre méthode), `[gestionnaire]` manquante ou en trop,
`lien-connexion` sans `[gestionnaire]` ou au schéma invalide, dépendance en `extraire-7z` ou
`squirrel`, `telecharger` hors HTTPS, `empreinte` qui n'est pas un sha256.
Une recette invalide n'empêche pas les autres de se charger. Un dossier de recettes illisible
(droits refusés) est signalé de la même façon ; un dossier absent est ignoré.

## Où WiMao cherche les recettes

Par ordre de priorité (le premier dossier l'emporte à nom de recette égal) :

1. `<données de WiMao>/recettes` : les recettes de l'utilisateur ;
2. le dossier de la variable d'environnement `WIMAO_RECETTES` si elle est définie, sinon
   `/usr/share/wimao/recettes` (paquet) ;
3. à défaut, le dossier `recettes/` du dépôt (développement).

Pour ajouter une recette : voir « Ajouter une recette » dans [`CONTRIBUTING.md`](../CONTRIBUTING.md).
