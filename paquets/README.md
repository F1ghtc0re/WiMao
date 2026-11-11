# Les paquets Debian, RPM et AUR de WiMao

Ce dossier contient tout ce qui fabrique, contrôle et teste les trois paquets `.deb` de WiMao, ainsi que les trois
paquets `.rpm` (Fedora) qui en sont tirés (section « Les paquets RPM (Fedora) » plus bas).
Pour *installer* WiMao, lisez plutôt le `README.md` à la racine ; pour *publier* une version,
`docs/publier-une-version.md`.

Ces `.deb` sont aussi servis par un **dépôt `apt` signé** (`https://f1ghtc0re.github.io/WiMao`) : il ne
fabrique rien, il publie les `.deb` des Releases. Ce qui le construit est ailleurs : l'index se prépare et
se signe chez le propriétaire (`scripts/publier-depot-apt.sh`, dossier `depot-apt/`), le site s'assemble en
CI (`scripts/assembler-site-apt.sh`, `.github/workflows/depot-apt.yml`). Voir `docs/publier-une-version.md`,
étape 5, et `SECURITY.md`, « Le dépôt APT ». Fedora a son pendant, le **dépôt `dnf` signé** (`scripts/publier-depot-dnf.sh`,
dossier `depot-dnf/`) : voir `docs/publier-une-version.md`, étape 6, et `SECURITY.md`, « Le dépôt dnf ».

Arch Linux a son pendant, la **recette AUR** `wimao-bin` : voir la section « La recette AUR (Arch Linux) » plus bas,
`docs/publier-une-version.md`, étape 7, et `SECURITY.md`, « Le paquet AUR ».

## Les trois paquets

| Paquet | Version | Installe | Script | Dossier des fichiers de contrôle |
|---|---|---|---|---|
| `wimao-wine` | `11.20+wimao1` | `/opt/wimao/wine` : Wine 11.20 (mode WoW64) avec le correctif `patches-v2/` (garde `wm_state_serial`), sans fichiers de développement | `fabriquer-wine.sh` | `wine/` |
| `wimao-bridge` | `5.1.1+dxvk3.1.1-1` | `/opt/wimao/yabridge` (yabridge 5.1.1) et `/opt/wimao/dxvk` (DLL de DXVK 3.1.1) | `fabriquer-bridge.sh` | `bridge/` |
| `wimao` | `0.4.0-1` (version du workspace Cargo, suivie de `-1`) | `/usr/bin/{wimao,wimao-cli}`, recettes, traductions, icônes, `.desktop`, fiche AppStream | `fabriquer-wimao.sh` | `wimao/` |

Cibles : **amd64 seulement**, **Ubuntu 24.04+ et Debian 13+**. Tous les paquets sont fabriqués dans
un conteneur `ubuntu:24.04` (image figée par son empreinte `@sha256:`), la plus ancienne des cibles :
un paquet fabriqué là s'installe aussi sur les systèmes plus récents.

`carla` et `x11-utils` sont des `Recommends` de `wimao`, pas des `Depends` : Carla est absente de
Debian 13, où l'installation doit réussir quand même (seul le test d'ouverture des plugins est alors
désactivé, avec un message clair).

Chaque dossier de paquet contient : `control.in` (modèle du fichier `control`, avec `@VERSION@` et
`@DEPENDANCES@` remplacés à la fabrication), `changelog`, `copyright`, `lintian-overrides` et, pour
`wimao` et `bridge`, `paquets-compilation.txt` (paquets Ubuntu nécessaires pour compiler : une seule
liste, lue à la fois par la CI et par `scripts/signer-version.sh`). `wine/dependances.md` explique
d'où viennent les dépendances de `wimao-wine`.

## Les paquets RPM (Fedora)

Les trois RPM ne sont **pas recompilés** : `fabriquer-rpm.sh` extrait un `.deb` déjà fabriqué et l'emballe avec
`rpmbuild`. Les fichiers installés sont donc exactement ceux des `.deb`, aux mêmes chemins (`/opt/wimao/…`,
`/usr/bin/…`). Seules changent les métadonnées : noms de dépendances Fedora, version RPM (celle du `.deb` sans
son `-1` final, `Release` = `1`) et architecture `x86_64`. Cibles : **Fedora 44 et 45**.

