# Gestionnaires d'éditeurs

Beaucoup d'éditeurs livrent leurs plugins par leur propre application : le gestionnaire. Vous vous y connectez avec votre compte, puis il télécharge, installe et active vos produits. WiMao sait installer et ouvrir ces quatre gestionnaires :

- **Native Access 2** (Native Instruments : Kontakt, Massive, Guitar Rig…) ;
- **Arturia Software Center** (Pigments, Analog Lab…) ;
- **inMusic Software Center** (plugins AIR : Hype, Velvet, Mini Grand…) ;
- **Plugin Alliance** (Installation Manager).

Ils sont listés dans la section **Gestionnaires** de la fenêtre principale.

## Installer un gestionnaire

Cliquez sur **Installer** à côté de son nom. WiMao le télécharge et l'installe tout seul. Le bouton devient ensuite **Ouvrir**.

Pour Native Access, WiMao pose une question **Lien de connexion** si un autre programme gère déjà ce lien : répondez **Utiliser WiMao**. Sans cela, le retour de connexion depuis votre navigateur n'arriverait pas jusqu'à Native Access.

## Une session de gestionnaire

1. Cliquez sur **Ouvrir**. Si d'autres programmes Windows sont ouverts, WiMao demande l'accord pour les fermer.
2. WiMao crée un point de restauration, puis ouvre le gestionnaire. Un bandeau indique qu'il est ouvert et que WiMao ponte les nouveaux plugins.
3. Connectez-vous à votre compte. Selon le gestionnaire, votre navigateur peut s'ouvrir pour la connexion, puis vous ramener au gestionnaire.
4. Installez et activez vos produits comme d'habitude. Pendant ce temps, WiMao ponte chaque nouveau plugin dès qu'il apparaît.
5. Quand vous avez fini, fermez le gestionnaire ou cliquez sur **J'ai fini** dans le bandeau.

Si un installeur tourne encore, WiMao attend sa fin. Vous pouvez choisir **Continuer d'attendre** ou **Arrêter quand même**. Un message résume ensuite la session, par exemple « Native Access 2 fermé : 3 plugins installés ».

Pendant une session, les autres actions (installer, réparer, sauvegarder) sont grisées : leur infobulle dit pourquoi.

## Conseils par gestionnaire

- **Native Access** : il a besoin d'un service de téléchargement et d'un dossier de téléchargement. WiMao les installe et les règle tout seul.
- **Arturia Software Center** : une erreur WebView2 au démarrage est sans conséquence.
- **inMusic Software Center** : ses installeurs sont relancés en silence par WiMao. Si un produit affiche encore « Install » après son installation, cliquez à nouveau dessus ou rouvrez le Software Center.
- **Plugin Alliance** : l'Installation Manager peut se figer à 98 % sur le dernier produit. Attendez que tous les produits soient indiqués comme installés, puis fermez-le. Le fermer plus tôt fait perdre les paquets téléchargés.

Un gestionnaire ne s'ouvre pas, ou la connexion ne revient pas ? Voir [Problèmes fréquents](aide:problemes).

## Autres applications

Certains installeurs ajoutés avec **Ajouter un installeur** n'installent aucun plugin : ils posent le lanceur de l'éditeur (par exemple BLEASS Plugins Installer). WiMao repère le nouveau raccourci du menu Démarrer et range ce lanceur dans la section **Gestionnaires**, sous les gestionnaires connus. Sa ligne indique « Application ajoutée · Installé ».

- **Ouvrir** lance une session exactement comme pour un gestionnaire : point de restauration, plugins pontés au fur et à mesure, puis **J'ai fini**.
- Le menu « ⋯ » propose **Retirer de la liste**, après confirmation. Cela n'enlève que la ligne : rien n'est désinstallé.

À la fin de l'installation, WiMao affiche « {nom} installé : ouvrez-le depuis la section Gestionnaires ».

Limite : un lanceur sans raccourci dans le menu Démarrer n'est pas détecté.

En ligne de commande : `wimao-cli application lister`, `wimao-cli application ouvrir <nom>` et `wimao-cli application retirer <nom>`.
