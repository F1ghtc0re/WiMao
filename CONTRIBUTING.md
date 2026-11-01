# Contribuer à WiMao

## Préparer sa machine

- Rust stable via <https://rustup.rs> (version 1.87 ou plus) avec `rustfmt` et `clippy`
  (`rustup component add rustfmt clippy`).
- Paquet `gettext` (`msgfmt`, `xgettext`, `msgmerge`) : requis pour compiler (les traductions sont compilées par `build.rs`).
- Outils appelés par WiMao et ses tests (`sudo apt install 7zip xvfb x11-utils cabextract`) : `7z` (paquet `7zip`, extraction des archives ; les tests d'archives l'exigent), `Xvfb` (paquet `xvfb`) et `xwininfo` (paquet `x11-utils`) pour le test d'ouverture d'un plugin, `cabextract` pour `msvcp140.dll`.
- Interface graphique (crate `wimao`) : `sudo apt install libgtk-4-dev libadwaita-1-dev` (GTK 4.12 et libadwaita 1.5 au moins).
- Wine et yabridge ne servent qu'aux tests d'intégration (voir plus bas).

## Lancer les tests

```sh
cargo test --workspace                   # tests unitaires
cargo fmt --check                        # format
cargo clippy --workspace --all-targets -- -D warnings  # aucun avertissement toléré
RUSTDOCFLAGS=-Dwarnings cargo doc --workspace --no-deps  # documentation sans avertissement (comme la CI)
scripts/verifier-traductions.sh          # traductions fr et es complètes
```

Les tests de l'interface (`cargo test -p wimao`) n'ouvrent aucune fenêtre et n'ont pas besoin d'écran :
le test de `travail.rs` n'utilise que la boucle d'événements de GLib. Pour **voir** la fenêtre sans
la poser sur votre bureau : `GDK_BACKEND=x11 xvfb-run -a -s "-screen 0 1280x800x24" cargo run -p wimao`
(avec `HOME` et `WIMAO_DONNEES` temporaires), puis capture avec `import -window root capture.png`
(ImageMagick) ; `ADW_DEBUG_COLOR_SCHEME=prefer-dark` donne le thème sombre.

### Tests d'intégration avec Wine

Les tests marqués `#[ignore = "Wine requis"]` n'ont pas lieu dans `cargo test` : ils exigent un
vrai Wine. Le plus complet, `crates/wimao-recettes/tests/integration.rs`, fait passer trois
installeurs factices (NSIS, MSI, Inno Setup) par l'orchestrateur d'installation, vérifie que le
faux plugin `WiMaoTest.vst3` est installé et ponté, puis revient en arrière ; deux autres tests du même fichier vérifient l'entrée d'historique laissée par une installation et le retour à partir du point lu dans l'historique. Le second fichier,
`crates/wimao-recettes/tests/gestionnaire.rs`, joue une session de gestionnaire d'éditeur de bout
en bout avec un faux gestionnaire (`FauxGestionnaire.exe`) : installation par sa recette
(`extraire-7z`), ouverture, le faux gestionnaire lance `test-nsis.exe` **sans** `/S` (l'installeur
attend alors un clic), le pilote le ferme et le relance en silencieux, le plugin est ponté, la
session se ferme (`session_pilote_et_ponte`) ; puis un lien `wimao-test://…` relayé par
`relayer_lien` doit arriver au faux gestionnaire (`relais_transmet_l_adresse`). Ce relais ne
déclare aucun lien au bureau : il lance seulement le programme dans l'environnement Wine de test.

1. Outils de fabrication : `sudo apt install nsis msitools wixl 7zip xvfb curl` (`wixl` est un
   paquet séparé de `msitools` sur Ubuntu récent ; `7z` fabrique `faux-gestionnaire-setup.exe`).
   Le compilateur `x86_64-w64-mingw32-gcc` (paquet `gcc-mingw-w64-x86-64`) n'est utile que pour
   reconstruire les binaires Windows versionnés (`WiMaoTest.vst3`, `FauxGestionnaire.exe`), avec
   la commande écrite en tête de leur source. Wine : `/opt/wimao/wine` ou `/opt/wine-mao-v2`, sinon
   `WIMAO_WINE_DIR=<dossier qui contient bin/wine>` ; yabridge de même avec `WIMAO_YABRIDGE_DIR`.
