# Dépendances du paquet `wimao-wine`

Les dépendances viennent de trois sources, assemblées dans `DEBIAN/control` par
`paquets/fabriquer-wine.sh` (modèle : `paquets/wine/control.in`).

1. **Calculées à la fabrication** par `dpkg-shlibdeps` (champ `@DEPENDANCES@`). Rien n'est écrit
   en dur : les versions minimales (`libc6 (>= …)`, etc.) dépendent du système de compilation. La
   valeur réelle est celle produite par la CI dans `ubuntu:24.04` (la plus ancienne cible), ce qui
   garantit que le paquet s'installe aussi sur les systèmes plus récents.
2. **`Depends` fixes** : bibliothèques que Wine charge à la demande (`dlopen`), sans quoi il démarre
   mais perd une fonction essentielle.
3. **`Recommends` fixes** : bibliothèques à chargement facultatif.

## 1. Commande exacte du calcul

```sh
cd <dossier avec debian/control minimal>      # dpkg-shlibdeps exige ce fichier
dpkg-shlibdeps -O --ignore-missing-info \
    -l<paquet>/opt/wimao/wine/lib/wine/x86_64-unix \
    -e<ELF> [-e<ELF> ...] 2>/dev/null | sed -n 's/^shlibs:Depends=//p'
```

- Les `-e` portent sur tous les vrais ELF de `bin/` (`wine`, `wineserver` : les outils de développement `winegcc`, `widl`, `winebuild`… sont retirés du paquet avant le calcul) et
  de `lib/wine/x86_64-unix/` (les `.so` « Unix », `wine`, `wine-preloader`). Les DLL de
  `x86_64-windows/` et `i386-windows/` sont des PE : aucune dépendance Linux. Il n'y a pas de
  `i386-unix` (mode WoW64 : tout le Unix est en 64 bits).
- `-l` indique où sont `ntdll.so`, `win32u.so`… : ce sont des bibliothèques de Wine, pas des
  paquets, et sans `-l` `dpkg-shlibdeps` avertit pour chacune.
- `--ignore-missing-info` : tolère les bibliothèques dont le paquet ne fournit pas de fichier `shlibs`.
- Sortie sur la machine de développement (Ubuntu 26.04, **à titre indicatif seulement**, noms
  `t64` et versions plus récentes que la cible) :
  `libasound2t64, libc6, libgcc-s1, libgssapi-krb5-2, libkrb5-3, libpulse0, libudev1,
  libusb-1.0-0, libx11-6, libxext6, ocl-icd-libopencl1 | libopencl-1.1-1 | libopencl1`.
  Ce sont les bibliothèques **liées** (champ `NEEDED`) de `winealsa.so`, `winepulse.so`,
  `winebus.so`, `wineusb.so`, `winex11.so`, `opencl.so`, `kerberos.so`, `ntdll.so`, `wine`…

## 2. Relevé des bibliothèques chargées à la demande

Méthode : `ldd` sur chaque ELF du Wine de développement (`/opt/wine-mao-v2`) pour les
dépendances liées, et `strings -a` sur chacun pour les noms `lib*.so*` passés à `dlopen`
(Wine les écrit sous forme de `SONAME_LIBxxx`, résolus par `configure` grâce aux en-têtes
installés à la compilation : **un en-tête absent à la compilation supprime la fonction**).

