//! Assistant de tests : écrire un script exécutable sans « Text file busy » (ETXTBSY).
//!
//! Écrire un fichier puis l'exécuter dans le même processus est fragile quand les tests
//! tournent en parallèle : un autre fil qui fait `fork` (lancement d'une commande) hérite
//! pour un instant du descripteur ouvert en écriture, et `exec` du fichier échoue alors avec
//! ETXTBSY. Ici le fichier cible n'est jamais ouvert en écriture par ce processus : le texte
//! est écrit dans `<cible>.source`, puis copié avec les droits 755 par `install`, un autre
//! processus. Réservé aux tests.

use std::path::Path;
use std::process::Command;

/// Écrit `texte` dans `chemin` (dossiers créés au besoin) avec les droits `755`.
pub(crate) fn ecrire_executable(chemin: &Path, texte: &str) {
    std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
    let mut nom = chemin.file_name().unwrap().to_os_string();
    nom.push(".source");
    let source = chemin.with_file_name(nom);
    std::fs::write(&source, texte).unwrap();
    let statut = Command::new("install")
        .args(["-m", "755"])
        .arg(&source)
        .arg(chemin)
        .status()
        .unwrap();
    assert!(statut.success(), "install a échoué pour {chemin:?}");
    std::fs::remove_file(&source).unwrap();
}
