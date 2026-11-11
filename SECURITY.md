# Politique de sécurité

*English summary: report vulnerabilities privately through GitHub's "Report a vulnerability"
button (Security tab); we aim to answer within 7 days; only the latest release is supported.
Releases are signed (`SHA256SUMS.asc`) and carry build attestations; see "Vérifier une version" below.
The signed APT repository (https://f1ghtc0re.github.io/WiMao) is signed with the same key; see
"Le dépôt APT" below. The same key signs the dnf repository for Fedora; see "Le dépôt dnf". The Arch Linux (AUR) recipe is covered in "Le paquet AUR" (account risk and what to do
if the daily check fails).*

## Signaler une faille

Merci de **ne pas ouvrir de ticket public** pour une faille de sécurité.

Utilisez le signalement privé de GitHub : onglet **Security** du dépôt, bouton
**« Report a vulnerability »**. (Cette fonction doit être activée par le propriétaire dans
les réglages du dépôt, *Settings > Code security > Private vulnerability reporting* ; si le
bouton n'apparaît pas, ouvrez un ticket public sans détails en demandant un contact privé.)

Délai de réponse visé : **7 jours** pour un premier retour.

## Versions suivies

Seule la **dernière version publiée** reçoit des correctifs de sécurité.

## Clé de signature perdue ou volée

Les publications sont signées avec une clé GPG dédiée. Si cette clé est perdue ou
compromise :

1. un certificat de révocation est publié dans ce dépôt et dans la Release suivante ;
2. une nouvelle clé est générée, son empreinte est publiée dans ce dépôt ;
3. une nouvelle version est publiée, signée avec la nouvelle clé.

Les versions signées avec l'ancienne clé ne doivent plus être considérées comme fiables
tant qu'elles n'ont pas été re-publiées.

## Empreinte de la clé de signature

Empreinte de la clé de signature : 3980 C3D2 0A17 566A 075A  65CB CB97 DE3F BF41 DD46

C'est l'empreinte de la clé **principale** (identité `F1ghtc0re <162480684+F1ghtc0re@users.noreply.github.com>`,
créée le 2026-11-01, valable jusqu'au 2028-10-31) ; les versions sont signées par sa sous-clé de signature
(`5F1D4F6002B80C81`). La ligne ci-dessus est lue par `scripts/signer-version.sh`, qui refuse de signer si
l'étiquette n'est pas signée par cette clé. Comparez-la avec celle de `docs/cle-publique.asc`
(`gpg --show-keys --fingerprint docs/cle-publique.asc`).

## Vérifier une version

Depuis la page de la Release, téléchargez les paquets, `SHA256SUMS` et `SHA256SUMS.asc`, puis :

```bash
# 1. La signature de la liste d'empreintes (clé publique publiée dans le dépôt).
gpg --verify SHA256SUMS.asc SHA256SUMS

# 2. Les empreintes des fichiers téléchargés. La liste couvre six paquets (3 .deb, 3 .rpm) :
#    --ignore-missing passe sur ceux que vous n'avez pas téléchargés, donc vérifiez que
#    les trois paquets voulus sont bien marqués « OK ».
sha256sum -c --ignore-missing SHA256SUMS

# 3. L'attestation de fabrication (provenance GitHub), pour chaque paquet.
gh attestation verify <paquet>.deb --repo F1ghtc0re/WiMao
```

## Le dépôt APT

Le dépôt `apt` (`https://f1ghtc0re.github.io/WiMao`, servi par GitHub Pages en HTTPS) reprend la chaîne de
confiance des Releases ; il n'y ajoute aucune clé.

**Ce que garantit le dépôt.**

- L'index (`InRelease` et `Release.gpg`) est signé, **sur la machine du propriétaire**, par la sous-clé de
  la clé dont l'empreinte est publiée plus haut. La CI n'a aucune clé : elle vérifie cette signature, elle ne
  la fabrique pas.
- L'index contient la taille et l'empreinte SHA256 de chaque `.deb`. `apt` refuse tout paquet, ou tout
  fichier d'index, dont un seul octet diffère ; la CI le contrôle avant chaque mise en ligne en essayant
  d'installer un `.deb` altéré, qui doit être refusé.
- Le site n'est mis en ligne que depuis `main`, après vérification de la signature, de chaque empreinte et
  d'une vraie installation sur Ubuntu 24.04 et Debian 13. Seul le job de déploiement a le droit d'écrire sur
  GitHub Pages.
- Les `.deb` du dépôt sont ceux des Releases immuables, déjà signés (`SHA256SUMS.asc`) et attestés. La clé
  publique se pose dans `/etc/apt/keyrings/wimao.asc` et n'a de valeur que pour ce dépôt (`signed-by`) :
  comparez son empreinte avec celle de ce fichier (procédure dans le `README.md`).

**Pas de `Valid-Until`.** L'index n'a pas de date de péremption, parce que la signature est faite à la main :
une date obligerait à re-signer régulièrement, et un oubli casserait `apt update` chez tous les utilisateurs.
Le risque accepté : un intermédiaire pourrait resservir un **ancien index signé**, donc proposer seulement
d'anciennes versions authentiques (et non les dernières corrections de sécurité), sans pouvoir rien
modifier. L'HTTPS de GitHub Pages rend ce rejeu très difficile.

**Expiration de la sous-clé.** Une clé expirée est refusée par nos vérifications. La sous-clé de signature
expire le 2027-11-01 : le propriétaire doit la prolonger avant (`docs/publier-une-version.md`, étape 5),
sinon plus aucune publication n'est possible. Les utilisateurs gardent une copie locale de la clé publique
(`/etc/apt/keyrings/wimao.asc`) : après une prolongation, ils doivent la retélécharger (même empreinte),
sinon `apt update` refusera l'index (`EXPKEYSIG`) à partir de l'ancienne date d'expiration. Le README
l'explique ; la prolongation doit donc se faire assez tôt pour leur laisser le temps.

Si la clé est perdue ou volée, la procédure de « Clé de signature perdue ou volée » s'applique aussi au
dépôt : l'index doit être re-signé avec la nouvelle clé et la nouvelle clé publiée sur le site.

## Le dépôt dnf

Le dépôt `dnf` pour Fedora (`https://f1ghtc0re.github.io/WiMao/rpm`, fichier de réglages
`https://f1ghtc0re.github.io/WiMao/wimao.repo`) suit la même logique que le dépôt APT, avec la même clé.

- **C'est l'index qui est signé** : `repomd.xml.asc` couvre `repomd.xml`, qui contient les empreintes des
  métadonnées, qui contiennent la taille et l'empreinte SHA-256 de chaque RPM. `dnf` applique `repo_gpgcheck=1` :
  un index modifié d'un seul octet est refusé, et un RPM qui ne correspond pas à l'index aussi. La signature est
  faite sur la machine du propriétaire ; la CI vérifie, elle ne signe pas.
- **Les RPM ne sont pas signés un par un** (`gpgcheck=0`) : signer un RPM le modifierait après la CI et casserait
  la comparaison octet pour octet, l'attestation de provenance et `SHA256SUMS`. `dnf` affiche donc « skipped
  OpenPGP checks » à l'installation : c'est normal. Un RPM téléchargé à la main se vérifie avec `SHA256SUMS.asc`,
  comme un `.deb`, et son attestation avec `gh attestation verify <paquet>.rpm --repo F1ghtc0re/WiMao`.
- Les RPM ne sont pas hébergés sur le site : l'index pointe vers les Releases GitHub (immuables, signées et
  attestées). La CI télécharge chacun, contrôle son adresse, sa taille et son SHA-256, puis le jette.
- **Pas d'expiration de l'index**, comme pour apt : quelqu'un qui contrôlerait le site, ou une PR fusionnée qui
  restaurerait un vieil index signé, pourrait maintenir des utilisateurs sur une ancienne version authentique ;
  jamais leur faire installer un paquet forgé. L'index ne décrit que la dernière version.
- **Expiration de la sous-clé** (2027-11-01) : comme pour apt, voir ci-dessus ; la prolongation concerne les deux
  dépôts.
- **Limite connue** : l'installation et l'ouverture d'un plugin n'ont été prouvées que dans des conteneurs
  Fedora, qui n'appliquent pas SELinux ; l'ouverture des plugins sur un bureau Fedora réel n'est pas prouvée.

## Le paquet AUR

Le paquet `wimao-bin` de l'AUR (Arch Linux) n'est qu'une **recette** (`PKGBUILD` et `.SRCINFO`, quelques Ko) :
elle télécharge les `.deb` de la Release GitHub, vérifie leurs empreintes SHA-256 et la signature de
`SHA256SUMS.asc` (clé de `validpgpkeys` = l'empreinte de ce fichier), puis installe. Les binaires ne passent pas
par l'AUR ; la chaîne de confiance est donc celle des Releases.

- **Le risque propre à l'AUR.** L'AUR sert ce que le **compte** du mainteneur y envoie, sans relecture, et
  `makepkg` exécute le `PKGBUILD`. Quiconque obtiendrait le mot de passe du compte AUR ou sa clé SSH pourrait y
  mettre une recette piégée (par exemple sans contrôle de signature) que les utilisateurs exécuteraient.
  Contrairement aux dépôts `apt` et `dnf`, aucune signature de WiMao ne couvre la recette elle-même.
- **Protections.**
  1. La recette fait foi dans ce dépôt : elle est relue par PR, essayée pour de vrai dans un Arch jetable
     (`scripts/publier-aur.sh`) et éprouvée par la CI (`aur.yml`), qui prouve aussi que la recette *refuse* un
     `.deb` modifié, une empreinte fausse et une signature altérée.
  2. L'envoi se fait **uniquement depuis la machine du propriétaire** (`scripts/envoyer-aur.sh`), avec une clé SSH
     **dédiée à l'AUR** (`~/.ssh/aur`, ed25519, protégée par une phrase de passe), qui ne va jamais sur GitHub ni
     en CI. Le script n'envoie que `PKGBUILD` et `.SRCINFO` tels qu'ils sont sur `main`, puis relit l'AUR.
  3. L'empreinte de la clé d'hôte de `aur.archlinux.org` est figée dans le dépôt
     (`paquets/aur/aur.archlinux.org.known_hosts`) : pas de « confiance au premier contact ».
  4. Une **surveillance quotidienne** (`.github/workflows/aur-surveillance.yml`) compare `PKGBUILD` et `.SRCINFO`
     de l'AUR à ceux de `main` ; une différence fait échouer le job et GitHub prévient le propriétaire par e-mail.
- **Ce que vous pouvez faire, côté utilisateur.** Lire le `PKGBUILD` avant de construire (`yay` le montre, ainsi que
  les changements aux mises à jour). Il ne doit contenir que des téléchargements depuis
  `https://github.com/F1ghtc0re/WiMao/releases/download/`, des empreintes, `validpgpkeys` égal à l'empreinte
  ci-dessus, et l'extraction des `.deb`. Toute autre commande est suspecte : ne construisez pas et signalez-le.
- **Limite.** La surveillance détecte une modification, elle ne l'empêche pas : une personne qui installerait entre
  la modification et la correction serait exposée (au plus un jour environ). Elle ne compare que `PKGBUILD` et
  `.SRCINFO`. Voir aussi `docs/ameliorations-reportees.md`.
- **Expiration de la sous-clé** (2027-11-01) : comme pour apt et dnf, les utilisateurs Arch doivent réimporter la
  clé après la prolongation.

### Compromission de compte : que faire si la surveillance de l'AUR échoue

Le job « Surveillance AUR » échoue dans deux cas : un envoi **oublié** (une nouvelle recette vient d'être fusionnée
sur `main` et n'est pas encore partie ; c'est voulu, c'est un rappel), ou un envoi **non voulu**. Dans le premier
cas, lancez `scripts/envoyer-aur.sh vX.Y.Z`. Dans le doute, ou si aucune recette n'a été fusionnée récemment :

1. **ne fusionnez rien** de nouveau et n'envoyez rien tant que ce n'est pas clair ;
2. regardez l'historique du paquet sur `https://aur.archlinux.org/packages/wimao-bin` (onglet « Git Commit
   Log ») et comparez le `PKGBUILD` publié à celui de `main` ;
3. **changez le mot de passe du compte AUR**, activez la double authentification si elle est proposée, et
   **remplacez la clé SSH** : nouvelle clé `~/.ssh/aur` (`ssh-keygen -t ed25519 -f ~/.ssh/aur -C "wimao-aur"`),
   ancienne clé retirée du profil AUR, nouvelle clé collée à la place ;
4. **renvoyez la recette de `main`** avec `scripts/envoyer-aur.sh vX.Y.Z` : elle remplace le contenu piégé ;
   la surveillance doit redevenir verte. Si le compte est perdu, demandez aux administrateurs de l'AUR de
   reprendre ou supprimer le paquet (procédure « request » de l'AUR) ;
5. si la recette piégée est restée en ligne, **prévenez les utilisateurs** (avis de sécurité GitHub, notes de
   version) : toute personne ayant construit le paquet pendant ce temps doit considérer sa machine comme
   suspecte. Signalez la faille comme indiqué en haut de ce fichier.
