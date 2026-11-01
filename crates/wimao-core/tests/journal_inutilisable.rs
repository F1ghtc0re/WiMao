//! Un journal inutilisable ne doit jamais faire échouer `lancer` (processus de test à part :
//! l'initialisation du journal global est unique par processus).

use std::process::Command;
use std::time::Duration;
use wimao_core::{journal, lancer};

#[test]
fn lancer_reussit_meme_si_le_journal_est_inutilisable() {
    let d = tempfile::tempdir().unwrap();
    // Le « dossier » du journal est un fichier ordinaire : toute écriture échoue.
    let bloc = d.path().join("fichier");
    std::fs::write(&bloc, "x").unwrap();
    journal::initialiser(bloc.join("journal.log"));
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "echo ok; exit 3"]);
    for _ in 0..2 {
        let s = lancer(&mut cmd, Duration::from_secs(5)).expect("lancer ne doit pas échouer");
        assert_eq!(s.code, 3);
        assert!(s.texte.contains("ok"));
    }
}
