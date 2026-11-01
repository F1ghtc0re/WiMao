/* Source du faux gestionnaire d'éditeur FauxGestionnaire.exe (programme Windows, ~40 Ko).
 *
 * Rôle : jouer, dans le test d'intégration crates/wimao-recettes/tests/gestionnaire.rs, un
 * gestionnaire comme Native Access ou l'inMusic Software Center, sans compte ni réseau.
 *  - Relais : si un argument contient « :// », c'est un lien de connexion relayé par WiMao
 *    (relayer_lien). Il est ajouté, suivi d'un saut de ligne, à C:\faux-gestionnaire-liens.txt,
 *    et le programme quitte aussitôt.
 *  - Sinon, le 1er argument est le chemin Windows complet d'un installeur. Il est lancé SANS
 *    option silencieuse, comme le ferait un vrai gestionnaire : le pilote de WiMao doit le
 *    repérer (son chemin exact figure dans la ligne de commande), le fermer et le relancer
 *    en silencieux. Le faux gestionnaire attend la fin de CE processus (celui que le pilote
 *    ferme), au plus ATTENTE_INSTALLEUR_MS, puis encore ATTENTE_FINALE_MS, écrit « fin »
 *    dans C:\faux-gestionnaire-fin.txt et quitte. Ce fichier prouve au test que le
 *    gestionnaire a fini normalement : le pilote ne l'a pas fermé avec son installeur.
 *    Il n'observe pas la relance : elle est synchrone côté WiMao (Session::tick).
 *
 * Aucune dépendance hors de Windows (kernel32 et le msvcrt de mingw).
 *
 * Reconstruction (le binaire est déjà dans le dépôt), depuis tests/fixtures :
 *   x86_64-w64-mingw32-gcc -O2 -s -municode -o gestionnaire/FauxGestionnaire.exe \
 *     gestionnaire/FauxGestionnaire.c
 */
#include <windows.h>
#include <wchar.h>

/* Fichier où les liens relayés sont consignés (lu par le test relais_transmet_l_adresse). */
#define FICHIER_LIENS L"C:\\faux-gestionnaire-liens.txt"
/* Fichier écrit juste avant de quitter normalement (lu par le test session_pilote_et_ponte). */
#define FICHIER_FIN L"C:\\faux-gestionnaire-fin.txt"
/* Attente maximale de la fin de l'installeur lancé : 120 s, largement assez pour que le
 * pilote (un passage toutes les 2 s) le ferme, sans bloquer le test pour toujours s'il ne le
 * fait pas. */
#define ATTENTE_INSTALLEUR_MS 120000
/* Attente après la fin de l'installeur, avant de quitter : comme un vrai gestionnaire qui
 * reste ouvert après une installation, la session le voit encore ouvert pendant ce temps. */
#define ATTENTE_FINALE_MS 10000

/* Ajoute `ligne` (convertie en UTF-8) et un saut de ligne au fichier `fichier`, créé s'il
 * manque. Rend 0 si tout est écrit, 1 sinon. */
static int consigner(const wchar_t *fichier, const wchar_t *ligne)
{
    char texte[4096];
    int taille = WideCharToMultiByte(CP_UTF8, 0, ligne, -1, texte, sizeof texte - 1, NULL, NULL);
    if (taille <= 0)
        return 1;
    texte[taille - 1] = '\n'; /* remplace le zéro final, compté dans `taille` */
    HANDLE f = CreateFileW(fichier, FILE_APPEND_DATA, FILE_SHARE_READ, NULL,
                           OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
    if (f == INVALID_HANDLE_VALUE)
        return 1;
    DWORD ecrits = 0;
    BOOL ok = WriteFile(f, texte, (DWORD)taille, &ecrits, NULL);
    CloseHandle(f);
    return ok && ecrits == (DWORD)taille ? 0 : 1;
}

/* Lance `installeur` sans argument, attend sa fin (bornée), puis ATTENTE_FINALE_MS, et
 * écrit FICHIER_FIN. Rend 0, ou 1 si le lancement ou l'écriture échoue. */
static int lancer_installeur(const wchar_t *installeur)
{
    /* Ligne de commande « "<chemin>" » : le chemin complet, entre guillemets (il peut
     * contenir des espaces), est ce que le pilote cherche dans les arguments du processus.
     * CreateProcessW peut modifier ce tampon : il ne doit pas être une constante. */
    wchar_t ligne[MAX_PATH + 3];
    if (wcslen(installeur) > MAX_PATH)
        return 1;
    swprintf(ligne, sizeof ligne / sizeof ligne[0], L"\"%ls\"", installeur);
    STARTUPINFOW demarrage = {.cb = sizeof demarrage};
    PROCESS_INFORMATION processus;
    if (!CreateProcessW(installeur, ligne, NULL, NULL, FALSE, 0, NULL, NULL, &demarrage,
                        &processus))
        return 1;
    CloseHandle(processus.hThread);
    WaitForSingleObject(processus.hProcess, ATTENTE_INSTALLEUR_MS);
    CloseHandle(processus.hProcess);
    Sleep(ATTENTE_FINALE_MS);
    return consigner(FICHIER_FIN, L"fin");
}

int wmain(int argc, wchar_t **argv)
{
    for (int i = 1; i < argc; i++)
        if (wcsstr(argv[i], L"://"))
            return consigner(FICHIER_LIENS, argv[i]);
    if (argc < 2)
        return 2; /* rien à faire : ni lien, ni installeur */
    return lancer_installeur(argv[1]);
}
