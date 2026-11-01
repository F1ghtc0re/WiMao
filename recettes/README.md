# Recettes fournies

Recettes (fichiers `.toml`) livrées avec WiMao :

| Fichier | Type | Produit |
|---|---|---|
| `native-access.toml` | gestionnaire | Native Access 2 (Native Instruments) |
| `arturia-software-center.toml` | gestionnaire | Arturia Software Center |
| `inmusic-software-center.toml` | gestionnaire | inMusic Software Center (AIR…) |
| `plugin-alliance.toml` | gestionnaire | Plugin Alliance Installation Manager |
| `serum-2.toml` | installeur | Serum 2 (Xfer Records) |
| `valhalla-supermassive.toml` | installeur | Valhalla Supermassive (gratuit) |

- Format et référence de chaque clé : [`docs/recettes.md`](../docs/recettes.md).
- Ajouter une recette : voir « Ajouter une recette » dans [`CONTRIBUTING.md`](../CONTRIBUTING.md).
- Une recette invalide n'empêche pas les autres de se charger : elle est signalée avec sa ligne.
- Les valeurs viennent des installations à la main décrites dans `NOTES.md`. Une recette sans
  `telecharger` (Plugin Alliance, Serum 2, Valhalla Supermassive) demande à l'utilisateur de
  choisir le fichier ; sans `empreinte`, l'empreinte obtenue est écrite dans le journal.
