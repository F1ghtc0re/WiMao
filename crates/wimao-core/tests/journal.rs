//! Test d'intégration du journal global : processus de test à part, pour que
//! l'initialisation (unique par processus) ne touche pas les autres tests.

use std::process::Command;
use std::time::Duration;
use wimao_core::{journal, lancer};

#[test]
fn lancer_ecrit_dans_le_journal_une_fois_initialise() {
    let d = tempfile::tempdir().unwrap();
    let fichier = d.path().join("journal.log");
    journal::initialiser(fichier.clone());
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "echo bonjour-journal; exit 4"]);
    let s = lancer(&mut cmd, Duration::from_secs(5)).unwrap();
    assert_eq!(s.code, 4);
    let t = std::fs::read_to_string(&fichier).unwrap();
    assert!(
        t.contains("bonjour-journal") && t.contains("code: 4"),
        "{t}"
    );
    assert_eq!(journal::fichier_actif(), Some(fichier.as_path()));
}
