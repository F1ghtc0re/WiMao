# Importer un environnement existant

Avant WiMao, vos plugins étaient peut-être déjà installés à la main, dans un environnement Wine rangé dans le dossier `~/.wine-mao`. Plutôt que de tout réinstaller, WiMao peut le reprendre tel quel.

## Quand l'import est-il proposé ?

Au premier lancement, quand WiMao n'a pas encore d'environnement à lui. S'il trouve un `~/.wine-mao` valide, l'assistant affiche sa taille et son nombre de plugins, puis propose :

- **Importer cet environnement (recommandé)** : WiMao reprend vos plugins, leurs réglages et leurs licences ;
- **Créer un nouvel environnement** : WiMao repart de zéro et laisse `~/.wine-mao` intact.

## Ce que fait l'import

1. WiMao vérifie qu'aucun plugin ne tourne : fermez votre logiciel de musique avant de commencer. Si un programme Windows est ouvert, il le nomme et demande l'accord avant de le fermer.
2. Il copie à part les réglages de l'environnement et ceux de yabridge : une petite sauvegarde, au cas où.
3. Il déplace le dossier dans son propre dossier de données. Le déplacement est instantané, même pour plus de 100 Go : rien n'est copié.
4. À l'ancienne place, il laisse un lien vers le nouvel emplacement. Les plugins déjà pontés continuent donc de fonctionner : votre logiciel de musique ne voit aucune différence.

L'import apparaît ensuite dans l'historique. Vos réglages personnels de yabridge (`~/.config/yabridgectl`) ne sont jamais modifiés.

## Import refusé : deux disques différents

Le déplacement instantané n'est possible que sur un même disque. Si `~/.wine-mao` et le dossier de données de WiMao sont sur deux disques différents, WiMao refuse l'import et explique pourquoi. Il ne fait jamais de copie : l'environnement est trop volumineux. Rien n'est modifié : votre ancien environnement fonctionne toujours là où il est. Vous pouvez aussi déplacer le dossier vous-même, puis relancer WiMao.

## Annuler l'import

Tant que c'est possible, le menu principal propose **Annuler l'import**. WiMao retire alors le lien, remet le dossier à sa place d'origine dans `~/.wine-mao` et met à jour les liens de vos plugins avec votre propre configuration de yabridge. Rien n'est supprimé.

L'annulation est refusée si `~/.wine-mao` n'est plus le lien créé par l'import : WiMao préfère ne rien écraser. Comme l'import, elle demande que votre logiciel de musique soit fermé.

Après l'annulation, WiMao n'a plus d'environnement : l'assistant du premier lancement revient. Les points de restauration créés jusque-là ne sont plus proposés dans l'historique : ils décrivent l'environnement rendu à `~/.wine-mao`. Voir [Premiers pas](aide:premiers-pas) et [Problèmes fréquents](aide:problemes).
