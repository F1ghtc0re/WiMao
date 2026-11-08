# Publier une version de WiMao

Ce guide s'adresse au **propriétaire du dépôt**. Il décrit, pas à pas, comment une version devient
une « Release » GitHub que n'importe qui peut vérifier. La conception (et le « pourquoi ») est au § 8
de `docs/superpowers/specs/2026-11-01-wimao-sous-projet-4-design.md`.

## Vue d'ensemble : qui garantit quoi

```
 PR de version fusionnée ─> étiquette signée vX.Y.Z ─> CI : fabrique, contrôle, atteste
                                                          │   (Release BROUILLON, non publiée)
                                                          v
              vous : scripts/signer-version.sh vX.Y.Z ─> vérifie, refabrique, signe SHA256SUMS
                                                          │
                                                          v
                                           vous relisez le brouillon et le publiez
                                                          │
                                                          v
              vous : scripts/publier-depot-apt.sh vX.Y.Z ─> signe l'index APT, ouvre une PR
                                                          │
                                                          v
                                  vous fusionnez la PR ─> CI : vérifie, installe, déploie le site
                                                          │
                                                          v
              vous : scripts/publier-depot-dnf.sh vX.Y.Z ─> signe l'index dnf (Fedora), ouvre une PR
                                                          │
                                                          v
                                  vous fusionnez la PR ─> CI : vérifie, installe, déploie le site
```

- La **CI** fabrique les trois `.deb`, puis en tire trois `.rpm` pour Fedora (même contenu, réemballé), et GitHub *atteste* d'où ils viennent (provenance : tel
  workflow, telle étiquette).
- **Vous** refabriquez `wimao` et `wimao-bridge` chez vous, dans les mêmes conditions, puis les trois RPM à
  partir des `.deb` que vous venez de vérifier : si vos fichiers sont identiques octet pour octet à ceux de la
  CI, c'est que la CI n'a rien ajouté. Alors seulement vous *signez* la liste d'empreintes `SHA256SUMS`, qui
  couvre les six paquets (une seule signature, donc une seule phrase de passe).
- Personne d'autre que vous ne publie : la CI ne crée qu'un brouillon.
- Le **dépôt APT** (étape 5) suit la même règle : l'index est signé **chez vous**, avec la même sous-clé ;
  la CI n'a aucune clé, elle ne fait que vérifier cette signature, contrôler chaque `.deb` et servir les
  fichiers sur GitHub Pages.
- Le **dépôt dnf** (étape 6) suit la même règle : l'index (`repomd.xml`) est signé chez vous, la CI vérifie.

## Avant la toute première version (une seule fois)

À cocher dans l'ordre. Les points marqués (guidé) font l'objet d'un accompagnement pas à pas avec le
contrôleur du projet ; ne transmettez jamais un mot de passe ni une phrase de passe.

- [x] **Double authentification** activée sur le compte GitHub (guidé).
- [x] **Clé de signature** créée, sauvegardée, et clé publique ajoutée au dépôt
      (`docs/cle-publique.asc`) : voir « Créer la clé de signature » plus bas (guidé).
