# Avertissements lintian acceptés

`paquets/tester-installation.sh` passe les trois paquets à `lintian --fail-on error,warning` :
**aucune erreur, et aucun avertissement hors de cette liste**.

**Source unique de vérité** : le script lit CE fichier. Chaque ligne de la forme

    - `étiquette` : justification

devient une étiquette ignorée (`--suppress-tags-from-file`). Tout autre texte de ce fichier est
ignoré par le script. Ajouter une étiquette ici, c'est donc l'accepter : la justification est
obligatoire, et la relecture doit la contester si elle est faible.

Les écarts « voulus » propres à un paquet ne sont pas ici mais dans son fichier
`paquets/<paquet>/lintian-overrides` (installé dans le paquet, avec son commentaire) : ils
restent attachés au paquet qu'ils concernent. Cette liste ne garde que ce qui vaut pour
l'ensemble des paquets ou ne peut pas être réglé par un override.

## Étiquettes acceptées

- `initial-upload-closes-no-bugs` : le changelog annonce une première version (« Initial release »)
  sans numéro de bogue à fermer ; WiMao n'est pas envoyé à Debian, il n'y a pas de bogue ITP.
- `no-manual-page` : `wimao` et `wimao-cli` n'ont pas de page de manuel ; l'aide est dans
  `wimao-cli --help` (traduite) et dans le guide de l'interface. À corriger si une page `man` est écrite.
