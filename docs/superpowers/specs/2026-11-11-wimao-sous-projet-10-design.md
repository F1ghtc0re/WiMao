# WiMao — sous-projet 10 : paquet AUR pour Arch Linux

Date : 2026-11-11 · Statut : forme validée par le propriétaire (« oui A ») · Branche : `sous-projet-10`

## 1. But

Un utilisateur d'Arch Linux (ou d'une dérivée : Manjaro, EndeavourOS…) installe et met à jour WiMao avec son
outil habituel, avec le même niveau de sécurité qu'avec apt et dnf :

```sh
curl -fsSL https://f1ghtc0re.github.io/WiMao/wimao.asc | gpg --import   # une fois : la clé de WiMao
yay -S wimao-bin                                                          # ou : git clone + makepkg -si
```

**Critère de réussite :** dans un Arch vierge (conteneur), la commande ci-dessus installe WiMao ;
`wimao-cli verifier` ouvre le plugin de test Dexed ; un paquet de la Release modifié d'un seul octet, une
empreinte fausse dans le `PKGBUILD` ou une signature `SHA256SUMS.asc` invalide font échouer `makepkg` ; une
modification du paquet sur l'AUR qui ne vient pas de `main` est détectée en moins de 24 h.

**Hors périmètre :** paquet compilé depuis les sources (`wimao` sans `-bin`), paquets séparés pour Wine et le
pont (forme B écartée, § 3.1), dépôt binaire Arch signé (`pacman -S` sans AUR), architectures autres que x86_64,
Flatpak (sous-projet suivant).

## 2. Constat (essai du 2026-11-11, conteneur `archlinux:latest`, glibc 2.44)

- Le contenu des trois `.deb` de la Release v0.7.0 (empreintes contrôlées par `SHA256SUMS`), extrait tel quel
  dans un Arch : `wine --version` répond `wine-11.20` ; `wimao` et `wimao-cli` ne manquent d'aucune
  bibliothèque ; `wimao-cli --version` répond `0.7.0` ; yabridge est complet.
- Les dépendances existent dans les dépôts officiels (`core`, `extra`) : `carla`, `7zip`, `cabextract`,
  `xorg-server-xvfb`, `xorg-xwininfo`, `libadwaita`, `gtk4`, `zstd`, `tar`, `ocl-icd` (qui fournit
  `opencl-icd-loader`), `mesa`, `libcups`, `dbus`, `libxinerama`, `libxxf86vm`. `SDL2` s'appelle `sdl2-compat`.
- Tailles : `wimao-wine` 72 Mo, `wimao-bridge` 12 Mo, `wimao` 3 Mo.

## 3. Le paquet `wimao-bin`

### 3.1 Forme : un seul paquet

Un seul `PKGBUILD`, `pkgname=wimao-bin`, qui installe le contenu des **trois** `.deb` de la Release. Une seule
recette à relire, à envoyer et à surveiller ; une seule commande pour l'utilisateur. Contrepartie assumée :
chaque mise à jour retélécharge Wine (72 Mo) même s'il n'a pas changé.

`provides=(wimao)`, `conflicts=(wimao)`. `pkgver` = version du workspace (`0.7.0`), `pkgrel=1` (augmenté
seulement si la recette change sans nouvelle version de WiMao). `arch=(x86_64)`.
`license=('GPL-3.0-or-later' 'LGPL-2.1-or-later' 'Zlib')` (WiMao et yabridge, Wine, DXVK).
`options=(!strip !debug)` : les fichiers sont installés **octet pour octet** tels que dans les `.deb` (ne pas
retoucher les binaires Windows de Wine).

### 3.2 Dépendances

Traduction des listes `paquets/rpm/*.deps` et `*.recommends` en noms Arch, dans un fichier lisible
`paquets/aur/dependances` (une ligne par dépendance, `#` pour les commentaires, préfixe `opt:` pour une
dépendance facultative avec sa raison) :

