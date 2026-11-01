# WiMao — Sous-projet 5 : première version publique (conception)

Date : 2026-11-01. Statut : conception approuvée oralement le 2026-11-01, spec à relire.

## 1. But

Publier **WiMao 0.4.0** comme première version publique : un dépôt GitHub public, sans donnée
personnelle, protégé contre les modifications non autorisées, et une Release signée que n'importe qui
peut vérifier (sommes, signature OpenPGP, attestations de provenance).

Critères de réussite :

1. Aucun nom d'utilisateur, nom de machine ni adresse e-mail personnelle dans les fichiers **et dans
   l'historique** du dépôt public (seule l'adresse `noreply` de GitHub et le pseudonyme F1ghtc0re y
   figurent, comme aujourd'hui).
2. Le dépôt public est protégé : `main` ne change que par PR avec CI verte ; seules les personnes
   autorisées créent, déplacent ou suppriment une étiquette `v*` ; secrets détectés et bloqués à l'envoi ;
   failles signalables en privé ; CI à jeton en lecture seule ; CI des contributeurs extérieurs soumise à
   approbation.
3. La Release `v0.4.0` est publiée avec les trois `.deb`, `SHA256SUMS`, `SHA256SUMS.asc`,
   `cle-publique.asc` et le fichier d'attestation, après `scripts/signer-version.sh v0.4.0` réussi.

Hors champ : AUR, Flatpak, dépôt APT, PPA (reportés) ; les mises à jour Dependabot ouvertes sur l'ancien
dépôt (PR #7 à #11, versions majeures d'actions et de `serial_test`) : Dependabot les reproposera sur le
nouveau dépôt, elles seront traitées après la publication.

## 2. Ce que l'utilisateur a décidé

- **Dépôt neuf** plutôt que réécriture de l'historique : GitHub conserve les commits des PR (#1 à #11)
  même après une réécriture forcée ; seul un dépôt neuf efface réellement les anciens commits.
- L'ancien dépôt est **renommé `WiMao-archive` et reste privé** : rien n'est perdu.
- Le nouveau dépôt garde le nom **`F1ghtc0re/WiMao`** : aucune adresse du code (Cargo.toml, AppStream,
  README, SECURITY.md, dialogues) ne change.
- Le passage en public est confirmé par l'utilisateur **au moment de le faire** (action visible de tous,
  difficile à annuler).

## 3. Données personnelles

### 3.1 État constaté (2026-11-01)

