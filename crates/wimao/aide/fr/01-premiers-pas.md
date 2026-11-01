# Premiers pas

WiMao fait fonctionner vos plugins audio Windows (VST3, VST2, CLAP) sous Linux, dans votre logiciel de musique (DAW) : Bitwig, Ardour, REAPER… Vous n'avez rien à configurer à la main : WiMao installe vos plugins, les relie à Linux et vérifie qu'ils s'ouvrent.

## L'environnement Wine, c'est quoi ?

Wine permet de lancer des programmes Windows sous Linux. Pour cela, il range tout dans un dossier qui imite un ordinateur Windows : un disque `C:`, ses programmes et ses réglages. C'est ce dossier que WiMao appelle l'**environnement Wine**.

Vos plugins Windows et les applications des éditeurs (Native Access, Arturia Software Center…) s'installent dans cet environnement. WiMao le crée, l'entretient et le sauvegarde pour vous : vous n'avez jamais besoin de l'ouvrir.

Pour que votre logiciel de musique voie ces plugins, WiMao les « ponte » : il crée pour chacun un petit plugin Linux qui fait le lien avec le plugin Windows.

## Le premier lancement

Au premier lancement, un assistant vous accueille. Cliquez sur **Commencer**, puis :

- s'il ne trouve aucun environnement, il propose **Créer l'environnement** ;
- si vous utilisiez déjà un environnement dans `~/.wine-mao`, il propose **Importer cet environnement (recommandé)** ou **Créer un nouvel environnement**. Voir [Importer un environnement existant](aide:import).

Une fois l'environnement prêt, choisissez **Ajouter un installeur**, **Ouvrir un gestionnaire d'éditeur** ou **Plus tard**.

## La fenêtre principale

- La liste de vos plugins, avec l'état de chacun : **Prêt**, **À vérifier**, **Problème** ou **Pas testé**.
- La section **Gestionnaires** : les applications des éditeurs, à installer ou à ouvrir.
- En haut à gauche, **Ajouter un installeur**. En haut à droite : l'historique, **Réparer**, **Sauvegarder** et le menu principal.

Le menu principal permet aussi de changer de **Langue** et d'ouvrir ce **Mode d'emploi** (touche F1).

## Pour bien commencer

1. Ajoutez vos plugins : [Ajouter un plugin](aide:ajouter-un-plugin) ou [Gestionnaires d'éditeurs](aide:gestionnaires).
2. Indiquez une seule fois à votre logiciel de musique où les trouver : [Utiliser les plugins dans votre logiciel de musique](aide:logiciel).
3. Faites de temps en temps une sauvegarde sur un disque externe : [Sauvegarder et restaurer](aide:sauvegarde).
