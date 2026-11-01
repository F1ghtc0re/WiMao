//! Catalogue des recettes : lecture des dossiers et recherche par nom de fichier.
//!
//! Utilise [`crate::recette::charger`], la crate `glob` et [`wimao_core::Chemins`].
//! Utilisé par l'installation d'un plugin (`installation.rs`) et par les commandes
//! `installer` et `detecter` de `wimao-cli`, et par l'interface.
//! Une recette invalide ne bloque jamais les autres : son erreur est rendue à part.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use glob::{MatchOptions, Pattern};
use wimao_core::i18n::trf;
use wimao_core::{Chemins, Erreur};

use crate::recette::{charger, Recette};

/// Variable d'environnement qui remplace le dossier des recettes fournies.
const VAR_RECETTES: &str = "WIMAO_RECETTES";
/// Dossier des recettes fournies quand WiMao est installé par son paquet.
const RECETTES_PAQUET: &str = "/usr/share/wimao/recettes";
/// Dossier des recettes de l'utilisateur, sous les données de WiMao.
const SOUS_DOSSIER_UTILISATEUR: &str = "recettes";
/// Extension des fichiers de recette.
const EXTENSION: &str = "toml";

/// Les recettes chargées, dans l'ordre de priorité des dossiers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue(pub Vec<Recette>);

impl Catalogue {
    /// Charge tous les fichiers `.toml` des dossiers donnés (triés par nom, pour un
    /// résultat stable). Une recette invalide donne une erreur et n'empêche pas les autres.
    /// Un dossier absent est ignoré. À nom de recette égal, le premier dossier l'emporte :
    /// l'utilisateur peut ainsi remplacer une recette fournie.
    pub fn charger(dossiers: &[PathBuf]) -> (Catalogue, Vec<Erreur>) {
        let mut recettes: Vec<Recette> = Vec::new();
        let mut noms: HashSet<String> = HashSet::new();
        let mut erreurs = Vec::new();
        for dossier in dossiers {
            let (fichiers, mut erreurs_dossier) = fichiers_toml(dossier);
            erreurs.append(&mut erreurs_dossier);
            for fichier in fichiers {
                match charger(&fichier) {
                    Ok(r) if noms.insert(r.nom.clone()) => recettes.push(r),
                    Ok(_) => {} // doublon d'un dossier moins prioritaire
                    Err(e) => erreurs.push(e),
                }
            }
        }
        (Catalogue(recettes), erreurs)
    }

    /// Recette désignée par `nom` : son `nom` ou le nom de son fichier sans `.toml`
    /// (`native-access` pour `native-access.toml`), sans tenir compte des majuscules.
    /// Le nom de la recette l'emporte sur un nom de fichier identique d'une autre recette.
    pub fn trouver_par_nom(&self, nom: &str) -> Option<&Recette> {
        let nom = nom.trim().to_lowercase();
        let par_nom = self.0.iter().find(|r| r.nom.to_lowercase() == nom);
        par_nom.or_else(|| {
            self.0.iter().find(|r| {
                let racine = r.fichier.as_ref().and_then(|f| f.file_stem());
                racine.is_some_and(|f| f.to_string_lossy().to_lowercase() == nom)
            })
        })
    }

    /// Recette dont un motif `reconnaitre` correspond au nom du fichier (sans le dossier),
    /// sans tenir compte des majuscules. La première trouvée l'emporte.
    pub fn trouver(&self, fichier: &Path) -> Option<&Recette> {
        let nom = fichier.file_name()?.to_str()?;
        let options = MatchOptions {
            case_sensitive: false,
            ..MatchOptions::new()
        };
        self.0.iter().find(|r| {
            r.reconnaitre
                .iter()
                .any(|m| Pattern::new(m).is_ok_and(|p| p.matches_with(nom, options)))
        })
    }
}

/// Recettes du dépôt, dernier recours de développement : `recettes/` deux niveaux au-dessus de
/// cette crate. Absent des builds de paquet (cfg `wimao_paquet`) : `CARGO_MANIFEST_DIR` est un
/// chemin de la machine de fabrication et ne doit pas se retrouver dans le binaire.
#[cfg(not(wimao_paquet))]
const RECETTES_DEPOT: Option<&str> = Some(concat!(env!("CARGO_MANIFEST_DIR"), "/../../recettes"));
#[cfg(wimao_paquet)]
const RECETTES_DEPOT: Option<&str> = None;

