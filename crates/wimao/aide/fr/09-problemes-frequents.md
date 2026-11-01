# Problèmes fréquents

## Un plugin est marqué « Problème »

Son test d'ouverture a échoué. Lisez l'explication sous son nom, puis :

1. Essayez-le quand même dans votre logiciel de musique : le test de WiMao utilise un autre hôte, qui plante parfois là où votre logiciel fonctionne.
2. Cliquez sur **Réparer** : un composant manquant est souvent en cause.
3. Si le plugin vient d'un gestionnaire, vérifiez qu'il y est bien installé et activé.

Un plugin marqué **À vérifier** s'est chargé, mais son interface n'a pas pu être vérifiée : essayez-le dans votre logiciel de musique.

## « Un plugin est en cours d'utilisation »

Le message complet est « Fermez votre logiciel de musique avant de continuer : un plugin est en cours d'utilisation ». WiMao refuse d'arrêter Wine pendant qu'un plugin travaille dans votre logiciel de musique, pour ne pas le faire planter. Rien n'a changé : fermez votre logiciel de musique, puis recommencez.

## Un gestionnaire ne s'ouvre pas

- Patientez : le premier démarrage d'un gestionnaire peut être lent.
- Si une fenêtre reste noire ou vide, fermez-la, puis cliquez sur **Réparer** : la réparation arrête Wine, réapplique les réglages des gestionnaires et redémarre leurs services. Rouvrez ensuite le gestionnaire.
- Le bouton **Ouvrir** est grisé ? Une autre opération ou une session est en cours : attendez la fin.

## Le navigateur ne revient pas au gestionnaire après la connexion

Pour Native Access, le retour de connexion passe par un lien `native-access://` que WiMao transmet.

- Laissez le gestionnaire ouvert pendant la connexion et acceptez l'ouverture du lien.
- Si WiMao affiche « Un lien de connexion n'a pas atteint le gestionnaire », rouvrez le lien depuis le navigateur.
- Si vous aviez répondu **Garder l'actuel** à la question **Lien de connexion**, le lien va vers un autre programme. Vous pouvez essayer d'ajouter à nouveau l'installeur de Native Access avec **Ajouter un installeur** : WiMao repose alors la question et vous répondez **Utiliser WiMao**. Sans garantie que cela suffise.
- Pour un plugin qui s'active par le navigateur (Serum 2, par exemple), suivez la consigne affichée lors de son installation.

## Disque plein pendant une sauvegarde

WiMao vérifie la place libre avant de commencer et indique la place nécessaire et celle disponible. Si le disque se remplit ou se débranche pendant l'écriture, WiMao efface les morceaux déjà écrits : votre environnement n'est pas touché. Libérez de la place ou changez de disque, puis recommencez. Voir [Sauvegarder et restaurer](aide:sauvegarde).

## L'import est refusé : deux disques différents

WiMao ne déplace l'environnement que sur un même disque et ne le copie jamais (trop volumineux). Rien n'a été modifié, et votre ancien environnement fonctionne toujours. Voir [Importer un environnement existant](aide:import).

## Toujours bloqué ?

Choisissez **Copier le rapport** dans le menu principal, puis collez-le dans votre demande d'aide. Une installation fautive peut aussi être défaite : voir [Historique et retour en arrière](aide:historique).
