# WiMao — Sous-projet 6 : dépôt APT signé (conception)

Date : 2026-11-07. Statut : conception approuvée oralement le 2026-11-07, spec à relire.

## 1. But

Que n'importe qui puisse installer WiMao avec `apt` et reçoive ensuite les mises à jour avec celles du
système, en gardant la chaîne de confiance des sous-projets 4 et 5 : **la signature se fait seulement sur
la machine du propriétaire**, la CI n'a aucune clé et ne fait qu'assembler, contrôler et servir des fichiers
déjà signés.

Critères de réussite :

1. Sur Ubuntu 24.04 et Debian 13 neufs, ces commandes installent WiMao 0.4.0 et ses deux dépendances depuis
   `https://f1ghtc0re.github.io/WiMao` :
   ```sh
   sudo install -d -m 0755 /etc/apt/keyrings
   curl -fsSL https://f1ghtc0re.github.io/WiMao/wimao.asc | sudo tee /etc/apt/keyrings/wimao.asc > /dev/null
   echo "deb [signed-by=/etc/apt/keyrings/wimao.asc] https://f1ghtc0re.github.io/WiMao stable main" \
     | sudo tee /etc/apt/sources.list.d/wimao.list
   sudo apt update && sudo apt install wimao
   ```
2. `apt` refuse le dépôt si un octet de l'index ou d'un paquet est modifié (signature OpenPGP + empreintes).
3. Le site n'est mis en ligne que depuis `main`, après vérification de la signature, de chaque empreinte et
   d'une vraie installation sur les deux distributions.
4. Sur la machine du propriétaire, `apt policy wimao` montre le dépôt comme origine.

