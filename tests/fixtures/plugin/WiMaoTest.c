/* Source du faux plugin WiMaoTest.vst3 (DLL Windows presque vide, 4 Ko).
 *
 * Rôle : donner aux installeurs factices un vrai fichier PE x86-64 à installer.
 * yabridgectl lit l'en-tête PE d'un .vst3 pour en déduire l'architecture et cherche la
 * fonction exportée `GetPluginFactory` : sans elle il classe le fichier parmi les
 * « non-plugin .dll » et ne le ponte pas (constaté en écrivant le test d'intégration).
 * Le plugin n'a aucune vraie fonction VST3 : il ne se charge pas dans un logiciel de musique.
 *
 * Reconstruction (le binaire est déjà dans le dépôt) :
 *   x86_64-w64-mingw32-gcc -shared -nostdlib -s -Wl,--entry=0 -o \
 *     plugin/WiMaoTest.vst3/Contents/x86_64-win/WiMaoTest.vst3 plugin/WiMaoTest.c
 */
__declspec(dllexport) void *GetPluginFactory(void) { return 0; }