/// Dossiers de recettes par ordre de priorité : `<données>/recettes`, puis
/// `WIMAO_RECETTES` ou le dossier du paquet, sinon `recettes/` du dépôt (développement
/// seulement : pas dans un paquet, où le dossier du paquet est utilisé même s'il est absent).
pub fn dossiers_recettes(chemins: &Chemins) -> Vec<PathBuf> {
    dossiers_depuis(
        chemins,
        |nom| std::env::var_os(nom),
        |p| p.is_dir(),
        RECETTES_DEPOT.map(Path::new),
    )
}

/// Cœur de [`dossiers_recettes`], avec l'environnement, le test d'existence et le repli de
/// développement injectés (les tests ne modifient pas l'environnement global et couvrent
/// ainsi aussi le comportement d'un paquet, où `depot` vaut `None`).
///
/// Sans `depot`, le dossier du paquet est renvoyé même s'il n'existe pas : un dossier absent
/// est ignoré sans erreur par [`Catalogue::charger`] (voir `fichiers_toml`).
fn dossiers_depuis(
    chemins: &Chemins,
    env: impl Fn(&str) -> Option<OsString>,
    existe: impl Fn(&Path) -> bool,
    depot: Option<&Path>,
) -> Vec<PathBuf> {
    let paquet = Path::new(RECETTES_PAQUET);
    let fournies = match env(VAR_RECETTES).filter(|v| !v.is_empty()) {
        Some(v) => PathBuf::from(v),
        None => match depot {
            Some(depot) if !existe(paquet) => depot.to_path_buf(),
            _ => paquet.to_path_buf(),
        },
    };
    vec![chemins.donnees.join(SOUS_DOSSIER_UTILISATEUR), fournies]
}

/// Fichiers `.toml` d'un dossier, triés, et les erreurs rencontrées.
///
/// Seul un dossier absent (`NotFound`) est ignoré sans bruit : c'est normal pour
/// `<données>/recettes` avant la première recette. Tout autre échec (droits refusés,
/// entrée illisible) est rendu avec le chemin concerné, sinon des recettes disparaîtraient
/// sans explication.
fn fichiers_toml(dossier: &Path) -> (Vec<PathBuf>, Vec<Erreur>) {
    let mut erreurs = Vec::new();
    let entrees = match std::fs::read_dir(dossier) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), erreurs),
        Err(e) => return (Vec::new(), vec![erreur_dossier(dossier, &e)]),
    };
    let mut fichiers = Vec::new();
    for entree in entrees {
        match entree {
            Ok(e) => fichiers.push(e.path()),
            Err(e) => erreurs.push(erreur_dossier(dossier, &e)),
        }
    }
    fichiers.retain(|p| p.is_file() && p.extension().is_some_and(|x| x == EXTENSION));
    fichiers.sort();
    (fichiers, erreurs)
}

