# Échantillons de détection des installeurs

Fichiers fabriqués pour les tests de `detection.rs` : aucun extrait d'un installeur
propriétaire n'est dans le dépôt (le dépôt est public). `nsis.bin` et `msi.bin` sont de
**vrais** installeurs fabriqués par nous (`tests/fabriquer-fixtures.sh`, à partir de
`tests/fixtures/`), copiés en entier : `nsis.bin` commence par un vrai en-tête PE (`MZ`),
`msi.bin` par l'en-tête OLE d'un MSI (**pas** de `MZ` : un MSI n'est pas un exécutable).
Les autres (`inno.bin`, `wixburn.bin`, `installaware.bin`, `inconnu.bin`) sont
**synthétiques** : un en-tête `MZ` suivi d'octets à zéro, avec la chaîne-marqueur écrite à une
position fixe. `inconnu.bin` porte un texte neutre (`rien de special`) qui ne ressemble à aucun
marqueur : c'est un exécutable `MZ` que la détection doit classer `Inconnu`.

Pourquoi entiers et non tronqués à 64 Kio : le marqueur `Nullsoft` du vrai NSIS est à
l'octet 118 260 (fichier de 120 Kio) et celui du vrai Inno à l'octet 719 764 (fichier de
1,8 Mio) ; un fichier coupé à 64 Kio ne serait plus reconnu. L'échantillon Inno reste donc
synthétique (150 Kio) plutôt que de peser 1,8 Mio.

| Fichier | Contenu | Position du marqueur (échantillon) | Position observée dans un vrai installeur |
|---|---|---|---|
| `inno.bin` | `Inno Setup Setup Data (6.1.0) (u)` | octet 100 000, donc au-delà du premier bloc de lecture de 64 Kio (exerce la lecture en flux) ; 150 Kio | ~734 Kio (octet 751 376) dans un fichier de 698 Mio |
| `wixburn.bin` | `.wixburn` | octet 0x270 (624) ; 2 Kio | octet 624 (nom de section de l'en-tête PE) |
| `nsis.bin` | **vrai** `test-nsis.exe` (NSIS 3, `tests/fixtures/nsis/test.nsi`) | octet 118 260 ; 120 Kio | idem (c'est un vrai installeur) |
| `installaware.bin` | `InstallAware Setup` | octet 1200 ; 2 Kio | non observé |
| `msi.bin` | **vrai** `test.msi` (wixl, `tests/fixtures/msi/test.wxs`) | octet 0 (en-tête OLE `D0 CF 11 E0 A1 B1 1A E1`) ; 10 Kio | idem (c'est un vrai MSI) |
| `inconnu.bin` | `MZ` + `rien de special` (texte neutre, octet 500) | aucun marqueur ; 2 Kio | sans objet |

Fabrication des échantillons synthétiques (Python) : `b = bytearray(b'MZ' + bytes(taille))`,
puis `b[position:position+len(marqueur)] = marqueur`, écrit tel quel dans le fichier.
`nsis.bin` et `msi.bin` se refont avec `tests/fabriquer-fixtures.sh` (copies de `test-nsis.exe`
et `test.msi`).

Pour essayer de vrais installeurs : `WIMAO_TEST_INSTALLEURS="inno=/chemin/Setup.exe;wixburn=/chemin/Autre.exe"
cargo test -p wimao-recettes installeurs_reels -- --ignored --nocapture`.