| Fichier | Rôle |
|---|---|
| `fabriquer-rpm.sh` | `fabriquer-rpm.sh <paquet.deb> <sortie/>`, **dans un conteneur Fedora 45** (paquets `rpm-build`, `dpkg`, `zstd`). Exige `SOURCE_DATE_EPOCH`. Écrit `<nom>-<version>-1.x86_64.rpm`. Refuse un `.deb` inconnu ou non `amd64`. |
| `rpm/<paquet>.deps` | Dépendances obligatoires (`Requires`) du RPM, une par ligne, noms Fedora. |
| `rpm/<paquet>.recommends` | Dépendances facultatives (`Recommends`) : `Carla` et `xwininfo` pour `wimao`, par exemple. |
| `rpm/<paquet>.resume` | Résumé d'une ligne. |
| `rpm/wimao.spec.in` | Modèle du `.spec` ; chaque réglage de reproductibilité y est commenté. |
| `tester-installation-rpm.sh` | Pendant Fedora de `tester-installation.sh`. **À lancer seulement dans un conteneur jetable.** Variable obligatoire `WIMAO_ESSAI_WINE_SYSTEME` (`oui` ou `non`, voir plus bas). |

À savoir :

- La détection automatique des dépendances de `rpmbuild` est désactivée (`AutoReqProv: no`) : elle ajouterait des
  dépendances internes à Wine qu'aucun paquet ne fournit. Les dépendances sont donc celles des fichiers `.deps`.
- Aucun script d'installation (pas de `%post`) ; pas de paquet de débogage.
- **Reproductibilité** : deux fabrications à partir des mêmes `.deb` donnent des RPM identiques octet pour octet
  (`SOURCE_DATE_EPOCH`, compression `zstd` fixée, nom de machine fixe, pas de `%changelog` daté du jour). L'image
  Fedora est figée par empreinte, mais `rpm-build` et `zstd` y sont installés depuis les dépôts vivants de Fedora :
  une mise à jour de ces outils entre la CI et la refabrication locale peut changer les octets. Il faut alors
  relancer la CI, puis signer peu après.
- **Jobs de la CI** : `rpm` fabrique les trois RPM deux fois et compare ; `sommes` les ajoute à `SHA256SUMS` (six
  paquets au total) ; `installation-rpm` les installe pour de vrai dans Fedora 45 avec le `wine` de Fedora déjà
  présent (vérifie l'absence de conflit), Fedora 45 sans lui et Fedora 44 sans lui (ce dernier mode prouve que
  `wimao-wine.deps` suffit), ouvre le plugin Dexed avec `wimao-cli verifier`, vérifie `rpm -V`, puis retire les
  paquets. `brouillon` joint les RPM à la Release.
- **SELinux** : un conteneur ne l'applique pas. L'installation est prouvée, l'ouverture des plugins sur un bureau
  Fedora réel ne l'est pas encore (voir `docs/ameliorations-reportees.md`).
- **Dépôt `dnf`** : `scripts/publier-depot-dnf.sh vX.Y.Z` (chez le propriétaire) construit l'index avec
  `createrepo_c` dans le conteneur Fedora figé et signe `repomd.xml`. `scripts/lib/lire-index-dnf.py` relit et
  contrôle cet index (une adresse, une taille et une somme SHA-256 par RPM, aucun `xml:base`) ; il sert aussi à la
  CI. Le workflow `depot-apt.yml` ajoute le job `installation-dnf` (Fedora 44 et 45, index altéré refusé).

## La recette AUR (Arch Linux)

Sur l'AUR, `wimao-bin` n'est pas un paquet fabriqué mais une **recette** (`PKGBUILD` et `.SRCINFO`) : `makepkg`
télécharge les trois `.deb` de la Release GitHub, vérifie leurs empreintes SHA-256 **et** la signature de `SHA256SUMS`,
puis en tire un paquet Arch (mêmes fichiers que les `.deb`). Cible : **Arch Linux x86_64**. Le détail de chaque script
est dans son en-tête.

