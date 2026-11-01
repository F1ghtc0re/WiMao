//! Assistants de tests : écrire un script exécutable sans « Text file busy » (ETXTBSY), et
//! fabriquer une archive malveillante (`zip_malveillant`, partagée par les tests d'`archive.rs`
//! et d'`extraction.rs`).
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

/// Zip minimal à la main (7z refuse de fabriquer un nom en `../`) : une entrée vide
/// `../evil.vst3`, stockée sans compression (taille 0, CRC 0).
pub(crate) fn zip_malveillant(chemin: &Path) {
    let nom = b"../evil.vst3";
    let mut z = Vec::new();
    let entete = |central: bool| {
        let mut e = Vec::new();
        e.extend_from_slice(if central {
            b"PK\x01\x02"
        } else {
            b"PK\x03\x04"
        });
        if central {
            e.extend_from_slice(&[20, 0]); // version créatrice
        }
        e.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // version, drapeaux, méthode, date
        e.extend_from_slice(&[0u8; 12]); // CRC, tailles
        e.extend_from_slice(&(nom.len() as u16).to_le_bytes());
        e.extend_from_slice(&[0, 0]); // champ extra
        if central {
            e.extend_from_slice(&[0u8; 12]); // commentaire, disque, attributs
            e.extend_from_slice(&[0, 0, 0, 0]); // position de l'entrée locale
        }
        e.extend_from_slice(nom);
        e
    };
    z.extend(entete(false));
    let debut_central = z.len() as u32;
    let central = entete(true);
    let taille_central = central.len() as u32;
    z.extend(central);
    z.extend_from_slice(b"PK\x05\x06\0\0\0\0\x01\0\x01\0");
    z.extend_from_slice(&taille_central.to_le_bytes());
    z.extend_from_slice(&debut_central.to_le_bytes());
    z.extend_from_slice(&[0, 0]);
    std::fs::write(chemin, z).unwrap();
}
