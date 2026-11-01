//! Journal des commandes lancées par WiMao (`<données>/journal.log`).
//!
//! - Chaque appel de [`lancer`](crate::outils::lancer) y ajoute une entrée : heure,
//!   commande, code de sortie (ou cause de l'échec) et les 200 dernières lignes de
//!   sortie. C'est la matière première du [`rapport`](crate::rapport) de problème.
//! - Le journal est **global** : la CLI et l'interface appellent [`initialiser`] une
//!   fois au démarrage. Sans cet appel (tests, bibliothèque seule), rien n'est écrit.
//! - Au-delà de 5 Mo, le fichier est renommé `journal.1.log` (l'ancien `journal.1.log`
//!   est écrasé) et un nouveau journal commence : l'espace disque reste borné à ~10 Mo.
//!
//! # Une écriture du journal ne fait jamais échouer une commande
//!
//! C'est la **seule** erreur que WiMao choisit de ne pas propager : le journal sert à
//! comprendre un problème, il ne doit pas en créer un (disque plein, dossier en lecture
//! seule…) en faisant échouer une installation qui, sinon, aurait réussi. Une erreur
//! d'écriture est signalée **une seule fois** sur la sortie d'erreur, puis ignorée.
//!
//! Utilise `outils::heure_locale` ; utilisé par `outils::lancer` et `rapport`.

use crate::i18n::trf;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Taille à partir de laquelle le journal est renommé en `journal.1.log`.
pub const TAILLE_MAX: u64 = 5 * 1024 * 1024;
/// Nombre de lignes de sortie conservées par entrée.
pub const LIGNES_PAR_ENTREE: usize = 200;
/// Nom du journal précédent (après rotation), dans le dossier du journal.
const NOM_ROTATION: &str = "journal.1.log";

/// Journal global, posé une seule fois par [`initialiser`].
static JOURNAL: OnceLock<Journal> = OnceLock::new();

/// Comment une commande s'est terminée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fin {
    /// Le programme a tourné jusqu'au bout et rendu ce code.
    Code(i32),
    /// Le programme n'a pas pu être lancé ou a dépassé son délai (texte de l'erreur).
    Echec(String),
    /// Simple information, sans erreur (empreinte d'un téléchargement, bilan d'une session…).
    Note(String),
}

/// Un fichier journal, avec sa limite de taille et son verrou d'écriture.
pub(crate) struct Journal {
    fichier: PathBuf,
    limite: u64,
    /// Sérialise les écritures (plusieurs fils peuvent lancer des commandes).
    ecriture: Mutex<()>,
    /// Vrai une fois l'erreur d'écriture signalée : on ne la répète pas.
    signalee: AtomicBool,
}

/// Active le journal : les commandes lancées ensuite par `lancer` seront écrites dans
/// `fichier` (le dossier est créé au besoin). Appelé une fois par la CLI et par
/// l'interface ; un second appel est ignoré (le premier journal reste en place).
pub fn initialiser(fichier: PathBuf) {
    // `set` ne rend une erreur que si déjà initialisé : le premier gagne, sans conséquence.
    let _ = JOURNAL.set(Journal::nouveau(fichier, TAILLE_MAX));
}

/// Ajoute une entrée au journal global s'il est initialisé ; sans effet sinon.
pub(crate) fn enregistrer(commande: &str, fin: &Fin, sortie: &str) {
    if let Some(journal) = JOURNAL.get() {
        journal.ajouter(commande, fin, sortie);
    }
}

/// Note dans le journal un événement qui n'est pas une commande lancée (erreur de l'interface,
/// panique d'un fil de travail…) : `evenement` en titre, `detail` en cause. Sans effet si le
/// journal n'est pas initialisé. Comme tout le journal, ne renvoie jamais d'erreur.
pub fn noter(evenement: &str, detail: &str) {
    enregistrer(evenement, &Fin::Echec(detail.to_owned()), "");
}

/// Comme [`noter`], pour un événement qui **n'est pas** une erreur : la ligne commence par
/// « note: » au lieu de « échec: », pour que le lecteur du journal (ou d'un rapport de problème)
/// ne prenne pas une information (empreinte calculée, bilan, relance réussie) pour un échec.
pub fn noter_info(evenement: &str, detail: &str) {
    enregistrer(evenement, &Fin::Note(detail.to_owned()), "");
}

/// Fichier du journal global, s'il est initialisé (pour le rapport).
pub fn fichier_actif() -> Option<&'static Path> {
    JOURNAL.get().map(|j| j.fichier.as_path())
}

