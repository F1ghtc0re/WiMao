# Politique de sécurité

*English summary: report vulnerabilities privately through GitHub's "Report a vulnerability"
button (Security tab); we aim to answer within 7 days; only the latest release is supported.
Releases are signed (`SHA256SUMS.asc`) and carry build attestations; see "Vérifier une version" below.*

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

L'empreinte de la clé **principale** de signature sera publiée ici, sur une ligne commençant exactement
par « Empreinte de la clé de signature : » (lue par `scripts/signer-version.sh`, qui refuse de signer tant
qu'elle manque ou si l'étiquette n'est pas signée par cette clé). Comparez-la avec celle de
`docs/cle-publique.asc` (`gpg --show-keys --fingerprint docs/cle-publique.asc`).

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