- [x] **Règles du dépôt** : posées et contrôlées par `scripts/regler-depot-github.sh`
      (`--appliquer` juste après le passage en public, `--verifier` à tout moment ; valeurs exactes dans
      la spec du sous-projet 5, § 4). Sur un compte GitHub gratuit, ces règles n'existent que pour un
      dépôt **public** (refus « Upgrade to GitHub Pro or make this repository public », constaté le
      2026-11-01) : les poser juste après le passage en public, **avant** la première étiquette `v*`
      (guidé). Règles posées par `scripts/regler-depot-github.sh --appliquer` :
      - sur `main` : PR obligatoire (aucune approbation exigée : le propriétaire est seul et GitHub
        interdit d'approuver sa propre PR), discussions résolues, CI verte (`unitaires`, `integration`,
        `chaine`), commits signés, ni suppression ni réécriture d'historique, et **sans contournement**
        (pas même vous : en cas d'urgence, désactivez le ruleset dans *Settings > Rules*, puis
        réappliquez-le, car `--verifier` affiche ❌ tant qu'il est désactivé) ;
      - sur les **étiquettes `v*`** (motif `refs/tags/v*`) : création, déplacement et suppression
        réservés à l'administrateur, donc interdits aux jetons de workflow et à toute autre personne.
        Sans cela, une étiquette distante pourrait être déplacée sur un autre commit ;
        `signer-version.sh` le détecte (l'attestation doit porter sur le commit de l'étiquette signée
        que vous avez en local), mais mieux vaut l'empêcher. Vous pouvez donc toujours supprimer une
        étiquette ratée (voir « L'étiquette signée »).
- [x] **Alertes et correctifs Dependabot** : posés par `scripts/regler-depot-github.sh --appliquer` sur le
      nouveau dépôt (l'ancien dépôt les avait depuis le 2026-11-01).
- [x] **Signalement privé des failles** activé, car `SECURITY.md` y renvoie : fait par
      `scripts/regler-depot-github.sh --appliquer` (vérifiable avec `--verifier`).
- [x] **Données personnelles retirées avant de rendre le dépôt public.** Les fichiers ont été corrigés
      (`NOTES.md`, `crates/wimao-core/src/rapport.rs`, le plan du sous-projet 1) ; l'ancien historique,
      qui contenait le nom d'utilisateur du propriétaire dans sept commits, n'est pas publié (dépôt
      neuf). Le contrôle se fait par `scripts/verifier-donnees-perso.sh`, **à lancer avant chaque
      version** sur votre machine : il cherche, dans les fichiers suivis par git et sans tenir compte de
      la casse, votre nom d'utilisateur, le nom de la machine et l'e-mail de `git config user.email`
      (sauf une adresse `noreply.github.com`), plus les termes que vous listez, un par ligne, dans le
      fichier non suivi `.git/donnees-perso`. Il fouille aussi les fichiers binaires suivis (nom seul).
      Il affiche `fichier:ligne` sans répéter le terme ; code 0 =
      rien trouvé. La CI le lance aussi, mais sur le runner (où il ne prouve que son bon fonctionnement).
      **Avant de rendre le dépôt public, puis avant chaque version**, remplissez le fichier non suivi
      `.git/donnees-perso` avec votre vrai nom, vos adresses e-mail personnelles et les noms de vos autres
      machines (un terme par ligne) : `git config user.email` est une adresse `noreply`, donc sans ce
      fichier rien de personnel ne serait cherché.
- [x] **Dépôt rendu public** (2026-11-01, dépôt neuf à commit unique ; l'ancien, renommé `WiMao-archive`, reste privé). Obligatoire *avant* la première étiquette : les attestations de
      provenance exigent un dépôt public (sur un dépôt privé, il faudrait GitHub Enterprise Cloud ;
      l'étape d'attestation du job `brouillon` échouerait).
- [x] **Outils sur votre machine** (voir « Ce qu'il faut sur votre machine »).

## Créer la clé de signature

Principe : une **clé principale** qui ne sert qu'à certifier (elle reste hors ligne, sauvegardée) et une
**sous-clé de signature** qui sert au quotidien. Si la sous-clé est volée, vous la révoquez avec la clé
principale ; l'identité de la clé reste.

1. Créer la clé principale, **réservée à la certification** (elle ne signe rien elle-même), en ECC
   (Curve 25519), valable 2 ans :

   ```sh
   gpg --quick-generate-key "F1ghtc0re <162480684+F1ghtc0re@users.noreply.github.com>" ed25519 cert 2y
   gpg --list-secret-keys --keyid-format long      # notez l'EMPREINTE de la clé (40 caractères)
   ```

   GnuPG demande la phrase de passe. Utilisez **exactement** le pseudonyme `F1ghtc0re` et l'adresse
   `162480684+F1ghtc0re@users.noreply.github.com`, jamais un vrai nom ni une adresse personnelle : la
   clé publique est publiée, et son identité y est encodée en base64, donc invisible pour
   `scripts/verifier-donnees-perso.sh`.

2. Ajouter la sous-clé de signature (valable 1 an, renouvelable) :

   ```sh
   gpg --quick-add-key <EMPREINTE> ed25519 sign 1y
   gpg -K                # contrôle : « sec … [C] » (certification) puis « ssb … [S] » (signature)
   ```

3. Créer **tout de suite** un certificat de révocation, et le ranger avec les sauvegardes :

   ```sh
   gpg --output revocation.asc --gen-revoke <EMPREINTE>
   ```

4. **Sauvegarder hors ligne** (clé USB chiffrée ou papier, rangée ailleurs que dans cet ordinateur ; au
   moins deux copies) :

   ```sh
   gpg --armor --export-secret-keys <EMPREINTE> > cle-secrete-complete.asc
   ```

   Conservez la phrase de passe **avec la sauvegarde hors ligne** (papier rangé avec la clé USB, par
   exemple), pas seulement sur cet ordinateur : sans elle la sauvegarde est inutilisable. Sans la clé,
   il faudra recommencer (voir « Révocation »). **Ces fichiers ne vont jamais dans le dépôt.**

5. (Facultatif, mieux) Ne garder sur la machine de signature que la sous-clé, la clé principale restant
   sur la sauvegarde. **D'abord, vérifiez que la sauvegarde s'importe** dans un trousseau jetable :

   ```sh
   export GNUPGHOME="$(mktemp -d)"            # trousseau vide, valable dans ce terminal seulement
   gpg --import cle-secrete-complete.asc      # doit annoncer « clefs secrètes importées : 1 »
   rm -rf "$GNUPGHOME"; unset GNUPGHOME       # retour à votre trousseau habituel
   ```

   Ensuite seulement, exportez `gpg --export-secret-subkeys <EMPREINTE> > sous-cle.asc`, supprimez la
   clé secrète (`gpg --delete-secret-keys <EMPREINTE>`) puis importez `sous-cle.asc`. `gpg -K` doit
   alors afficher `sec#` (le `#` signifie « clé principale absente »).

6. **Publier la clé publique** dans le dépôt, par une PR :

   ```sh
   gpg --armor --export <EMPREINTE> > docs/cle-publique.asc
   ```

   **Avant d'ouvrir la PR**, contrôlez l'identité publiée : `gpg --show-keys docs/cle-publique.asc` ;
   les lignes `uid` ne doivent montrer que le pseudonyme et l'adresse `noreply`.

   Indiquez l'empreinte complète (40 caractères) dans la description de la PR et dans `SECURITY.md`, pour
   qu'on puisse la comparer avec celle du fichier : `gpg --show-keys --fingerprint docs/cle-publique.asc`.
   Dans `SECURITY.md`, la ligne doit avoir **exactement** cette forme (les espaces entre les groupes de
   chiffres sont admis), en début de ligne, car `scripts/signer-version.sh` la lit :

   ```text
   Empreinte de la clé de signature : XXXX XXXX XXXX XXXX XXXX XXXX XXXX XXXX XXXX XXXX
   ```

   C'est l'empreinte de la clé **principale** (celle de `sec`), pas celle de la sous-clé. Tant que cette ligne
   manque, le script refuse de signer. Il refuse aussi une étiquette signée par une autre clé que celle-ci.
   Idéalement, publiez-la aussi ailleurs (profil GitHub, site personnel) : plusieurs sources
   concordantes rendent une substitution plus difficile.

7. **Rien à régler dans Git pour les étiquettes.** Il y a deux signatures différentes, qui ne doivent pas
   se gêner :
   - les **commits** sont signés par une clé **SSH**, réglée pour ce seul dépôt (jamais en `--global`) :

     ```sh
     git config --local gpg.format ssh
     git config --local user.signingkey ~/.ssh/<votre-cle>.pub      # chemin de la clé publique SSH
     git config --local commit.gpgsign true
     ```

     (GitHub affiche « Verified » si cette même clé publique est ajoutée à votre compte comme
     *clé de signature*.)
   - les **étiquettes de version** sont signées par la **sous-clé OpenPGP** de l'étape 4, en forçant le
     format pour cette seule commande (voir « L'étiquette signée »). Ne réglez donc jamais
     `user.signingkey` en `--global` : cela casserait l'une des deux signatures.

## Ce qu'il faut sur votre machine

- `git`, `gpg`, `sha256sum` ;
- pour le dépôt APT (étape 5) : `gpgv`, `gzip` et `apt-ftparchive`, qui vient du paquet `apt-utils`
  (`sudo apt install apt-utils`) ;
- pour le dépôt dnf (étape 6) : `gpgv`, `python3`, `zstd` et `podman` (l'index est construit avec
  `createrepo_c` dans un conteneur Fedora, rien à installer d'autre sur votre machine) ;
- `podman` **sans racine** (rootless) : la refabrication se fait dans un conteneur, car votre machine
  n'est pas l'Ubuntu 24.04 de la CI (ni le Fedora 45 des RPM) ;
- **GitHub CLI à jour** (`gh`), connecté (`gh auth login`), avec la commande `gh attestation`. Le `gh`
  des dépôts d'Ubuntu (2.46) est **trop ancien** : installez-le depuis le dépôt officiel de GitHub
  (<https://github.com/cli/cli/blob/trunk/docs/install_linux.md>). Le script s'arrête avec ce conseil
  s'il détecte un `gh` trop ancien ;
- votre sous-clé de signature dans le trousseau GnuPG.

## Publier, pas à pas

### 1. La demande de fusion de version

Dans une PR, mettez à jour :

- la version du workspace dans `Cargo.toml` (`[workspace.package] version`), puis `cargo build` pour
  que `Cargo.lock` suive ;
- `paquets/wimao/changelog` : nouvelle entrée en tête, au format Debian (`wimao (X.Y.Z-1) unstable;
  urgency=medium`, puces `  * …`, ligne de signature ` -- Nom <e-mail>  date`) ; la fabrication refuse
  si le changelog et `Cargo.toml` annoncent des versions différentes ;
- la date du `<release>` de `crates/wimao/data/fr.wimao.WiMao.metainfo.xml` (la date **réelle** de la
  publication) ;
- si Wine ou yabridge/DXVK changent : le changelog et les versions de `paquets/wine/` et
  `paquets/bridge/`, et les scripts de fabrication ;
- lancer `scripts/verifier-donnees-perso.sh` (code 0 attendu).

La CI doit être verte. Vous validez, puis fusionnez.

### 2. L'étiquette signée

Sur `main` à jour, puis :

```sh
git switch main && git pull
git -c gpg.format=openpgp tag -s -u <EMPREINTE> vX.Y.Z -m "WiMao X.Y.Z"   # exactement trois nombres
git push origin vX.Y.Z
```

`<EMPREINTE>` est celle de la clé principale (celle de `SECURITY.md`) : GnuPG choisit tout seul la sous-clé de
signature. `-c gpg.format=openpgp` est indispensable : sans lui, Git utiliserait la clé SSH réglée pour les
commits et l'étiquette ne serait pas vérifiable par `signer-version.sh`.

L'envoi de l'étiquette lance le workflow `paquets.yml`. Il fabrique les trois paquets, contrôle la
reproductibilité (y compris Wine, compilé deux fois sans cache : c'est long), les teste sur Ubuntu 24.04 et
Debian 13, fabrique les trois RPM à partir des `.deb` (deux fois, comparés) et les teste sur Fedora 44 et 45,
écrit `SHA256SUMS` (six paquets), atteste la provenance des `.deb` et des `.rpm` et crée un **brouillon** de
Release contenant les trois `.deb`, les trois `.rpm`, `SHA256SUMS` et le fichier d'attestation
`wimao-X.Y.Z.intoto.jsonl`.

Si un job échoue, rien n'est créé : corrigez par une PR, supprimez l'étiquette (`git push --delete
origin vX.Y.Z` puis `git tag -d vX.Y.Z`) et recommencez avec une nouvelle étiquette.

### 3. Vérifier et signer

Dans une copie du dépôt à jour (les étiquettes récupérées : `git fetch --tags`), propre (rien à
commiter) :

```sh
scripts/signer-version.sh vX.Y.Z
```

Le script, dans l'ordre :

1. vérifie que le dépôt est propre, que `SECURITY.md` publie l'empreinte de la clé de signature, que
   `docs/cle-publique.asc` contient cette clé, et que l'étiquette est locale et signée **par cette clé**
   (`git verify-tag`, format OpenPGP) : une signature valide d'une autre clé de votre trousseau est refusée.
   Puis il **déverrouille la clé tout de suite** (il signe un texte jetable, sans rien écrire) : **la fenêtre
   de phrase de passe s'ouvre donc au début**, avant la longue refabrication, et non à la fin. Répondez-y
   dès qu'elle apparaît ;
2. télécharge les fichiers du brouillon (`gh release download`) : `.deb`, `.rpm` et `SHA256SUMS` ;
3. contrôle `sha256sum -c SHA256SUMS` et que ce sont exactement les `.deb` et les `.rpm` listés (trois de
   chaque) ;
4. vérifie l'attestation de chaque `.deb` et de chaque `.rpm` (`gh attestation verify` : produite par le
   workflow de ce dépôt, pour cette étiquette, **sur le commit de l'étiquette que vous avez signée**, hors
   runner auto-hébergé) ;
5. **refabrique** `wimao` et `wimao-bridge` dans un conteneur `podman` identique à celui de la CI
   (image, version de Rust et paquets lus dans le dépôt), et compare octet pour octet. **Toute
   différence arrête le script** : ne signez pas, cherchez d'abord pourquoi. Puis il **refabrique les trois
   RPM à partir des `.deb` qu'il vient de vérifier** (conteneur Fedora 45, même image que le job `rpm`) et
   les compare de la même façon ;
6. signe `SHA256SUMS` en `SHA256SUMS.asc` avec la clé dont l'empreinte est publiée (jamais « la clé par
   défaut » du trousseau) : GnuPG vous demande la phrase de passe (le script ne la voit jamais) ; le
   script affiche l'empreinte de la clé qui a signé ;
7. joint `SHA256SUMS.asc` **et la clé publique** `docs/cle-publique.asc` au brouillon (`gh release upload`).

La refabrication utilise le réseau (image, paquets Ubuntu, Rust, crates) et prend quelques minutes. Les
paquets Ubuntu sont ceux d'aujourd'hui : si Ubuntu a publié une mise à jour d'une bibliothèque depuis la
CI, un écart de dépendances est possible ; signez alors peu après la CI, ou relancez-la. Il en va de même pour
les RPM : `rpm-build` et `zstd` sont installés dans l'image Fedora depuis les dépôts vivants de Fedora, et une
mise à jour de l'un d'eux peut changer les octets d'un RPM.

**Attention à la durée de la phrase de passe.** Par défaut, GnuPG garde la phrase de passe en mémoire
**10 minutes** (`default-cache-ttl` de `gpg-agent`) après son dernier usage. Si la refabrication (des `.deb`,
puis des RPM) dure plus longtemps, GnuPG l'a oubliée et **la fenêtre peut se rouvrir à la signature** (étape
6) : surveillez-la et répondez-y. Rien de grave dans ce cas, mais le script attendra votre réponse.
Si vous préférez, allongez le délai (`default-cache-ttl` et `max-cache-ttl` dans `~/.gnupg/gpg-agent.conf`,
puis `gpgconf --reload gpg-agent`).

`wimao-wine` n'est pas refabriqué chez vous (plusieurs heures) : sa reproductibilité est prouvée par la
CI sur chaque étiquette (job `reproductible-wine`), et il reste couvert par la somme et l'attestation.

Pour **s'entraîner** sans rien envoyer : `scripts/signer-version.sh --aide` décrit l'option `--a-blanc`
(avec une clé jetable, sans aucun appel à `gh`), utilisée par la CI et par
`scripts/tests/tester-signer-version.sh`.

### 4. Relire et publier

Sur GitHub, ouvrez le brouillon (*Releases*), relisez le titre, les notes (modifiables) et les
fichiers (trois `.deb`, trois `.rpm`, `SHA256SUMS`, `SHA256SUMS.asc`, `cle-publique.asc`, le `.intoto.jsonl`), puis **publiez vous-même**.
Testez ensuite la vérification comme un utilisateur (voir le `README.md`, « Installation », méthode 2).

### 5. Publier dans le dépôt APT

Cette étape se fait **après** la publication de la Release (le script refuse un brouillon) : le dépôt
`apt` ne contient pas les `.deb`, il pointe vers ceux de la Release. Ce qui est publié sur
`https://f1ghtc0re.github.io/WiMao` est décrit au § 3 de la spec du sous-projet 6
(`docs/superpowers/specs/2026-11-07-wimao-sous-projet-6-design.md`).

**Une fois : réglages GitHub.** `scripts/regler-depot-github.sh --appliquer` active GitHub Pages (source
« GitHub Actions », HTTPS imposé) et limite l'environnement `github-pages` à la seule branche `main` ;
`--verifier` le contrôle à tout moment.

**À chaque version**, sur `main` à jour et propre :

```sh
git switch main && git pull
scripts/publier-depot-apt.sh vX.Y.Z
```

Le script, dans l'ordre :

1. vérifie les outils, que le dépôt est propre et à jour, et que la Release `vX.Y.Z` existe et n'est pas
   un brouillon ;
2. télécharge les `.deb`, `SHA256SUMS` et `SHA256SUMS.asc`, et vérifie la signature de `SHA256SUMS.asc`
   (par la clé publiée, avec un trousseau jetable tiré de `docs/cle-publique.asc`) puis chaque empreinte ;
3. construit l'index (`Packages`, `Packages.gz`, `Release`) en gardant les versions déjà publiées ; relancer
   la même version remplace ses entrées au lieu de les dupliquer ;
4. **signe** `Release` (`InRelease` et `Release.gpg`) avec la sous-clé. **Une ou deux fenêtres de phrase de
   passe GNOME s'ouvrent, et chacune attend environ une minute** : le script vous prévient avant. Il ne voit
   jamais la phrase de passe ;
5. contrôle ce qu'il vient de signer, crée la branche `depot-apt-vX.Y.Z` (seulement les fichiers de
   `depot-apt/`, quelques Ko), envoie le commit et ouvre la PR, puis indique la marche à suivre. Il ne
   fusionne jamais.

Ensuite :

1. Attendez la CI de la PR (`Dépôt APT`). Le job `assembler` vérifie la signature, télécharge chaque `.deb`
   depuis les Releases et contrôle sa taille et son SHA256. Le job `installation` installe vraiment `wimao`
   depuis le site assemblé, sur Ubuntu 24.04 et Debian 13, puis essaie un `.deb` altéré d'un octet, qui doit
   être refusé.
2. Fusionnez la PR, mais seulement si le workflow « Dépôt APT » (jobs `assembler` et `installation`) est vert.
   Il n'est pas parmi les vérifications obligatoires de `main`, car il ne tourne que sur certains chemins.
   Sur `main`, le job `deployer` (seul à disposer de `pages: write` et `id-token: write`) met le site en ligne. Rien n'est publié depuis une autre branche.
   Attention à la taille : chaque version ajoute environ 89 Mo dans `pool/` (mesuré sur la v0.4.0) et GitHub
   limite un site Pages publié à 1 Go ; vers la 11e ou 12e version le déploiement échouera (voir
   `docs/ameliorations-reportees.md`).
3. Vérifiez comme un utilisateur : les quatre commandes du `README.md` (« Installation », méthode 1), puis
   `apt policy wimao`, qui doit montrer `https://f1ghtc0re.github.io/WiMao stable/main`.

Si le script s'arrête (code 1), rien n'a été modifié : lisez le message, corrigez, relancez. Si la branche
`depot-apt-vX.Y.Z` existe déjà (essai précédent), il la refuse : supprimez-la d'abord. Pour s'entraîner
sans rien envoyer : `scripts/publier-depot-apt.sh --aide` décrit l'option `--a-blanc` (clé jetable, aucun
appel à `gh` ni à `git`), utilisée par `scripts/tests/tester-publier-depot-apt.sh`. Le script
`scripts/assembler-site-apt.sh` est celui de la CI (codes 0 = site complet, 1 = contrôle échoué, 2 = usage,
3 = dépôt APT encore vide).

### 6. Publier dans le dépôt dnf (Fedora)

Même principe que l'étape 5, pour Fedora. Cette étape se fait **après** la publication de la Release (le script
refuse un brouillon) et, de préférence, après la fusion de la PR de l'étape 5 : le dépôt `dnf` ne contient pas
les RPM, il pointe vers ceux de la Release. L'index ne décrit que **la dernière version** publiée. Ce qui est
publié est décrit au § 4 de la spec du sous-projet 8
(`docs/superpowers/specs/2026-11-08-wimao-sous-projet-8-design.md`).

**À chaque version**, sur `main` à jour et propre :

```sh
git switch main && git pull
scripts/publier-depot-dnf.sh vX.Y.Z
```

Le script, dans l'ordre :

1. vérifie les outils, que le dépôt est propre et à jour, et que la Release `vX.Y.Z` existe et n'est pas
   un brouillon ;
2. télécharge les `.rpm`, `SHA256SUMS` et `SHA256SUMS.asc`, vérifie la signature de `SHA256SUMS.asc` (par la
   clé publiée, avec un trousseau jetable tiré de `docs/cle-publique.asc`), puis chaque empreinte ;
3. construit l'index avec `createrepo_c` dans le conteneur Fedora figé (`depot-dnf/`, quelques Ko) ; les
   adresses de téléchargement pointent vers la Release. Relancer la même version redonne les mêmes octets ;
4. **signe** `repomd.xml` avec la sous-clé. **Une seule fenêtre de phrase de passe s'ouvre, et elle attend
   environ une minute** : le script vous prévient avant. Il ne voit jamais la phrase de passe ;
5. contrôle ce qu'il vient de signer (signature, cohérence de l'index, adresse, taille et empreinte de chaque
   RPM), crée la branche `depot-dnf-vX.Y.Z` (seulement les fichiers de `depot-dnf/`), envoie le commit et ouvre
   la PR. Il ne fusionne jamais.

Ensuite :

1. Attendez la CI de la PR (`Dépôt APT`, qui couvre aussi Fedora). Le job `assembler` vérifie la signature de
   l'index dnf, télécharge chaque RPM depuis les Releases pour contrôler son adresse, sa taille et son SHA-256
   (puis le jette), et ajoute `rpm/` et `wimao.repo` au site. Le job `installation-dnf` installe vraiment `wimao`
   depuis ce site dans Fedora 44 et 45, après avoir prouvé qu'un `repomd.xml` modifié d'un seul caractère est
   refusé.
2. Fusionnez la PR, mais seulement si les jobs `assembler`, `installation` et `installation-dnf` sont verts.
   Sur `main`, le job `deployer` met le site en ligne. Le site ne grossit presque pas (l'index fait quelques
   Ko, les RPM restent sur GitHub).
3. Vérifiez comme un utilisateur, dans un Fedora : les deux commandes du `README.md` (« Installation sur
   Fedora »), puis `sudo dnf install wimao`.

Si le script s'arrête (code 1), rien n'a été modifié : lisez le message, corrigez, relancez. Si la branche
`depot-dnf-vX.Y.Z` existe déjà, il la refuse : supprimez-la d'abord. Pour s'entraîner sans rien envoyer :
`scripts/publier-depot-dnf.sh --aide` décrit l'option `--a-blanc` (clé jetable, aucun appel à `gh` ni à `git`).

Tant que `depot-dnf/` ne contient pas d'index, le site reste celui du seul dépôt APT (la page d'accueil ne
parle pas de Fedora). La première publication d'un index dnf fait apparaître la section Fedora et `wimao.repo`.

**Prolongez la sous-clé AVANT son expiration (le 2027-11-01).** Les vérifications de signature
(`signer-version.sh`, par son propre contrôle GOODSIG sur `git verify-tag` ; `publier-depot-apt.sh` et la CI du
dépôt APT, par `scripts/lib/publication.sh`) refusent une clé expirée : passé cette date, la CI du dépôt APT échoue et vous ne pouvez plus rien publier. Prolonger
ne change pas l'empreinte : `gpg --quick-set-expire <EMPREINTE> 1y '*'` sur la machine de signature (la clé
principale doit y être, voir « Révocation »), puis exportez de nouveau `docs/cle-publique.asc` (PR), car la
CI lit la date d'expiration dans ce fichier. Le site (dépôts APT et dnf) reprend cette clé (`wimao.asc`)
automatiquement : la fusion de cette PR sur main relance le workflow « Dépôt APT », qui la republie.
Les utilisateurs ont une copie locale de la clé : annoncez la prolongation (notes de version) pour qu'ils
la retéléchargent (README, « EXPKEYSIG ») avant l'ancienne date. Mettez-vous un rappel pour
août 2027.

