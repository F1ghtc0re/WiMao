# Améliorations reportées

Ce fichier recense les **petits défauts connus** relevés pendant les relectures des sous-projets 1 à 4.
Aucun ne bloque l'usage. Ils ont été reportés pour ne pas élargir le sous-projet concerné. Il reprend aussi
les **décisions** prises en cours de route, avec leur raison, pour qu'un développeur ne les
défasse pas par erreur.

Mode d'emploi :
- avant de traiter un point, vérifier qu'il est toujours vrai : le code a pu changer depuis ;
- une fois un point corrigé, le retirer de ce fichier dans le même commit que la correction ;
- les chemins `fichier.rs:ligne` datent de la fusion du sous-projet 1 (commit `62b0ffe`).

## 1. À traiter en priorité

Aucun point : tous ceux de cette section ont été corrigés par le sous-projet 3 (restauration depuis
la fenêtre, points périmés après une restauration, liens cassés après « revenir », mise à jour
détectée par la ctime, `wineboot -u` avant les installeurs, dossier de travail vérifié, écriture
atomique des DLL, deux files de points de restauration). Les défauts relevés depuis sont dans
« Après le sous-projet 3 », plus bas.

## 2. Interface (crate `wimao`)

- `rafraichir` : après une panique, `panique()` réactive les actions sans rien dire de plus.
- L'icône est cherchée via `CARGO_MANIFEST_DIR`, ce qui ne marche qu'en développement. Le paquet
  (sous-projet 4) devra l'installer dans `hicolor`.
- `travail.rs` : le test n'a pas de délai maximal.

## 3. Traductions et messages

- Les erreurs de chargement de `regles.toml` (embarqué) et celles de `build.rs` sont en français,
  sans `tr`. Elles visent les développeurs, mais restent à traduire ou à passer en anglais.
- L'aide intégrée de clap (Usage, Options, Commands) n'est pas traduite.
- « Wine introuvable » s'affiche comme « not found: WiMao's Wine (…): install … », à cause du
  préfixe commun d'`Erreur::Introuvable`.
- Ajouter une langue demande de la déclarer à trois endroits : `i18n.rs`, `fenetre.rs` et
  `scripts/verifier-traductions.sh`. La spec dit « aucune modification du code ». Il faudrait
  déduire les langues des `.mo` présents, ou corriger la spec.
- Les `.po` contiennent des entrées obsolètes `#~`, des commentaires de code extraits par erreur et
  une date (`POT-Creation-Date`) qui pollue les diffs.
- xgettext affiche « unterminated character constant » à cause des `'static`. Aucune chaîne n'est
  perdue (vérifié), mais il faut le noter dans le script.
- `scripts/verifier-traductions.sh` : le motif `crates/*/tests/*` est trop large, et le script ne
  détecte pas un `.po` périmé.
- `build.rs` : un `.mo` périmé peut rester dans `OUT_DIR`.
- `dossier_traductions` choisit `/usr/share/locale` dès que `fr` y est présent. Il faudrait aussi
  vérifier `es`.
- Codes de langue limités à deux lettres.

## 4. Moteur (crate `wimao-core`)

### Wine et outils
- `reg_lire` : un code 0 sans ligne trouvée donne `None` sans rien signaler.
- `outils.rs` : un `try_wait` en erreur laisse le groupe de processus vivant ; le code -1 n'est pas
  documenté ; aucun test pour une sortie de plus de 64 Kio.
- `ureq` décompresse gzip par défaut, ce qui pourrait fausser une empreinte. À désactiver pour les
  téléchargements vérifiés.
- Le téléchargement d'Inno Setup des fixtures n'écrit pas dans un `.part`.

### Correctifs (Mono, DXVK, msvcp140)
- Le wineserver n'est pas arrêté si `wineboot` échoue.
- Un `mono-2.0` laissé par une installation interrompue donne un faux « déjà installé ».
- Mono : seule la vue 64 bits du registre est lue.
- `cabextract` n'est vérifié qu'après les installeurs.
- Originaux sauvegardés depuis un préfixe déjà corrigé : on garde alors de faux originaux.
- La DLL msvcp140 14.44 provoque un avertissement « version inconnue » à chaque fois. Il faut
  ajouter ses empreintes après validation avec Maschine.
