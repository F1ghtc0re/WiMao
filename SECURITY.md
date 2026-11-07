# Politique de sécurité

*English summary: report vulnerabilities privately through GitHub's "Report a vulnerability"
button (Security tab); we aim to answer within 7 days; only the latest release is supported.
Releases are signed (`SHA256SUMS.asc`) and carry build attestations; see "Vérifier une version" below.
The signed APT repository (https://f1ghtc0re.github.io/WiMao) is signed with the same key; see
"Le dépôt APT" below.*

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

# 2. Les empreintes des fichiers téléchargés.
sha256sum -c SHA256SUMS

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
