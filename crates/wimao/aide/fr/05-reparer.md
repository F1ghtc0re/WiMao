# Réparer

Avec le temps, l'environnement Wine peut se dérégler : un composant manque, un plugin n'est plus relié à Linux, Wine reste bloqué. Le bouton **Réparer** cherche ces défauts et les corrige.

## Quand l'utiliser ?

- Un plugin ou un gestionnaire refuse de s'ouvrir.
- Un message d'erreur de WiMao propose **Réparer** : un clic suffit, seul le défaut concerné est corrigé.
- Au démarrage, WiMao signale que des plugins ne sont pas encore pontés, par exemple si WiMao a été fermé pendant une session de gestionnaire. Le message propose **Réparer**.
- En cas de doute, vous pouvez essayer **Réparer** : vos plugins ne sont pas modifiés, et WiMao crée normalement d'abord un point de restauration, auquel vous pouvez revenir depuis l'historique.

## Comment faire

1. Fermez votre logiciel de musique.
2. Cliquez sur **Réparer**, en haut à droite (l'icône en forme d'engrenage).
3. WiMao cherche les problèmes. Réparer arrête Wine : si des programmes Windows sont ouverts, WiMao les nomme et demande **Fermer et réparer**. **Annuler** abandonne toute la réparation, sans rien modifier.
4. WiMao crée un point de restauration, puis corrige chaque défaut trouvé.

À la fin, un message indique **Réparation terminée** ou **Aucun problème détecté**.

## Ce que WiMao sait réparer

- des plugins installés mais pas encore pontés vers Linux ;
- Wine bloqué : il tourne alors que plus rien ne l'utilise ;
- des dossiers de l'environnement (Documents, Bureau…) qui mènent à vos dossiers personnels : les installeurs les rempliraient de leurs fichiers ;
- des composants Windows absents ou abîmés : Wine Mono (pour les programmes .NET), DXVK (pour l'affichage) et la bibliothèque msvcp140 de Microsoft ;
- le dossier de téléchargement de Native Access.

Chaque réparation réapplique aussi les réglages des gestionnaires installés et redémarre leurs services.

## Et si la réparation se passe mal ?

Le point de restauration créé juste avant permet de revenir en arrière : voir [Historique et retour en arrière](aide:historique). WiMao garde les trois derniers points créés avant une réparation, séparément de ceux des installations.

Si le défaut revient, utilisez **Copier le rapport** dans le menu principal et joignez-le à votre demande d'aide. WiMao en retire votre nom d'utilisateur, vos adresses e-mail et le nom de votre ordinateur. Rien n'est envoyé : c'est vous qui collez le rapport. Voir aussi [Problèmes fréquents](aide:problemes).