## Révocation et clé perdue

La politique est dans `SECURITY.md` (« Clé de signature perdue ou volée »). En pratique :

- **Sous-clé volée ou perdue, clé principale intacte** : sur une machine de confiance, réimportez
  d'abord la sauvegarde hors ligne de la clé principale (`gpg --import cle-secrete-complete.asc`),
  puis `gpg --edit-key <EMPREINTE>`, `key <n>`, `revkey`, `save` ; créez une nouvelle sous-clé
  (`gpg --quick-add-key <EMPREINTE> ed25519 sign 1y`) ; mettez à jour `docs/cle-publique.asc` (PR) ;
  republiez les versions concernées. Retirez ensuite de nouveau la clé principale secrète de la
  machine de signature (étape 5 de « Créer la clé de signature »).
- **Clé principale perdue ou compromise** : importez le certificat de révocation (`gpg --import
  revocation.asc`), exportez la clé révoquée dans `docs/cle-publique.asc` (ou publiez le certificat à
  côté), générez une nouvelle clé (voir plus haut), publiez sa nouvelle empreinte dans le dépôt, puis
  re-signez les versions encore suivies. Dites-le dans les notes de la prochaine Release.
- Les versions signées par l'ancienne clé ne sont plus fiables tant qu'elles ne sont pas re-signées.

## Ajouter une cible ou changer d'image

L'image `ubuntu:24.04@sha256:…` est écrite à plusieurs endroits de `paquets.yml` (limite de GitHub :
l'image d'un conteneur n'accepte pas de variable). Changez-les **toutes** (et la clé de cache du job
`wine`) ; `paquets/verifier-workflows.sh`, lancé par la CI, contrôle chacune. `scripts/signer-version.sh`
lit l'image et la version de Rust dans le job `wimao` : rien à y modifier.