- `remove_dir_all` ignorés sans commentaire (`extraire_cab`, `dxvk::extraire`).

### Inventaire, pontage, test des plugins
- Pas de test d'erreur d'E/S (chmod 000), ni de VST2 en sous-dossier ou de `.DLL` en majuscules.
- `contient_motif` panique sur un motif vide.
- `lire_lien` ne normalise pas les `..` d'une cible relative.
- Un plugin hors des dossiers connus est classé « ponté » sans contrôle, et le CLAP n'est pas
  vérifié.
- `Erreur::NonPrisEnCharge` sert aussi pour un PATH invalide et une erreur TOML.
- Xvfb : sa survie est vérifiée par une pause fixe. Lire le PID dans `/tmp/.X<n>-lock` serait
  déterministe, et ce chemin est en dur hors de `Reglages`.
- Le journal du plugin est relu en entier toutes les 3 s.

### Restauration et sauvegarde
- Un échec sur le 2ᵉ ou le 3ᵉ `.reg` laisse le registre à moitié restauré sans le dire.
- Des fichiers `.part` peuvent rester dans le préfixe après un plantage. Ils apparaissent alors dans
  l'aperçu.
- Les points de restauration incomplets ne sont jamais élagués, et un dossier étranger est compté
  comme un point.
- `plugins_en_cours` ignore `yabridge-group` et ne canonicalise pas `WINEPREFIX`.
- Restauration : la place libre côté données n'est pas vérifiée ; `verifier_vraisemblable` suit les
  liens symboliques.
- Un `hard_link` est impossible sur exFAT. Les sauvegardes sur un tel disque ne sont pas testées
  (FAT32 non testé sur un vrai volume).

### Diagnostic et réparation
- Un verrou orphelin dont le PID a été réutilisé n'est pas reconnu, et il reste une course possible
  dans `lier_verrou`.
- `VCRUNTIME140` mène à `Msvcp140Absent`, qui ne pose que msvcp140.
- Pas de test pour une section dont le nom est le préfixe d'une autre.
- Une archive contenant un installeur « Inconnu » n'est pas couverte.

## 5. Recettes et installation (crate `wimao-recettes`)

- La détection s'arrête au premier bloc contenant un motif, une limite non documentée. Elle lit
  aussi chaque `.exe`/`.dll` en entier.
- Collisions non signalées : même nom dans deux sous-dossiers, bundles fusionnés, archives au même
  nom, installeurs laissés en cache.
- Les erreurs d'E/S d'`archive.rs` ne donnent pas le chemin. Le message de 7z est doublé
  (« WARNING »), et `ligne_utile` retient toute ligne contenant « error ».
- Un nom en double dans un même dossier de recettes est écarté sans erreur, et la comparaison des
  noms est sensible à la casse.
- Le test de l'archive passe sans rien vérifier si 7z est absent.
- `tests/echantillons/README.md` : « en-tête MZ » est faux pour `msi.bin`.

## 6. Tests et intégration continue

- **Inno Setup en CI** : l'installeur d'Inno Setup 6.4 échoue sur les machines de GitHub avec
  « Error reading FPreparingMemo.Lines.Strings: A call to an OS function failed ». Ça arrive avec
  le Wine d'Ubuntu (9.0) comme avec celui de WineHQ (11.0), en `en_US` et avec des polices. Il
  passe sur un poste de bureau. La CI le saute (`WIMAO_FIXTURES_SANS_INNO=1`) et le test se fait en
  local (CONTRIBUTING.md). Cause non trouvée.
- wixl ancien en CI : avertissement « no property named 'Platform' ».
- Pas de cache CI (Cargo, Wine).
- Les tests de langue ne remettent pas l'état global en place s'ils échouent, et les doctests
  `tr`/`trf` n'ont pas d'assertion.