/// Commande lisible : programme et arguments séparés par des espaces.
pub(crate) fn ligne_de_commande(cmd: &Command) -> String {
    std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|a| a.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

impl Journal {
    /// Journal dans `fichier`, renommé quand il atteint `limite` octets.
    pub(crate) fn nouveau(fichier: PathBuf, limite: u64) -> Journal {
        Journal {
            fichier,
            limite,
            ecriture: Mutex::new(()),
            signalee: AtomicBool::new(false),
        }
    }

    /// Écrit une entrée ; une erreur est signalée une fois puis ignorée (voir l'en-tête).
    pub(crate) fn ajouter(&self, commande: &str, fin: &Fin, sortie: &str) {
        // Un fil mort pendant l'écriture ne doit pas empêcher les autres de journaliser.
        let _garde = self.ecriture.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = self.ecrire(&entree(commande, fin, sortie)) {
            if !self.signalee.swap(true, Ordering::Relaxed) {
                eprintln!(
                    "{}",
                    trf(
                        "Cannot write to the log {path}: {error} (further log errors are not shown)",
                        &[
                            ("path", &self.fichier.display().to_string()),
                            ("error", &e.to_string()),
                        ],
                    )
                );
            }
        }
    }

    /// Fait la rotation si besoin puis ajoute `texte` à la fin du fichier.
    fn ecrire(&self, texte: &str) -> std::io::Result<()> {
        if let Some(dossier) = self.fichier.parent() {
            std::fs::create_dir_all(dossier)?;
        }
        self.tourner_si_besoin()?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.fichier)?;
        f.write_all(texte.as_bytes())
    }

    /// Renomme le journal en `journal.1.log` s'il a atteint la limite.
    fn tourner_si_besoin(&self) -> std::io::Result<()> {
        match std::fs::metadata(&self.fichier) {
            Ok(m) if m.len() >= self.limite => {
                let ancien = self.fichier.with_file_name(NOM_ROTATION);
                std::fs::rename(&self.fichier, ancien)
            }
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

/// Texte d'une entrée : en-tête (heure, commande), code, puis les dernières lignes
/// de sortie indentées pour rester lisibles.
fn entree(commande: &str, fin: &Fin, sortie: &str) -> String {
    let mut t = format!("[{}] $ {commande}\n", horodatage());
    match fin {
        Fin::Code(c) => t.push_str(&format!("  code: {c}\n")),
        Fin::Echec(cause) => t.push_str(&format!("  échec: {cause}\n")),
        Fin::Note(texte) => t.push_str(&format!("  note: {texte}\n")),
    }
    let lignes: Vec<&str> = sortie.lines().collect();
    let debut = lignes.len().saturating_sub(LIGNES_PAR_ENTREE);
    for l in &lignes[debut..] {
        t.push_str("    ");
        t.push_str(l);
        t.push('\n');
    }
    t
}

/// `AAAA-MM-JJ HH:MM:SS` à l'heure locale (`?` si l'heure est illisible : le journal
/// ne doit pas échouer pour si peu).
fn horodatage() -> String {
    match crate::outils::heure_locale() {
        Ok(tm) => format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        ),
        Err(_) => "?".to_string(),
    }
}

/// Les `n` dernières lignes du fichier `chemin` ; vide s'il n'existe pas.
/// Erreurs d'entrée/sortie autres qu'« absent » remontées.
pub(crate) fn dernieres_lignes(chemin: &Path, n: usize) -> std::io::Result<Vec<String>> {
    let octets = match std::fs::read(chemin) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let texte = String::from_utf8_lossy(&octets);
    let lignes: Vec<&str> = texte.lines().collect();
    let debut = lignes.len().saturating_sub(n);
    Ok(lignes[debut..].iter().map(|l| l.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_note_n_est_pas_presentee_comme_un_echec() {
        let t = entree(
            "telechargement",
            &Fin::Note("https://x.org/a.exe sha256=ab".into()),
            "",
        );
        assert!(t.contains("  note: https://x.org/a.exe sha256=ab\n"), "{t}");
        assert!(!t.contains("échec"), "{t}");
    }

    #[test]
    fn une_entree_contient_commande_code_et_sortie() {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::nouveau(d.path().join("sous/journal.log"), TAILLE_MAX);
        j.ajouter("echo salut", &Fin::Code(3), "a\nb\n");
        let t = std::fs::read_to_string(d.path().join("sous/journal.log")).unwrap();
        assert!(t.contains("$ echo salut") && t.contains("code: 3") && t.contains("    b\n"));
    }

    #[test]
    fn seules_les_200_dernieres_lignes_de_sortie_sont_gardees() {
        let sortie: String = (0..300).map(|i| format!("ligne{i}\n")).collect();
        let t = entree("x", &Fin::Code(0), &sortie);
        assert!(!t.contains("ligne99\n") && t.contains("ligne100\n") && t.contains("ligne299\n"));
    }

    #[test]
    fn rotation_vers_journal_1_au_dela_de_la_limite() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("journal.log");
        let j = Journal::nouveau(f.clone(), 100);
        j.ajouter("premiere", &Fin::Code(0), &"x".repeat(200));
        j.ajouter("seconde", &Fin::Code(0), "");
        let ancien = std::fs::read_to_string(d.path().join("journal.1.log")).unwrap();
        let actuel = std::fs::read_to_string(&f).unwrap();
        assert!(ancien.contains("premiere") && !actuel.contains("premiere"));
        assert!(actuel.contains("seconde"));
    }

    #[test]
    fn une_erreur_d_ecriture_est_ignoree() {
        // Le « dossier » du journal est un fichier : l'écriture échoue, sans panique.
        let d = tempfile::tempdir().unwrap();
        let bloc = d.path().join("fichier");
        std::fs::write(&bloc, "x").unwrap();
        let j = Journal::nouveau(bloc.join("journal.log"), TAILLE_MAX);
        j.ajouter("a", &Fin::Code(0), "");
        j.ajouter("b", &Fin::Code(0), "");
        assert!(j.signalee.load(Ordering::Relaxed));
    }

    #[test]
    fn dernieres_lignes_d_un_fichier_absent_est_vide() {
        let d = tempfile::tempdir().unwrap();
        assert!(dernieres_lignes(&d.path().join("non"), 5)
            .unwrap()
            .is_empty());
    }
}
