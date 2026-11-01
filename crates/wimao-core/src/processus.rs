//! Processus d'un environnement Wine : lecture de `/proc` et arrêt propre.
//!
//! Utilise : [`Chemins`] (le préfixe à observer), [`import::noms_du_prefixe`] (les noms de
//! l'environnement), `libc` (signaux).
//! Utilisé par : `restauration` (plugins en cours), `diagnostic` (`wineserver` actif,
//! programmes ouverts), `import` (programmes ouverts dans `~/.wine-mao`) et `wimao-recettes`
//! (pilote d'installeurs, sessions de gestionnaire, programmes à nommer avant « Réparer »).
//!
//! Règle de sécurité : un processus n'appartient à notre environnement que si sa variable
//! `WINEPREFIX` (lue dans `/proc/<pid>/environ`) vaut exactement **un des noms** de
//! l'environnement de WiMao ([`import::noms_du_prefixe`]) : `c.prefixe`, plus `~/.wine-mao`
//! tant que c'est le lien laissé par l'import. Ainsi, une application ouverte dans un autre
//! environnement Wine (par exemple un `~/.wine-mao` jamais importé, ou rendu par l'annulation)
//! n'est jamais vue, donc jamais arrêtée ; mais un plugin ponté avant l'import, qui tourne
//! sous l'ancien nom, est bien vu avant d'arrêter Wine.

use crate::chemins::Chemins;
use crate::erreur::{Erreur, Resultat};
use crate::import;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Dossier virtuel du système de fichiers des processus Linux.
const RACINE_PROC: &str = "/proc";
/// Début de la variable d'environnement qui désigne le préfixe d'un processus Wine.
const VARIABLE_PREFIXE: &str = "WINEPREFIX=";
/// Intervalle entre deux vérifications de la disparition d'un processus arrêté.
const INTERVALLE_ATTENTE: Duration = Duration::from_millis(100);
/// Numéro (à partir de 1, comme dans `man 5 proc`) du champ « état » de `/proc/<pid>/stat`,
/// le premier après le nom entre parenthèses.
const CHAMP_ETAT: usize = 3;
/// Numéro du champ `starttime` (heure de démarrage) de `/proc/<pid>/stat`.
const CHAMP_HEURE_DE_DEMARRAGE: usize = 22;
/// Extension (avec le point, en minuscules) d'un programme Windows.
const EXTENSION_PROGRAMME: &str = ".exe";

/// Programmes que Wine lance lui-même : ils ne sont pas des logiciels de l'utilisateur et
/// [`programmes_ouverts`] les masque.
pub const PROGRAMMES_SYSTEME_WINE: &[&str] = &[
    "services.exe",
    "winedevice.exe",
    "plugplay.exe",
    "explorer.exe",
    "rpcss.exe",
    "svchost.exe",
    "conhost.exe",
    "start.exe",
    "wineboot.exe",
    "tabtip.exe",
];

/// Un processus qui tourne dans un environnement Wine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessusWine {
    /// Numéro du processus Linux.
    pub pid: u32,
    /// Ligne de commande découpée sur les octets nuls de `/proc/<pid>/cmdline`
    /// (UTF-8, les octets invalides sont remplacés).
    pub arguments: Vec<String>,
}

impl ProcessusWine {
    /// Ligne de commande en un seul texte : les arguments joints par une espace.
    pub fn ligne(&self) -> String {
        self.arguments.join(" ")
    }

    /// Nom de fichier du premier argument qui finit par `.exe` (casse ignorée), pris après
    /// le dernier `\` ou `/` : `C:\X\Setup.exe` donne `Setup.exe`. `None` s'il n'y en a pas.
    pub fn nom_programme(&self) -> Option<String> {
        let argument = self
            .arguments
            .iter()
            .find(|a| a.to_lowercase().ends_with(EXTENSION_PROGRAMME))?;
        let nom = argument.rsplit(['\\', '/']).next().unwrap_or(argument);
        Some(nom.to_string())
    }
}

/// Liste les processus dont `WINEPREFIX` vaut un des noms de l'environnement de `c`
/// ([`import::noms_du_prefixe`]).
///
/// Erreur : [`crate::Erreur::Io`] si `/proc` est illisible.
pub fn processus_du_prefixe(c: &Chemins) -> Resultat<Vec<ProcessusWine>> {
    processus_du_prefixe_dans(Path::new(RACINE_PROC), &import::noms_du_prefixe(c))
}

