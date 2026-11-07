//! Lancement de commandes externes (wine, yabridgectl, installeurs…) avec délai maximal.
//!
//! Tous les programmes externes passent par [`lancer`] : sortie capturée pour les
//! règles d'erreurs, et garantie qu'un installeur bloqué ne bloque pas WiMao.
//! Utilisé par les modules d'installation, de pontage et de vérification.
//! Écrit dans le [`journal`] chaque commande lancée. Contient aussi [`taille_lisible`], partagée
//! par la CLI et l'interface.

use crate::erreur::{Erreur, Resultat};
use crate::i18n::{langue_courante, tr, trf};
use crate::journal;
use std::io::Read;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Intervalle de vérification de la fin du processus.
const PAS_ATTENTE: Duration = Duration::from_millis(20);
/// Temps laissé au fil de lecture pour finir après la fin du processus. Un petit-enfant
/// détaché qui garde le tuyau ouvert ne doit pas bloquer WiMao indéfiniment.
const DELAI_LECTURE: Duration = Duration::from_secs(2);

/// Résultat d'une commande terminée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sortie {
    /// Code de sortie ; `128 + n` si le processus a été tué par le signal `n`.
    pub code: i32,
    /// Sortie standard et d'erreur mélangées dans l'ordre d'écriture, conservées
    /// pour que les règles d'erreurs puissent y chercher des motifs.
    pub texte: String,
}

/// Lance `cmd` et attend sa fin, au plus `delai`.
///
/// Si le journal est initialisé ([`journal::initialiser`]), la commande, son code
/// et les dernières lignes de sa sortie y sont ajoutés ; une panne du journal n'a
/// jamais d'effet sur le résultat.
///
/// Un code de sortie non nul n'est **pas** une erreur ici : l'appelant décide
/// (certains installeurs renvoient un code non nul mais ont réussi).
///
/// Le processus devient chef de son propre groupe (`process_group(0)`) : au délai
/// on tue le groupe entier, pour qu'aucun petit-enfant (par exemple le `wineserver`
/// ou un installeur lancé par un script) ne survive.
///
/// Erreurs : [`Erreur::Delai`] si le délai est dépassé ; [`Erreur::Introuvable`]
/// (message traduit qui nomme le programme) s'il n'existe pas ; [`Erreur::Io`] s'il
/// ne peut pas être lancé pour une autre raison (droits…).
///
/// ```no_run
/// use std::{process::Command, time::Duration};
/// use wimao_core::lancer;
/// let sortie = lancer(Command::new("echo").arg("salut"), Duration::from_secs(5)).unwrap();
/// assert_eq!(sortie.code, 0);
/// ```
pub fn lancer(cmd: &mut Command, delai: Duration) -> Resultat<Sortie> {
    let commande = journal::ligne_de_commande(cmd);
    let resultat = lancer_sans_journal(cmd, delai);
    // Jamais d'erreur renvoyée par le journal : voir son module.
    match &resultat {
        Ok(sortie) => {
            journal::enregistrer(&commande, &journal::Fin::Code(sortie.code), &sortie.texte)
        }
        Err(e) => journal::enregistrer(&commande, &journal::Fin::Echec(e.to_string()), ""),
    }
    resultat
}

/// Corps de [`lancer`], sans écriture dans le journal.
fn lancer_sans_journal(cmd: &mut Command, delai: Duration) -> Resultat<Sortie> {
    let programme = cmd.get_program().to_string_lossy().into_owned();
    // Un seul tuyau pour stdout et stderr : l'ordre des lignes est conservé.
    let (lecteur, ecrivain) = std::io::pipe()?;
    cmd.stdin(Stdio::null())
        .stdout(ecrivain.try_clone()?)
        .stderr(ecrivain)
        .process_group(0);
    let mut enfant = cmd
        .spawn()
        .map_err(|e| erreur_de_lancement(e, &programme, cmd.get_current_dir()))?;
    // `cmd` garde sinon l'extrémité d'écriture ouverte et la lecture ne finirait jamais.
    cmd.stdout(Stdio::null()).stderr(Stdio::null());

    let (tampon, fin_lecture) = lire_en_fond(lecteur);
    let statut = attendre(&mut enfant, delai, &programme)?;
    // Peu importe si la lecture n'a pas fini dans le temps : on garde ce qui est lu.
    let _ = fin_lecture.recv_timeout(DELAI_LECTURE);
    let octets = tampon.lock().map(|t| t.clone()).unwrap_or_default();
    let code = statut
        .code()
        .or_else(|| statut.signal().map(|s| 128 + s))
        .unwrap_or(-1);
    Ok(Sortie {
        code,
        texte: String::from_utf8_lossy(&octets).into_owned(),
    })
}