| Obligatoires (`depends`) | Facultatives (`optdepends`) |
|---|---|
| `freetype2`, `fontconfig`, `libx11`, `libxext`, `libxrender`, `libxrandr`, `libxi`, `libxcursor`, `libxcomposite`, `libxfixes`, `vulkan-icd-loader`, `gnutls`, `libpulse`, `alsa-lib`, `opencl-icd-loader`, `libadwaita`, `gtk4`, `xorg-server-xvfb`, `7zip`, `cabextract`, `zstd`, `tar` | `carla` (test d'ouverture des plugins), `xorg-xwininfo` (idem), `mesa`, `libxinerama`, `libxxf86vm`, `libcups`, `sdl2-compat`, `dbus` |

### 3.3 Sources et double verrou

```text
source=(
  https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/wimao_X.Y.Z-1_amd64.deb
  https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/wimao-bridge_…_amd64.deb
  https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/wimao-wine_…_amd64.deb
  https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/SHA256SUMS
  https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/SHA256SUMS.asc
)
sha256sums=(<empreinte de chacun>)
validpgpkeys=(3980C3D20A17566A075A65CBCB97DE3FBF41DD46)
```

1. **Empreintes écrites dans la recette** (`sha256sums`) : un fichier de la Release remplacé après coup est
   refusé par `makepkg`, même si l'attaquant contrôlait la Release.
2. **Signature** : `makepkg` vérifie `SHA256SUMS.asc` sur `SHA256SUMS` avec la clé de `validpgpkeys` (celle de
   `SECURITY.md`) ; `prepare()` contrôle ensuite `sha256sum -c SHA256SUMS --ignore-missing` pour les trois
   `.deb`. La recette elle-même est donc reliée à la signature du propriétaire.

`package()` extrait `data.tar.*` de chaque `.deb` dans `$pkgdir` (`bsdtar`, fourni par `libarchive`, présent
avec `base-devel`). Aucun script d'installation (`install=` absent). Les dossiers `usr/share/lintian` (propres
à Debian) sont retirés.

### 3.4 Fabrication de la recette (`paquets/fabriquer-pkgbuild.sh`)

`paquets/fabriquer-pkgbuild.sh <SHA256SUMS> <sortie/> [--local]` :

- lit le modèle `paquets/aur/PKGBUILD.in` et `paquets/aur/dependances` ;
- tire les noms et empreintes des trois `.deb` de `SHA256SUMS` (refus si l'un manque, s'il y en a d'autres
  ou si une version est mal formée) ; la version de WiMao doit être celle de `Cargo.toml` ;
- écrit `PKGBUILD` et `.SRCINFO` (`makepkg --printsrcinfo`, dans le conteneur Arch figé) ;
- `--local` : sources = noms de fichiers locaux, **sans** `SHA256SUMS.asc` ni `validpgpkeys` (pour la CI des
  paquets, avant que la signature existe). Ce mode écrit en tête du `PKGBUILD` « ESSAI LOCAL, NE PAS
  PUBLIER », et `envoyer-aur.sh` refuse un `PKGBUILD` qui contient cette ligne.

Même entrée, même sortie, octet pour octet (aucune date, ordre fixe).

La recette publiée est **versionnée dans le dépôt WiMao** : `paquets/aur/wimao-bin/PKGBUILD` et `.SRCINFO`.
L'AUR n'en reçoit qu'une copie (§ 4).

## 4. Publication

### 4.1 Le risque propre à l'AUR

L'AUR sert ce que le **compte du mainteneur** y envoie, sans relecture. Quiconque obtient le mot de passe
du compte AUR ou sa clé SSH peut remplacer la recette par une recette piégée, que les utilisateurs
exécuteraient (`makepkg` lance le `PKGBUILD`). Les protections :

1. la recette fait foi dans le dépôt WiMao, relue par PR et testée par la CI (§ 5) ;
2. l'envoi se fait **uniquement depuis la machine du propriétaire**, avec une clé SSH **dédiée à l'AUR**
   (`~/.ssh/aur`, ed25519, protégée par une phrase de passe), qui ne va jamais sur GitHub ni en CI ;
