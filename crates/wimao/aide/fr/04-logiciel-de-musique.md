# Utiliser les plugins dans votre logiciel de musique

WiMao range les plugins pontés dans trois dossiers de votre dossier personnel, un par format :

- `~/.vst3/yabridge` pour les plugins VST3 ;
- `~/.vst/yabridge` pour les plugins VST2 ;
- `~/.clap/yabridge` pour les plugins CLAP.

Le signe `~` désigne votre dossier personnel. Ces dossiers commencent par un point : ils sont cachés. Dans un sélecteur de fichiers, affichez les dossiers cachés (souvent avec Ctrl+H) ou tapez directement le chemin.

Ce sont de simples plugins Linux : tout logiciel de musique Linux qui charge ces formats peut les utiliser (Bitwig, Ardour, REAPER, Renoise, Carla, Qtractor…). Pour l'instant, seul Bitwig a été testé avec WiMao.

## Réglage à faire une seule fois

La plupart des logiciels parcourent déjà `~/.vst3` et `~/.clap`, sous-dossiers compris : vos plugins VST3 et CLAP sont donc souvent trouvés tout seuls. Le VST2 n'a pas d'emplacement standard : il faut en général ajouter `~/.vst/yabridge` à la main. En cas de doute, ajoutez les trois dossiers.

### Bitwig

1. Ouvrez Bitwig, puis ses réglages : **Paramètres → Emplacements**.
2. Dans la partie des emplacements de plugins, ajoutez les trois dossiers ci-dessus.
3. Cliquez sur **Rescan** pour que Bitwig relise les dossiers.

### Ardour

Dans les préférences, partie Plugins, ajoutez si besoin les dossiers VST ci-dessus, puis lancez la recherche des plugins depuis le gestionnaire de plugins (Plugin Manager).

### Autres logiciels

REAPER, Renoise… : ajoutez les mêmes dossiers dans leurs réglages de plugins, puis relancez la recherche des plugins.

Vos plugins Windows apparaissent alors dans le navigateur de plugins, à côté des plugins Linux. Vous les utilisez exactement comme les autres.

## Après chaque ajout

Il n'y a rien à refaire dans les réglages. Après une installation ou une session de gestionnaire, relancez simplement la recherche des plugins dans votre logiciel (ou redémarrez-le) pour voir les nouveaux plugins.

## Fermer votre logiciel de musique avant certaines actions

Quand un plugin Windows est ouvert dans votre logiciel, Wine travaille pour lui. WiMao refuse donc d'arrêter Wine tant qu'un plugin tourne : sinon, le logiciel perdrait le plugin en pleine séance. Fermez-le avant de :

- installer un plugin ou ouvrir un gestionnaire ;
- réparer ;
- sauvegarder ou restaurer ;
- revenir en arrière depuis l'historique ;
- importer un environnement.

Si vous oubliez, WiMao affiche « Fermez votre logiciel de musique avant de continuer : un plugin est en cours d'utilisation ». Rien n'est modifié : fermez votre logiciel et recommencez.

## Bon à savoir

- Le premier chargement d'un plugin Windows peut prendre quelques secondes de plus que celui d'un plugin Linux.
- Un plugin marqué **Problème** dans WiMao peut parfois fonctionner dans votre logiciel : le test de WiMao utilise un autre logiciel hôte. Essayez-le avant de vous inquiéter.
- Après un retour en arrière, les plugins retirés disparaissent de ces dossiers. Relancez la recherche des plugins pour que votre logiciel les oublie.

Un plugin refuse de s'ouvrir ? Voir [Problèmes fréquents](aide:problemes).

## Logiciel de musique installé en Flatpak

Un logiciel installé en Flatpak (Bitwig depuis Flathub, REAPER, Ardour, Carla, LMMS, Zrythm, Qtractor, Renoise) est isolé du reste du système. WiMao s'en occupe :

- il crée tout seul le lien `~/.var/app/<id>/data/yabridge` vers le yabridge de WiMao, au démarrage, au pontage et avec **Réparer**. Un lien déjà présent n'est jamais remplacé ;
- il rafraîchit les dates des plugins pontés, pour que le logiciel les relise.

Si le Flatpak ne voit pas `/opt/wimao`, le dossier de données de WiMao, `~/.vst3`, `~/.vst` ou `~/.clap`, ses plugins pontés ne se chargeront pas. Cliquez sur **Réparer** : une fenêtre vous l'explique et un bouton copie la commande `flatpak override --user --filesystem=…` à lancer. WiMao ne la lance jamais : c'est à vous de décider. En ligne de commande, `wimao-cli diagnostiquer` donne le même avertissement.

**Bitwig** garde en mémoire (cache) un plugin qui a échoué à se charger. Redémarrez Bitwig : le rafraîchissement des dates fait le reste.

Un logiciel Flatpak absent de la liste ? Créez le lien vous-même : `ln -s /opt/wimao/yabridge ~/.var/app/<id>/data/yabridge`.