/// Erreur d'un lancement raté : un programme absent devient [`Erreur::Introuvable`]
/// qui le nomme (au lieu d'un « No such file or directory (os error 2) » muet sur le
/// programme en cause) ; les autres causes restent des [`Erreur::Io`].
///
/// Si la commande a un dossier de travail (`dossier`) qui n'existe pas, c'est lui le
/// coupable : `spawn` renvoie alors la même erreur « introuvable » que pour un programme
/// absent, et accuser le programme enverrait chercher au mauvais endroit.
fn erreur_de_lancement(e: std::io::Error, programme: &str, dossier: Option<&Path>) -> Erreur {
    if e.kind() != std::io::ErrorKind::NotFound {
        return Erreur::Io(e);
    }
    match dossier.filter(|d| !d.exists()) {
        Some(d) => Erreur::Introuvable(trf(
            "the working folder {folder}",
            &[("folder", &d.display().to_string())],
        )),
        None => Erreur::Introuvable(trf(
            "the program {program} (is it installed?)",
            &[("program", programme)],
        )),
    }
}

/// Lit le tuyau dans un fil séparé (sinon un processus bavard remplirait le tuyau
/// et se bloquerait). Renvoie le tampon partagé et un canal signalant la fin.
fn lire_en_fond(
    mut lecteur: impl Read + Send + 'static,
) -> (Arc<Mutex<Vec<u8>>>, mpsc::Receiver<()>) {
    let tampon = Arc::new(Mutex::new(Vec::new()));
    let (envoi, reception) = mpsc::channel();
    let tampon_fil = Arc::clone(&tampon);
    std::thread::spawn(move || {
        let mut bloc = [0u8; 8192];
        while let Ok(n) = lecteur.read(&mut bloc) {
            if n == 0 {
                break;
            }
            if let Ok(mut t) = tampon_fil.lock() {
                t.extend_from_slice(&bloc[..n]);
            }
        }
        let _ = envoi.send(());
    });
    (tampon, reception)
}

/// Attend la fin de `enfant` ; au délai, tue tout son groupe de processus.
fn attendre(
    enfant: &mut std::process::Child,
    delai: Duration,
    programme: &str,
) -> Resultat<std::process::ExitStatus> {
    let echeance = Instant::now() + delai;
    loop {
        if let Some(statut) = enfant.try_wait()? {
            return Ok(statut);
        }
        if Instant::now() >= echeance {
            tuer_le_groupe(enfant);
            return Err(Erreur::Delai {
                programme: programme.to_string(),
            });
        }
        std::thread::sleep(PAS_ATTENTE);
    }
}

/// Envoie SIGKILL à tout le groupe de l'enfant puis récupère l'enfant (évite un zombie).
fn tuer_le_groupe(enfant: &mut std::process::Child) {
    // `process_group(0)` a donné à l'enfant un groupe dont l'identifiant est son pid.
    let pgid = enfant.id() as libc::pid_t;
    // SAFETY: `kill` ne touche pas à la mémoire ; `-pgid` vise le groupe que nous
    // venons de créer, et un échec (groupe déjà fini) est sans conséquence.
    unsafe {
        libc::kill(-pgid, libc::SIGKILL);
    }
    let _ = enfant.wait();
}

/// Heure locale actuelle (`localtime_r` de la libc, déjà une dépendance : pas de
/// crate de dates en plus). Partagée par le journal et les noms de sauvegarde.
pub(crate) fn heure_locale() -> std::io::Result<libc::tm> {
    // SAFETY: `time` accepte un pointeur nul ; `localtime_r` écrit seulement dans
    // `tm` (structure C, tout à zéro est valide) et lit `maintenant`.
    unsafe {
        let maintenant = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&maintenant, &mut tm).is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(tm)
    }
}

