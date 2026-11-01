; Installeur NSIS factice : installe le bundle WiMaoTest.vst3 dans le dossier VST3 commun.
; Fabriqué par tests/fabriquer-fixtures.sh (makensis -DPLUGIN=<dossier plugin> ...).
; Sert uniquement aux tests d'intégration de WiMao : aucun code d'un éditeur.

Unicode true
Name "WiMaoTest"
OutFile "${SORTIE}"
; Programme 64 bits : $PROGRAMFILES64 = C:\Program Files. Le niveau « user » évite toute
; demande d'élévation de droits sous Wine.
RequestExecutionLevel user
; Pas de « SilentInstall silent » : lancé SANS /S, l'installeur doit attendre un clic sur sa
; première page sans rien installer. C'est ce qu'exige le test de session de gestionnaire
; (crates/wimao-recettes/tests/gestionnaire.rs) : le faux gestionnaire le lance ainsi, et seul
; le pilote de WiMao (relance avec /S) doit l'installer. Avec /S, NSIS saute ces pages et
; installe sans fenêtre.
; La page de dossier sert seulement de point d'attente : le dossier choisi n'est pas utilisé
; (la section écrit dans un chemin fixe, SetOutPath). InstallDir donne seulement à la page une
; valeur affichée cohérente au lieu d'un champ vide.
InstallDir "$PROGRAMFILES64\Common Files\VST3"
Page directory
Page instfiles

Section
  ; `/S` (installation silencieuse) est reconnu par NSIS sans rien déclarer ici.
  SetOutPath "$PROGRAMFILES64\Common Files\VST3\WiMaoTest.vst3\Contents\x86_64-win"
  File "${PLUGIN}\WiMaoTest.vst3\Contents\x86_64-win\WiMaoTest.vst3"
SectionEnd