/// Erreur de lecture d'un dossier de recettes, avec son chemin.
fn erreur_dossier(dossier: &Path, e: &std::io::Error) -> Erreur {
    Erreur::Recette {
        fichier: dossier.to_path_buf(),
        ligne: None,
        message: trf(
            "Cannot read the recipe: {error}",
            &[("error", &e.to_string())],
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALIDE: &str = include_str!("../tests/recettes/valide.toml");

    fn ecrire(dossier: &Path, nom: &str, texte: &str) {
        std::fs::write(dossier.join(nom), texte).unwrap();
    }

    #[test]
    fn trouver_sans_tenir_compte_de_la_casse() {
        let d = tempfile::tempdir().unwrap();
        ecrire(d.path(), "na.toml", VALIDE);
        let (cat, erreurs) = Catalogue::charger(&[d.path().to_path_buf()]);
        assert!(erreurs.is_empty());
        let trouvee = cat.trouver(Path::new("/tmp/native access installer.exe"));
        assert_eq!(trouvee.map(|r| r.nom.as_str()), Some("Native Access 2"));
        assert!(cat.trouver(Path::new("autre.exe")).is_none());
    }

    #[test]
    fn trouver_par_nom_accepte_le_nom_ou_le_fichier() {
        let d = tempfile::tempdir().unwrap();
        ecrire(d.path(), "native-access.toml", VALIDE);
        let (cat, _) = Catalogue::charger(&[d.path().to_path_buf()]);
        for nom in [
            "Native Access 2",
            "native access 2",
            "NATIVE-ACCESS",
            " native-access ",
        ] {
            let trouvee = cat.trouver_par_nom(nom);
            assert_eq!(
                trouvee.map(|r| r.nom.as_str()),
                Some("Native Access 2"),
                "{nom}"
            );
        }
        assert!(cat.trouver_par_nom("inconnu").is_none());
    }

    #[test]
    fn recette_invalide_n_empeche_pas_les_autres() {
        let d = tempfile::tempdir().unwrap();
        ecrire(d.path(), "bonne.toml", VALIDE);
        ecrire(
            d.path(),
            "mauvaise.toml",
            "nom = \"X\"\ntelechargerr = \"y\"\n",
        );
        ecrire(d.path(), "notes.txt", "pas une recette");
        let (cat, erreurs) = Catalogue::charger(&[d.path().to_path_buf()]);
        assert_eq!(cat.0.len(), 1);
        assert_eq!(erreurs.len(), 1);
    }

    #[test]
    fn premier_dossier_gagne_a_nom_egal() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        ecrire(
            a.path(),
            "x.toml",
            &VALIDE.replace("--no-sandbox", "--prioritaire"),
        );
        ecrire(b.path(), "x.toml", VALIDE);
        let (cat, _) = Catalogue::charger(&[a.path().to_path_buf(), b.path().to_path_buf()]);
        assert_eq!(cat.0.len(), 1);
        assert_eq!(
            cat.0[0].gestionnaire.as_ref().unwrap().arguments[0],
            "--prioritaire"
        );
    }

    #[test]
    fn dossier_illisible_signale() {
        use std::os::unix::fs::PermissionsExt;
        // Root ignore les droits : le test n'aurait aucun sens.
        if std::fs::read_dir("/root").is_ok() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let ferme = d.path().join("ferme");
        std::fs::create_dir(&ferme).unwrap();
        std::fs::set_permissions(&ferme, std::fs::Permissions::from_mode(0o000)).unwrap();
        let (cat, erreurs) = Catalogue::charger(std::slice::from_ref(&ferme));
        std::fs::set_permissions(&ferme, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(cat.0.is_empty());
        assert!(matches!(&erreurs[..], [Erreur::Recette { fichier, .. }] if *fichier == ferme));
    }

    #[test]
    fn dossier_absent_ignore() {
        let (cat, erreurs) = Catalogue::charger(&[PathBuf::from("/inexistant")]);
        assert!(cat.0.is_empty() && erreurs.is_empty());
    }

    #[test]
    fn dossiers_par_priorite() {
        let c = Chemins::pour_test(Path::new("/tmp/x"));
        let depot = Some(Path::new("/depot/recettes"));
        let avec_var = dossiers_depuis(&c, |_| Some("/ailleurs".into()), |_| true, depot);
        assert_eq!(
            avec_var,
            vec![c.donnees.join("recettes"), PathBuf::from("/ailleurs")]
        );
        let paquet = dossiers_depuis(&c, |_| None, |_| true, depot);
        assert_eq!(paquet[1], PathBuf::from(RECETTES_PAQUET));
        let developpement = dossiers_depuis(&c, |_| None, |_| false, depot);
        assert_eq!(developpement[1], PathBuf::from("/depot/recettes"));
    }

    /// Build de paquet (pas de dossier de dépôt compilé) : le dossier du paquet est utilisé
    /// même absent, et jamais un chemin de développement.
    #[test]
    fn dossiers_sans_repli_de_developpement() {
        let c = Chemins::pour_test(Path::new("/tmp/x"));
        for existe in [true, false] {
            let dossiers = dossiers_depuis(&c, |_| None, |_| existe, None);
            assert_eq!(dossiers[1], PathBuf::from(RECETTES_PAQUET));
        }
    }
}
