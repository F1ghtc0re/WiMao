# Ajouter un plugin

Vous avez téléchargé l'installeur Windows d'un plugin (un fichier `.exe` ou `.msi`, ou une archive `.zip` ou `.7z`) ? WiMao s'occupe du reste.

## Lancer l'installation

1. Cliquez sur **Ajouter un installeur** (ou appuyez sur Ctrl+O), puis choisissez le fichier. Vous pouvez aussi le glisser sur la fenêtre.
2. Si un programme Windows est encore ouvert (un gestionnaire, par exemple), WiMao le nomme et demande **Fermer et continuer** : l'installation doit arrêter Wine.
3. WiMao crée d'abord un point de restauration, puis lance l'installeur en silence, sans questions.
4. Il ponte ensuite les nouveaux plugins vers Linux et teste l'ouverture de chacun, dans une fenêtre invisible.

À la fin, un message résume ce qui a été fait, par exemple « 2 plugins installés ». Les plugins apparaissent dans la liste. Un plugin réinstallé dans une version plus récente porte la marque **Mis à jour** jusqu'à la fermeture de la fenêtre.

Si le plugin doit être activé avec votre compte (Serum 2, par exemple), une fenêtre « Activation de Serum 2 » explique quoi faire.

## Quand l'installation silencieuse ne suffit pas

Certains installeurs refusent de s'installer en silence. WiMao le remarque : aucun plugin n'a été ajouté. Il propose alors **Ouvrir l'installeur**. La fenêtre habituelle de l'installeur s'ouvre : suivez-la comme sous Windows, en gardant les dossiers proposés. Quand elle se ferme, WiMao ponte et teste les nouveaux plugins.

## L'état des plugins

- **Prêt** : le plugin s'ouvre, il est utilisable dans votre logiciel de musique.
- **À vérifier** : le plugin se charge, mais son interface n'a pas pu être vérifiée. Essayez-le dans votre logiciel de musique.
- **Problème** : le test a échoué. Voir [Problèmes fréquents](aide:problemes).
- **Pas testé** : le plugin n'a pas encore été testé. Les plugins CLAP ne sont pas encore testés.

Sous le nom de chaque plugin, une courte explication accompagne cet état.

## Bon à savoir

- Si un plugin existe en VST3 et en VST2, WiMao ne garde que le VST3 : vous n'aurez pas de doublons dans votre logiciel de musique.
- Les plugins Windows 32 bits ne sont pas pris en charge. Presque tous les plugins récents sont en 64 bits.
- Fermez votre logiciel de musique avant d'installer : WiMao refuse d'arrêter Wine pendant qu'un plugin est utilisé.
- Une installation ratée ? Vous pouvez revenir en arrière : voir [Historique et retour en arrière](aide:historique).