| Bibliothèque (soname) | Chargée par | Paquet | Rang | Raison |
|---|---|---|---|---|
| `libfreetype.so.6` | `win32u.so`, `dwrite.so` | `libfreetype6` | Depends | Sans elle, aucun texte : les interfaces de plugins sont vides |
| `libfontconfig.so.1` | `win32u.so` | `libfontconfig1` | Depends | Recherche des polices installées |
| `libX11.so.6`, `libXext.so.6` | `winex11.so` (liées) | `libx11-6`, `libxext6` | Depends | Fenêtres (calculées aussi par `dpkg-shlibdeps`) |
| `libXrender.so.1` | `winex11.so` | `libxrender1` | Depends | Rendu des fenêtres |
| `libXrandr.so.2` | `winex11.so` | `libxrandr2` | Depends | Taille et position des écrans |
| `libXi.so.6` | `winex11.so` | `libxi6` | Depends | Souris et tablettes (fenêtres de plugins) |
| `libXcursor.so.1` | `winex11.so` | `libxcursor1` | Depends | Curseurs |
| `libXcomposite.so.1` | `winex11.so` | `libxcomposite1` | Depends | Fenêtres incrustées (yabridge) |
| `libXfixes.so.3` | `winex11.so` | `libxfixes3` | Depends | Presse-papiers, curseur |
| `libvulkan.so.1` | `win32u.so`, `winex11.so` | `libvulkan1` | Depends | Base de DXVK (interfaces graphiques des plugins) |
| `libgnutls.so.30` | `secur32.so`, `crypt32.so` | `libgnutls30t64` | Depends | Chiffrement des licences et activations des plugins |
| `libpulse.so.0` | `winepulse.so` (liée) | `libpulse0` | Depends | Son PulseAudio / PipeWire |
| `libasound.so.2` | `winealsa.so` (liée) | `libasound2t64` | Depends (calculé) | Son ALSA |
| `libGL.so.1`, `libEGL.so.1` | `winex11.so`, `win32u.so` | `libgl1`, `libegl1` | Recommends | OpenGL : plugins sans DXVK ; Wine démarre sans |
| `libXinerama.so.1` | `winex11.so` | `libxinerama1` | Recommends | Multi-écrans : repli sur XRandR |
| `libXxf86vm.so.1` | `winex11.so` | `libxxf86vm1` | Recommends | Gamma / plein écran, accessoire |
| `libcups.so.2` | `winspool.so` (absente du relevé, voir plus bas) | `libcups2t64` | Recommends | Impression, sans objet pour des plugins audio |
| `libSDL2-2.0.so.0` | `winebus.so` | `libsdl2-2.0-0` | Recommends | Manettes de jeu uniquement |
| `libdbus-1.so.3` | `mountmgr.so`, `winebth.so` | `libdbus-1-3` | Recommends | Détection des disques et appareils, facultative |
| `libudev.so.1`, `libusb-1.0.so.0` | `winebus.so`, `wineusb.so` (liées) | calculé | Depends (calculé) | Périphériques |
| `libkrb5.so.3`, `libgssapi_krb5.so.2` | `kerberos.so` (liées) | calculé | Depends (calculé) | Authentification réseau |
| `libOpenCL.so.1` | `opencl.so` (liée) | calculé | Depends (calculé) | Calcul OpenCL (alternative `ocl-icd-libopencl1 \| …` produite par `dpkg-shlibdeps`) |
| `libodbc.so` | `odbc32.so` | — | non retenu | Bases de données, hors usage ; la fonction se désactive seule |

Ajouts par rapport à la liste de départ de la consigne : `libegl1` (Wine charge `libEGL` avec
`libGL`) et `libdbus-1-3`, tous deux en `Recommends` ; ils n'ont aucune incidence sur le démarrage.

Particularités du Wine de développement : il a été compilé sans `libcups2-dev` ni `libsdl2-dev`
(`build/configure-v2.log` : « libcups … won't be supported »), donc `libcups` et `libSDL2` n'y
apparaissent pas dans le relevé. Le paquet est compilé **avec** ces en-têtes (voir
`PAQUETS_COMPILATION`), d'où leur présence en `Recommends`. Le relevé est à refaire sur le vrai
paquet à la tâche 11 (`strings` sur les `.so` du paquet CI).

## 3. Noms de paquets valables sur Ubuntu 24.04 et Debian 13

Vérifiés sur packages.ubuntu.com (noble) et packages.debian.org (trixie) :

- `libgnutls30t64` et `libcups2t64` : **seuls** noms existants dans les deux distributions
  (`libgnutls30` et `libcups2` n'existent plus : renommage `t64` du passage à `time_t` 64 bits). Une
  alternative `libfoo | libfoot64` est donc inutile et n'a pas été écrite.
- `libasound2t64` : idem (produit par `dpkg-shlibdeps` lui-même).
- Sans renommage, identiques dans les deux : `libfreetype6`, `libfontconfig1`, `libx11-6`,
  `libxext6`, `libxrender1`, `libxrandr2`, `libxi6`, `libxcursor1`, `libxcomposite1`,
  `libxfixes3`, `libxinerama1`, `libxxf86vm1`, `libvulkan1`, `libpulse0`, `libgl1`, `libegl1`,
  `libsdl2-2.0-0`, `libdbus-1-3`.

## 4. Paquets de compilation (`PAQUETS_COMPILATION`)

Affichés par `paquets/fabriquer-wine.sh --paquets-compilation` (la CI les installe). Ils couvrent
les options `configure` `--enable-archs=i386,x86_64` (`gcc-mingw-w64` fournit les deux compilateurs
PE), `flex`/`bison`, et les en-têtes de chacune des bibliothèques du tableau ci-dessus. `clang` n'est pas
nécessaire (le Wine de développement est compilé avec `gcc`). Le Wine de développement a signalé
« PE compiler supporting C++17 not found » : `g++-mingw-w64` n'est pas installé, par fidélité au Wine
validé ; l'ajouter changerait l'ensemble des modules compilés.