3. l'empreinte de la clé d'hôte de `aur.archlinux.org` est figée dans le script (pas de « trust on first
   use ») ;
4. une **surveillance quotidienne** compare l'AUR à `main` (§ 4.4).

### 4.2 Étape 1 : `scripts/publier-aur.sh vX.Y.Z`

Sur le modèle de `publier-depot-dnf.sh` :

1. contrôles (outils, dépôt propre et à jour, Release publiée et pas brouillon) ;
2. téléchargement de `SHA256SUMS` et `SHA256SUMS.asc`, vérification de la signature (trousseau jetable tiré de
   `docs/cle-publique.asc`, `scripts/lib/publication.sh`) ;
3. `fabriquer-pkgbuild.sh` → `paquets/aur/wimao-bin/` ;
4. essai réel dans le conteneur Arch figé : `makepkg` (télécharge depuis la Release, vérifie empreintes et
   signature), `pacman -U`, `wimao-cli --version` ;
5. branche `aur-vX.Y.Z`, commit, PR. Il ne fusionne jamais.

Aucune phrase de passe n'est demandée à cette étape (rien n'est signé).

### 4.3 Étape 2 : `scripts/envoyer-aur.sh vX.Y.Z`

Après la fusion de la PR, sur `main` à jour et propre :

1. refuse si `paquets/aur/wimao-bin/PKGBUILD` n'annonce pas `pkgver=X.Y.Z`, s'il contient la ligne d'essai
   local, ou si `.SRCINFO` ne correspond pas au `PKGBUILD` (régénéré et comparé) ;