| Fichier | Rôle |
|---|---|
| `fabriquer-pkgbuild.sh` | `fabriquer-pkgbuild.sh <SHA256SUMS> <sortie/> [--local]` : écrit `PKGBUILD` et `.SRCINFO`. Exige exactement trois `.deb` dans `SHA256SUMS` (les `.rpm` sont ignorés) et la version de `Cargo.toml`. Même entrée, même sortie. `--local` marque la recette « ESSAI LOCAL, NE PAS PUBLIER ». |
| `aur/PKGBUILD.in`, `aur/prepare.in`, `aur/dependances` | Modèle de la recette, étape `prepare` et dépendances en noms de paquets Arch. |
| `aur/commun-aur.sh` | Bibliothèque sourcée : image Arch figée par empreinte (`IMAGE_ARCH`, seule définition), `makepkg` sous un utilisateur non privilégié, fonctions d'essai. |
| `aur/aur.archlinux.org.known_hosts` | Clé d'hôte figée de l'AUR, utilisée par `scripts/envoyer-aur.sh`. |
| `tester-installation-aur.sh` | Recette en mode `--local` puis installation réelle, Dexed, désinstallation, **dans un conteneur Arch jetable**. Job `installation-aur` de `paquets.yml`, avant l'étiquette. |
| `tester-recette-aur.sh` | Éprouve la vraie recette (Release, signature) puis prouve que trois sources trafiquées sont refusées. Workflow `aur.yml`, sur les PR qui touchent `paquets/aur/`. |
| `tests/tester-fabriquer-pkgbuild.sh` | Teste `fabriquer-pkgbuild.sh` (sortie stable, refus d'une liste incomplète ou d'une mauvaise version). |

