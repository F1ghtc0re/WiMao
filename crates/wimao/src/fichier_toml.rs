//! Lecture et écriture de petits fichiers TOML (réglages, mémoire des résultats).
//!
//! Partagé par `etat.rs` et `reglages.rs` pour ne dire qu'une fois comment on lit sans
//! planter et comment on écrit sans jamais laisser un fichier tronqué. Utilise `serde` et
//! `toml`. Utilisé par ces deux modules.

use std::io::ErrorKind;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;
use wimao_core::i18n::trf;
use wimao_core::Resultat;

/// Lit `fichier`. Absent : valeur par défaut, sans avertissement (premier démarrage).
/// Illisible ou mal formé : valeur par défaut **et** un avertissement traduit, à montrer
/// et à journaliser par l'appelant (le fichier n'est que de la mémoire d'affichage : le perdre
/// ne doit pas empêcher d'ouvrir la fenêtre, mais ne doit pas rester muet).
pub fn lire<T: DeserializeOwned + Default>(fichier: &Path) -> (T, Option<String>) {
    let texte = match std::fs::read_to_string(fichier) {
        Ok(texte) => texte,
        Err(e) if e.kind() == ErrorKind::NotFound => return (T::default(), None),
        Err(e) => return (T::default(), Some(avertissement(fichier, &e.to_string()))),
    };
    match toml::from_str(&texte) {
        Ok(valeur) => (valeur, None),
        Err(e) => (T::default(), Some(avertissement(fichier, &e.to_string()))),
    }
}

/// Phrase d'avertissement sur un fichier inutilisable.
fn avertissement(fichier: &Path, cause: &str) -> String {
    trf(
        "cannot use {path}: {cause}",
        &[("path", &fichier.display().to_string()), ("cause", cause)],
    )
}

/// Écrit `valeur` dans `fichier` (dossier créé au besoin), **atomiquement** : écriture dans un
/// fichier voisin `.tmp` puis renommage, pour qu'une coupure ne laisse jamais un fichier tronqué.
///
/// # Erreurs
/// [`wimao_core::Erreur::Io`] si le disque refuse l'écriture.
pub fn ecrire<T: Serialize>(fichier: &Path, valeur: &T) -> Resultat<()> {
    let texte = toml::to_string(valeur).map_err(std::io::Error::other)?;
    if let Some(dossier) = fichier.parent() {
        std::fs::create_dir_all(dossier)?;
    }
    let mut temporaire = fichier.as_os_str().to_owned();
    temporaire.push(".tmp");
    std::fs::write(&temporaire, texte)?;
    std::fs::rename(&temporaire, fichier)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    #[test]
    fn ecriture_puis_lecture_et_cas_d_erreur() {
        let t = TempDir::new().unwrap();
        let fichier = t.path().join("sous/dossier/a.toml");
        let mut valeur = BTreeMap::new();
        valeur.insert("k".to_owned(), "v".to_owned());
        ecrire(&fichier, &valeur).unwrap();
        let (relu, avertissement): (BTreeMap<String, String>, _) = lire(&fichier);
        assert_eq!(relu, valeur);
        assert!(avertissement.is_none());
        // Absent : défaut sans avertissement ; abîmé : défaut avec avertissement.
        let (vide, a): (BTreeMap<String, String>, _) = lire(&t.path().join("absent.toml"));
        assert!(vide.is_empty() && a.is_none());
        std::fs::write(&fichier, "pas du toml [").unwrap();
        let (vide, a): (BTreeMap<String, String>, _) = lire(&fichier);
        assert!(vide.is_empty() && a.is_some());
    }
}