2. Fabriquer les installeurs (environ 15 s ; la première fois, télécharge Inno Setup 6.4.3 avec
   vérification de son empreinte SHA-256 et l'installe dans un préfixe Wine **temporaire**, supprimé
   ensuite ; rien n'est écrit dans `~/.wine-mao`) :
   `tests/fabriquer-fixtures.sh /chemin/vers/fixtures`
   → `test-nsis.exe`, `test.msi`, `faux-gestionnaire-setup.exe`, `test-inno.exe`
   (`faux-gestionnaire-setup.exe` est fabriqué avant l'étape Inno Setup, donc aussi en CI).
   `tests/fabriquer-fixtures.sh --seulement-nsis <dossier>` ne fabrique que `test-nsis.exe` (makensis suffit,
   ni Wine ni wixl ; utilisé par les tests d'installation des paquets) ; `tests/tester-fixtures.sh` teste cette option.
   La CI ne fabrique pas `test-inno.exe` (`WIMAO_FIXTURES_SANS_INNO=1`) : l'installeur d'Inno Setup
   échoue sur l'image GitHub, cause non trouvée. **Avant une demande de fusion qui touche aux
   installeurs, lancez donc ce test en local, sans cette variable.**
3. Lancer **toujours** avec un `HOME`, des dossiers `XDG_*` et un dossier de données temporaires,
   comme la CI : sinon Wine pourrait écrire des entrées de menu et des icônes dans votre vrai
   `~/.local/share` (incident réel, voir `NOTES.md`). Les tests utilisent `Chemins::pour_test`, qui
   isole déjà Wine (`XDG_*` sous le `HOME` de test, `winemenubuilder.exe=d`) ; ces variables sont
   une seconde protection. `CARGO_HOME` et `RUSTUP_HOME` sont fixés avant de changer `HOME`, sinon
   cargo ne trouve plus sa chaîne d'outils :
   ```sh
   export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
   export HOME="$(mktemp -d)"
   export XDG_DATA_HOME="$HOME/.local/share" XDG_CONFIG_HOME="$HOME/.config" XDG_CACHE_HOME="$HOME/.cache"
   export WIMAO_DONNEES="$HOME/donnees"
   WIMAO_FIXTURES=/chemin/vers/fixtures xvfb-run -a cargo test --workspace -- --ignored
   ```
   Pour le seul test des installeurs factices :
   `cargo test -p wimao-recettes --test integration -- --ignored --nocapture` (mêmes variables).
   Compter 1 à 2 minutes (trois environnements Wine neufs). Pour la session de gestionnaire :
   `cargo test -p wimao-recettes --test gestionnaire -- --ignored --nocapture` (environ 1 minute ;
   ses deux tests passent l'un après l'autre et partagent le cache de téléchargement
   `target/tmp/cache-gestionnaire`). Sans `WIMAO_FIXTURES`, le test se
   saute en le disant ; avec `WIMAO_FIXTURES` mais un fichier manquant, il échoue.
4. En CI, le job `integration` de `.github/workflows/ci.yml` fait la même chose avec le Wine
   d'Ubuntu et lance `cargo test --workspace -- --ignored`.

Les sources des fixtures sont dans `tests/fixtures/` : `plugin/` (le faux plugin : une DLL de 4 Ko
qui exporte `GetPluginFactory`, sans quoi yabridgectl ne la ponte pas ; sa source `WiMaoTest.c` donne
la commande de reconstruction), `nsis/test.nsi`, `msi/test.wxs`, `inno/test.iss`, et
`gestionnaire/FauxGestionnaire.c` (source du faux gestionnaire, binaire `FauxGestionnaire.exe`
versionné à côté, commande de reconstruction en tête). Sa recette de test est
`crates/wimao-recettes/tests/recettes-gestionnaire/faux-gestionnaire.toml`.

## Vérifications de la chaîne de fabrication

Si vous touchez aux workflows, aux scripts de `paquets/` ou à `scripts/signer-version.sh`, lancez en
local ce que fait le job `chaine` de la CI (aucun outil particulier, hors `shellcheck`) :

```sh
paquets/verifier-workflows.sh              # workflows figés (empreintes de commit, images, permissions)
paquets/tester-verifier-workflows.sh       # le contrôle ci-dessus refuse bien chaque forme non figée
scripts/tests/tester-signer-version.sh     # signature des versions, en mode à blanc
shellcheck -x paquets/*.sh scripts/signer-version.sh scripts/tests/*.sh   # aucun avertissement
```

Règles : une action GitHub est toujours désignée par l'empreinte complète de son commit
(`uses: …@<40 caractères> # v1.2.3`), une image de conteneur par `@sha256:…`, le workflow commence
par `permissions: contents: read`, et toute archive téléchargée a son empreinte SHA-256 écrite dans
le dépôt. Les dépendances Rust sont contrôlées par `cargo deny` et `cargo audit` (voir `deny.toml`).

Fabriquer et tester les paquets : `paquets/README.md`. Le test d'installation
(`paquets/tester-installation.sh`) ne se lance **que dans un conteneur jetable**, jamais sur votre machine.
Le workflow `paquets.yml` tourne sur chaque PR ; seul un `wine` à froid dure environ 70 minutes (son
`.deb` est ensuite mis en cache).

Pour les tests, n'exportez jamais `WIMAO_PAQUET=1` : cette variable est réservée à la fabrication des
paquets, et trois tests de l'interface échouent avec elle.

## Règle de fusion : demande de fusion et validation du propriétaire

**Toute modification de `main` passe par une demande de fusion (PR)** : pas de commit direct. Le
propriétaire du dépôt (`.github/CODEOWNERS`) doit la valider, et la CI (`ci.yml` et `paquets.yml`) doit
être verte avant la fusion. Les publications de versions suivent `docs/publier-une-version.md`.

## Conventions

- **Français** partout : identifiants, commentaires, documentation, messages de commit.
  Les chaînes visibles par l'utilisateur ont un `msgid` anglais traduit dans `po/` (voir
  « Ajouter une chaîne traduisible ») ; aucune ne doit échapper à `tr`/`trf`/`trn`.
- Chaque fichier Rust commence par un commentaire `//!` : rôle, ce qu'il utilise, qui l'utilise.
- Chaque élément public a un commentaire `///` (rôle, paramètres, erreurs possibles).
- Les commentaires expliquent le **pourquoi** ; toute valeur fixe ou contournement de Wine
  cite son origine (`NOTES.md`, bug Wine, documentation).
- Fonctions courtes (moins de 60 lignes), une responsabilité chacune, pas de valeur « magique ».
- Pas de `unwrap()` ni `expect()` hors des tests : les erreurs remontent dans `Erreur`.
- Si vous ajoutez ou renommez un fichier, mettez à jour `ARCHITECTURE.md` dans le même commit.
- Messages de commit : une ligne claire en français décrivant le résultat.
- Ne touchez jamais à `~/.wine-mao` ni `~/.config/yabridgectl` ; les tests utilisent
  `Chemins::pour_test` et un dossier temporaire.

## Guides pas à pas

Ajouter une recette, une chaîne traduisible, une règle d'erreur ou une langue.

### Ajouter une recette

1. Créer un fichier `recettes/<produit>.toml` à la racine du dépôt (nom en minuscules, sans espaces).
   Partir de l'exemple de `docs/recettes.md` ou de `crates/wimao-recettes/tests/recettes/valide.toml`.
2. Remplir au minimum `nom` et `reconnaitre` (un ou plusieurs motifs du nom du fichier d'installeur,
   par exemple `["Serum2*.exe"]` ; les majuscules ne comptent pas). Ajouter les `[correctifs]`
   constatés à la main (arguments de lancement, registre, services) et, pour chaque texte que
   l'utilisateur doit voir, une consigne en `fr`, `en` et `es`. Chaque clé est décrite dans `docs/recettes.md`.
   Pour un gestionnaire d'éditeur (`type = "gestionnaire"`), ajouter la section `[gestionnaire]`
   (obligatoire) et, selon le cas, `[[installation.extraire]]`, `[[dependances]]` et `lien-connexion` :
   `wimao-cli gestionnaire installer <nom>` et `wimao-cli gestionnaire ouvrir <nom>` (ou les boutons
   « Installer » et « Ouvrir » de la section « Gestionnaires » de l'interface) s'en servent.
3. Vérifier le fichier : `wimao-cli` n'a pas de commande dédiée ; `wimao-cli detecter <installeur>`
   affiche la recette retenue, et une recette invalide est signalée (« Warning, ignored recipe: … »,
   avec le numéro de ligne) par `wimao-cli installer`. Dans un test, appeler
   `Catalogue::charger(&[std::path::PathBuf::from("recettes")])` : la liste d'erreurs rendue doit
   être vide. Lancer aussi `cargo test -p wimao-recettes`.
4. Si le produit a un comportement particulier, noter dans `NOTES.md` le problème réel qui justifie un correctif.
5. Commiter la recette, message en français. Il n'y a pas de test par recette : la vérification de
   l'étape 3 en tient lieu.

### Ajouter une chaîne traduisible

1. Écrire le texte **en anglais** dans le code : `tr("Ready")`, ou `trf("{n} plugins found", &[("n", "3")])`
   quand il contient des valeurs (`{nom}` est remplacé par la valeur). Pour une chaîne à marquer
   sans la traduire sur place, utiliser `tr_noop("...")` (une fonction, pas une macro : `xgettext` ne reconnaît un mot-clé que suivi d'une parenthèse). Le texte doit être un littéral
   (xgettext ne lit pas les variables) et aucune chaîne visible ne doit échapper à `tr`.
   Pour un **pluriel**, utiliser `trn("{n} plugin", "{n} plugins", n, &[])` : `{n}` est remplacé par le nombre
   et la forme est choisie par les règles de la langue (`Plural-Forms` du `.po`) ; jamais de « (s) ». Dans
   `po/fr.po` et `po/es.po`, remplir `msgstr[0]` (singulier) et `msgstr[1]` (pluriel).
2. Lancer `scripts/verifier-traductions.sh` : il met à jour `po/wimao.pot` et ajoute la nouvelle
   entrée, vide, dans `po/fr.po` et `po/es.po`, puis échoue en la listant.
3. Remplir le `msgstr` dans `po/fr.po` et `po/es.po`, relancer le script : il doit rendre le code 0.
4. Commiter le code et les `.po`/`.pot` ensemble.

### Ajouter une règle d'erreur

Une règle reconnaît un texte d'erreur de Wine ou d'un installeur et le remplace par un message
compréhensible, avec si possible la réparation à proposer. Exemple : une DLL `foo.dll` introuvable.

1. Repérer le texte exact de l'erreur (journal Wine, sortie de l'installeur) et noter dans `NOTES.md`
   le problème réel qui la justifie : chaque règle cite la section d'où elle vient.
2. Ajouter dans `crates/wimao-core/ressources/regles.toml`, avec un commentaire qui cite cette section :

   ```toml
   [[regle]]
   motif = '(?i)foo\.dll.*not found'            # expression régulière (crate regex), sans casse ici
   message = "The foo library is missing: use Repair to install it"   # msgid ANGLAIS
   reparation = "Msvcp140Absent"                # facultatif : un Probleme réparable
   ```

   Les règles sont essayées dans l'ordre : mettre les plus précises avant les plus générales.
3. Recopier le même `message` dans `MESSAGES` de `crates/wimao-core/src/regles.rs`, avec `tr_noop("…")` :
   c'est ce qui permet à `xgettext` de le trouver. Le test `chaque_message_du_toml_est_declare`
   échoue si on l'oublie.
4. Traduire : `scripts/verifier-traductions.sh`, remplir `po/fr.po` et `po/es.po`, relancer jusqu'au code 0
   (voir « Ajouter une chaîne traduisible »).
5. Si la réparation n'existe pas encore : ajouter une variante à `Probleme` (`diagnostic.rs`), son nom dans
   `Probleme::depuis_nom`, son ordre dans `Probleme::rang`, son texte dans `message_probleme`, sa
   détection dans `diagnostiquer` et sa réparation dans `reparer_un` (exemple récent :
   `DossiersRelies`).
6. Ajouter un cas au test `chaque_regle_initiale_donne_son_probleme` (`regles.rs`) avec un vrai texte d'erreur,
   puis `cargo test -p wimao-core`.

### Ajouter une langue

Exemple avec l'allemand (`de`). Une langue se déclare à **trois** endroits, en plus de son fichier `.po` :

1. Créer le fichier : `msginit --no-translator --locale=de --input=po/wimao.pot --output-file=po/de.po`
   (lancer d'abord `scripts/verifier-traductions.sh` pour avoir un `po/wimao.pot` à jour).
   Vérifier la ligne `Plural-Forms` de l'en-tête (règles de pluriel de la langue, utilisées par `trn`).
2. Traduire chaque `msgstr` de `po/de.po` (et `msgstr[0]`, `msgstr[1]`… des pluriels), en gardant les
   `{nom}` tels quels. `build.rs` compile tout seul chaque `po/*.po` en `.mo`.
3. Déclarer la langue :
   - `scripts/verifier-traductions.sh`, variable `LANGUES=(fr es de)` : la vérification (et la CI)
     la contrôle ;
   - `crates/wimao-core/src/i18n.rs`, constante `LANGUES_PRISES_EN_CHARGE` (et sa taille) : sans
     cela, la langue du système (`LANGUAGE`, `LC_ALL`, `LANG`…) n'est pas reconnue et `--langue de`
     donne l'anglais ;
   - `crates/wimao/src/fenetre/entete.rs`, constante `LANGUES` : ajouter `("de", "Deutsch")` (nom écrit dans
     sa propre langue) pour la proposer dans le menu de l'interface.
4. Vérifier : `scripts/verifier-traductions.sh` doit rendre 0 ; `cargo run -p wimao-cli -- --langue de rapport`
   doit afficher de l'allemand ; ajouter un cas `init_langue(Some("de"))` aux tests de
   `crates/wimao-core/src/i18n.rs`.
