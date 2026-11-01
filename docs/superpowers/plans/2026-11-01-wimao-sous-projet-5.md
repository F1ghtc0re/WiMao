# WiMao sous-projet 5 — Première version publique : plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** publier WiMao 0.4.0 dans un dépôt GitHub public neuf, sans donnée personnelle, protégé, avec une Release signée et vérifiable.

**Architecture:** deux scripts shell testés (`verifier-donnees-perso.sh`, `regler-depot-github.sh`) et un nettoyage, livrés par une PR sur l'ancien dépôt (branche `sous-projet-5`) ; puis des opérations faites par le contrôleur, avec l'utilisateur pour ce qui demande une phrase de passe ou un accord : sauvegarde, dépôt neuf à commit unique, passage en public, réglages, clé OpenPGP, étiquette et Release.

**Tech Stack:** bash (`set -euo pipefail`, shellcheck -x), git, `gh api`, `jq`, GnuPG, GitHub Actions ; Rust (tests de `rapport.rs` seulement).

**Spec:** `docs/superpowers/specs/2026-11-01-wimao-sous-projet-5-design.md`

## Global Constraints

- Tout en français : messages des scripts, commentaires (détaillés, utiles à un développeur qui découvre le projet), documentation, messages de commit.
- En-tête des scripts sur le modèle de `scripts/tests/tester-signer-version.sh` : rôle, cas couverts, `Usage`, `Variables`, `Écrit`, `Code`.
- `shellcheck -x` sans aucun avertissement ; pas de `grep -q` dans un tuyau sous `pipefail` (passer par une variable ou `< <(…)`) ; pas de `A && B || C` (écrire un `if`).
- Les tests n'écrivent que dans un `mktemp -d` supprimé à la fin.
- Commits signés (réglage local existant), terminés par `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Aucun terme personnel (nom d'utilisateur, nom de machine, e-mail) écrit dans le dépôt, y compris dans les tests (faux noms : `alice`, `machine-test`, `alice@exemple.invalid`).
- Nom de remplacement dans `rapport.rs` : `alice` ; dans `NOTES.md` et le plan du SP1 : `<utilisateur>`.
- Contrôles de CI exigés sur `main` : `unitaires`, `integration`, `chaine`.
- Termes ignorés par `verifier-donnees-perso.sh` : `root`, `runner`, `ubuntu`, `localhost`, `user`, `utilisateur` ; longueur minimale 3.
- Jamais de phrase de passe dans le chat ; `sudo` seulement pour `apt`, `install` (déjà permis), chaque commande expliquée avant.

## Review Focus

- Un terme personnel qui contient un caractère spécial d'expression régulière (`.` dans un nom de machine ou une adresse) : il doit être cherché littéralement (`git grep -F`), sans faux positif ni erreur. Test au Task 1.
- Le journal de CI d'un dépôt public : la sortie de `verifier-donnees-perso.sh` ne doit jamais afficher le terme trouvé. Test au Task 1.
- Relancer `regler-depot-github.sh --appliquer` deux fois : aucun ruleset en double (mise à jour par nom). Test à blanc au Task 2 (le faux `gh` simule un ruleset existant).
- Un réglage GitHub indisponible (404/422 sur `immutable-releases`) : ⚠️ et non arrêt ; toute autre erreur : ❌ et code 1. Test au Task 2.
- Un dépôt encore privé : `--appliquer` refuse avec un message clair, sans rien modifier. Test au Task 2.

---

### Task 1 : nettoyage des données personnelles et `verifier-donnees-perso.sh`

**Files:**
- Modify: `crates/wimao-core/src/rapport.rs` (doc du module l. 13-14, doc l. 49-55 et 96, tests l. 251-273)
- Modify: `NOTES.md` (l. 139 ; mentions « dépôt privé » l. 41 et section « Dépôt Git »)
- Modify: `docs/superpowers/plans/2026-10-31-wimao-sous-projet-1.md` (l. 326, 404, 472 et toute autre occurrence)
- Create: `scripts/verifier-donnees-perso.sh`
- Create: `scripts/tests/tester-verifier-donnees-perso.sh`
- Modify: `.github/workflows/ci.yml` (job `chaine`, après « Test de la signature des versions »)
- Modify: `docs/publier-une-version.md` (case « Données personnelles » : décrire le script, à lancer avant chaque version)

**Interfaces:**
- Produces: `scripts/verifier-donnees-perso.sh [--aide] [--dossier <dépôt>] [--terme <texte>]... [--seulement]` ; sortie `fichier:ligne` par occurrence, sans le terme ; code 0 (rien), 1 (occurrence), 2 (usage).

- [ ] **Step 1 : remplacer le nom** dans `rapport.rs` par `alice` (mêmes cas : `/home/alice/x alice@a.fr ALICE alicex` → `~/x <e-mail> <user> alicex` ; `/home/alicex/y` inchangé ; `user=Alice.` → `user=<user>.` ; `alice-pc`), dans `NOTES.md` et le plan du SP1 par `<utilisateur>`. Le contrôleur fournit le nom réel à chercher hors du brief : `git grep -n -i "$(id -un)"` doit ne plus rien renvoyer à la fin de cette étape.
- [ ] **Step 2 : `cargo test -p wimao-core rapport`** : PASS (mêmes tests, nouveaux noms).
- [ ] **Step 3 : écrire `scripts/tests/tester-verifier-donnees-perso.sh`** (dépôt git temporaire avec fichiers suivis ; toujours `--seulement --terme …` pour ne pas dépendre de la machine, sauf le cas `.git/donnees-perso`). Cas, chacun avec code de retour et sortie attendus :
  - `--terme alice` sur un fichier suivi `a.txt` contenant `Bonjour ALICE` en ligne 2 → code 1, sortie contient `a.txt:2`, **ne contient pas** `alice` (insensible à la casse) ;
  - terme absent → code 0 ;
  - `--terme ab` → ignoré avec message, code 0 ;
  - `--terme runner` alors que `runner` est présent → ignoré, code 0 ;
  - `--terme machine.test` : un fichier contient `machineXtest` → code 0 (recherche littérale) ; un autre `machine.test` → code 1 ;
  - fichier **non suivi** contenant le terme → code 0 ;
  - `.git/donnees-perso` contenant `# commentaire`, une ligne vide et `secretnom`, sans `--seulement` ni `--terme` mais avec `USER=runner` et un faux `hostname` en tête du PATH qui répond `runner` → `secretnom` trouvé, code 1 ;
  - `git config user.email alice@users.noreply.github.com` dans le dépôt de test, même environnement → adresse ignorée, code 0 ;
  - option inconnue → code 2.
