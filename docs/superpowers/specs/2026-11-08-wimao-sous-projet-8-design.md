# WiMao — sous-projet 8 : paquets RPM et dépôt dnf signé (Fedora)

Date : 2026-11-08 · Statut : validé par le propriétaire (« réutiliser nos binaires », « comme pour apt ») ·
Branche : `sous-projet-8`

## 1. But

Un utilisateur de Fedora installe et met à jour WiMao aussi simplement qu'un utilisateur d'Ubuntu, avec le
même niveau de sécurité.

```sh
sudo curl -fsSL -o /etc/yum.repos.d/wimao.repo https://f1ghtc0re.github.io/WiMao/wimao.repo
sudo dnf install wimao
```

**Critère de réussite :** dans un système Fedora vierge (conteneur, versions maintenues), la commande
ci-dessus installe WiMao ; `wimao-cli verifier` ouvre le plugin de test Dexed ; un index modifié d'un seul
octet est refusé ; `sudo dnf upgrade` installe la version suivante.

**Hors périmètre :** AUR et Flatpak (sous-projets suivants), architectures autres que x86_64, recompilation
sur Fedora, signature individuelle de chaque RPM (§ 4), openSUSE et autres distributions RPM.

## 2. Constat (essais du 2026-11-08, conteneur `fedora:latest`, Fedora 45)

- Les binaires des `.deb` 0.5.0 (Wine, yabridge, wimao-cli) trouvent leur glibc (2.44) et `wine --version`
  répond `wine-11.20`. Il ne manque que des bibliothèques ordinaires, à déclarer en dépendances.
- rpm 6.1 et dnf5 5.4 : une version comme `5.1.1+dxvk3.1.1` est acceptée.
- Un `repomd.xml` signé par une sous-clé ed25519 est vérifié par `repo_gpgcheck=1`. Un `repomd.xml` modifié
  d'un octet rend le dépôt inutilisable (« No match for argument »).
- `createrepo_c --location-prefix` fait télécharger les RPM depuis une autre adresse que l'index.
- Tous les paquets système voulus existent : `Carla`, `7zip`, `cabextract`, `xorg-x11-server-Xvfb`, `xwininfo`,
  `libadwaita`, `alsa-lib`, `pulseaudio-libs`, `vulkan-loader`, `gnutls`, `ocl-icd`.

## 3. Les paquets

### 3.1 Principe : réemballer les `.deb`, ne rien recompiler

Nouveau script `paquets/fabriquer-rpm.sh <paquet.deb> <sortie/>`. Il extrait le contenu d'un `.deb` déjà
fabriqué et l'emballe avec `rpmbuild`, dans un conteneur Fedora **figé par empreinte**. Les fichiers installés
sont donc exactement ceux des `.deb`, aux mêmes chemins (`/opt/wimao/…`, `/usr/bin/…`) : le logiciel ne change
pas.

| RPM | Version | Release | Dépendances obligatoires | Facultatives (`Recommends`) |
|---|---|---|---|---|
| `wimao-wine` | celle du `.deb` (`11.20+wimao1`) | `1` | `freetype`, `fontconfig`, `libX11`, `libXext`, `libXrender`, `libXrandr`, `libXi`, `libXcursor`, `libXcomposite`, `libXfixes`, `vulkan-loader`, `gnutls`, `pulseaudio-libs`, `alsa-lib`, `libOpenCL.so.1()(64bit)` (fourni par `ocl-icd` ou par `OpenCL-ICD-Loader`, qui s'excluent) | `mesa-libGL`, `mesa-libEGL`, `libXinerama`, `libXxf86vm`, `cups-libs`, `SDL2`, `dbus-libs` |
| `wimao-bridge` | `5.1.1+dxvk3.1.1` | `1` | `wimao-wine >= 11.20+wimao1` | — |
| `wimao` | celle du workspace, sans le `-1` Debian | `1` | `wimao-wine >= 11.20+wimao1`, `wimao-bridge >= 5.1.1+dxvk3.1.1`, `libadwaita >= 1.5`, `gtk4`, `xorg-x11-server-Xvfb`, `7zip`, `cabextract`, `zstd`, `tar` | `Carla`, `xwininfo` |

Les listes sont déclarées dans un fichier par paquet (`paquets/rpm/<paquet>.deps`, une dépendance par ligne),
lisible sans lire le script. La détection automatique de `rpmbuild` est désactivée (`AutoReqProv: no`) : elle
ajouterait des dépendances internes à Wine qu'aucun paquet ne fournit.

Les paquets sont `x86_64`, sans paquet de débogage, et n'exécutent aucun script d'installation (pas de `%post`).

### 3.2 Reproductibilité

Deux fabrications à partir des mêmes `.deb` donnent des RPM identiques octet pour octet : `SOURCE_DATE_EPOCH`
(date du commit), `%use_source_date_epoch_as_buildtime`, `%clamp_mtime_to_source_date_epoch`, `%_buildhost`
fixe, compression `zstd` fixée, sans `%changelog` généré à la date du jour. Le contrôle est fait en CI et
refait sur la machine du propriétaire par `signer-version.sh`, comme pour les `.deb`.

### 3.3 CI (`.github/workflows/paquets.yml`)

- Nouveau job `rpm` (après `wine`, `bridge`, `wimao`) : fabrique les trois RPM, les fabrique une seconde fois et
  compare. Image Fedora figée par empreinte (règle de `paquets/verifier-workflows.sh`).
- `sommes` : les RPM entrent dans `SHA256SUMS`. Ils sont couverts par l'attestation de provenance, comme les
  `.deb`.
- Nouveau job `installation-rpm` : installation réelle et test d'ouverture (Dexed par `carla-single`, même
  logique que `tester-installation.sh`), sur les **deux versions de Fedora maintenues** (44 et 45).
