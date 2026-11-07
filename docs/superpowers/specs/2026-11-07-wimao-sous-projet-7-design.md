# WiMao — sous-projet 7 : fiabilité pour l'utilisateur

Date : 2026-11-07 · Statut : à valider · Branche : `sous-projet-7`

## 1. But

Corriger ce que l'utilisateur **voit** de faux ou de maladroit à l'usage, sans ajouter de fonction :

1. le test d'ouverture des plugins n'accuse plus un plugin à tort ;
2. l'avertissement « lien yabridge » est visible dans l'interface ;
3. l'historique s'affiche dans la langue choisie ;
4. sept finitions de l'historique, de la liste et de l'assistant.

**Critère de réussite :** sur la machine du propriétaire, `wimao-cli verifier` ne marque plus DrumSynth
« ❌ » ni Vocal Doubler « ⚠️ » (s'ils se chargent) ; « Réparer » montre le lien yabridge qui mène à
`/opt/wine-mao-v2/yabridge` ; changer de langue traduit les nouvelles entrées de l'historique.

**Hors périmètre :** Valhalla (le site refuse les téléchargements automatiques), le nettoyage interne
sans effet visible, AUR, Flatpak, les cas limites de concurrence de l'historique.

## 2. Constat (essai du 2026-11-07, `docs/ameliorations-reportees.md`)

Essai fait sur la machine du propriétaire avec `carla-single` sur Xvfb, comme `test_plugin.rs` :

- **DrumSynth** (AIR) : interface ouverte (« DrumSynth (GUI) »), Carla vivant pendant 5 minutes, malgré
  des lignes `JUCE Assertion failure in juce_VST3PluginFormat.cpp:3636` dans le journal. Le « Problème »
  du 2026-11-01 venait d'un plantage **de Carla** (assertion Carla `parameterId < count`, puis
  segmentation), intermittent, pas du plugin.
- **Vocal Doubler** (iZotope) : yabridge écrit `[Wine STDERR] Finished initializing '…/Vocal Doubler.vst3'`,
  donc le plugin est chargé dans Wine ; mais aucune fenêtre n'apparaît en 5 minutes sur Xvfb, même avec
  `LIBGL_ALWAYS_SOFTWARE=1` (journal : `libEGL warning: DRI3 error`). Son interface marche dans Bitwig.

Conclusion : l'hôte de test (Carla sur Xvfb) n'est pas un vrai logiciel de musique. Le journal de
yabridge permet de savoir si **le plugin** s'est chargé ; c'est ce fait qui doit décider du verdict.

## 3. Test d'ouverture des plugins (`crates/wimao-core/src/test_plugin.rs`, `tableau.rs`)

### 3.1 Signaux lus dans le journal de Carla (qui contient aussi la sortie de yabridge)

| Signal | Reconnu par |
|---|---|
| **Chargé** | une ligne qui contient `[Wine STDERR] Finished initializing` |
| **Plantage dans Wine** (plugin) | une ligne qui contient `[Wine STDERR]` **et** un motif de plantage |
| **Plantage hors Wine** (Carla) | un motif de plantage dans une ligne **sans** `[Wine STDERR]` |
| **Sans audio** | `Failed to init engine` (inchangé) |

Motifs de plantage : ceux d'aujourd'hui (`aborting`, `stack overflow`, `Unhandled exception`) plus
`page fault` (message de Wine). Ils restent dans une constante unique, commentée.

### 3.2 Verdicts

Nouveau cas de `ResultatTest` : **`Charge`** (sans texte) — « le plugin s'est chargé ; son interface
n'a pas pu être vérifiée par le test ». Ordre de décision (le premier qui s'applique) :