- Le test qui désactive chmod 000 se base sur un `/root` lisible. Il vaudrait mieux tester
  `geteuid`.
- Des dépendances (`gettext`, `serial_test`, etc.) sont hors de `workspace.dependencies`.

## 7. Décisions prises pendant le sous-projet 1

À ne pas défaire sans en peser la raison.

1. **Traductions lues directement dans les `.mo`** (crate `gettext`) : ne dépend pas des locales
   générées par le système. Sinon, un utilisateur qui choisit l'espagnol sans `es_ES` générée
   resterait en anglais.
2. **Pas de repli sur l'écran réel** pour tester un plugin. Sans Xvfb, WiMao le signale. Le paquet
   `.deb` dépendra de `xvfb`.
3. **Aucun extrait d'installeur propriétaire dans le dépôt**, qui deviendra public. On utilise des
   échantillons synthétiques et des installeurs factices. Un test `#[ignore]` sur les vrais
   installeurs est possible via `WIMAO_TEST_INSTALLEURS`.
4. **« Créer un point » et « revenir » refusent si un plugin tourne** (processus
   `yabridge-host*.exe`), pour ne pas faire planter le logiciel de musique. `apercu_retour` donne la
   liste exacte de ce qui sera supprimé.
5. **Le diagnostic lit `user.reg`/`system.reg` directement**, sans lancer Wine : vraie lecture
   seule, pas de wineserver démarré, pas d'attente.
6. **`wimao-cli verifier --prefixe` n'écrit rien**, pas même dans le journal.
7. **winemenubuilder est désactivé seulement en isolation** (tests, fixtures, CI). En usage réel, on
   le garde pour que les gestionnaires (Native Access, Arturia Software Center, inMusic) apparaissent
   dans le menu du bureau.
8. **Dossiers Documents, Bureau, etc. : de vrais dossiers dans le préfixe**, sans lien vers le
   dossier personnel. Le diagnostic repère et répare les liens recréés par une mise à jour de Wine.
   On ne bloque pas les mises à jour de Wine (`.update-timestamp`).
9. **CI avec le Wine stable de WineHQ**, sans le faux installeur Inno Setup (voir § 6).
10. **Le journal** vit dans `journal.rs` (`OnceLock`), initialisé par la CLI et l'interface.

## Après le sous-projet 2 (gestionnaires, pilote, liens de connexion)

Relevés pendant les relectures du sous-projet 2 ; chemins valables au commit `4c28d65`.

### À traiter en priorité

- **Valhalla Supermassive non vérifié de bout en bout.** valhalladsp.com refuse les téléchargements
  automatiques : la recette n'a ni `telecharger` ni `empreinte`, et son motif `reconnaitre` est à
  confirmer avec le vrai fichier (NOTES.md, vérification réelle du sous-projet 2).

### Autres

- « Réparer » nomme aussi `NTKDaemon.exe` (service de Native Access) dès que Native Access est
  installé : sûr (le service est redémarré ensuite), mais bavard.
- La question avant « Réparer » et la réparation se font en deux temps : un programme ouvert entre
  les deux n'est pas nommé.
- Le refus de « Réparer » n'a pas de test de bout en bout (retour immédiat).
- Le raccourci du lien de connexion (`.desktop`, `liens.rs`) ne fige ni `WIMAO_DONNEES` ni
  `WIMAO_WINE_DIR` : avec des données hors défaut, le navigateur relaie vers l'environnement par
  défaut. Aucun filtrage de l'adresse (blancs, caractères de contrôle) ; il n'y a pas de shell.
- `journaux/<gestionnaire>.log` et `journaux/liens.log` grossissent sans rotation.
- Un échec d'arrêt du gestionnaire à la fin d'une session n'est écrit qu'au journal : l'utilisateur
  n'est pas prévenu qu'une instance survit (fenêtre noire d'inMusic à l'ouverture suivante).
- Si le pilote ET le relevé échouent dans un même tour de session, les événements de ce tour sont
  perdus (les relances restent dans le bilan).