2. clone `ssh://aur@aur.archlinux.org/wimao-bin.git` dans un dossier temporaire, avec
   `GIT_SSH_COMMAND="ssh -i ~/.ssh/aur -o IdentitiesOnly=yes -o UserKnownHostsFile=<fichier figé>
   -o StrictHostKeyChecking=yes"` (rien n'est écrit dans `~/.ssh/config`) ;
3. y copie `PKGBUILD` et `.SRCINFO`, commit `wimao-bin X.Y.Z-1` (auteur : pseudonyme et adresse `noreply`
   de `SECURITY.md`, jamais une adresse personnelle), affiche le diff, puis `git push` ;
4. relit l'AUR par HTTPS et vérifie que le contenu publié est identique à `main`.

**La phrase de passe de `~/.ssh/aur` est demandée par la fenêtre GNOME** (agent SSH) : le script prévient
avant. Option `--a-blanc` : un dépôt git local tient lieu d'AUR, aucun réseau (pour les tests).

### 4.4 Surveillance (`.github/workflows/aur-surveillance.yml`)

Chaque jour (et à la demande) : clone public `https://aur.archlinux.org/wimao-bin.git`, compare `PKGBUILD` et
`.SRCINFO` à ceux de `main`. Différence → le job échoue, et GitHub prévient le propriétaire par e-mail. Tant
que le paquet n'existe pas sur l'AUR, le job réussit avec une note. Permissions minimales (`contents: read`),
actions figées par empreinte (règles de `paquets/verifier-workflows.sh`).

Une fusion sur `main` d'une nouvelle recette fait échouer la surveillance jusqu'à l'envoi : c'est voulu (rappel
d'envoyer). La procédure dit d'envoyer juste après la fusion.

### 4.5 Une seule fois (propriétaire, guidé)

1. Créer le compte sur `https://aur.archlinux.org` (pseudonyme `F1ghtc0re`, adresse non personnelle ; le mot
   de passe n'est jamais transmis). Activer la double authentification si l'AUR la propose.
2. `ssh-keygen -t ed25519 -f ~/.ssh/aur -C "wimao-aur"` (phrase de passe tapée par le propriétaire), puis coller
   `~/.ssh/aur.pub` dans le profil AUR.
3. (Facultatif) publier la clé OpenPGP de WiMao sur `keyserver.ubuntu.com`, pour que `yay` la trouve seul.
   C'est une publication : décision du propriétaire au moment venu. Sans cela, le README donne la commande
   `curl … | gpg --import` et l'empreinte à comparer.

## 5. CI

- `paquets.yml`, nouveau job `installation-aur` (après `wine`, `bridge`, `wimao`, `sommes`) : conteneur Arch
  **figé par empreinte**, `fabriquer-pkgbuild.sh --local` avec les `.deb` de la CI, `makepkg` en utilisateur non
  root, `pacman -U`, test d'ouverture de Dexed (même logique que `tester-installation-rpm.sh`). Prouve que la
  recette fonctionne **avant** l'étiquette.
- Nouveau workflow `aur.yml`, sur les PR qui touchent `paquets/aur/**` : avec la recette réelle (Release,
  signature), `makepkg` + installation + Dexed ; puis trois refus attendus : un `.deb` modifié d'un octet (servi
  localement à la place de l'original), une empreinte fausse dans le `PKGBUILD`, un `SHA256SUMS.asc` altéré.
  Vérifie aussi que `.SRCINFO` correspond au `PKGBUILD`.
- `aur-surveillance.yml` (§ 4.4).

## 6. Documentation

- `README.md` : section « Arch Linux » (import de la clé et empreinte à comparer, `yay -S wimao-bin` ou
  `makepkg -si`, désinstallation).
- `docs/publier-une-version.md` : étape 7 « Publier sur l'AUR » (les deux scripts, quand les lancer) ; « Une
  seule fois » : compte AUR et clé SSH.
- `SECURITY.md` : le risque du compte AUR et ses protections (§ 4.1) ; que faire si la surveillance échoue
  (ne pas fusionner, vérifier le compte, changer la clé SSH, renvoyer la recette de `main`).
- `paquets/README.md`, `ARCHITECTURE.md`, `docs/installation-detaillee.md`.
- Mode d'emploi intégré (fr, en, es) : mention d'Arch là où Ubuntu et Fedora sont cités.
- `docs/ameliorations-reportees.md` : limites du § 7.

## 7. Tests

- `shellcheck` sur chaque script ; essais automatiques sur le modèle de `scripts/tests/` :
  - `fabriquer-pkgbuild.sh` : sortie identique d'une fois sur l'autre ; refus d'un `SHA256SUMS` incomplet, d'une
    version mal formée, d'une version différente de `Cargo.toml` ; `--local` marqué ;
  - `envoyer-aur.sh --a-blanc` : envoi vers un faux AUR local ; refus d'un dépôt pas propre, d'une version qui ne
    correspond pas, d'un `PKGBUILD` d'essai local, d'un `.SRCINFO` périmé.
- CI : § 5.
- Essai réel en fin de sous-projet : `publier-aur.sh v0.7.0` (PR), puis, après création du compte,
  `envoyer-aur.sh v0.7.0` et `yay -S wimao-bin` dans un conteneur Arch.

## 8. Limites connues

- **Compte AUR :** la sécurité de l'AUR repose sur ce compte ; la surveillance détecte une modification, elle ne
  l'empêche pas. Un utilisateur qui installe dans la fenêtre entre la modification et la correction serait
  exposé. `yay` montre le diff de la recette avant de la lancer : le README invite à le lire.
- **Clé OpenPGP :** sans publication sur un serveur de clés, l'utilisateur doit l'importer à la main une fois.
- **Arch évolue chaque jour :** une future version de glibc ou d'une bibliothèque peut casser les binaires
  compilés pour Ubuntu 24.04. Le job `installation-aur` (image figée) ne le verra pas ; `aur.yml` met le
  conteneur à jour (`pacman -Syu`) et le verra à la prochaine recette. Un essai régulier est reporté.
- **Taille :** chaque mise à jour retélécharge 87 Mo (forme A).
- **Expiration de la clé :** comme pour apt et dnf, la sous-clé expire le 2027-11-01 ; après prolongation, les
  utilisateurs Arch doivent réimporter la clé.