- `brouillon` : les trois RPM rejoignent le brouillon de Release.

## 4. Dépôt dnf signé

### 4.1 Choix de sécurité

Comme pour apt, **c'est l'index qui est signé** : `repomd.xml.asc` couvre `repomd.xml`, qui contient les
empreintes des métadonnées, qui contiennent l'empreinte SHA-256 de chaque RPM. La chaîne est complète.

Les RPM ne sont pas signés un par un : signer modifierait le fichier après la CI et casserait la comparaison
octet pour octet, l'attestation et `SHA256SUMS`. Le `.repo` contient donc `repo_gpgcheck=1` et `gpgcheck=0`. Un
RPM téléchargé à la main se vérifie avec `SHA256SUMS.asc`, comme un `.deb`.

Conséquence assumée : dnf affiche « skipped OpenPGP checks » à l'installation ; le mode d'emploi l'explique.

### 4.2 Contenu

Publié sur le site existant, à `https://f1ghtc0re.github.io/WiMao/` :

- `rpm/repodata/…` et `rpm/repodata/repomd.xml.asc` ;
- `wimao.repo` (`baseurl=https://f1ghtc0re.github.io/WiMao/rpm`, `repo_gpgcheck=1`, `gpgcheck=0`,
  `gpgkey=https://f1ghtc0re.github.io/WiMao/wimao.asc`, la clé déjà publiée pour apt).

Les RPM **ne sont pas copiés sur le site** : l'index pointe vers
`https://github.com/F1ghtc0re/WiMao/releases/download/vX.Y.Z/` (`--location-prefix`). Le site ne grossit donc pas
avec les RPM (limite GitHub Pages de 1 Go).

L'index ne décrit que **la dernière version** publiée. Revenir à une version antérieure passe par les fichiers
des Releases. C'est plus simple et suffisant en bêta.

### 4.3 Publication

Nouveau script `scripts/publier-depot-dnf.sh vX.Y.Z`, sur le modèle de `publier-depot-apt.sh` :

1. mêmes contrôles (outils, dépôt propre et à jour, Release publiée et pas brouillon) ;
2. téléchargement des RPM et de `SHA256SUMS`/`SHA256SUMS.asc`, vérification de la signature (trousseau jetable
   tiré de `docs/cle-publique.asc`) et de chaque empreinte ;
3. index construit par `createrepo_c` dans le conteneur Fedora figé (`depot-dnf/`, quelques Ko) ;
4. signature de `repomd.xml` avec la sous-clé, contrôle de ce qui vient d'être signé ;
5. branche `depot-dnf-vX.Y.Z`, commit et PR. Il ne fusionne jamais.

`depot-apt.yml` : l'assemblage vérifie aussi la signature de `depot-dnf/`, ajoute `rpm/` et `wimao.repo` au site, et
un job installe depuis ce site dans Fedora, puis prouve qu'un index altéré est refusé. Le déploiement ne se fait
toujours que depuis `main`. Tant que `depot-dnf/` n'existe pas, le site reste celui d'aujourd'hui.

## 5. Signature locale (`scripts/signer-version.sh`)

Le script contrôle aussi les RPM (présence, empreintes, attestation) et les refabrique à partir des `.deb`
qu'il vient de vérifier, puis les compare octet pour octet. `SHA256SUMS` couvre six paquets au lieu de trois : il
n'y a toujours qu'une signature, donc une seule phrase de passe.

Les deux premières étapes de `docs/publier-une-version.md` ne changent pas. Une étape « Publier dans le dépôt
dnf » suit celle du dépôt APT. La procédure dit de déverrouiller la clé **avant** la refabrication.

## 6. Documentation

- `README.md` : section Fedora dans « Installation » (commandes, vérification manuelle, signification de
  « skipped OpenPGP checks », retrait du dépôt).
- `paquets/README.md`, `ARCHITECTURE.md`, `docs/publier-une-version.md`, `SECURITY.md` si besoin.
- Mode d'emploi intégré (fr, en, es) : mention de Fedora là où Ubuntu/Debian sont cités.
- `docs/ameliorations-reportees.md` : limites du § 8.

## 7. Tests

- `shellcheck` sur chaque script ; essais automatiques des nouveaux scripts avec une clé jetable, sur le modèle de
  `scripts/tests/tester-signer-version.sh`.
- Refus attendus, testés : empreinte de RPM fausse, signature absente, Release encore brouillon, version mal
  formée, dépôt non propre.
- CI : reproductibilité, installation réelle sur Fedora 44 et 45, index altéré refusé.
- Essai réel en fin de sous-projet : mise à jour de la version N-1 vers N à travers un dépôt de test, dans un
  conteneur Fedora.

## 8. Limites connues

- **SELinux :** les conteneurs ne reproduisent pas un Fedora de bureau avec SELinux actif. L'installation y est
  prouvée, pas l'ouverture des plugins sur un bureau réel. Le premier essai réel viendra d'un utilisateur Fedora ;
  le problème éventuel sera traité comme un bug et documenté.
- **Compatibilité :** les binaires sont compilés pour Ubuntu 24.04 (glibc 2.39). Fedora 40 et plus récent conviennent ;
  la CI sur les versions maintenues détectera une rupture future.
- **Taille des Releases :** environ 85 Mo de plus par version, sans effet sur le site.
- **Expiration de la clé :** comme pour apt, la sous-clé expire le 2027-11-01.
- **Pas d'expiration de l'index :** comme pour apt, rien ne fait expirer l'index dnf. Quelqu'un qui contrôlerait le
  site, ou une PR fusionnée qui restaurerait un vieil index signé, pourrait maintenir des utilisateurs sur une
  ancienne version authentique ; jamais leur faire installer un paquet forgé.
