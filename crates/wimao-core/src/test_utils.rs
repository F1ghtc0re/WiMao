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

/// Met `dossier` en tête du `PATH` le temps du test et le rétablit à la fin.
///
/// Le `PATH` n'est jamais réduit à `dossier` seul : les tests qui tournent en parallèle ont
/// besoin des autres commandes (`install`, `sh`…). Pour simuler un `flatpak` absent ou en
/// échec, on y pose un faux `flatpak` qui échoue. À utiliser avec `#[serial]`.
pub(crate) struct PathTemporaire(Option<std::ffi::OsString>);

impl PathTemporaire {
    pub(crate) fn poser(dossier: &Path) -> Self {
        let ancien = std::env::var_os("PATH");
        let ancien_texte = ancien.as_deref().unwrap_or_default().to_string_lossy();
        std::env::set_var("PATH", format!("{}:{ancien_texte}", dossier.display()));
        PathTemporaire(ancien)
    }
}

impl Drop for PathTemporaire {
    fn drop(&mut self) {
        match &self.0 {
            Some(a) => std::env::set_var("PATH", a),
            None => std::env::remove_var("PATH"),
        }
    }
}

/// Écrit un faux `flatpak` (script shell) dans `dossier`.
pub(crate) fn faux_flatpak(dossier: &Path, corps: &str) {
    ecrire_executable(&dossier.join("flatpak"), &format!("#!/bin/sh\n{corps}\n"));
}