/// Date locale d'un jour voisin d'aujourd'hui (`jours` = -1 pour hier, 0 pour aujourd'hui).
///
/// On ne retranche pas 86 400 secondes : un jour de changement d'heure dure 23 ou 25 h et
/// l'on tomberait parfois sur le mauvais jour. On recule plutôt le jour du mois, on fixe
/// midi (loin de toute bascule) et `mktime` normalise (1er du mois, changement d'année…).
pub(crate) fn jour_local_decale(jours: i32) -> std::io::Result<libc::tm> {
    let mut tm = heure_locale()?;
    tm.tm_mday += jours;
    tm.tm_hour = 12;
    tm.tm_min = 0;
    tm.tm_sec = 0;
    // -1 : laisser la libc décider de l'heure d'été à cette date.
    tm.tm_isdst = -1;
    // SAFETY: `mktime` lit et normalise la structure C que nous lui prêtons.
    if unsafe { libc::mktime(&mut tm) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(tm)
}

/// Taille en unités binaires (base 1024), avec une décimale au-delà de l'octet, écrite dans la
/// langue courante : `114.3 GiB` en anglais, `114,3 Gio` en français (unités `o`, `Kio`, `Mio`,
/// `Gio`, `Tio`), `114,3 GiB` en espagnol. Chaque unité est un msgid (`{value} GiB`…) et la
/// virgule décimale est posée pour le français et l'espagnol.
///
/// Partagée par la CLI (aperçu de l'import) et l'interface (assistant de premier lancement).
pub fn taille_lisible(octets: u64) -> String {
    if octets < 1024 {
        return trf("{value} B", &[("value", &octets.to_string())]);
    }
    let mut valeur = octets as f64;
    let mut rang = 0;
    while valeur >= 1024.0 && rang < 4 {
        valeur /= 1024.0;
        rang += 1;
    }
    let mut texte = format!("{valeur:.1}");
    // Juste sous une puissance de 1024 (1 048 575 octets), l'arrondi à une décimale rend
    // « 1024.0 » : on passe alors à l'unité suivante, « 1.0 MiB » (sauf à la dernière unité).
    if texte == "1024.0" && rang < 4 {
        valeur /= 1024.0;
        rang += 1;
        texte = format!("{valeur:.1}");
    }
    if matches!(langue_courante().as_str(), "fr" | "es") {
        texte = texte.replace('.', ",");
    }
    // Les msgids sont écrits en toutes lettres : `xgettext` ne lit que des chaînes littérales.
    let modele = match rang {
        1 => tr("{value} KiB"),
        2 => tr("{value} MiB"),
        3 => tr("{value} GiB"),
        _ => tr("{value} TiB"),
    };
    modele.replace("{value}", &texte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Erreur;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    #[serial_test::serial]
    fn les_tailles_sont_lisibles_dans_chaque_langue() {
        let cas: [(u64, [&str; 3]); 10] = [
            (0, ["0 B", "0 o", "0 B"]),
            (1023, ["1023 B", "1023 o", "1023 B"]),
            (1024, ["1.0 KiB", "1,0 Kio", "1,0 KiB"]),
            (1536, ["1.5 KiB", "1,5 Kio", "1,5 KiB"]),
            (5 * 1024 * 1024, ["5.0 MiB", "5,0 Mio", "5,0 MiB"]),
            (
                123 * 1024 * 1024 * 1024,
                ["123.0 GiB", "123,0 Gio", "123,0 GiB"],
            ),
            (
                u64::MAX,
                ["16777216.0 TiB", "16777216,0 Tio", "16777216,0 TiB"],
            ),
            (122_700_000_000, ["114.3 GiB", "114,3 Gio", "114,3 GiB"]),
            // L'arrondi à une décimale donnerait « 1024.0 » : on passe à l'unité suivante.
            (1_048_575, ["1.0 MiB", "1,0 Mio", "1,0 MiB"]),
            (1_073_741_823, ["1.0 GiB", "1,0 Gio", "1,0 GiB"]),
        ];
        for (rang, langue) in ["en", "fr", "es"].into_iter().enumerate() {
            crate::i18n::init_langue(Some(langue));
            for (octets, attendu) in &cas {
                assert_eq!(taille_lisible(*octets), attendu[rang], "{langue} {octets}");
            }
        }
        crate::i18n::init_langue(Some("en"));
    }

    #[test]
    fn dossier_de_travail_absent_est_nomme() {
        let d = tempfile::tempdir().unwrap();
        let absent = d.path().join("dossier-disparu");
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "true"]).current_dir(&absent);
        match lancer(&mut cmd, Duration::from_secs(5)) {
            Err(Erreur::Introuvable(texte)) => {
                assert!(texte.contains(absent.to_str().unwrap()), "{texte}");
            }
            autre => panic!("Introuvable attendu, obtenu {autre:?}"),
        }
    }

    #[test]
    fn code_et_sorties_melangees() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo a; echo b >&2; exit 3"]);
        let s = lancer(&mut cmd, Duration::from_secs(5)).unwrap();
        assert_eq!(s.code, 3);
        assert!(s.texte.contains('a') && s.texte.contains('b'));
    }

    #[test]
    fn delai_depasse_tue_le_processus() {
        let debut = Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let r = lancer(&mut cmd, Duration::from_secs(1));
        assert!(matches!(r, Err(Erreur::Delai { .. })));
        assert!(debut.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn delai_tue_aussi_les_petits_enfants() {
        // Marqueur unique : 31337 secondes, pour ne pas confondre avec d'autres `sleep`.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 31337 & sleep 31337"]);
        let r = lancer(&mut cmd, Duration::from_secs(1));
        assert!(matches!(r, Err(Erreur::Delai { .. })));
        std::thread::sleep(Duration::from_millis(300));
        let restants = Command::new("pgrep")
            .args(["-f", "sleep 31337"])
            .output()
            .unwrap();
        assert!(restants.stdout.is_empty(), "un sleep a survécu");
    }

    #[test]
    fn programme_inexistant_donne_une_erreur_qui_le_nomme() {
        let mut cmd = Command::new("programme-qui-n-existe-pas");
        match lancer(&mut cmd, Duration::from_secs(1)) {
            Err(Erreur::Introuvable(texte)) => {
                assert!(texte.contains("programme-qui-n-existe-pas"), "{texte}")
            }
            autre => panic!("attendu Introuvable, obtenu {autre:?}"),
        }
    }
}
