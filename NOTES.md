# WiMao — Notes de suivi

Wine personnel pour la MAO (Bitwig 6 + yabridge) sous Ubuntu 26.04.

## Machine
- Ubuntu 26.04.1 LTS (resolute), noyau 7.0.0-42, x86_64, 28 threads (i7 14700KF)
- GPU RTX 4060 Ti, pilote NVIDIA 595.99.02
- Audio : PipeWire 1.6.2 + pipewire-jack
- GCC 15.2.0, mingw-w64 13.2.0 (disponible dans les dépôts)

## État initial (2026-10-30)
- Aucun Wine installé (ni paquet, ni binaire dans le PATH, ni /opt/wine*)
- Aucun yabridge installé
- gh connecté (compte F1ghtc0re)

## Plan
1. Wine 9.21 + wine-staging 9.21, compilé depuis les sources, installé dans /opt/wine-mao
2. yabridge 5.1.1 (binaires GitHub), forcé sur /opt/wine-mao, préfixe ~/.wine-mao
3. Test avec un plugin gratuit, puis Serum, Plugin Alliance, AIR, NI, Arturia
4. (Plus tard) Variante 2 : Wine récent avec le commit 9.22 sur wm_state_serial annulé

## Décisions
### WoW64 « nouveau mode » (--enable-archs=i386,x86_64) : OUI
- Disponible depuis Wine 9.0 (encore qualifié d'expérimental en 9.x). Le code 32 bits
  Windows (PE) tourne via la couche WoW64, sans bibliothèques Unix 32 bits.
  Il n'y a donc aucun paquet :i386 à installer.
- Les installeurs 32 bits (InnoSetup, etc.) fonctionnent. Le point faible connu en 9.x
  concerne les performances OpenGL des applications 32 bits, ce qui ne touche pas nos plugins 64 bits.
- Conséquence pour yabridge : yabridge-host-32.exe est une application winelib 32 bits
  qui a besoin du côté Unix 32 bits. Elle ne fonctionnera PAS. Il n'y aura donc pas de
  pont pour les plugins 32 bits. Les plugins visés (Serum, PA, AIR, NI, Arturia) sont
  tous en 64 bits, donc c'est acceptable.
- Si un plugin 32 bits devient indispensable, il faudra un build « classique » (64 bits + 32 bits séparés).

### Dépôt Git
- Le dossier wine-mao devient un dépôt Git contenant les scripts de build, les patchs et NOTES.md
  (pas les sources de Wine, trop volumineuses : elles sont téléchargées par le script).
- Le dépôt GitHub (privé jusqu'à la première version publique) sert de sauvegarde. Pour la variante 2, le patch
  d'annulation (revert) sera rangé dans patches/.

## Organisation Git (dépôt github.com/F1ghtc0re/WiMao, public à partir de la première version)
- `main` = ce qui est validé et installé, et marche dans Bitwig. On n'y travaille jamais directement.
- Une branche par chantier, fusionnée dans main seulement après test et accord :
  - `etape1-wine-9.21-staging` : script de build Wine 9.21 + staging → /opt/wine-mao
  - `etape2-yabridge` : installation et configuration de yabridge + préfixe ~/.wine-mao
  - `variante2-revert-wm-state-serial` (plus tard) : Wine récent avec le revert → /opt/wine-mao-v2
- Travail en parallèle : chaque branche a son propre dossier (git worktree) dans
  .claude/worktrees/ (ignoré par Git), donc les branches ne se marchent pas dessus.
- Chaque variante s'installe dans son propre dossier /opt, et chaque test dans son propre préfixe.
- Règle de synchronisation entre machines : `git pull` avant de commencer, `git push` en fin de session.
  GitHub fait référence.

## Problèmes anticipés
- GCC 15 utilise par défaut le standard C23, ce qui casse la compilation de vieux codes
  (bool, prototypes vides). Si ça casse : CFLAGS="-std=gnu17" ou -Wno-error=...
- FFmpeg 8 sur 26.04 contre Wine 9.21 : support ffmpeg (winedmo) non nécessaire pour la MAO.
  Désactivation prévue si ça pose problème.

## Étape 2 : yabridge 5.1.1 (fait le 2026-10-30)
- Archive officielle yabridge-5.1.1.tar.gz (sha256 d11f0307…cd1a47), installée par scripts/install-yabridge.sh
- Fichiers réels : /opt/wine-mao/yabridge. Lien : ~/.local/share/yabridge → /opt/wine-mao/yabridge
  (emplacement imposé : les chainloaders ne cherchent que là ou dans /usr/lib, /usr/local/lib)
- Forçage du Wine : dans yabridge-host.exe et yabridge-host-32.exe, `WINELOADER="wine"` est remplacé
  par `/opt/wine-mao/bin/wine`. Vérifié : l'hôte 64 bits démarre même sans aucun wine dans le PATH.
- Hôte 32 bits : ne démarre pas (« File not found »), comme prévu en WoW64. Plugins 32 bits non pris en charge.
- Préfixe ~/.wine-mao (Windows 10). Dossiers déclarés dans yabridgectl :
  Program Files/Common Files/VST3, Common Files/CLAP, Program Files/VstPlugins, Steinberg/VstPlugins
- Après installation d'un plugin : `~/.local/share/yabridge/yabridgectl sync`
  (crée ~/.vst3/yabridge, ~/.clap/yabridge, ~/.vst/yabridge, qui sont scannés par Bitwig)
- Lancer un installeur Windows : `WINEPREFIX=~/.wine-mao /opt/wine-mao/bin/wine installeur.exe`

## Étape 3 : tests de plugins
- Valhalla Supermassive 5.0.0 (zip officiel depuis valhalladsp.com/demos-downloads, hébergé sur
  valhallaproduction.s3, sha256 67e0cedb…14911d). Installeur Inno Setup 32 bits, installation silencieuse
  (`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`) : OK en WoW64.
  Formats installés : VST3 (Common Files/VST3), VST2 (Common Files/VST2, dossier ajouté à yabridgectl), AAX (ignoré).
  yabridgectl sync : 2 plugins pontés.
- Bitwig : 6.0.11 (.deb). Lancement avec journaux yabridge :
  `YABRIDGE_DEBUG_LEVEL=1 YABRIDGE_DEBUG_FILE=~/Applis/wine-mao/build/yabridge-bitwig.log bitwig-studio`
- Test Bitwig (2026-10-30) : Supermassive VST3 chargé, interface ouverte et intégrée (X11 embed, 820×435).
  Seules erreurs : err:ole UI Automation {4ce576fa…} non enregistré (sans effet). Son et redimensionnement
  non testés en détail. wine version « <NOT FOUND> » dans le journal : purement informatif (pas de wine dans le PATH).
- Serum 2.1.5 (Install_Xfer_Serum2_2.1.5.exe, depuis ~/Téléchargements) : installeur NSIS 32 bits, `/S` silencieux OK.
  Le paquet ne contient que VST3 + AAX (ni CLAP, ni VST2). Sons dans Documents\Xfer\Serum 2 Presets (1,4 Go).
  Install_Xfer_Serum2_Update_2.1.5.exe = même plugin sans les sons, donc inutile.
  yabridgectl sync : Serum2.vst3 ponté. Vérifié : le binaire de la mise à jour est identique (sha256).
  Dans Bitwig : écran « not yet authorized », boutons OK/Help impossibles à cliquer (gel JUCE).
  HideWineExports=Y (option staging « Hide Wine version ») ajouté par précaution, mais ce n'était pas la cause.
  Vraie cause, trouvée avec WINEDEBUG=+event,+winebrowser dans carla-single : les clics arrivent bien et Serum
  appelle winebrowser → xdg-open → Chrome, mais Chrome lancé depuis Wine n'ouvre jamais l'onglet
  (vérifié dans l'historique de Chrome ; un script relais avec environnement propre n'a rien changé).
  CONTOURNEMENT : relever l'URL de licence (machine + port `ret=localhost:NNNN`, visible avec `ss -ltnp | grep wineserver`
  ou dans les traces), puis l'ouvrir depuis un terminal Linux : `google-chrome 'URL'`.
  Serum 2 activé le 2026-10-30 (machine 0057ef432c2a5528) → AppData/Roaming/Xfer/Serum 2/Serum2.lic
  À FAIRE : trouver pourquoi Chrome ne reçoit pas les liens envoyés par Wine (ça concernera NI et Arturia).
  Scintillement de l'interface quand le clavier s'anime : Serum 2 = VSTGUI en Direct2D sur D3D11/DXGI, avec
  redessin partiel. wined3d (OpenGL) ne conserve pas toujours l'image entre deux affichages.
  Essais annulés : wined3d renderer=vulkan (zones noires), d2d1 désactivé (Serum ne démarre plus).
  Solution : DXVK 3.1.1 (d3d11, dxgi, d3d10core seulement), via scripts/install-dxvk.sh (`--retirer` pour annuler).
  Testé dans carla-single + notes MIDI (aseqsend → pw-link vers « Serum 2:events-in ») : rendu complet, RTX 4060 Ti.
  Les `err: readMonitorEdidFromKey` / `DXGI: Failed to parse display metadata` de DXVK sont sans conséquence.
  Après DXVK : seul le graphique ENV (milieu de la fenêtre) scintille encore quand des notes jouent. Mesuré dans
  carla-single (scratchpad mesure-env.sh) : la zone est noire 1 image sur 2, déjà le cas avant DXVK.
  Pistes écartées : dxvk.conf dxgi.numBackBuffers=1 (aucun effet ; DXVK ne lit dxvk.conf qu'avec DXVK_CONFIG_FILE,
  pas à côté de yabridge-host.exe) ; d2d1.dll compilé depuis Wine 11.20 (AddArc implémenté) : aucun effet, retiré.
  Aucune erreur d2d/dxgi pendant les images noires. Trace +d2d : Serum dessine via bitmaps hors écran et DrawBitmap.
  Problème NON résolu (cosmétique, le son n'est pas affecté). À retester avec la variante 2 (Wine récent complet).
  Rappel : xdotool ne peut pas cliquer dans les fenêtres X11 sous GNOME Wayland (seuls les mouvements passent).
- Plugin Alliance (2026-10-30, sous v2) : Installation Manager 1.4.0 (zip public plugin-alliance.com/pages/installation-manager,
  sha256 0ff7eca9…eb8c5), MSI installé avec `msiexec /i … /qn`. Connexion au compte faite par l'utilisateur dans l'application.
  26 produits installés (VST3 dans Common Files/VST3, VST2 dans Steinberg/VstPlugins/Plugin Alliance, AAX inutiles).
  Chaque produit = 4 paquets (vst3, vst2, aaxnative, aaxdsp). L'IM se fige à 98 % sur le dernier produit (SPL Free Ranger)
  alors que les fichiers sont complets ; fermer l'IM vide son dossier Temp (paquets perdus).
  VST2 PA exclus : `yabridgectl blacklist add ".../Steinberg/VstPlugins/Plugin Alliance"` (pas de doublons dans Bitwig).
  `yabridgectl sync --prune` attend une confirmation au clavier : utiliser `sync < /dev/null` sans --prune.
  bx_solo vérifié dans carla-single : se charge ; 1re ouverture = message « data tracking » à valider (OK).
- Arturia (2026-10-30, sous v2) : Arturia Software Center 2.13.0.3440, installeur Windows public
  `https://downloads.arturia.net/products/asc/soft/Arturia_Software_Center__2_13_0_3440.exe` (Inno Setup,
  installé avec `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP-`) dans Program Files (x86)/Arturia. Interface OK
  (erreur WebView2 bénigne), connexion faite par l'utilisateur dans l'application. Lancer depuis son dossier.
  Analog Lab Pro 5.12.6 installé et activé via l'ASC (VST3 Common Files/VST3/Analog Lab V.vst3), ponté,
  vérifié dans carla-single : interface PRO, son OK (note MIDI enregistrée via pw-record).
  Attention : Pigments, Augmented VOICES, Analog Lab Pro étaient à 4/5 activations avant Wine.
  Pigments 7.0.1 et Augmented VOICES 2.0.2 installés/activés via l'ASC, pontés, vérifiés dans carla-single
  (interface sans démo, son OK). L'ASC installe aussi des VST2 (Program Files/VstPlugins/*.dll) : exclus avec
  `yabridgectl blacklist add` fichier par fichier et ponts ~/.vst/yabridge correspondants supprimés (pas de doublons).
  carla-single est un script : `pgrep -x carla-single` ne le trouve pas, tuer par PID.
- Native Instruments (2026-10-30, sous v2) : Native Access 2 (assets.native-instruments.com/downloads/Native-Access_2.exe,
  NSIS electron-builder). L'installeur `/S` échoue en silence (dossier vide) : il teste via PowerShell (stub dans Wine)
  si NA tourne. Contournement : `7z x` de l'exe puis `7z x '$PLUGINSDIR/app-64.7z'` dans Program Files/Native Instruments/
  Native Access. NA réclame ensuite ses « dependencies » = resources/daemon/win/NTKDaemon 1.32.0 Setup PC.exe
  (InstallAware), installé avec `/s` → service NTKDaemonService (Start=3, démarré par `wine net start`).
  Lancement : `Native Access.exe --no-sandbox --disable-gpu`. Connexion : winebrowser ouvre bien Chrome sous v2 ;
  retour par native-access://authorize, relayé par scripts/native-access-lien.sh déclaré dans
  ~/.local/share/applications/wimao-native-access.desktop (xdg-mime x-scheme-handler/native-access).
  Sans sorties valides, la 2e instance Electron plante (Error: open EBADF) : le script les redirige vers un journal.
  « Download failed: Review your download location » (daemon.log : ERROR_CODE_DOWNLOAD_DIR_MISSING, Download: '') :
  le daemon ne trouve pas de dossier par défaut sous Wine. Correctif : `reg add 'HKCU\Software\Native Instruments\
  Native Access' /v DownloadLocation /d 'C:\users\<utilisateur>\Downloads\NI'` puis `net stop/start NTKDaemonService`
  (ntk.json et la clé NTKDaemon ne marchent pas). Journal du daemon : users/Public/Documents/Native Instruments/Logs/NTK/daemon.log.
  Guitar Rig 7 Pro 7.0.2 installé et activé par NA (installeur InstallAware lancé par le daemon), VST3 ponté,
  vérifié dans carla-single (sinus 220 Hz via pw-play → sortie OK, 1554 presets, pas de démo).
  Les VST2 NI vont dans Program Files/Native Instruments/VSTPlugins 64 bit (hors yabridge : pas de doublons).
  2026-10-31 : Komplete Kontrol 3.3.4, Kontakt 6/7/8 Player, Maschine 2 et 3, Maschine 2 Factory Library et
  Maschine Central installés et activés par NA (codes de sortie InstallAware 100 ou 110 : OK). 6 VST3 pontés.
  Maschine 3 plantait au chargement : « unimplemented function msvcp140.dll.??4?$basic_istream… aborting » puis
  stack overflow. Le VC_redist de NA n'écrase pas msvcp140.dll de Wine (version annoncée plus haute).
  Correctif : scripts/install-msvcp140.sh (DLL Microsoft extraites du Package Cache du préfixe, override
  msvcp140=native,builtin, sauvegarde des DLL Wine dans src/msvcp140/sauvegarde, option --retirer).
  Vérifiés ensuite dans carla-single : Maschine 3 et 2, Kontakt 6/7/8, Komplete Kontrol, Serum 2, Pigments, bx_solo.
  Nom de fenêtre Carla = nom affiché du plugin (« Serum 2 (GUI) »), pas le nom du fichier .vst3.
  2026-10-31 (2e file, 29 éléments) : 36 produits activés au total. Échec : Previews (Native_Browser_Preview_Library.iso,
  ERROR_CODE_MOUNT_FAILED : NA monte les .iso, impossible sous Wine ; échoue aussi sur le Mac de l'utilisateur, ignoré).
  L'ISO est effacée juste après l'échec (la capturer par lien dur pendant le téléchargement si besoin un jour).
  Nouveaux VST3 pontés et vérifiés : Massive, Phasis, Raum, Reaktor 6, Replika, Solid Bus Comp, Ozone 12 Elements,
  Vocal Doubler (iZotope, sous-dossier VST3/iZotope). VST2 Vocal Doubler (Steinberg/VstPlugins) exclu.
  iZotope : rendu Direct3D 9 ; captures xwd blanches (contenu non capturable sous XWayland), mais affichage
  confirmé correct par l'utilisateur. DXVK d3d9 ajouté par précaution (install-dxvk.sh, DLLS += d3d9).
  Surveillance de file NA fiable : 10 min sans variation de (taille Downloads/NI, taille Logs/NTK, installeurs actifs).
- AIR / inMusic (2026-10-31, sous v2) : inMusic Software Center 1.40.0 (https://inmusic.to/ISCPC → cdn.inmusicbrands.com,
  installeur Squirrel qui exige .NET). Installation manuelle : 7z de l'exe puis du .nupkg ; lib/net45 copié dans
  AppData/Local/inmusic_software_center/app-1.40.0 (+ Update.exe, RELEASES, packages/). Lancement :
  `inMusic Software Center.exe --no-sandbox --disable-gpu`. Connexion : retour par http://localhost:41401 (direct, sans relais).
  L'ISC télécharge dans Downloads/inMusic et lance les installeurs AVEC interface (clics nécessaires).
  Installeurs Inno (Install-<Nom>-v1.2.1.14-release.exe) : relancés à la main avec /VERYSILENT (Hype, Jura, TubeSynth).
  Mini Grand (WiX Burn) échouait (0x80070643) : le MSI FactoryContent a une action .NET (VelvetFixContentLocation) et
  des fichiers externes absents du Package Cache. Correctifs : Wine Mono 11.3.0 installé dans le préfixe
  (src/wine-mono, sha256 df2dfc16…418d3, version attendue par Wine 11.20) ; contenu copié à la main
  (Mini Grand.big → ProgramData/Air Music Technology/AIR Content/Mini Grand, reg HKLM\Software\AIRMusicTech\Mini Grand
  Content=…) ; plugin installé par `MiniGrand_1.4.0_Setup.exe -quiet`.
  9 VST3 pontés : DB-33, DB-33FX, DrumSynth, Electric, Essential Keyboards, Hype, Jura, MiniGrand, TubeSynth.
  VST2 AIR (Steinberg/VstPlugins/*_x64.dll, VstPlugins/Essential Keyboards) exclus de yabridge.
  Vérifiés dans carla-single : DB-33, Electric, Essential Keyboards, MiniGrand, Hype OK. DrumSynth : Carla plante sur
  sa propre assertion « parameterId < count » (bug de Carla, pas de Wine) ; à juger dans Bitwig.
  L'activation se fait dans l'ISC lors de son installation : Jura et TubeSynth installés à la main restent « Deactivated ».
  2026-10-31 (suite) : Jura, TubeSynth et tous les suivants activés par l'utilisateur dans l'ISC (Jura s'active sous
  Wine alors que l'activation échoue sous Windows chez lui). Installés : Vacuum Pro, Velvet (contenu OK grâce à Wine
  Mono), Xpand!2, AIR Ether, Flavor, Half Speed, Vocal Tuner. « Pilote » : surveille les installeurs lancés par l'ISC
  (ligne de commande contenant Downloads\\inMusic\\…exe sans /VERYSILENT ni -quiet), les tue et les relance en silencieux
  (Inno : /VERYSILENT ; WiX Burn : -quiet). L'ISC réaffiche alors « Install » : il revérifie la présence au clic ou à
  son redémarrage. VST2 *_x64.dll exclus. scripts/tester-plugins.sh : test d'ouverture des VST3 dans carla-single.
  Les 9 vérifiés OK : VacuumPro, Velvet, Xpand!2, AIR Ether, AIR Flavor, AIR Half Speed, AIR Vocal Tuner, Jura, TubeSynth.
  Fermer l'ISC : le processus principal a pour ligne de commande « inMusic Software Center.exe … » SANS « C:\ » ;
  s'il survit, une relance passe la main à cette instance (verrou Electron) et la fenêtre reste noire.
  Tuer tous les processus correspondant à « inMusic Software Center.exe » (jamais avec ce motif dans la même commande).

## Variante 2 : Wine 11.20 + annulation de la garde wm_state_serial (2026-10-30)
- Commit fautif de Wine 9.22 : d8b5a3ae129 (Rémi Bernon, MR 6569), bug Wine 58552 (UNCONFIRMED, non annulé upstream).
  Il ajoute `if (data->wm_state_serial) return 0;` dans window_update_client_config (winex11.drv/window.c).
- Patch : patches-v2/0001-…, repris de la pièce jointe 79098 du bug 58552 (ligne neutralisée).
  Attention : le bug signale un décalage intermittent après fermeture et réouverture d'un plugin dans Bitwig.
- Build : scripts/build-wine-v2.sh → Wine 11.20 (sha256 eac8ca44…6fe28, signature OK), sans staging, WoW64,
  installé dans /opt/wine-mao-v2. cmd 64 et 32 bits OK.
- Préfixe de TEST : copie de ~/.wine-mao dans /opt/wine-mao-v2/prefix (wineboot -u avec
  WINEDLLOVERRIDES="mscoree,mshtml=", sinon une fenêtre Mono/Gecko bloque).
- Test sans toucher l'installation : WINELOADER=/opt/wine-mao-v2/bin/wine WINEPREFIX=/opt/wine-mao-v2/prefix
  devant la commande de l'hôte (le script yabridge-host.exe respecte un WINELOADER défini).
- Serum 2 + DXVK sous v2 : graphique ENV noir 0/60 (contre 30/60 en v1), zone VELO affichée. Le scintillement est corrigé.
- Test Bitwig v2 (2026-10-30, utilisateur) : plus de scintillement, clics au bon endroit. Fermeture puis réouverture : à confirmer.
- **2026-10-30 : la v2 devient le Wine PAR DÉFAUT** (choix de l'utilisateur).
  `scripts/choisir-wine.sh v1|v2` (Bitwig fermé) : fait pointer ~/.local/share/yabridge → /opt/wine-mao{,-v2}/yabridge
  (copie de yabridge avec WINELOADER forcé), lance wineboot -u sur ~/.wine-mao, revérifie isolation et DXVK.
  Sauvegarde du préfixe v1 avant bascule : /opt/wine-mao/sauvegarde-prefixe-v1 (1,9 Go).
  Au 1er wineboot -u en 11.20 : 4× « unimplemented function advapi32.dll.SystemFunction036 » (composant 9.21 lancé
  avant remplacement). Un 2e wineboot -u est propre. Vérifié ensuite : cmd 64/32 OK, DXVK intact, Serum OK, ENV 0/60.
  Retour en v1 : choisir-wine.sh v1 ; si le préfixe pose problème, restaurer la sauvegarde.
  Le préfixe de test /opt/wine-mao-v2/prefix a été supprimé.
  Fermeture puis réouverture de la fenêtre d'un plugin dans Bitwig : PAS ENCORE TESTÉ (risque signalé dans le bug 58552).
- Wine 11 utilise EGL : les « libEGL warning … dri2 » (Mesa essaie avant NVIDIA) sont sans conséquence.
- Piège : `pkill -f motif` tue aussi la commande shell qui contient ce motif ; tuer les PID un par un.

## Problèmes rencontrés
- **Patchs staging ignorés en silence (1er build).** gitapply.sh appelle `git apply`. Comme src/wine-9.21
  se trouve dans le dépôt Git WiMao, git apply lisait les chemins depuis la racine de WiMao et ignorait
  tous les fichiers, avec un code retour 0. Le premier build était donc un Wine 9.21 standard.
  Correctif : `GIT_CEILING_DIRECTORIES=src` + contrôle de « (Staging) » dans configure.ac (voir le script).
  Ce piège vaudra aussi pour la variante 2.
- **Les installeurs écrivaient dans ~/Documents.** Wine relie Documents, Desktop, Downloads, Music, Pictures
  et Videos du préfixe aux dossiers Linux de l'utilisateur. Les sons de Serum sont donc arrivés dans ~/Documents/Xfer.
  Correctif : liens remplacés par de vrais dossiers du préfixe, Xfer déplacé dans le préfixe,
  `.update-timestamp` = disable (comme winetricks sandbox). Intégré à install-yabridge.sh.
  Après une mise à jour de Wine, lancer `WINEPREFIX=~/.wine-mao /opt/wine-mao/bin/wineboot -u` à la main.
- **Plantage au démarrage (wineboot) avec binutils 2.45.90 (mingw d'Ubuntu 26.04).**
  `Unhandled page fault on write access to 0000000140020A88` puis `could not load kernel32.dll, status c0000135`.
  L'adresse est dans .idata, qui est READONLY avec les binutils récents. winebuild 9.21 y met les
  données des imports différés (delay-load), et le chargeur plante en écrivant dedans (bug Wine 57819).
  Correctif : rétroportage de 2 commits de Jacek Caban (Wine 10.2, MR 7328), dans patches/ :
  `fd599628` (ntdll : IAT offset signé) et `c9519f68` (winebuild : pas de .idata pour delay-load).
  La variante 2 (Wine récent) les contient déjà.

## Accès sudo
- /etc/sudoers.d/claude-wine-mao : NOPASSWD pour apt, apt-get, dpkg, install, mkdir, chown.
  Fichier en 644 (le chmod 440 ne s'est pas appliqué, mais sudo l'accepte).
  À retirer en fin de projet : `sudo rm /etc/sudoers.d/claude-wine-mao`

## Journal
- 2026-10-30 : état des lieux, choix WoW64, création de NOTES.md
- 2026-10-30 : dépôt GitHub privé F1ghtc0re/wine-mao créé, conventions de branches
- 2026-10-30 : projet renommé WiMao (dépôt GitHub F1ghtc0re/WiMao). Les chemins restent wine-mao.
- 2026-10-30 : dépendances installées, Wine 9.21 compilé (WoW64) mais SANS staging (voir Problèmes), recompilation lancée
- 2026-10-30 : build staging OK mais plantage de wineboot (binutils) ; rétroportage de 2 correctifs Wine 10.2, recompilation
- 2026-10-30 : ÉTAPE 1 OK. `wine-9.21 (Staging)` + 2 correctifs, dans /opt/wine-mao. Test de préfixe : wineboot OK, cmd 64 et 32 bits OK (Windows 10)
- 2026-10-30 : ÉTAPES 2 et 3 OK (yabridge + Supermassive dans Bitwig), fusion dans main
- 2026-10-30 : Serum 2.1.5 installé ; préfixe isolé du dossier personnel
- 2026-10-30 : Serum 2 activé et validé dans Bitwig par l'utilisateur, fusion dans main
- 2026-10-30 : DXVK 3.1.1 installé dans ~/.wine-mao contre le scintillement de Serum 2
- 2026-10-30 : Arturia Software Center installé, Analog Lab Pro installé, activé et ponté
- 2026-10-30 : Pigments et Augmented VOICES installés, activés et pontés (VST3 seulement)
- 2026-10-30 : Native Access 2 installé (extraction 7z + NTKDaemon), connexion au compte OK
- 2026-10-31 : Guitar Rig 7 Pro installé, activé et ponté via Native Access
- 2026-10-31 : Kontakt 6/7/8, Komplete Kontrol, Maschine 2/3 pontés ; msvcp140 Microsoft pour Maschine 3
- 2026-10-31 : 2e file NI (36 produits), 8 plugins pontés dont Massive, Reaktor 6, Ozone ; DXVK d3d9
- 2026-10-31 : inMusic Software Center + 9 plugins AIR pontés ; Wine Mono 11.3.0 pour Mini Grand
- 2026-10-31 : sauvegarde du préfixe (scripts/sauvegarde-prefixe.sh) sur disque WD Element (FAT32 → morceaux de 3,9 Go), 81 Go
- 2026-10-31 : AIR complet (Vacuum Pro, Velvet, Xpand!2, Ether, Flavor, Half Speed, Vocal Tuner, Jura, TubeSynth)

## WiMao — test d'ouverture sous Xvfb
- 2026-10-31 : `tester_plugin` (crates/wimao-core/src/test_plugin.rs) vérifié en lecture seule sur l'installation réelle
  (préfixe ~/.wine-mao, ~/.vst3/yabridge/Serum2.vst3, Bitwig fermé) : résultat `Pret` en 13 s, fenêtre « (GUI) » vue sur
  Xvfb :90, aucun processus restant, rien d'affiché sur l'écran réel. Xvfb installé (`apt-get install xvfb`).

## WiMao sous-projet 1 — vérification réelle (2026-10-31)
- `wimao-cli --langue fr verifier --prefixe ~/.wine-mao` (lecture seule, Bitwig fermé, environ 15 min sous Xvfb) :
  64 plugins pontés testés → **62 ✅**, **1 ⚠️** (Vocal Doubler : chargé, interface non vérifiée), **1 ❌** (DrumSynth :
  « L'hôte de plugins s'est arrêté », plantage de Carla déjà connu, voir Étape 3). Code de sortie 2 (au moins un ❌), conforme.
  Aucun dossier de données WiMao créé, aucun lien de ~/.vst3/yabridge, ~/.vst/yabridge ni configuration yabridgectl modifié.
  Tous les plugins notés OK à l'étape 3 sont ✅ (dont Serum2, MiniGrand, DB-33, Electric, Essential Keyboards, Hype, Jura,
  TubeSynth, VacuumPro, Velvet, Xpand!2, AIR Ether/Flavor/Half Speed/Vocal Tuner, bx_solo, Pigments, Kontakt 7/8).
- Installeur WiX Burn réel (copie de MiniGrand_1.4.0_Setup.exe, HOME/WIMAO_DONNEES/XDG_* de test, Xvfb) :
  `detecter` → « WiX Burn » ; `installer --sans-test` → environnement créé (Mono, DXVK, msvcp140), point de restauration,
  lancement avec `-quiet -norestart` → code de sortie 67 et aucun nouveau plugin : le contenu de MiniGrand vient de
  l'inMusic Software Center (téléchargement lié au compte), l'installeur seul ne suffit pas. WiMao propose alors
  `--interactif`, comme prévu. À améliorer : afficher le code de sortie de l'installeur dans le message.
- Incident corrigé : `tests/fabriquer-fixtures.sh` (tâche 16) avait lancé Wine avec le vrai HOME ; winemenubuilder avait
  ajouté au menu du bureau des raccourcis Inno Setup et des associations (.iss, .reg, .vbs, .chm, .hlp, .crt, .msp)
  pointant vers un préfixe temporaire effacé. 51 fichiers mis à la corbeille avec l'accord de l'utilisateur, caches
  régénérés ; le script utilise désormais un HOME/XDG temporaire et `winemenubuilder.exe=d`.
- Modifications du préfixe réel constatées après la sauvegarde de 13h33 : 15h02 = test Serum2 de la tâche 8 (Wine
  réécrit ses .reg en s'arrêtant) ; 14h18 = caches de l'inMusic Software Center, origine non trouvée. Rien d'abîmé.

## WiMao sous-projet 2 — vérification réelle (2026-11-01)

Environnement de test : HOME, XDG_* et WIMAO_DONNEES temporaires, `WIMAO_WINE_DIR=/opt/wine-mao-v2`, Xvfb :99,
CLI `wimao-cli` compilée en release. `~/.wine-mao`, les vrais menus et `~/.config/mimeapps.list` n'ont pas bougé.
Aucun contrat de licence accepté, aucune connexion à un compte, aucun produit payant installé.

- **Native Access 2** : `gestionnaire installer` (téléchargement réel, 172 Mio,
  sha256 82d7b7977d4fbc19db32aee8778c75906f6ddf935bcf2958f2d8d45141b2b2cc) → extraction 7z, NTKDaemon 1.32.0 en
  silencieux, réglages, lien `native-access://` déclaré dans l'environnement de test ; 17 s. `gestionnaire ouvrir` →
  NTKDaemonService démarré, fenêtre du contrat de licence (étape qui précède la connexion). Ctrl+C → fermeture propre ;
  seuls les services restent (NTKDaemon, voulu par la spec : les services ne sont pas arrêtés).
- **Relais** : `xdg-open 'native-access://…'` (dans l'environnement de test) → `wimao-cli relayer-lien` → Native Access
  relancé avec l'adresse (sortie dans `journaux/liens.log`) ; Electron la transmet à l'instance ouverte. La réception
  finale n'est pas visible dans les journaux de Native Access ; elle est prouvée par le test d'intégration (faux
  gestionnaire) et sera confirmée à la première vraie connexion.
- **Arturia Software Center** 2.13.0.3440 (sha256 af23dab17e4353f497d85610fc415e506d90a36476a77fc226346b39fc01758e) :
  installé en 5 s, s'ouvre sur l'écran de connexion. Fermeture de la fenêtre simulée → la session se termine seule.
  `ArturiaSoftwareCenterAgent.exe` (agent de fond) reste ouvert : ne pas l'ajouter à `fermer`, sinon la session
  attendrait aussi sa fin et ne se terminerait jamais seule.
- **inMusic Software Center** : `https://inmusic.to/ISCPC` redirige vers « inMusic Software Center-1.40.0 Setup.exe »
  (Squirrel : Update.exe, RELEASES, inmusic_software_center-1.40.0-full.nupkg, setupIcon.ico, background.gif).
  Installé en 5 s dans `AppData\Local\inmusic_software_center\app-1.40.0` ; écran de connexion ; fermeture propre.
- **Plugin Alliance** : zip public `https://www.plugin-alliance.com/cdn/shop/files/pa_installation_manager_win_1_4_0.zip`
  (sha256 0ff7eca91d657e0e16c470dd260548b3bb3576c3c489905fe5c2a7f84b3eb8c5, identique au relevé du 2026-10-30), qui
  contient `PA-InstallationManager-v1.4.0.msi`. **Défaut trouvé et corrigé** : `msiexec` de Wine plantait (page fault,
  code 5) quand le dossier de travail dépassait ~125 caractères (WiMao se plaçait dans le dossier du MSI), et la
  fenêtre de plantage de winedbg bloquait l'installation silencieuse jusqu'au délai de 30 min. Désormais les MSI sont
  lancés depuis `/` et `winedbg.exe=d` est posé pour tous les installeurs. Installé ensuite en 3 s dans un
  environnement neuf ; s'ouvre sur son contrat de licence ; fermeture propre.
- **Valhalla Supermassive** : non vérifié de bout en bout. valhalladsp.com refuse les téléchargements automatiques
  (403) ; la recette reste sans `telecharger` ni `empreinte`, et son motif `reconnaitre` est encore à confirmer avec
  le vrai fichier. Le chemin Inno Setup silencieux est couvert par les tests d'intégration.
- Autres corrections : le journal notait les informations (empreinte d'un téléchargement, bilan, relance réussie)
  comme des « échec: » (désormais « note: ») ; un message de la CLI tutoyait (harmonisé au vouvoiement, fr et es).

## WiMao sous-projet 3 — vérification réelle (2026-11-01)

Environnement de test : HOME, XDG_* et WIMAO_DONNEES temporaires (`env -i`, `dbus-run-session`), `WIMAO_WINE_DIR=/opt/wine-mao-v2`,
Xvfb :93, interface `wimao` compilée en release. Tout s'est fait dans l'interface ; le sélecteur de fichiers (portail GTK)
s'ouvre sous Xvfb, aucune étape n'a dû passer par la CLI (la CLI a seulement servi à relire `restauration lister` et
`diagnostiquer`). `~/.wine-mao` et `~/.config/yabridgectl/config.toml` n'ont pas bougé (dates relevées avant et après).

- **Assistant, création** : sans `~/.wine-mao`, « Commencer » → « Créer l'environnement » → environnement prêt en 23 s
  (Mono, DXVK, VC++), page « Votre environnement est prêt ».
- **Assistant, import** : faux `~/.wine-mao` créé par `wineboot` dans le HOME temporaire, avec un plugin de test ponté par
  un yabridgectl « de l'utilisateur ». L'assistant propose « Importer cet environnement (recommandé) » (555,7 Mio, 1 plugin) ;
  import en moins d'une seconde : `~/.wine-mao` devient un lien vers le préfixe, l'historique note « Import ».
  « Annuler l'import » (menu) remet tout en place : dossier, `.reg`, `config.toml` de yabridgectl, liens de `~/.vst3/yabridge`
  et nombre de fichiers identiques à l'état d'avant (comparaison faite trois fois) ; l'assistant se rouvre.
- **Serum 2** (installeur réel de ~/Téléchargements, 1,3 Go, NSIS `/S`) : installé depuis la fenêtre en 21 s, ponté.
  « Revenir à avant ceci » : 7363 fichiers supprimés, le lien de `~/.vst3/yabridge` retiré, en environ 1 s.
- **Sauvegarde puis « Restaurer une sauvegarde »** depuis la fenêtre : archive de 319 Mo, restauration en 1 s, fichier
  ajouté après la sauvegarde disparu, ancien préfixe gardé en `prefixe.ancien-…` ; l'ancien point est « périmé » et
  « Revenir à avant ceci » n'est plus proposé.
- **Mode d'emploi** en fr, en et es après changement de langue dans le menu (la fenêtre se reconstruit) ; captures en clair
  et en sombre dans `.superpowers/sdd/2026-11-01-wimao-sous-projet-3/captures-t15/`.
- **Défauts trouvés et corrigés** :
  - un VST3 en bundle (Serum 2 et la plupart des VST3 récents) passait pour non ponté : yabridgectl relie le binaire
    `Contents/x86_64-win/<nom>.vst3`, et WiMao ne comparait la cible du lien qu'au dossier. D'où le toast « 1 plugin n'est
    pas encore ponté » à chaque lancement, un diagnostic faux, et aucun test d'ouverture après l'installation ;
  - Carla arrêté faute de serveur audio (« Failed to init engine ») donnait « Problème : l'hôte de plugins s'est arrêté » :
    c'est maintenant une remarque qui nomme la cause (PipeWire ou JACK injoignable) ;
  - après un retour vers un point ancien, les points plus récents restaient proposés (retour à avant une installation déjà
    défaite) : ils sont désormais périmés.
- **Non corrigé** : l'historique garde les sujets et détails dans la langue du moment où l'action a été faite
  (« avant l'installation de … », « 7363 fichiers supprimés » restent en français une fois l'interface en espagnol).
  Le test d'ouverture de Serum n'a pas pu conclure dans l'environnement isolé (pas de PipeWire joignable) ; il reste
  vérifié par le sous-projet 1 sur le vrai préfixe.