/// Cœur de [`processus_du_prefixe`] avec la racine de `/proc` injectée (tests) : les
/// processus dont `WINEPREFIX` vaut exactement un des chemins de `noms`.
///
/// Ignorés sans erreur : les entrées non numériques (`self`, `uptime`…) et les processus
/// dont `cmdline` ou `environ` est illisible (terminé entre-temps, ou d'un autre
/// utilisateur, qui ne peut de toute façon pas utiliser notre préfixe).
pub(crate) fn processus_du_prefixe_dans(
    proc: &Path,
    noms: &[PathBuf],
) -> Resultat<Vec<ProcessusWine>> {
    let mut trouves = Vec::new();
    for entree in std::fs::read_dir(proc)? {
        let dossier = entree?.path();
        let Some(pid) = dossier
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(|nom| nom.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(arguments) = lire_si_sur_prefixe(&dossier, noms) {
            trouves.push(ProcessusWine { pid, arguments });
        }
    }
    Ok(trouves)
}

/// Arguments du processus `dossier` (`/proc/<pid>`) s'il tourne sur un des chemins de `noms`,
/// sinon `None`. Comparaison exacte des chemins (aucun `canonicalize`) : `noms` contient déjà
/// chaque nom légitime de l'environnement.
fn lire_si_sur_prefixe(dossier: &Path, noms: &[PathBuf]) -> Option<Vec<String>> {
    let environnement = std::fs::read(dossier.join("environ")).ok()?;
    let sur_le_prefixe = environnement
        .split(|&o| o == 0)
        .filter_map(|variable| variable.strip_prefix(VARIABLE_PREFIXE.as_bytes()))
        .any(|valeur| {
            noms.iter()
                .any(|nom| Path::new(OsStr::from_bytes(valeur)) == nom)
        });
    if !sur_le_prefixe {
        return None;
    }
    let commande = std::fs::read(dossier.join("cmdline")).ok()?;
    Some(decouper_arguments(&commande))
}

/// Découpe le contenu de `cmdline` (arguments terminés par NUL) en texte.
fn decouper_arguments(commande: &[u8]) -> Vec<String> {
    let sans_fin = commande.strip_suffix(&[0]).unwrap_or(commande);
    if sans_fin.is_empty() {
        return Vec::new();
    }
    sans_fin
        .split(|&o| o == 0)
        .map(|a| String::from_utf8_lossy(a).into_owned())
        .collect()
}

/// Noms des programmes Windows ouverts dans l'environnement (sous tous ses noms,
/// [`import::noms_du_prefixe`]) : sans doublon, triés, hors [`PROGRAMMES_SYSTEME_WINE`]
/// (comparés sans tenir compte de la casse).
///
/// Erreur : [`crate::Erreur::Io`] si `/proc` est illisible.
pub fn programmes_ouverts(c: &Chemins) -> Resultat<Vec<String>> {
    programmes_ouverts_selon(Path::new(RACINE_PROC), c)
}

/// [`programmes_ouverts`] avec la racine de `/proc` injectée (tests, `diagnostic`).
pub(crate) fn programmes_ouverts_selon(proc: &Path, c: &Chemins) -> Resultat<Vec<String>> {
    programmes_ouverts_dans(proc, &import::noms_du_prefixe(c))
}

/// Comme [`programmes_ouverts`], pour l'environnement Wine `prefixe` quel qu'il soit : sert à
/// l'import, qui doit nommer les programmes ouverts dans `~/.wine-mao` avant d'arrêter son
/// `wineserver` (`import::programmes_ouverts`).
///
/// Erreur : [`crate::Erreur::Io`] si `/proc` est illisible.
pub fn programmes_ouverts_sur(prefixe: &Path) -> Resultat<Vec<String>> {
    programmes_ouverts_dans(Path::new(RACINE_PROC), &[prefixe.to_path_buf()])
}

/// Cœur de [`programmes_ouverts`] : racine de `/proc` et noms de l'environnement donnés.
fn programmes_ouverts_dans(proc: &Path, prefixes: &[PathBuf]) -> Resultat<Vec<String>> {
    let mut noms: Vec<String> = processus_du_prefixe_dans(proc, prefixes)?
        .iter()
        .filter_map(ProcessusWine::nom_programme)
        .filter(|nom| {
            let minuscules = nom.to_lowercase();
            !PROGRAMMES_SYSTEME_WINE.contains(&minuscules.as_str())
        })
        .collect();
    noms.sort();
    noms.dedup();
    Ok(noms)
}

/// Arrête des processus : `SIGTERM`, attente jusqu'à `delai`, puis `SIGKILL` aux survivants.
///
/// Un processus déjà disparu n'est pas une erreur (il a pu finir entre la liste et l'arrêt).
/// Une erreur sur un pid (signal refusé) n'empêche **pas** d'arrêter les autres : tous sont
/// traités, puis la première erreur rencontrée est renvoyée.
///
/// Identité vérifiée avant `SIGKILL` : l'heure de démarrage du processus (`starttime`, champ
/// 22 de `/proc/<pid>/stat`) est relevée avant le `SIGTERM`. Si le processus est mort pendant
/// l'attente et que Linux a donné son numéro à un **autre** processus, l'heure diffère et le
/// nouveau venu n'est pas tué. Un pid dont l'heure ne se lit pas avant le `SIGTERM` (déjà
/// disparu) ne reçoit pas de `SIGKILL`.
///
/// Erreurs : [`crate::Erreur::Io`] si le signal est refusé (processus d'un autre
/// utilisateur) ; [`crate::Erreur::PidInvalide`] si un pid n'est pas un numéro de processus
/// valide (0 viserait tout un groupe).
pub fn arreter_processus(pids: &[u32], delai: Duration) -> Resultat<()> {
    arreter_processus_dans(Path::new(RACINE_PROC), pids, delai, &mut envoyer_signal)
}

/// Cœur de [`arreter_processus`], avec la racine de `/proc` et l'envoi des signaux injectés
/// (tests : un faux `/proc` où un pid change de processus pendant l'attente).
fn arreter_processus_dans(
    proc: &Path,
    pids: &[u32],
    delai: Duration,
    envoyer: &mut dyn FnMut(u32, libc::c_int) -> Resultat<()>,
) -> Resultat<()> {
    let mut premiere_erreur = None;
    // Heure de démarrage de chaque pid **avant** le premier signal : la carte d'identité.
    let identites: Vec<(u32, Option<u64>)> = pids
        .iter()
        .map(|&pid| (pid, heure_de_demarrage(proc, pid)))
        .collect();
    for &pid in pids {
        if let Err(e) = envoyer(pid, libc::SIGTERM) {
            premiere_erreur.get_or_insert(e);
        }
    }
    let limite = Instant::now() + delai;
    while Instant::now() < limite && pids.iter().any(|&p| existe(proc, p)) {
        std::thread::sleep(INTERVALLE_ATTENTE);
    }
    for (pid, avant) in identites {
        let meme_processus = avant.is_some() && heure_de_demarrage(proc, pid) == avant;
        if meme_processus && existe(proc, pid) {
            if let Err(e) = envoyer(pid, libc::SIGKILL) {
                premiere_erreur.get_or_insert(e);
            }
        }
    }
    premiere_erreur.map_or(Ok(()), Err)
}

/// Heure de démarrage du processus `pid` (champ 22 `starttime` de `/proc/<pid>/stat`, en tops
/// d'horloge depuis le démarrage de la machine) ; `None` si le processus n'existe pas ou si
/// le fichier est illisible. Deux processus qui portent le même numéro l'un après l'autre
/// n'ont pas la même heure : c'est ce qui les distingue.
fn heure_de_demarrage(proc: &Path, pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(proc.join(pid.to_string()).join("stat")).ok()?;
    // Le nom (champ 2) est entre parenthèses et peut contenir espaces et parenthèses : on
    // repart de la dernière « ) ». Le premier champ après elle est l'état (champ 3).
    let (_, reste) = stat.rsplit_once(')')?;
    reste
        .split_whitespace()
        .nth(CHAMP_HEURE_DE_DEMARRAGE - CHAMP_ETAT)?
        .parse()
        .ok()
}

/// Envoie `signal` à `pid` ; « processus inexistant » (`ESRCH`) est accepté.
fn envoyer_signal(pid: u32, signal: libc::c_int) -> Resultat<()> {
    // `kill(0, …)` et les valeurs négatives visent un groupe de processus : à refuser.
    let pid_linux = libc::pid_t::try_from(pid)
        .ok()
        .filter(|&p| p > 0)
        .ok_or(Erreur::PidInvalide(pid))?;
    // SAFETY : appel système sans pointeur ; les arguments sont validés ci-dessus.
    if unsafe { libc::kill(pid_linux, signal) } == 0 {
        return Ok(());
    }
    let erreur = std::io::Error::last_os_error();
    if erreur.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(erreur.into())
    }
}