- `ArturiaSoftwareCenterAgent.exe` reste ouvert après la session : voulu (l'ajouter à `fermer`
  empêcherait la session de se terminer seule).
- Doublons : `test_utils` entre wimao-core et wimao-recettes ; `message_aucun_plugin` à trois
  endroits.
- Le sélecteur CSS de la bannière de session dépend de la structure interne d'`AdwBanner`.
- `comparer_versions` n'est pas transitive pour des versions mêlant lettres et chiffres.
- `FauxGestionnaire.c` ne distingue pas `WAIT_TIMEOUT`/`WAIT_FAILED` et perd les liens de plus de
  4 096 octets (sans effet sur les tests actuels).
- Le motif `reconnaitre` de Serum 2 couvre aussi l'installeur « Update » (même plugin, sans les
  sons) : inoffensif.
- Le test du rapport lance `wine --version` (sans effet sur un préfixe).

## Après le sous-projet 3 (historique, restauration, import, assistant, mode d'emploi)

Relevés pendant les relectures du sous-projet 3, réécrits ici sans le détail du code. Le sujet
« Valhalla Supermassive non vérifié de bout en bout » (section précédente) reste ouvert.

### Ce que voit l'utilisateur

- **Historique.**
  - Les libellés d'action sont à l'infinitif (« Réparer ») ; des noms (« Réparation ») seraient plus
    naturels dans une colonne.
  - Deux Ctrl+H rapides empilent deux pages « Historique ».
  - Le bouton de retour reste muet quand un travail est en cours.
- **Retour en arrière.**
  - L'aperçu ne montre pas tous les fichiers de pontage yabridge qui seront retirés.
  - Si une racine de plugins pose problème, les autres sont abandonnées et le compte partiel est
    perdu ; l'échec du marquage des points périmés n'est écrit qu'au journal technique.
  - Un dossier `.vst3` placé dans un autre `.vst3` donne une erreur « introuvable » au lieu d'être
    ignoré (disposition peu réaliste).
- **Mise à jour d'un plugin.**
  - La marque « Mis à jour » n'est posée qu'en fin de session de gestionnaire.
  - Elle est perdue quand on change de langue, car la fenêtre est reconstruite.
- **Assistant et import.**
  - Après un échec suivi d'un retour avec un environnement à moitié créé, « Créer » relance sur un
    environnement déjà présent.
  - Les icônes « prêt » et « erreur » ne suivent pas la couleur d'accent.
  - Les tailles s'affichent en « 1024,0 KiB » au lieu de « 1 MiB » à la limite.
- **Mode d'emploi.** La touche F1 et le bouton « Problèmes fréquents » n'ont pas été vérifiés à
  l'écran, et les titres balisés, les titres du type « 2024. foo » et les adresses contenant une
  parenthèse fermante ne sont pas testés.
- **Langue.** Changer de langue relit les réglages, ce qui peut renommer un fichier `reglages.toml`
  abîmé au passage.
- **Installation.** `wineboot -u` est lancé aussi pour les archives et les installeurs Squirrel :
  quelques secondes de plus, sans autre effet.
- **Concurrence.** Deux processus (interface et ligne de commande) qui écrivent l'historique en
  même temps ne sont pas protégés l'un de l'autre.