1. plantage dans Wine → `Probleme(ligne)` (inchangé : c'est le plugin) ;
2. fenêtre « (GUI) » vue, puis vivant après l'attente de 10 s → `Pret` (inchangé) ;
3. sans audio → `Remarque` (inchangé) ;
4. Carla arrêté ou plantage hors Wine : **chargé** → `Charge` ; **pas chargé** → `Probleme`
   (« The plugin host stopped » ou la ligne de plantage, comme aujourd'hui) ;
5. délai de 300 s écoulé sans fenêtre : **chargé** → `Charge` ; **pas chargé** →
   `Remarque(tr("The plugin did not finish loading within 5 minutes"))` ;
6. écran virtuel muet, outil absent, CLAP : inchangés (`Remarque`).

La cause exacte (Carla arrêté, délai, ligne d'assertion) est notée au **journal technique**
(`journal::noter`), pour le diagnostic, jamais montrée comme une faute du plugin.

### 3.3 Affichage (`tableau.rs`)

`Charge` donne **`Statut::Bon` (✅)** avec le détail constant (msgid, traduit fr/es) :
`"Loads correctly; open it in your music software to check its interface"`.
Le msgid `"Loaded, but the interface could not be checked"` disparaît (remplacé).

Choix assumé : ✅ plutôt que ⚠️, parce qu'un plugin chargé dans Wine fonctionne dans la très grande
majorité des cas et qu'une fausse alerte pousse l'utilisateur à désinstaller un plugin sain. Le détail
dit clairement ce qui n'a pas été vérifié.

### 3.4 Conséquences

- `wimao-cli verifier` : code de sortie inchangé dans son principe (❌ ⇒ code d'échec) ; un `Charge`
  compte comme réussi.
- `paquets/tester-installation.sh` (CI des paquets, plugin factice WiMaoTest) : accepte aujourd'hui
  ❌ « The plugin host stopped » ou ⚠️ « Loaded, but… » (codes 2 ou 0). Il doit accepter à la place
  ❌ « The plugin host stopped » (non chargé) ou ✅ avec le nouveau détail (code 0), et refuser tout
  le reste.
- Tests unitaires de `test_plugin.rs` (faux `carla-single`) : un cas par ligne du § 3.2.

## 4. Avertissement « lien yabridge » dans l'interface (`crates/wimao/src/reparation.rs`)

Aujourd'hui, si le diagnostic ne trouve que cet avertissement, « Réparer » répond « Aucun problème
détecté ». Désormais :

- les avertissements non réparables du diagnostic sont montrés dans une **boîte de dialogue**
  (`adw::AlertDialog`) titrée « Information », avec le texte existant de `message_lien_yabridge`
  (déjà traduit) et deux boutons : **« Copier la commande »** (met la commande `ln …` dans le
  presse-papiers) et **« Fermer »** ;
- s'il n'y a **aussi** rien à réparer, le toast « Aucun problème détecté » n'est pas affiché (la boîte
  le remplace) ; sinon la réparation se fait, puis la boîte s'affiche ;
- WiMao **ne modifie jamais** ce lien lui-même (décision du sous-projet 4, inchangée).

Le texte de la commande est fourni par le moteur (nouvelle fonction publique qui rend la commande seule,
à côté de `message_lien_yabridge`), pour ne pas la reconstruire dans l'interface.

## 5. Historique dans la langue choisie (`crates/wimao-core/src/historique.rs`)

### 5.1 Format

`Entree` gagne deux champs facultatifs, **en plus** de `sujet` et `details` (gardés, écrits dans la
langue du moment, pour la compatibilité avec les anciennes versions et la lecture directe du fichier) :

```json
{"date":"…","action":"retour","sujet":"avant l'installation de Serum 2","resultat":"reussi",
 "sujet_neutre":{"cle":"before installing {name}","valeurs":{"name":"Serum 2"}}}
```

- `sujet_neutre` / `details_neutre` : `{ cle: String, valeurs: BTreeMap<String, String> }`, omis si absents ;
- `cle` est un msgid anglais **déclaré** dans une liste fermée du module (constantes `tr_noop`) ;
  une clé inconnue (fichier modifié à la main, version plus récente) fait retomber sur `sujet` / `details` ;
- les `valeurs` ne sont jamais traduites (noms de plugins, nombres, chemins).

### 5.2 Affichage

`Entree::sujet_affiche()` et `Entree::details_affiches()` rendent le texte neutre traduit s'il existe et
si sa clé est connue, sinon le texte brut. La CLI (`historique`) et la page Historique s'en servent.
Les anciennes entrées s'affichent comme aujourd'hui.

### 5.3 Écriture

`ajouter` reçoit le texte neutre facultatif (nouvelle signature ou fonction voisine). Chaque appelant
qui écrit aujourd'hui une phrase traduite fournit sa clé : diagnostic (réparation), sauvegarde,
restauration (retour), import (cinq appels), installation, gestionnaire. Les causes d'échec venues d'une
erreur restent du texte libre (déjà traduit au moment de l'écriture) : limite documentée.

## 6. Finitions

1. **Libellés d'action en noms** : « Réparation », « Retour en arrière », « Sauvegarde »,
   « Restauration »… Les msgids restent ceux d'aujourd'hui mais passent par un **contexte de
   traduction** `history` (nouvelle fonction `trc(contexte, msgid)` dans `i18n.rs`, `pgettext` ;
   mot-clé `--keyword=trc:1c,2` dans `scripts/verifier-traductions.sh`), pour ne pas changer le
   bouton « Réparer » qui partage le msgid `Repair`. Traductions fr et es fournies.
2. **Double Ctrl+H** : `ouvrir_historique` ne pousse pas de page si l'Historique est déjà affiché.
3. **Tailles** : `taille_lisible` passe à l'unité suivante quand l'arrondi donne `1024,0`
   (1 048 575 octets → « 1,0 Mio », plus « 1024,0 Kio »).
4. **Icônes de l'assistant** : « prêt » et « erreur » utilisent les classes de style d'Adwaita
   (`success` / `accent` pour prêt, `error` pour erreur) au lieu de couleurs fixes, et suivent donc le
   thème et la couleur d'accent.
5. **Marque « Mis à jour »** : conservée quand la fenêtre est reconstruite au changement de langue
   (l'ensemble `mis_a_jour` est repris par la nouvelle fenêtre).
6. **Bilan d'installation** : un plugin ⚠️ n'est plus compté « avec un problème » ; il est compté
   « à vérifier » (`{n} to check`, au pluriel par `trn`) ; seuls les ❌ comptent « avec un problème ».
7. **« Créer » sur un environnement à moitié créé** (échec, puis retour à l'accueil) : la création
   reprend l'environnement existant sans erreur ni doublon. Un test reproduit le cas (création
   interrompue, puis nouvelle création) et vérifie un environnement complet.

## 7. Traductions

Tout nouveau texte est un msgid anglais, traduit en fr et es dans `po/`. `scripts/verifier-traductions.sh`
reste vert.

## 8. Tests

- Tests unitaires Rust pour chaque point (§ 3.2 un cas par ligne ; § 5 lecture d'anciennes et de
  nouvelles entrées, clé inconnue, changement de langue ; § 6 un test par finition quand c'est
  testable hors écran).
- `paquets/tester-installation.sh` adapté (§ 3.4), CI des paquets verte.
- Vérification réelle en fin de sous-projet (contrôleur, machine du propriétaire) :
  `wimao-cli verifier` sur DrumSynth et Vocal Doubler ; « Réparer » dans l'interface (boîte du lien
  yabridge) ; changement de langue avec la page Historique ouverte.

## 9. Documentation

`docs/ameliorations-reportees.md` : retirer les points traités (fausses alertes du test, lien yabridge
invisible, langue de l'historique, et les sept finitions) ; ajouter la limite du § 5.3. Mode d'emploi
intégré (`aide`) : expliquer le sens de « Se charge correctement… » s'il décrit les verdicts.