- [ ] **Step 4 : lancer le test** : échec (script absent).
- [ ] **Step 5 : écrire `scripts/verifier-donnees-perso.sh`** selon la spec § 3.3 : termes = `$USER` (sinon `id -un`), `hostname`, `git -C <dépôt> config user.email` sauf `*noreply.github.com`, lignes de `<dépôt>/.git/donnees-perso` ; recherche `git -C <dépôt> grep -n -i -F -I -e <terme>` dont on ne garde que `fichier:ligne` (`cut -d: -f1,2`) ; message final « Aucune donnée personnelle trouvée. » ou « N occurrence(s) : à retirer avant de publier. ».
- [ ] **Step 6 : lancer le test** : PASS ; `shellcheck -x` des deux scripts : rien ; `scripts/verifier-donnees-perso.sh` sur le dépôt réel : code 0.
- [ ] **Step 7 : CI** : étape `Données personnelles (test du contrôle)` lançant le test, puis `Données personnelles` lançant le script, avec un commentaire qui explique ce qu'il vérifie sur un runner ; doc `publier-une-version.md`.
- [ ] **Step 8 : `cargo test`** complet : PASS ; **commit**.

### Task 2 : `regler-depot-github.sh`

**Files:**
- Create: `scripts/regler-depot-github.sh`
- Create: `scripts/tests/tester-regler-depot-github.sh`
- Modify: `.github/workflows/ci.yml` (job `chaine`)
- Modify: `docs/publier-une-version.md` (case « Règles du dépôt » et « Signalement privé » : renvoyer au script ; valeurs exactes dans la spec § 4)

