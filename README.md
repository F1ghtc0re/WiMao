# WiMao

WiMao fait tourner vos plugins audio Windows (VST2, VST3, CLAP) sous Linux, sans configuration
à la main. Il prépare Wine, installe vos plugins et les rend visibles dans votre logiciel de
musique.

**WiMao est en bêta** : il fonctionne, mais vous pouvez rencontrer des défauts. Merci de les signaler !

| Assistant de premier lancement | Liste des plugins | Historique |
|---|---|---|
| ![Assistant de premier lancement](docs/captures/clair-assistant.png) | ![Liste des plugins](docs/captures/clair-liste.png) | ![Historique des actions](docs/captures/clair-historique.png) |

## Crédits

WiMao ne ferait rien sans le travail des autres. Un grand merci à :

- **[yabridge](https://github.com/robbert-vdh/yabridge)**, de Robbert van der Helm (GPL-3.0) : c'est lui
  qui fait charger les plugins Windows par les logiciels Linux ;
- **[Wine](https://www.winehq.org)** (LGPL-2.1) : il fait tourner les programmes Windows ;
- **[DXVK](https://github.com/doitsujin/dxvk)** (licence zlib) : l'affichage des fenêtres de plugins ;
- **[Carla](https://github.com/falkTX/Carla)**, de falkTX (GPL-2.0) : il sert à tester l'ouverture des plugins.

WiMao lui-même est libre (GPL-3.0 ou ultérieure, voir `LICENSE`).

## Installation

Ubuntu 24.04 et plus récent, Debian 13 et plus récent (64 bits Intel/AMD). Copiez ces commandes
dans un terminal, l'une après l'autre :

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://f1ghtc0re.github.io/WiMao/wimao.asc | sudo tee /etc/apt/keyrings/wimao.asc > /dev/null
echo "deb [signed-by=/etc/apt/keyrings/wimao.asc] https://f1ghtc0re.github.io/WiMao stable main" \
  | sudo tee /etc/apt/sources.list.d/wimao.list
sudo apt update && sudo apt install wimao
```

Fedora 44 et 45 (64 bits Intel/AMD) :

```sh
sudo curl -fsSL -o /etc/yum.repos.d/wimao.repo https://f1ghtc0re.github.io/WiMao/wimao.repo
sudo dnf install wimao
```

WiMao se met ensuite à jour avec le reste du système. Les vérifications de sécurité (empreinte
de la clé, signatures), `curl` absent, Fedora et SELinux, et le retrait des dépôts sont expliqués
dans l'[installation détaillée](docs/installation-detaillee.md).

## Premiers pas

1. Lancez **WiMao** depuis le menu des applications : l'assistant s'occupe du reste.
2. Ajoutez un plugin avec « Ajouter un installeur », ou ouvrez le gestionnaire de votre éditeur.
3. Dans votre logiciel de musique, ajoutez les dossiers `~/.vst3`, `~/.clap` et `~/.vst/yabridge`.
4. Relancez la recherche des plugins : les vôtres apparaissent. Le mode d'emploi est dans WiMao (touche F1).

## En savoir plus

- [Installation détaillée](docs/installation-detaillee.md) : vérifications, Fedora, compilation, ligne de commande ;
- [Mode d'emploi](crates/wimao/aide/fr/) : aussi dans WiMao, touche F1 ;
- [SECURITY.md](SECURITY.md) : signaler une faille, vérifier une version ;
- [CONTRIBUTING.md](CONTRIBUTING.md) : participer au projet ;
- [LICENSE](LICENSE) : GPL-3.0 ou ultérieure.