Publication : `scripts/publier-aur.sh vX.Y.Z` (essai dans Arch, branche, PR ; ne signe rien) puis, après fusion,
`scripts/envoyer-aur.sh vX.Y.Z` (envoi à l'AUR depuis la machine du propriétaire). Le workflow
`aur-surveillance.yml` compare chaque jour l'AUR à `main`.

## Rôle de chaque fichier

| Fichier | Rôle |
|---|---|
| `commun.sh` | Fonctions partagées (téléchargement avec empreinte SHA-256 vérifiée, normalisation des droits et des dates, assemblage du `.deb`). Se charge avec `source`, ne se lance pas. |
| `fabriquer-wine.sh` | Compile Wine et fabrique `wimao-wine`. Plusieurs heures à froid, plusieurs Go de disque. `--paquets-compilation` affiche les paquets Debian à installer avant. |
| `fabriquer-bridge.sh` | Télécharge yabridge et DXVK (empreintes vérifiées) et fabrique `wimao-bridge`. |
| `fabriquer-wimao.sh` | Compile `wimao` et `wimao-cli` en mode paquet, puis fabrique `wimao`. |
| `tester-installation.sh` | Installe les trois `.deb` dans un système vierge, les contrôle, les retire. **À lancer seulement dans un conteneur jetable** (voir « Tester »). |
| `verifier-workflows.sh` | Vérifie que les workflows GitHub sont « figés » : chaque action identifiée par l'empreinte de son commit, chaque image par son `@sha256:`, droits d'écriture refusés hors du job `brouillon`, déclencheurs `pull_request_target` / `workflow_run` et secrets refusés. |
| `ecrire-notes-release.sh` | Écrit les notes du brouillon de Release (partagé par `brouillon` et `brouillon-a-blanc`). |
| `tester-verifier-workflows.sh` | Teste le script précédent : chaque forme non figée doit être refusée. |
| `lintian.md` | Liste, justifiée, des avertissements `lintian` acceptés. C'est la source de vérité lue par le test d'installation. |

Les en-têtes de chaque script détaillent leurs variables, ce qu'ils écrivent et où : lisez-les, ils
sont la référence.

## Fabriquer en local

Chaque script prend un dossier de sortie. Les `.deb` y sont écrits, avec un cache de
téléchargements et un dossier de travail (`travail-*`, supprimé puis recréé à chaque fabrication).

```sh
# Paquets Debian nécessaires pour compiler (liste unique, partagée avec la CI) :
sed 's/#.*//' paquets/wimao/paquets-compilation.txt | xargs sudo apt-get install -y --no-install-recommends
sed 's/#.*//' paquets/bridge/paquets-compilation.txt | xargs sudo apt-get install -y --no-install-recommends
paquets/fabriquer-wine.sh --paquets-compilation | xargs sudo apt-get install -y --no-install-recommends

# Fabrication (Rust requis pour wimao ; compter plusieurs heures pour Wine) :
paquets/fabriquer-bridge.sh /tmp/wimao-paquets
paquets/fabriquer-wimao.sh  /tmp/wimao-paquets
paquets/fabriquer-wine.sh   /tmp/wimao-paquets
```

Le plus simple est de ne fabriquer que `wimao` et `wimao-bridge` : `wimao-wine` est lent, et la CI
le met en cache. Un `.deb` obtenu sur votre machine ne sera pas identique octet pour octet à celui
de la CI, sauf si vous reproduisez son conteneur (voir « Reproductibilité »).

`fabriquer-wine.sh` accepte `WIMAO_WINE_DEJA_COMPILE` pour sauter la compilation lors d'un essai
d'empaquetage ; le paquet obtenu n'est alors **pas** celui de la version, jamais à livrer.

**Ne lancez pas `export WIMAO_PAQUET=1` dans votre terminal** : les scripts le posent eux-mêmes pour
la seule compilation des paquets. Avec cette variable, `cargo test` échoue (trois tests de
l'interface qui lisent les traductions dans le dépôt).

## Reproductibilité

Fabriquer deux fois les mêmes sources doit donner des `.deb` identiques octet pour octet : c'est ce
qui permet à une personne de confiance de vérifier que la CI n'a rien ajouté. Les moyens :

- date des fichiers fixée à celle du dernier commit (`SOURCE_DATE_EPOCH`, `git log -1 --format=%ct`) ;
- propriétaire `root:root` (`dpkg-deb --root-owner-group`), droits normalisés, compression `xz` fixée ;
- à la compilation de `wimao`, la variable `WIMAO_PAQUET=1` pose le cfg Rust `wimao_paquet` : les
  repères de développement (chemins du dépôt, du dossier de compilation) ne sont pas compilés, et
  `--remap-path-prefix` efface les chemins insérés par `rustc`. Le script refuse tout chemin de la
  machine de fabrication resté dans un binaire.

Ce qui est prouvé, et par quoi :

| Paquet | Preuve | Où |
|---|---|---|
| `wimao`, `wimao-bridge` | refabriqués dans un second conteneur, `sha256` identiques, **depuis n'importe quel dossier** de la même image | job `reproductible` (chaque lancement) |
| `wimao`, `wimao-bridge` | refabriqués avec `podman` sur la machine du propriétaire | `scripts/signer-version.sh` ; essai à blanc en CI : job `signature-a-blanc` |
| `wimao-wine` | deux compilations à froid (sans cache) donnent le même `.deb` | job `reproductible-wine` (étiquettes `v*` et lancement manuel) |

La reproductibilité est garantie *dans la même image Ubuntu* : les paquets Ubuntu sont ceux du jour de
la fabrication. Si Ubuntu publie une mise à jour d'une bibliothèque entre deux fabrications, les
dépendances relevées par `dpkg-shlibdeps` peuvent changer ; relancer la CI ou signer peu après elle.

## Tester

| Quoi | Commande | Où |
|---|---|---|
| Workflows figés | `paquets/verifier-workflows.sh` | local ou CI (job `chaine`) |
| Ce contrôle lui-même | `paquets/tester-verifier-workflows.sh` | local ou CI (job `chaine`) |
| Script de signature (à blanc) | `scripts/tests/tester-signer-version.sh` | local ou CI (job `chaine`) |
| Installation complète | `paquets/tester-installation.sh <dossier des .deb> <dossier des fixtures>` | **conteneur jetable** (CI : job `installation`) |

Le test d'installation est joué dans la CI sur **Ubuntu 24.04 et Debian 13**. Il enchaîne :
vérification de `SHA256SUMS`, `apt install` des trois `.deb`, `lintian` (aucune erreur, aucun
avertissement hors de `lintian.md`), validation de la fiche AppStream (`appstreamcli validate --no-net`) et
du `.desktop` (`desktop-file-validate`) installés, permissions (rien d'inscriptible par le groupe ou les autres, pas
de `setuid`, tout à `root:root`, aucun script de mainteneur), recherche de chemins de la machine de
fabrication (`strings`), installation d'un faux plugin NSIS puis du vrai plugin Dexed 1.1.0
(`wimao-cli verifier` doit réussir à l'ouvrir, avec un serveur audio JACK factice), lancement de
l'interface sous Xvfb (processus vivant, fenêtre, capture, aucune panique ni erreur au journal de WiMao), et `apt remove` (les données de l'utilisateur doivent rester).

**N'exécutez jamais `tester-installation.sh` sur votre machine de travail** : il installe des
paquets, crée un utilisateur `essai` et écrit dans `/opt`, `/usr` et `/home`. Pour le rejouer en local,
utilisez un conteneur (`podman run --rm -it -v …`) de l'image voulue.

## Lintian

`lintian --fail-on error,warning` est exécuté sur les trois paquets. Les écarts propres à un paquet
sont dans son `lintian-overrides` (commentés) ; ceux qui valent pour tous sont dans `lintian.md`.
Ajouter un écart, c'est l'accepter : la justification est obligatoire.

## La CI : `.github/workflows/paquets.yml`

| Job | Rôle |
|---|---|
| `wine` | Compile `wimao-wine` (le `.deb` final est mis en cache : environ 70 minutes à froid). |
| `bridge`, `wimao` | Fabriquent `wimao-bridge` et `wimao`. |
| `reproductible` | Les refabrique dans un second conteneur et compare les `sha256`. |
| `reproductible-wine` | Refabrique Wine sans cache (étiquettes `v*` et lancement manuel seulement). |
| `rpm` | Réemballe les trois `.deb` en RPM (conteneur Fedora 45 figé), deux fois, et compare octet pour octet. |
| `sommes` | Réunit les trois `.deb` et les trois RPM, écrit `SHA256SUMS`. |
| `installation` | Test d'installation décrit ci-dessus (Ubuntu 24.04 et Debian 13). |
| `installation-aur` | Installation réelle de la recette AUR (`--local`) et ouverture d'un plugin, sur Arch Linux (image de départ figée, puis mise à jour `pacman -Syu` : l'Arch d'aujourd'hui). |
| `installation-rpm` | Installation réelle des RPM et ouverture d'un plugin, sur Fedora 45 (avec et sans le `wine` de Fedora) et Fedora 44. |
| `signature-a-blanc` | Rejoue `scripts/signer-version.sh --a-blanc` avec une clé jetable (refabrique les `.deb`, puis les RPM). |
| `brouillon` | Attestation de provenance et Release **brouillon** sur une étiquette `v*` (ou lancement manuel avec `a_blanc` faux). Seul job avec des droits d'écriture. |
| `brouillon-a-blanc` | Même logique **sans rien créer** et sans droit d'écriture, sur demande de fusion et lancement manuel par défaut. |

Le lancement manuel (onglet *Actions*, « Run workflow ») propose l'option `a_blanc` (vraie par
défaut). La CI générale `.github/workflows/ci.yml` a les jobs `unitaires`, `integration` et `chaine`
(`shellcheck -x` de tous les scripts, workflows figés et leurs tests, `cargo deny`, `cargo audit`, test de `signer-version.sh`).

## Limites connues

Voir `docs/ameliorations-reportees.md`, section « Après le sous-projet 4 ».