**Interfaces:**
- Produces: `scripts/regler-depot-github.sh [--appliquer | --verifier | --a-blanc] [--depot PROPRIÉTAIRE/NOM] [--aide]` ; défaut `--verifier F1ghtc0re/WiMao` ; code 0 (tout ✅ ou ⚠️), 1 (au moins un ❌ ou dépôt privé avec `--appliquer`), 2 (usage).
- En `--a-blanc`, chaque requête est imprimée sur une ligne `REQUÊTE <MÉTHODE> <chemin>` suivie du corps JSON sur une ligne (ou `{}`), pour que le test les lise.

- [ ] **Step 1 : écrire le test** `scripts/tests/tester-regler-depot-github.sh`. Faux `gh` en tête du PATH qui consigne ses appels. Cas :
  - `--a-blanc` : aucun appel au faux `gh` ; chaque corps passe `jq -e .` ; ruleset `main protégée` : `.target=="branch"`, `.conditions.ref_name.include==["~DEFAULT_BRANCH"]`, `.bypass_actors[0]` = `{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"pull_request"}`, contrôles exigés = `["chaine","integration","unitaires"]` (triés), règles `deletion`, `non_fast_forward`, `required_signatures`, `pull_request` (`required_approving_review_count==0`, `required_review_thread_resolution==true`) ; ruleset `étiquettes de version` : `.target=="tag"`, include `["refs/tags/v*"]`, règles `creation`, `update`, `deletion`, bypass `always` ; `default_workflow_permissions=="read"` et `can_approve_pull_request_reviews==false` ; `allowed_actions=="selected"` ; `patterns_allowed==["dtolnay/rust-toolchain@*"]` ; `approval_policy=="all_external_contributors"` ; requêtes `PUT …/private-vulnerability-reporting`, `…/vulnerability-alerts`, `…/automated-security-fixes`, `…/immutable-releases` présentes ; `security_and_analysis.secret_scanning_push_protection.status=="enabled"` ;
  - `--appliquer` avec un faux `gh` qui répond `"visibility":"private"` → code 1, message contenant `public`, aucun appel en écriture (`-X POST|PUT|PATCH`) consigné ;
  - `--appliquer`, dépôt public, faux `gh` qui liste déjà un ruleset nommé `main protégée` (id 42) → un `PUT repos/<d>/rulesets/42` et **aucun** `POST` pour ce nom ;
  - faux `gh` qui répond 404 à `immutable-releases` → ligne ⚠️, code 0 ; qui répond une erreur à `vulnerability-alerts` → ❌, code 1 ;
  - option inconnue → code 2.
- [ ] **Step 2 : lancer le test** : échec.
- [ ] **Step 3 : écrire le script.** Une fonction par réglage (`regler_ruleset_main`, `regler_ruleset_etiquettes`, `regler_signalement_prive`, …), chacune avec son corps JSON construit par `jq -n` (jamais par concaténation de chaînes) et sa vérification ; une fonction `requete <méthode> <chemin> [corps]` qui imprime à blanc ou appelle `gh api -X … --input -`. Visibilité lue par `gh api repos/<d> --jq .visibility`. Ruleset existant retrouvé par `gh api repos/<d>/rulesets --jq` sur le nom.
- [ ] **Step 4 : test** : PASS ; `shellcheck -x` : rien ; `--verifier` sur l'ancien dépôt privé : s'exécute sans erreur de script (des ❌ sont attendus, le dépôt étant privé et sans rulesets).
- [ ] **Step 5 : CI** (étape `Réglages du dépôt (à blanc)`), doc ; **commit**.

### Task 3 : relecture finale, PR, fusion (contrôleur)

- [ ] Relecture finale de la branche (modèle le plus capable) ; corrections.
- [ ] `cargo test` complet et `scripts/verifier-donnees-perso.sh` : verts.
- [ ] Commit de la spec et du plan sur la branche ; envoi ; PR `sous-projet-5` → `main` (corps terminé par la ligne « 🤖 Generated with … ») ; CI verte (`ci.yml` et `paquets.yml`).
- [ ] **Fusion avec l'accord de l'utilisateur**, demandé en même temps que les deux accords du Task 5 (un seul aller-retour) ; branche supprimée ; `main` locale à jour.

### Task 4 : outils sur la machine (contrôleur, spec § 6)

