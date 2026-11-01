# Sauvegarder et restaurer

Une sauvegarde copie tout l'environnement Wine dans une seule archive : tout ce qui est installé dans cet environnement, donc vos plugins, leurs réglages, leurs licences et les sons qu'ils y ont placés. Ce qui se trouve ailleurs sur vos disques n'en fait pas partie. Si votre disque lâche ou qu'une mise à jour casse tout, vous retrouvez votre installation sans tout retélécharger.

## Faire une sauvegarde

1. Fermez votre logiciel de musique et branchez un disque externe.
2. Cliquez sur **Sauvegarder**, en haut à droite (l'icône en forme de disque).
3. Choisissez le dossier de destination, de préférence sur le disque externe.
4. Attendez la fin : cela prend plusieurs minutes, selon la taille de vos plugins et de leurs sons.

WiMao vérifie d'abord qu'il y a assez de place libre. Il range la sauvegarde dans un dossier `WiMao-sauvegarde`, sous la forme d'une archive datée, par exemple `wimao-2026-11-01-14h30.tar.zst`, accompagnée d'un fichier `.sha256`. Ce fichier contient les empreintes qui permettent de vérifier que l'archive est intacte. Gardez toujours les deux ensemble.

À la fin, un message indique où la sauvegarde a été enregistrée. Le bouton **Afficher** ouvre le dossier.

## Bon à savoir

- Sur un disque formaté en FAT32, qui n'accepte pas les fichiers de plus de 4 Go, l'archive est découpée en morceaux de 3,9 Go (`.part000`, `.part001`…). Gardez tous les morceaux.
- Trois dossiers de chaque utilisateur Windows de l'environnement ne sont pas sauvegardés : le dossier temporaire, le dossier **Downloads** (les installeurs téléchargés) et le cache de DXVK. Ils sont inutiles ou se recréent tout seuls.
- Si l'écriture échoue (disque débranché ou plein), WiMao efface les morceaux déjà écrits. Il ne laisse jamais une sauvegarde incomplète qui aurait l'air complète.

## Restaurer une sauvegarde

1. Fermez votre logiciel de musique et branchez le disque qui contient la sauvegarde.
2. Dans le menu principal, choisissez **Restaurer une sauvegarde…**.
3. Sélectionnez le fichier `.sha256` de la sauvegarde.
4. WiMao affiche la date et la taille de l'archive. Confirmez.

WiMao vérifie d'abord les empreintes de tous les morceaux. Si l'une est fausse, il s'arrête sans rien toucher. Sinon, il extrait l'archive à côté de l'environnement actuel, puis échange les deux. Votre environnement actuel n'est pas effacé : il est gardé sous le nom `prefixe.ancien-<date>`, dans le dossier de données de WiMao. Vous pourrez le supprimer vous-même une fois rassuré.

À la fin, un message confirme **Sauvegarde restaurée** et la liste des plugins est relue.

Après une restauration, les anciens points de restauration ne correspondent plus à l'environnement : l'historique ne les propose plus. Voir [Historique et retour en arrière](aide:historique).

Un problème de place ? Voir [Problèmes fréquents](aide:problemes).