- Le nom d'utilisateur Linux figure dans `NOTES.md` (ligne 139, chemin Windows `C:\users\…\Downloads`),
  dans la documentation et les tests de `crates/wimao-core/src/rapport.rs`, et dans
  `docs/superpowers/plans/2026-10-31-wimao-sous-projet-1.md` (exemples d'anonymisation et chemins).
- Sept anciens commits le contiennent (ils disparaissent avec le dépôt neuf).
- Le nom de la machine et l'adresse e-mail personnelle n'apparaissent nulle part. Tous les commits ont
  l'auteur `F1ghtc0re <…@users.noreply.github.com>`.
- `NOTES.md` décrit le matériel (processeur, carte graphique) : utile pour reproduire un problème, non
  identifiant, **conservé**. Ses mentions « dépôt privé » sont mises à jour.

### 3.2 Remplacements

- Tests et documentation de `rapport.rs` : le nom réel est remplacé par **`alice`** (et ses variantes :
  `/home/alice`, `alice@a.fr`, `ALICE`, `alicex`, `alice-pc`, `Alice`). Les cas testés restent
  exactement les mêmes (préfixe d'un autre nom, casse, machine contenant le nom, etc.).
- `NOTES.md` et le plan du sous-projet 1 : le nom est remplacé par `<utilisateur>`.

### 3.3 Garde-fou : `scripts/verifier-donnees-perso.sh`

Script à lancer avant toute publication (et par la CI à chaque PR), qui cherche dans **les fichiers
suivis par git** (`git grep`, fichiers texte **et binaires**), sans tenir compte de la casse :

- le nom d'utilisateur (`$USER`, à défaut `id -un`) ;
- le nom de la machine (`hostname`) ;
- l'adresse e-mail de `git config user.email` **si elle n'est pas une adresse `noreply` de GitHub** ;
- des termes supplémentaires facultatifs, un par ligne, dans le fichier non suivi
  `.git/donnees-perso` (les lignes vides et celles qui commencent par `#` sont ignorées).

Aucun de ces termes n'est écrit dans le dépôt : ils sont lus sur la machine au moment de l'exécution.

Règles :

- un terme de moins de 3 caractères est ignoré (avec un message), pour éviter les faux positifs ;
- les termes génériques `root`, `runner`, `ubuntu`, `localhost`, `user`, `utilisateur` sont ignorés : ce
  sont les noms des machines de CI et des exemples, pas des données personnelles ;
- fichiers binaires : une seconde passe (`git grep -l -a`, moins les fichiers texte déjà vus) les fouille
  et n'affiche que le nom, sous la forme `fichier (binaire)` ; mêmes codes d'erreur que la passe texte ;
- sortie : la liste `fichier:ligne` de chaque occurrence (ou `fichier (binaire)`), **sans** répéter le terme trouvé (le journal
  de CI d'un dépôt public ne doit pas divulguer le terme) ; code de retour 1 s'il y a au moins une
  occurrence, 0 sinon, 2 en cas d'erreur d'usage ;
- options : `--aide` ; `--terme <texte>` (répétable) ajoute un terme, et `--seulement` n'utilise que les
  termes passés par `--terme` (pour les tests) ; `--dossier <dépôt>` cherche dans un autre dépôt git que
  le dossier courant.

Dans la CI (job `chaine`), le script tourne sur le runner : il y cherche `runner` (ignoré) et le nom de
la machine de CI ; il sert surtout à garantir que le script reste fonctionnel. Il est testé par
`scripts/tests/tester-verifier-donnees-perso.sh` (dépôt git temporaire, faux noms).

## 4. Réglages du dépôt GitHub : `scripts/regler-depot-github.sh`

Les réglages sont posés par un script, et non à la main, pour qu'ils soient relus, reproductibles (si le
dépôt devait être recréé) et vérifiables à tout moment.

Usage : `scripts/regler-depot-github.sh [--appliquer | --verifier | --a-blanc] [--depot PROPRIÉTAIRE/NOM]`
(défaut : `--verifier`, dépôt `F1ghtc0re/WiMao`).

- `--a-blanc` : affiche chaque requête (méthode, chemin, corps JSON) sans appeler `gh` ; utilisé par le
  test.
- `--appliquer` : pose les réglages, puis enchaîne `--verifier`.
- `--verifier` : relit chaque réglage par `gh api` et affiche ✅ ou ❌ par ligne ; code 1 si un ❌.
- Le script refuse `--appliquer` si le dépôt n'est pas public (les rulesets et les attestations
  l'exigent sur un compte gratuit), avec un message qui l'explique.
- Les rulesets sont identifiés par leur **nom** : s'il existe déjà, il est mis à jour (`PUT`), sinon
  créé (`POST`). Relancer le script ne crée donc pas de doublon.

Réglages :

| Réglage | Requête | Valeur |
|---|---|---|
| Ruleset « main protégée » | `POST/PUT repos/{d}/rulesets` | cible `branch`, inclut `~DEFAULT_BRANCH` ; règles `deletion`, `non_fast_forward`, `required_signatures`, `pull_request` (0 approbation exigée, résolution des discussions exigée), `required_status_checks` (`unitaires`, `integration`, `chaine`, politique stricte non) ; **aucun contournement** (`bypass_actors: []`) |
| Ruleset « étiquettes de version » | idem | cible `tag`, inclut `refs/tags/v*` ; règles `creation`, `update`, `deletion` ; contournement : rôle administrateur en mode `always` |
| Signalement privé des failles | `PUT repos/{d}/private-vulnerability-reporting` | activé |
| Alertes Dependabot | `PUT repos/{d}/vulnerability-alerts` | activées |
| Correctifs Dependabot | `PUT repos/{d}/automated-security-fixes` | activés |
| Détection des secrets | `PATCH repos/{d}` `security_and_analysis` | `secret_scanning` et `secret_scanning_push_protection` à `enabled` |
| Jeton des workflows | `PUT repos/{d}/actions/permissions/workflow` | `default_workflow_permissions: read`, `can_approve_pull_request_reviews: false` |
| Actions autorisées | `PUT repos/{d}/actions/permissions` puis `…/selected-actions` | `allowed_actions: selected` ; `github_owned_allowed: true`, `verified_allowed: false`, `patterns_allowed: ["dtolnay/rust-toolchain@*"]` |
| CI des PR extérieures | `PUT repos/{d}/actions/permissions/fork-pr-contributor-approval` | `approval_policy: all_external_contributors` |
| Releases immuables | `PUT repos/{d}/immutable-releases` | activées (une Release publiée ne peut plus être modifiée ; le brouillon reste modifiable, donc `signer-version.sh` peut y joindre la signature) |

Pourquoi 0 approbation : le propriétaire est seul ; GitHub interdit d'approuver sa propre PR, une
approbation exigée bloquerait toute fusion. La protection vient de la PR obligatoire, de la CI verte, des
commits signés et des discussions résolues ; personne ne peut la contourner, pas même l'administrateur
(jamais d'envoi direct sur `main`). En cas d'urgence, l'administrateur désactive temporairement le
ruleset dans Settings > Rules ; `--verifier` affiche alors un ❌ tant qu'il n'est pas réappliqué.

Pourquoi un seul ruleset d'étiquettes, contournable par l'administrateur : il protège contre les jetons
de workflow (le job `brouillon` a `contents: write` et pourrait sinon déplacer une étiquette) et contre
toute autre personne ; le propriétaire garde la possibilité de supprimer une étiquette ratée, comme le
prévoit `docs/publier-une-version.md`. Un jeton administrateur volé pourrait de toute façon modifier les
rulesets eux-mêmes : `signer-version.sh` reste le dernier contrôle (étiquette signée par la clé publiée).

Seul `immutable-releases` (réglage récent) tolère, à l'écriture, un 404 ou un 422 : le script l'affiche
alors en ⚠️ « non disponible sur ce dépôt », sans échouer. Tout autre refus, sur tout autre réglage, est
une erreur (❌). À la lecture, un 404 sur `immutable-releases` signifie « désactivé » (❌), sauf si
l'écriture du même lancement a été classée « non disponible ».

Test : `scripts/tests/tester-regler-depot-github.sh` lance `--a-blanc`, valide chaque corps JSON avec
`jq` et contrôle les valeurs clés du tableau (aucun contournement sur `main`, trois contrôles
de CI, motif `refs/tags/v*`, `default_workflow_permissions` à `read`, etc.) ; il tourne dans le job
`chaine` de la CI.

## 5. Bascule vers le dépôt neuf

Opérations faites par le contrôleur, dans cet ordre, après la fusion de la PR du sous-projet 5 dans
`main` de l'ancien dépôt :

1. **Sauvegarde** : `git bundle create ~/Applis/wine-mao-archive.bundle --all`, puis
   `git bundle verify`. Un seul fichier, à côté du dépôt (pas de nouveau dossier dans `~`).
2. **Contrôles avant création** : `scripts/verifier-donnees-perso.sh` sur `main` (code 0) ; relecture
   des quatre captures `docs/captures/*.png` (aucun nom personnel visible) ; `git status` propre.
3. **Renommage** de l'ancien dépôt : `gh api -X PATCH repos/F1ghtc0re/WiMao -f name=WiMao-archive`
   (il reste privé). L'ancien dépôt local garde un remote `archive` vers lui.
4. **Création** du nouveau dépôt, **privé d'abord** : `gh repo create F1ghtc0re/WiMao --private
   --description …`.
5. **Commit unique** : branche orpheline `main` contenant exactement l'arbre de l'ancien `main`
   (`git checkout --orphan`), un commit signé (SSH, réglage local existant) :
   « WiMao 0.4.0 — première version publique », avec la ligne Co-Authored-By habituelle. Envoi sur
   `origin` (nouveau dépôt). Le remote `origin` local pointe vers le nouveau dépôt.
6. **Vérification** : l'arbre envoyé est identique à celui de l'ancien `main` (`git diff` vide entre les
   deux) ; la CI du nouveau dépôt est verte, y compris `paquets.yml` (Wine recompilé à froid, environ
   70 min, sans cache sur un dépôt neuf).
7. **Passage en public**, après confirmation explicite de l'utilisateur :
   `gh repo edit F1ghtc0re/WiMao --visibility public --accept-visibility-change-consequences`.
8. **Réglages** : `scripts/regler-depot-github.sh --appliquer`, tout en ✅ (⚠️ admis seulement pour un
   réglage indisponible).

Retour arrière possible à chaque étape avant 7 : supprimer le nouveau dépôt (avec accord) et renommer
l'archive en `WiMao`. Après 7, le dépôt public ne contient de toute façon aucune donnée personnelle.

La branche `main` locale devient le commit unique ; l'ancien historique reste dans le bundle et dans
`WiMao-archive`.

## 6. Outils sur la machine

Installés par le contrôleur avec le `sudo` sans mot de passe déjà accordé (`apt`, `install`), chaque
commande expliquée avant d'être lancée :

- **podman** (paquet Ubuntu `podman`) ; contrôle : `podman run --rm` d'une image minuscule en mode sans
  racine.
- **GitHub CLI officiel** : clé du dépôt `cli.github.com` téléchargée dans le dossier de travail
  temporaire, empreinte comparée à celle publiée dans la documentation officielle
  (<https://github.com/cli/cli/blob/trunk/docs/install_linux.md>), posée par `sudo install -m 644` dans
  `/etc/apt/keyrings/`, source ajoutée dans `/etc/apt/sources.list.d/github-cli.list`, puis
  `sudo apt install gh`. Contrôle : `gh attestation --help` répond et `gh auth status` reste connecté.

## 7. Clé de signature OpenPGP

L'utilisateur crée lui-même la clé (phrase de passe), guidé pas à pas d'après « Créer la clé de
signature » de `docs/publier-une-version.md` : clé principale `cert` 2 ans, sous-clé `sign` 1 an,
certificat de révocation, sauvegarde hors ligne vérifiée dans un trousseau jetable, retrait de la clé
principale secrète (`sec#`). Le contrôleur ne voit jamais la phrase de passe ; il ne lance que des
commandes en lecture (`gpg -K`, `gpg --show-keys`).

Puis, par une **PR sur le nouveau dépôt** (premier usage réel de la protection de `main`) :
`docs/cle-publique.asc` et la ligne `Empreinte de la clé de signature : …` dans `SECURITY.md`. La PR
vérifie au passage que la CI est exigée et que l'envoi direct sur `main` est refusé
(`git push` refusé, constaté).

## 8. Publication de la 0.4.0

1. Si la date du jour n'est plus le 2026-11-01 : PR qui met la date réelle dans le `<release>` de
   `fr.wimao.WiMao.metainfo.xml` et dans l'entrée `0.4.0-1` de `paquets/wimao/changelog`. Sinon, rien.
2. L'utilisateur signe l'étiquette (commande fournie, à lancer avec `!`) :
   `git -c gpg.format=openpgp tag -s -u <EMPREINTE> v0.4.0 -m "WiMao 0.4.0"` puis
   `git push origin v0.4.0`.
3. La CI fabrique, contrôle (dont `reproductible-wine`) et crée le brouillon ; le contrôleur suit le
   workflow jusqu'au bout.
4. L'utilisateur lance `scripts/signer-version.sh v0.4.0` (la phrase de passe est demandée par GnuPG).
5. L'utilisateur relit et publie le brouillon. Le contrôleur vérifie ensuite comme un utilisateur
   (README, « Installation ») : téléchargement, `sha256sum -c`, `gpg --verify`, `gh attestation verify`.

## 9. Documentation

- `docs/publier-une-version.md` : cases cochées au fur et à mesure ; mention de
  `verifier-donnees-perso.sh` (avant chaque version) et de `regler-depot-github.sh` (réglages et
  vérification).
- `NOTES.md` : journal de la bascule (date, nouveau dépôt, archive, bundle).
- `docs/ameliorations-reportees.md` : point « données personnelles » et « rulesets » retirés une fois
  faits ; les PR Dependabot à reprendre ajoutées.
- `paquets/README.md` ou `README.md` : rien à changer (les adresses restent les mêmes).

## 10. Tests et vérifications

- `scripts/tests/tester-verifier-donnees-perso.sh` : dépôt temporaire ; trouve un terme (code 1, sortie
  `fichier:ligne` sans le terme) ; ignore la casse ; ne trouve rien (code 0) ; terme trop court ignoré ;
  terme générique ignoré ; fichier `.git/donnees-perso` lu (commentaires ignorés) ; fichier non suivi
  par git ignoré ; adresse `noreply` ignorée ; option inconnue (code 2).
- `scripts/tests/tester-regler-depot-github.sh` : voir § 4.
- `shellcheck -x` sur les nouveaux scripts (déjà fait par la CI pour tous les `.sh`).
- `cargo test` complet (les tests de `rapport.rs` modifiés passent, mêmes cas).
- Vérifications réelles listées aux § 5 à 8 (bundle vérifié, arbre identique, CI verte, réglages en ✅,
  envoi direct refusé, Release vérifiée comme un utilisateur).
