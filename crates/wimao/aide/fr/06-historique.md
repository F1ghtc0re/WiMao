# Historique et retour en arrière

WiMao note tout ce qu'il fait dans un historique : installations, sessions de gestionnaire, réparations, sauvegardes, restaurations, imports et retours en arrière. Vous pouvez y revenir à l'état d'avant une action.

## Ouvrir l'historique

Cliquez sur le bouton en forme d'horloge, dans la barre de titre, ou appuyez sur Ctrl+H.

Les actions sont regroupées par jour : **Aujourd'hui**, **Hier**, puis la date. Chaque ligne indique ce qui a été fait, sur quoi, à quelle heure, et le résultat : **Réussi**, **Échec** ou **Annulé**. Tant qu'aucune action n'a été faite, la page affiche « Rien pour l'instant ».

## Les points de restauration

Avant chaque installation, chaque session de gestionnaire et chaque réparation, WiMao crée un point de restauration : une photo des réglages de l'environnement Wine et de la liste de ses fichiers.

WiMao garde les dix derniers points des installations et des sessions, et les trois derniers points des réparations. Les plus anciens sont supprimés automatiquement.

## Revenir à avant une action

1. Fermez votre logiciel de musique.
2. Dans l'historique, trouvez l'action à défaire et cliquez sur **Revenir à avant ceci**.
3. WiMao affiche le point utilisé, sa date et le nombre de fichiers qui seront supprimés. Confirmez.

WiMao remet alors les réglages de l'environnement tels qu'ils étaient, et supprime les fichiers apparus depuis : par exemple les plugins d'une installation ratée. Les liens vers des plugins qui n'existent plus sont aussi retirés des dossiers de plugins. Le retour en arrière apparaît à son tour dans l'historique.

Attention : un retour en arrière défait aussi tout ce qui a été fait **après** ce point. Les fichiers modifiés depuis ne sont pas remis à leur ancien contenu : seuls les réglages le sont.

## Pourquoi le bouton est-il absent ou grisé ?

- L'action n'a pas créé de point de restauration (une sauvegarde, par exemple).
- Le point a été supprimé, car il était trop ancien.
- Une sauvegarde a été restaurée depuis : les points plus anciens ne correspondent plus à l'environnement et ne sont plus proposés.
- Vous êtes revenu à avant une action plus ancienne : les points créés ensuite ne correspondent plus à l'environnement et ne sont plus proposés.
- L'import a été annulé, ou un nouvel environnement a été créé : les points existants décrivent un autre environnement et ne sont plus proposés.
- Une autre opération est en cours : attendez qu'elle se termine.

Pour remettre l'environnement entier dans un état connu, utilisez plutôt une sauvegarde : voir [Sauvegarder et restaurer](aide:sauvegarde).