/// Vrai si `pid` est encore un processus vivant. Un zombie (terminé, pas encore récolté
/// par son parent) compte comme disparu : il ne s'exécute plus et `SIGKILL` n'y change rien.
fn existe(proc: &Path, pid: u32) -> bool {
    match std::fs::read_to_string(proc.join(pid.to_string()).join("stat")) {
        // Format : « pid (nom) E ... » ; le nom peut contenir des espaces et des
        // parenthèses, donc on repart de la dernière « ) ».
        Ok(stat) => stat
            .rsplit_once(')')
            .and_then(|(_, reste)| reste.trim_start().chars().next())
            .is_none_or(|etat| etat != 'Z'),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `/proc` factice : un processus `pid` avec cette ligne de commande (arguments NUL).
    fn faux_proc(proc: &Path, pid: &str, cmdline: &str, prefixe: &Path) {
        let d = proc.join(pid);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), cmdline).unwrap();
        let env = format!("HOME=/h\0WINEPREFIX={}\0", prefixe.display());
        std::fs::write(d.join("environ"), env).unwrap();
    }

    #[test]
    fn processus_du_prefixe_lit_les_arguments() {
        let d = tempfile::tempdir().unwrap();
        let prefixe = Path::new("/donnees/prefixe");
        faux_proc(d.path(), "42", "C:\\X\\Setup.exe\0/S\0", prefixe);
        let liste = processus_du_prefixe_dans(d.path(), &[prefixe.to_path_buf()]).unwrap();
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].pid, 42);
        assert_eq!(liste[0].arguments, ["C:\\X\\Setup.exe", "/S"]);
        assert_eq!(liste[0].ligne(), "C:\\X\\Setup.exe /S");
        assert_eq!(liste[0].nom_programme(), Some("Setup.exe".to_string()));
    }

    #[test]
    fn processus_d_un_autre_prefixe_sont_ignores() {
        let d = tempfile::tempdir().unwrap();
        faux_proc(
            d.path(),
            "42",
            "C:\\X\\Setup.exe\0/S\0",
            Path::new("/autre"),
        );
        let liste =
            processus_du_prefixe_dans(d.path(), &[PathBuf::from("/donnees/prefixe")]).unwrap();
        assert!(liste.is_empty());
    }

    #[test]
    fn entrees_illisibles_ou_non_numeriques_sont_ignorees() {
        let d = tempfile::tempdir().unwrap();
        let prefixe = Path::new("/donnees/prefixe");
        std::os::unix::fs::symlink("42", d.path().join("self")).unwrap();
        std::fs::write(d.path().join("uptime"), "1").unwrap();
        std::fs::create_dir(d.path().join("12")).unwrap(); // ni cmdline ni environ
        assert!(
            processus_du_prefixe_dans(d.path(), &[prefixe.to_path_buf()])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn nom_programme_ignore_la_casse_et_les_separateurs() {
        let p = ProcessusWine {
            pid: 1,
            arguments: vec!["wine".into(), "/home/u/Setup.EXE".into()],
        };
        assert_eq!(p.nom_programme(), Some("Setup.EXE".to_string()));
        let sans = ProcessusWine {
            pid: 2,
            arguments: vec!["wineserver".into()],
        };
        assert_eq!(sans.nom_programme(), None);
    }

    #[test]
    fn programmes_ouverts_exclut_le_systeme_de_wine() {
        let d = tempfile::tempdir().unwrap();
        let prefixe = Path::new("/donnees/prefixe");
        faux_proc(
            d.path(),
            "1",
            "C:\\windows\\system32\\services.exe\0",
            prefixe,
        );
        faux_proc(d.path(), "2", "C:\\NA\\Native Access.exe\0", prefixe);
        faux_proc(d.path(), "3", "C:\\NA\\Native Access.exe\0--x\0", prefixe);
        assert_eq!(
            programmes_ouverts_dans(d.path(), &[prefixe.to_path_buf()]).unwrap(),
            ["Native Access.exe"]
        );
    }

    #[test]
    fn arreter_processus_termine_un_enfant() {
        let mut enfant = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = enfant.id();
        arreter_processus(&[pid], Duration::from_secs(2)).unwrap();
        enfant.wait().unwrap(); // récolte le zombie
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
    }

    #[test]
    fn arreter_un_processus_disparu_n_est_pas_une_erreur() {
        let mut enfant = std::process::Command::new("true").spawn().unwrap();
        let pid = enfant.id();
        enfant.wait().unwrap();
        arreter_processus(&[pid], Duration::from_millis(200)).unwrap();
        assert!(matches!(
            arreter_processus(&[0], Duration::from_millis(1)),
            Err(Erreur::PidInvalide(0))
        ));
    }

    /// Écrit un faux `/proc/<pid>/stat` (état `S`, heure de démarrage `debut`).
    fn faux_stat(proc: &Path, pid: u32, debut: u64) {
        let d = proc.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        // Champs 3 à 21 quelconques, puis le champ 22 (starttime).
        let milieu = vec!["0"; CHAMP_HEURE_DE_DEMARRAGE - CHAMP_ETAT - 1].join(" ");
        let texte = format!("{pid} (nom (bizarre)) S {milieu} {debut} 0 0\n");
        std::fs::write(d.join("stat"), texte).unwrap();
    }

    #[test]
    fn heure_de_demarrage_lue_apres_le_nom() {
        let d = tempfile::tempdir().unwrap();
        faux_stat(d.path(), 42, 123456);
        assert_eq!(heure_de_demarrage(d.path(), 42), Some(123456));
        assert_eq!(heure_de_demarrage(d.path(), 43), None);
        // Le vrai processus de test a une heure de démarrage lisible.
        assert!(heure_de_demarrage(Path::new(RACINE_PROC), std::process::id()).is_some());
    }

    #[test]
    fn un_pid_repris_par_un_autre_processus_ne_recoit_pas_sigkill() {
        let d = tempfile::tempdir().unwrap();
        faux_stat(d.path(), 42, 100);
        let mut signaux = Vec::new();
        let proc = d.path().to_path_buf();
        let mut envoyer = |pid: u32, signal: libc::c_int| -> Resultat<()> {
            signaux.push((pid, signal));
            if signal == libc::SIGTERM {
                // Le processus meurt et Linux donne son numéro à un nouveau processus.
                faux_stat(&proc, pid, 200);
            }
            Ok(())
        };
        arreter_processus_dans(d.path(), &[42], Duration::from_millis(50), &mut envoyer).unwrap();
        assert_eq!(
            signaux,
            [(42, libc::SIGTERM)],
            "pas de SIGKILL au nouveau venu"
        );
    }

    #[test]
    fn le_meme_processus_toujours_vivant_recoit_sigkill() {
        let d = tempfile::tempdir().unwrap();
        faux_stat(d.path(), 42, 100);
        let mut signaux = Vec::new();
        let mut envoyer = |pid: u32, signal: libc::c_int| -> Resultat<()> {
            signaux.push((pid, signal));
            Ok(())
        };
        arreter_processus_dans(d.path(), &[42], Duration::from_millis(50), &mut envoyer).unwrap();
        assert_eq!(signaux, [(42, libc::SIGTERM), (42, libc::SIGKILL)]);
    }

    #[test]
    fn une_erreur_de_signal_n_empeche_pas_d_arreter_les_autres() {
        let mut enfant = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = enfant.id();
        // Le pid 0 est refusé d'emblée : il précède l'enfant dans la liste.
        let resultat = arreter_processus(&[0, pid], Duration::from_secs(2));
        assert!(matches!(resultat, Err(Erreur::PidInvalide(0))));
        // Tué par un signal (pas de code de sortie), et non arrivé au bout de son `sleep`.
        assert_eq!(enfant.wait().unwrap().code(), None);
    }
}