- [ ] Expliquer puis lancer `sudo apt install -y podman` ; contrôle `podman run --rm docker.io/library/busybox:latest true` (sans racine).
- [ ] Clé GitHub CLI : téléchargement dans le scratchpad, `gpg --show-keys` comparé à l'empreinte publiée dans `docs/install_linux.md` du dépôt `cli/cli` ; `sudo install -m 644` dans `/etc/apt/keyrings/githubcli-archive-keyring.gpg` ; fichier de source écrit dans le scratchpad puis `sudo install -m 644` vers `/etc/apt/sources.list.d/github-cli.list` ; `sudo apt update && sudo apt install -y gh`.
- [ ] Contrôles : `gh --version` (> 2.46), `gh attestation --help`, `gh auth status`.

### Task 5 : bascule vers le dépôt neuf (contrôleur, spec § 5)

- [ ] Accords de l'utilisateur (une seule question) : fusion de la PR, renommage de l'ancien dépôt, création du nouveau.
- [ ] `git bundle create ~/Applis/wine-mao-archive.bundle --all` ; `git bundle verify` : OK.
- [ ] `scripts/verifier-donnees-perso.sh` : code 0 ; lecture des 4 captures `docs/captures/*.png` : aucun nom personnel.
- [ ] Renommage `WiMao` → `WiMao-archive` (`gh api -X PATCH`) ; `git remote rename origin archive` puis `git remote set-url archive …/WiMao-archive.git`.
- [ ] `gh repo create F1ghtc0re/WiMao --private --description "Wine pour la MAO sous Linux : plugins Windows dans votre logiciel de musique"` ; `git remote add origin …`.
- [ ] `git checkout --orphan nouveau-main` ; commit signé « WiMao 0.4.0 — première version publique » (corps : renvoi vers WiMao-archive privé pour l'historique, Co-Authored-By) ; `git diff archive/main nouveau-main` vide ; `git branch -M nouveau-main main` ; `git push -u origin main`.
- [ ] CI du nouveau dépôt verte (`ci.yml`, `paquets.yml` ; Wine à froid environ 70 min).
- [ ] **Confirmation explicite de l'utilisateur**, puis `gh repo edit F1ghtc0re/WiMao --visibility public --accept-visibility-change-consequences`.
- [ ] `scripts/regler-depot-github.sh --appliquer` : tout ✅ (⚠️ admis seulement pour un réglage indisponible) ; Dependabot de nouveau actif.
- [ ] `NOTES.md` (journal de bascule) et `docs/ameliorations-reportees.md` (points faits retirés, PR Dependabot à reprendre) : ils partent dans la PR du Task 6.

### Task 6 : clé OpenPGP et PR de la clé publique (utilisateur guidé, spec § 7)

- [ ] Guider l'utilisateur étape par étape (commandes à lancer avec `!`, sans jamais voir la phrase de passe) : clé principale, sous-clé, révocation, sauvegarde hors ligne, import d'essai dans un trousseau jetable, retrait de la clé principale secrète ; contrôle `gpg -K` → `sec#` et `ssb … [S]`.
- [ ] Branche `cle-publique` : `docs/cle-publique.asc`, ligne d'empreinte dans `SECURITY.md`, notes du Task 5, cases cochées dans `docs/publier-une-version.md`.
- [ ] Constater qu'un `git push origin HEAD:main` direct est **refusé** ; PR, CI exigée visible et verte, fusion (accord de l'utilisateur).

### Task 7 : publication de la 0.4.0 (utilisateur et contrôleur, spec § 8)

- [ ] Si la date n'est plus le 2026-11-01 : PR de date (metainfo + changelog `wimao`).
- [ ] L'utilisateur signe et envoie l'étiquette `v0.4.0` (commande fournie) ; le contrôleur suit `paquets.yml` jusqu'au brouillon.
- [ ] L'utilisateur lance `scripts/signer-version.sh v0.4.0` : succès.
- [ ] L'utilisateur relit et publie le brouillon ; le contrôleur vérifie la Release comme un utilisateur (`sha256sum -c`, `gpg --verify SHA256SUMS.asc`, `gh attestation verify --repo F1ghtc0re/WiMao`).
- [ ] Mise à jour de la mémoire (`etat-wimao.md`).
