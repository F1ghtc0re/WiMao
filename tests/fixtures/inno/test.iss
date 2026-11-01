; Script Inno Setup factice : installe le bundle WiMaoTest.vst3 dans le dossier VST3 commun.
; Compilé par tests/fabriquer-fixtures.sh (ISCC.exe /DPlugin=<dossier plugin> /O<sortie> test.iss).
; Sert uniquement aux tests d'intégration de WiMao : aucun code d'un éditeur.

[Setup]
AppName=WiMaoTest
AppVersion=1.0
; Installeur 64 bits : {commonpf64} = C:\Program Files.
ArchitecturesInstallIn64BitMode=x64compatible
DefaultDirName={commonpf64}\Common Files\VST3
; Pas de page de dossier, de groupe de menus ni de désinstalleur : rien d'autre que le plugin.
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
Uninstallable=no
; Droits de l'utilisateur seulement : évite toute élévation sous Wine.
PrivilegesRequired=lowest
OutputBaseFilename=test-inno

[Files]
Source: "{#Plugin}\WiMaoTest.vst3\Contents\x86_64-win\WiMaoTest.vst3"; DestDir: "{app}\WiMaoTest.vst3\Contents\x86_64-win"