Hors champ : AUR, Flatpak, PPA ; d'autres architectures qu'`amd64` ; retrait d'une version du dépôt ;
paquet `wimao-archive-keyring` (la clé s'installe par la commande ci-dessus).

## 2. Organisation

```
 Release vX.Y.Z publiée (immuable, SHA256SUMS.asc)
        │
        v
 vous : scripts/publier-depot-apt.sh vX.Y.Z      ── vérifie, indexe, SIGNE (sous-clé locale)
        │   PR « depot-apt-vX.Y.Z » : seulement depot-apt/dists/… (quelques Ko)
        v
 CI depot-apt.yml (sur la PR et sur main)        ── vérifie la signature, télécharge les .deb des
        │                                           Releases, contrôle chaque empreinte, assemble
        │                                           le site, installe wimao sur Ubuntu 24.04 et Debian 13
        v
 sur main seulement : déploiement GitHub Pages   ── https://f1ghtc0re.github.io/WiMao
```

Les `.deb` ne sont **jamais** ajoutés au dépôt git : ils restent dans les Releases (immuables), d'où la CI
les télécharge à chaque assemblage. Le dépôt APT garde toutes les versions publiées (`apt install
wimao=<version>` permet de revenir en arrière).

## 3. Contenu du dépôt APT

Dans le dépôt git, dossier `depot-apt/` (suivi, modifié seulement par les PR du script) :

```
depot-apt/dists/stable/Release           # empreintes des index (SHA256), champs ci-dessous
depot-apt/dists/stable/InRelease         # Release signé en clair (gpg --clearsign)
depot-apt/dists/stable/Release.gpg       # signature détachée de Release (gpg --armor --detach-sign)
depot-apt/dists/stable/main/binary-amd64/Packages
depot-apt/dists/stable/main/binary-amd64/Packages.gz
```

Site publié = `depot-apt/` + `pool/` (les `.deb` téléchargés par la CI) + `wimao.asc` (copie de
`docs/cle-publique.asc`) + `index.html` (page courte : à quoi sert ce site, les quatre commandes du § 1,
l'empreinte de la clé, lien vers le dépôt).

- **Chemin des paquets** : `Filename: pool/vX.Y.Z/<nom du .deb>`. L'étiquette de la Release est dans le
  chemin : la CI en déduit l'adresse `https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/<nom>`.
- **Champs de `Release`** : `Origin: WiMao`, `Label: WiMao`, `Suite: stable`, `Codename: stable`,
  `Architectures: amd64`, `Components: main`, `Description: WiMao — Wine pour la MAO (paquets signés)`,
  `Date` (posée par `apt-ftparchive`), empreintes `SHA256` uniquement (pas de MD5 ni SHA1).
- **Pas de `Valid-Until`** : la signature est manuelle ; une date d'expiration obligerait à re-signer
  régulièrement sous peine de casser `apt update`. Risque accepté et documenté : un intermédiaire pourrait
  resservir un ancien index signé (donc seulement d'anciennes versions authentiques), sans pouvoir le
  modifier ; GitHub Pages est servi en HTTPS, ce qui rend ce rejeu très difficile.
- **Ordre de `Packages`** : entrées triées par nom de paquet puis par version (`dpkg --compare-versions`),
  pour qu'un même contenu donne toujours le même fichier.

## 4. `scripts/publier-depot-apt.sh`

Usage : `scripts/publier-depot-apt.sh vX.Y.Z [--a-blanc --dossier <artefacts> --cle <empreinte>] [--aide]`.

Étapes (mode réel) :

1. **Préconditions** : dépôt propre, sur `main` à jour avec `origin/main` ; outils `gh`, `gpg`, `gpgv`,
   `apt-ftparchive` (paquet `apt-utils`), `dpkg-deb`, `jq` ; l'étiquette `vX.Y.Z` respecte le format de
   `signer-version.sh` ; la Release `vX.Y.Z` existe et **n'est pas un brouillon**.
2. **Empreinte attendue** lue dans `SECURITY.md` (même règle que `signer-version.sh`, fonction partagée,
   voir § 6).
3. **Téléchargement** dans un dossier temporaire (`mktemp -d`, supprimé à la sortie) : les `.deb`,
   `SHA256SUMS`, `SHA256SUMS.asc` de la Release.
4. **Vérification** : `gpgv` avec un trousseau construit depuis `docs/cle-publique.asc` (jamais le
   trousseau personnel) ; la signature doit venir de la clé dont l'empreinte est publiée ;
   `sha256sum -c SHA256SUMS` ; les `.deb` présents sont exactement ceux listés.
5. **Index** : `apt-ftparchive packages` sur `pool/vX.Y.Z/`, fusionné avec le `Packages` existant de
   `depot-apt/` dont on retire d'abord toute entrée en `pool/vX.Y.Z/` (relancer pour la même version
   remplace au lieu de dupliquer) ; tri du § 3 ; `Packages.gz` par `gzip -9n` ;
   `apt-ftparchive release` avec les champs du § 3 (fichier de configuration écrit par le script).
6. **Signature** avec `--local-user <empreinte publiée>` : `InRelease` (`--clearsign`) et `Release.gpg`
   (`--armor --detach-sign`). La phrase de passe est demandée par GnuPG (le script ne la voit jamais) ; le
   script prévient avant : « la fenêtre de la phrase de passe va s'ouvrir, elle attend environ une minute ».
7. **Contrôle local** : `gpgv` sur `InRelease` et sur `Release.gpg` avec le trousseau du point 4 ; les
   empreintes de `Release` correspondent aux fichiers `Packages*`.
8. **PR** : branche `depot-apt-vX.Y.Z`, commit (signé par la configuration locale) des seuls fichiers de
   `depot-apt/`, envoi, `gh pr create` dont le corps liste les paquets ajoutés et leurs empreintes. Le script
   ne fusionne jamais.

Mode `--a-blanc` (tests et CI) : `--dossier` fournit `.deb`, `SHA256SUMS`, `SHA256SUMS.asc` ; `--cle`
l'empreinte d'une clé jetable (remplace SECURITY.md) ; travaille sur une copie de `depot-apt/` donnée par
`--sortie <dossier>` ; aucun appel à `gh` ni à `git` en écriture ; s'arrête après l'étape 7.

Codes : 0 succès ; 1 contrôle échoué (signature, empreinte, brouillon, dépôt sale…) ; 2 usage.

## 5. Workflow `.github/workflows/depot-apt.yml`

Déclencheurs : `pull_request` et `push` sur `main` limités aux chemins `depot-apt/**`,
`docs/cle-publique.asc`, `.github/workflows/depot-apt.yml`, `scripts/assembler-site-apt.sh` ;
`workflow_dispatch`. Permissions racine `contents: read`.

Jobs :

1. **`assembler`** (ubuntu-24.04) : lance `scripts/assembler-site-apt.sh site/`, qui
   - vérifie `InRelease` et `Release.gpg` par `gpgv` avec `docs/cle-publique.asc`, l'empreinte devant être
     celle de `SECURITY.md` ;
   - contrôle que les empreintes SHA256 de `Release` correspondent à `Packages` et `Packages.gz` ;
   - pour chaque entrée de `Packages` : télécharge `releases/download/<étiquette>/<nom>` (étiquette tirée de
     `pool/<étiquette>/`, format `vX.Y.Z` exigé), contrôle **taille et SHA256** annoncées, range le fichier
     dans `site/pool/<étiquette>/` ;
   - copie `depot-apt/dists`, `docs/cle-publique.asc` → `site/wimao.asc`, écrit `site/index.html` ;
   puis `actions/upload-pages-artifact` (artefact `github-pages`).
2. **`installation`** (matrice : `ubuntu:24.04@sha256:…` et `debian:13@sha256:…`, mêmes empreintes
   d'images que `paquets.yml`) : récupère l'artefact (archive `github-pages`, décompressée dans `/site`),
   pose la clé comme au § 1 et la source `deb [signed-by=/etc/apt/keyrings/wimao.asc] file:/site stable
   main` (la méthode `file:` vérifie signature et empreintes comme `https:`, sans serveur à lancer dans
   une image minimale), `apt-get update`, `apt-get install -y wimao`, puis contrôle que
   `dpkg-query -W` donne pour `wimao`, `wimao-bridge`, `wimao-wine` la **plus haute version** présente dans
   `Packages`, et que `wimao-cli --version` répond. Un second essai modifie un octet d'un `.deb` servi :
   `apt-get install --reinstall` doit échouer (« Hash Sum mismatch » ou équivalent).
3. **`deployer`** (seulement `push` sur `main` et `workflow_dispatch` sur `main`) : `needs: [assembler,
   installation]`, environnement `github-pages`, permissions du job `pages: write` et `id-token: write`,
   `actions/deploy-pages`.

`paquets/verifier-workflows.sh` (règle 5) admet, en plus du job `brouillon` de `paquets.yml`, les droits
`pages: write` et `id-token: write` du **seul** job `deployer` de `depot-apt.yml` ; son test le prouve dans
les deux sens (accepté là, refusé ailleurs). Toutes les actions sont figées par empreinte (règle 1), dont
`actions/upload-pages-artifact`, `actions/deploy-pages` (et `actions/configure-pages` si utilisé).

## 6. Code partagé

`scripts/lib/publication.sh` (sourcé, non exécutable) reçoit les fonctions communes à
`signer-version.sh`, `publier-depot-apt.sh` et `assembler-site-apt.sh` :

- `empreinte_publiee <racine du dépôt>` (déplacée depuis `signer-version.sh`, comportement inchangé) ;
- `trousseau_cle_publiee <racine> <dossier>` : crée un trousseau `gpgv` depuis `docs/cle-publique.asc` et
  vérifie que la clé principale qu'il contient a l'empreinte publiée ;
- `verifier_signature <trousseau> <signature> [<fichier>]` : `gpgv` ; succès seulement si la signature est
  valide **et** faite par la clé publiée (ou une de ses sous-clés).

`scripts/tests/tester-signer-version.sh` doit rester vert sans modification de ses attentes.

## 7. Réglages GitHub

`scripts/regler-depot-github.sh` gagne :

| Réglage | Requête | Valeur |
|---|---|---|
| GitHub Pages | `POST` (création) ou `PUT repos/{d}/pages` | `build_type: workflow`, `https_enforced: true` |
| Environnement `github-pages` | `PUT repos/{d}/environments/github-pages` | `deployment_branch_policy: {protected_branches: false, custom_branch_policies: true}` |
| Branche autorisée | `POST …/environments/github-pages/deployment-branch-policies` (si absente) | `{name: "main", type: "branch"}` ; aucune autre politique |

Vérification (`--verifier`) : Pages en mode `workflow`, HTTPS imposé, l'environnement n'accepte que `main`.
Même discipline qu'au sous-projet 5 (idempotent, `--a-blanc` testé, ✅/❌).

## 8. Machine du propriétaire

- `sudo apt install apt-utils` (pour `apt-ftparchive`), expliqué avant.
- Après le premier déploiement : clé et source posées par `sudo install -m 644` (fichiers préparés dans le
  dossier temporaire), `sudo apt update`, `apt policy wimao wimao-bridge wimao-wine` montre
  `https://f1ghtc0re.github.io/WiMao stable/main` ; les versions installées étant identiques, rien n'est
  réinstallé.

## 9. Documentation

- `README.md`, « Installation » : la méthode `apt` (commandes du § 1, empreinte à comparer) en premier ; la
  méthode « télécharger les `.deb` de la Release et vérifier » reste, pour qui préfère.
- `docs/publier-une-version.md` : nouvelle étape « 5. Publier dans le dépôt APT » après la publication de la
  Release ; la vue d'ensemble mise à jour.
- `SECURITY.md` : ce que garantit le dépôt APT (index signé par la clé publiée, empreintes des paquets),
  absence de `Valid-Until` et pourquoi.
- `paquets/README.md` : renvoi vers le dépôt APT.
- `docs/ameliorations-reportees.md` : « Dépôt APT signé » marqué fait dans « Distribution ».

## 10. Tests

- `scripts/tests/tester-publier-depot-apt.sh` (à blanc, clé jetable, faux `.deb` fabriqués par `dpkg-deb`,
  faux `gh` qui échoue s'il est appelé) :
  - premier passage : `Packages` contient les trois entrées en `pool/v9.9.9/`, empreintes justes ;
    `InRelease` et `Release.gpg` passent `gpgv` ; `Release` ne contient que des `SHA256` ;
  - même version relancée : contenu identique (pas de doublon) ;
  - seconde version `v9.9.10` : six entrées, triées ;
  - `SHA256SUMS.asc` signé par une autre clé : refus, code 1 ;
  - un `.deb` modifié : refus, code 1 ; un `.deb` en trop : refus, code 1 ;
  - aucun appel à `gh` ; option inconnue : code 2.
- `scripts/tests/tester-assembler-site-apt.sh` : à partir d'un `depot-apt/` produit par le test précédent
  et d'un faux téléchargement (variable de test qui remplace l'adresse des Releases par un dossier local) :
  site complet ; refus si `InRelease` est altéré, si un `.deb` téléchargé a une autre empreinte ou taille,
  si un chemin n'est pas en `pool/vX.Y.Z/`.
- `paquets/tester-verifier-workflows.sh` : cas `pages: write` admis dans `deployer` de `depot-apt.yml`,
  refusé ailleurs.
- `scripts/tests/tester-regler-depot-github.sh` : corps et vérifications des trois nouveaux réglages.
- Les trois tests ci-dessus et `shellcheck -x` dans le job `chaine` de `ci.yml`.
- Réel : job `installation` de `depot-apt.yml` sur la PR de `v0.4.0`, déploiement sur `main`, puis les
  commandes du § 1 sur la machine du propriétaire.
