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
```

- La **CI** fabrique les trois `.deb` et GitHub *atteste* d'où ils viennent (provenance : tel
  workflow, telle étiquette).
- **Vous** refabriquez `wimao` et `wimao-bridge` chez vous, dans les mêmes conditions : si vos fichiers
  sont identiques octet pour octet à ceux de la CI, c'est que la CI n'a rien ajouté. Alors seulement
  vous *signez* la liste d'empreintes `SHA256SUMS`.
- Personne d'autre que vous ne publie : la CI ne crée qu'un brouillon.

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
- `podman` **sans racine** (rootless) : la refabrication se fait dans un conteneur, car votre machine
  n'est pas l'Ubuntu 24.04 de la CI ;
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
Debian 13, écrit `SHA256SUMS`, atteste la provenance des `.deb` et crée un **brouillon** de Release
contenant les trois `.deb`, `SHA256SUMS` et le fichier d'attestation `wimao-X.Y.Z.intoto.jsonl`.

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
   (`git verify-tag`, format OpenPGP) : une signature valide d'une autre clé de votre trousseau est refusée ;
2. télécharge les fichiers du brouillon (`gh release download`) ;
3. contrôle `sha256sum -c SHA256SUMS` et que ce sont exactement les `.deb` listés ;
4. vérifie l'attestation de chaque `.deb` (`gh attestation verify` : produite par le workflow de ce
   dépôt, pour cette étiquette, **sur le commit de l'étiquette que vous avez signée**, hors runner
   auto-hébergé) ;
5. **refabrique** `wimao` et `wimao-bridge` dans un conteneur `podman` identique à celui de la CI
   (image, version de Rust et paquets lus dans le dépôt), et compare octet pour octet. **Toute
   différence arrête le script** : ne signez pas, cherchez d'abord pourquoi ;
6. signe `SHA256SUMS` en `SHA256SUMS.asc` avec la clé dont l'empreinte est publiée (jamais « la clé par
   défaut » du trousseau) : GnuPG vous demande la phrase de passe (le script ne la voit jamais) ; le
   script affiche l'empreinte de la clé qui a signé ;
7. joint `SHA256SUMS.asc` **et la clé publique** `docs/cle-publique.asc` au brouillon (`gh release upload`).

La refabrication utilise le réseau (image, paquets Ubuntu, Rust, crates) et prend quelques minutes. Les
paquets Ubuntu sont ceux d'aujourd'hui : si Ubuntu a publié une mise à jour d'une bibliothèque depuis la
CI, un écart de dépendances est possible ; signez alors peu après la CI, ou relancez-la.

`wimao-wine` n'est pas refabriqué chez vous (plusieurs heures) : sa reproductibilité est prouvée par la
CI sur chaque étiquette (job `reproductible-wine`), et il reste couvert par la somme et l'attestation.

Pour **s'entraîner** sans rien envoyer : `scripts/signer-version.sh --aide` décrit l'option `--a-blanc`
(avec une clé jetable, sans aucun appel à `gh`), utilisée par la CI et par
`scripts/tests/tester-signer-version.sh`.

### 4. Relire et publier

Sur GitHub, ouvrez le brouillon (*Releases*), relisez le titre, les notes (modifiables) et les
fichiers (trois `.deb`, `SHA256SUMS`, `SHA256SUMS.asc`, `cle-publique.asc`, le `.intoto.jsonl`), puis **publiez vous-même**.
Testez ensuite la vérification comme un utilisateur (voir le `README.md`, « Installation »).

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