- **Langue de l'historique.** Le sujet et les détails d'une entrée sont écrits dans la langue du
  moment (« avant l'installation de … », « 7363 fichiers supprimés ») et ne changent pas avec la
  langue. Il faudrait stocker des données neutres dans `historique.jsonl` et traduire à l'affichage.
- **Bilan d'installation.** Un plugin seulement « À vérifier » est compté « avec un problème ».
- **Points périmés (cas limites).**
  - Une coupure entre la mise à jour de la trace d'import et le marquage des points laisse des
    points utilisables ; `reprendre_interruption` ne refait pas ce marquage. Inverser l'ordre suffit
    (le marquage peut être refait sans risque).
  - `revenir` vérifie qu'un point n'est pas périmé avant de prendre le verrou : un autre processus
    peut le périmer entre les deux. Revérifier après le verrou.
  - `creer_environnement` périme tous les points dès que `system.reg` manque, même si le reste de
    l'environnement est intact (prudent, mais un point qui aurait pu le réparer devient inutilisable).
- **Test d'ouverture des plugins : fausses alertes** (vérifié le 2026-11-01 dans Bitwig 6, avec
  l'environnement importé). Le test de WiMao passe par Carla, qui ne se comporte pas comme un vrai
  logiciel de musique :
  - **DrumSynth** (AIR) est marqué **Problème** (« l'hôte de plugins s'est arrêté ») : c'est Carla
    qui plante (assertions JUCE `juce_VST3PluginFormat.cpp:3636` et Carla `parameterId < count`, puis
    erreur de segmentation). Dans Bitwig, le plugin se charge et son interface s'affiche ;
  - **Vocal Doubler** (iZotope) est marqué **À vérifier** alors qu'il se charge et s'affiche dans
    Bitwig.
  Pistes : distinguer un plantage de l'hôte de test d'un plantage du plugin dans Wine (le journal
  yabridge dit si le plugin a été chargé) ; essayer un autre hôte de test plus proche d'un vrai
  logiciel de musique ; ou présenter ce cas comme « à essayer dans votre logiciel de musique ».

### Qualité du code

Sans effet visible pour l'utilisateur :

- doublons : `demander_accord` / `confirmer_arret_wine` / `confirmer_fermeture_programmes` ; constantes
  (`NOM_ANCIEN`, choix « introuvable / déjà présent ») recopiées du moteur ;
- `#[allow(dead_code)]` sur `pousser` à retirer ; `apparence_mis_a_jour` publique sans besoin ;
  paramètre `_p` ignoré de `mettre_a_jour` ;
- lignes de documentation trop longues (`restauration.rs`, `processus.rs`, `fenetre/actions.rs`,
  `ARCHITECTURE.md`), en-tête de `liste.rs` à reformater, documentation périmée de `import.rs`
  (injections) et de `date_du_point` (suffixe `-2`) ;
- `tm_gmtoff` assigné à un `i64` (64 bits seulement) ; limite de la ctime (`chmod`) à documenter
  dans `Signature` ;
- messages sans contexte de traduction (installation / réparation) ; commentaire de traducteur en
  français extrait dans les `.po` ;
- le test `format_d_une_ligne` cherche `point` et `details` sans les guillemets de clé JSON ;
- tests manquants : formatage du décalage horaire, échec de `ajouter`, ordre `wineboot` /
  installeur, échec de `mettre_a_jour`, racines de liens et VST2/CLAP gardées, `RetourIncomplet`,
  échec de `revenir` / `reparer` / `Session::fermer`, question « programmes ouverts » de bout en
  bout, marque sans statut ; un test dépend de la langue anglaise (« 1 plugin processed ») et
  `jour_local_iso` est testé de façon alambiquée ;
- les chemins d'échec d'annulation d'import appellent le vrai `synchroniser_parent`.

## Après le sous-projet 4 (paquets Debian, chaîne de confiance)

Relevés pendant les relectures du sous-projet 4. Voir `paquets/README.md` et
`docs/publier-une-version.md` pour le fonctionnement.

### À faire au moment de la première vraie version

- **Date de publication de la fiche AppStream.** La date du `<release>` de
  `crates/wimao/data/fr.wimao.WiMao.metainfo.xml` est provisoire : la fixer à la date réelle de la publication.
- **Mises à jour Dependabot.** Sur le nouveau dépôt public, Dependabot a reproposé (2026-11-01) les mises à jour
  majeures des actions `actions/cache`, `actions/download-artifact`, `actions/upload-artifact` et de la crate
  `serial_test` : à examiner une par une après la première version (changements incompatibles possibles).

### Paquets et fabrication

- **Avertissement `xgettext`** signalé à la compilation sur `crates/wimao/src/travail.rs:112` : à comprendre
  et à corriger (la ligne est dans un test).
- **Bibliothèques de yabridge livrées en 0755.** Les `.so` de yabridge sont exécutables (comme dans
  l'archive officielle) : écart `lintian` assumé (`shared-library-is-executable`) dans
  `paquets/bridge/lintian-overrides`.
- **`g++-mingw-w64` non ajouté aux dépendances de compilation de Wine** (seul `gcc-mingw-w64` y est,
  dans `paquets/fabriquer-wine.sh`) : Wine se compile, mais les modules en C++ sont alors sautés ;
  à ajouter si l'un d'eux manque.
- **Dépendances de `wimao-wine` calculées sur Ubuntu 24.04.** Elles sont relevées par `dpkg-shlibdeps` dans
  l'image de fabrication ; leur validité sur Debian 13 n'est vérifiée que par le test d'installation
  de la CI (qui échouerait si une dépendance manquait).
- **Trois `build.rs` dupliquent le même bloc `cfg`** (`wimao`, `wimao-core`, `wimao-recettes`) qui pose
  `wimao_paquet` : à factoriser si un quatrième apparaît.
- **`WIMAO_PAQUET=1 cargo test` échoue** (trois tests de l'interface qui lisent les traductions dans
  le dépôt). Sans conséquence tant que la variable n'est jamais exportée pour les tests ; ne pas
  la poser dans un shell de développement.
- **Clé de cache de Wine.** Les paquets Ubuntu de compilation ne font pas partie de la clé de cache du job
  `wine` : après une mise à jour d'Ubuntu, un `.deb` en cache peut être périmé. Le rebuild à froid fait sur chaque étiquette
  (`reproductible-wine`) le détecte.
- **La CI dépend d'une publication tierce** : le plugin Dexed 1.1.0 du test d'installation est téléchargé
  depuis une Release GitHub tierce (empreinte SHA-256 figée dans `paquets.yml`, job `installation`). Si elle disparaît,
  le test échoue : prévoir un autre plugin libre ou un miroir.
- **Empreinte de `msvcp140.dll`.** La `msvcp140.dll` téléchargée chez Microsoft a désormais une empreinte
  inconnue (`97e71a3c…`). WiMao l'utilise quand même, en affichant un avis : rafraîchir les
  empreintes connues (`SHA256_CONNUE_X64` et `SHA256_CONNUE_X86` dans `crates/wimao-core/src/correctifs/msvcp.rs`).

### Distribution

Ordre prévu, du plus simple au plus lourd (conseil retenu pendant la conception) :

1. **`.deb`** (fait, ce sous-projet) ;
2. **AUR** (Arch Linux) : un `PKGBUILD` qui s'appuie sur les mêmes sources et les mêmes empreintes ;
3. **Flatpak / Flathub** : difficile, car l'interface a besoin de lancer Wine et yabridge et d'écrire dans
   `~/.vst3`, ce que le bac à sable limite ; demande une étude ;
4. **Dépôt APT signé** (fait, sous-projet 6 : `.deb` publiés dans un dépôt que `apt` met à jour tout seul,
   sur GitHub Pages ; voir `docs/publier-une-version.md`, étape 5) ;
5. **PPA** (Launchpad : paquets sources, fabrication chez Canonical) ;
6. **Snap** : déconseillé (confinement incompatible avec Wine et les plugins).

- **Avertissement du lien yabridge invisible dans l'interface.** Quand `~/.local/share/yabridge` mène à un
  autre yabridge que celui de WiMao, `wimao-cli diagnostiquer` l'affiche, mais le bouton « Réparer » de
  l'interface répond « Aucun problème détecté » si c'est le seul avertissement. L'afficher aussi dans
  l'interface (sans proposer de réparation automatique : le lien n'est jamais modifié par WiMao).
- **Contrôle des secrets de `verifier-workflows.sh`** : un commentaire en fin de ligne qui contient
  « secrets. » est refusé à tort (faux positif sans danger ; le contrôle reste du côté du refus).
