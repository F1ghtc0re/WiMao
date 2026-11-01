//! Règles d'erreurs : transforment le texte brut d'une erreur de Wine ou d'un installeur
//! en un message compréhensible, avec la réparation à proposer.
//!
//! Les règles vivent dans `ressources/regles.toml` (embarqué par `include_str!`) :
//! `motif` (expression régulière), `message` (msgid anglais) et `reparation` (nom d'un
//! [`Probleme`], facultatif). La première règle dont le motif correspond gagne.
//!
//! Utilise [`crate::i18n`] pour traduire et [`crate::diagnostic::Probleme`] pour la
//! réparation. Utilisé par l'interface et la CLI (`expliquer` sur la sortie d'un installeur
//! ou d'un plugin). Pour ajouter une règle : `CONTRIBUTING.md`, « Ajouter une règle d'erreur ».
//!
//! # Pourquoi `MESSAGES` en plus du fichier TOML
//!
//! `xgettext` ne lit que le code Rust. Chaque `message` du TOML est donc recopié ici avec
//! [`tr_noop`], ce qui le fait entrer dans `po/wimao.pot` ; le test
//! `chaque_message_du_toml_est_declare` échoue si on oublie la recopie.

use crate::diagnostic::Probleme;
use crate::i18n::{tr, tr_noop};
use regex::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

/// Le fichier des règles, compilé dans le programme.
const REGLES_TOML: &str = include_str!("../ressources/regles.toml");

/// Les messages de `regles.toml`, déclarés pour l'extraction des traductions.
/// Chaque texte doit être identique à celui du fichier TOML.
///
/// Lue seulement par les tests ; sa raison d'être est d'être lue par `xgettext`.
#[allow(dead_code)]
const MESSAGES: [&str; 6] = [
    tr_noop("A Microsoft C++ library (msvcp140) is missing: use Repair to install it"),
    tr_noop("Wine Mono (.NET support) is missing: use Repair to install it"),
    tr_noop("The graphics libraries (DXVK) are missing or broken: use Repair to reinstall them"),
    tr_noop("The WiX installer failed, often because a .NET component is missing"),
    tr_noop("Native Access has no download folder: use Repair"),
    tr_noop("A sign-in link could not reach the manager: open it again from the browser"),
];

/// Forme d'une règle dans le fichier TOML.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegleBrute {
    /// Expression régulière cherchée dans le texte d'erreur.
    motif: String,
    /// Message en anglais (msgid).
    message: String,
    /// Nom du [`Probleme`] à réparer, s'il y en a un.
    reparation: Option<String>,
}

/// Contenu du fichier TOML.
#[derive(Debug, Deserialize)]
struct FichierRegles {
    /// Les règles, dans l'ordre d'essai.
    #[serde(default)]
    regle: Vec<RegleBrute>,
}

/// Une règle prête à servir.
#[derive(Debug)]
struct Regle {
    /// Motif compilé.
    motif: Regex,
    /// Message anglais, à traduire avec [`tr`].
    message: String,
    /// Problème réparable associé.
    reparation: Option<Probleme>,
}

/// Compile les règles d'un texte TOML. Erreur : description lisible du premier défaut
/// (TOML invalide, regex invalide, nom de problème inconnu).
fn charger(texte: &str) -> Result<Vec<Regle>, String> {
    let fichier: FichierRegles = toml::from_str(texte).map_err(|e| e.to_string())?;
    fichier.regle.into_iter().map(compiler).collect()
}

/// Compile une règle brute.
fn compiler(brute: RegleBrute) -> Result<Regle, String> {
    let motif = Regex::new(&brute.motif).map_err(|e| format!("motif « {} » : {e}", brute.motif))?;
    let reparation =
        match brute.reparation {
            None => None,
            Some(nom) => Some(Probleme::depuis_nom(&nom).ok_or_else(|| {
                format!("réparation inconnue « {nom} » (règle « {} »)", brute.motif)
            })?),
        };
    Ok(Regle {
        motif,
        message: brute.message,
        reparation,
    })
}

/// Règles embarquées, compilées une seule fois. Un fichier invalide est un défaut du
/// programme (le test `les_regles_embarquees_sont_valides` l'attrape) : en production
/// on ne plante pas, on n'explique simplement rien.
fn regles() -> &'static [Regle] {
    static REGLES: OnceLock<Vec<Regle>> = OnceLock::new();
    REGLES.get_or_init(|| charger(REGLES_TOML).unwrap_or_default())
}

/// Explique un texte d'erreur : le message traduit de la première règle qui correspond
/// et le problème à réparer (s'il y en a un). `None` si aucune règle ne reconnaît le texte.
///
/// ```
/// # use wimao_core::regles::expliquer;
/// let texte = "err:module:import_dll Library MSVCP140.dll (needed by x.dll) not found";
/// assert!(expliquer(texte).is_some());
/// assert!(expliquer("tout va bien").is_none());
/// ```
pub fn expliquer(texte: &str) -> Option<(String, Option<Probleme>)> {
    regles()
        .iter()
        .find(|r| r.motif.is_match(texte))
        .map(|r| (tr(&r.message), r.reparation.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn les_regles_embarquees_sont_valides() {
        let regles = charger(REGLES_TOML).unwrap();
        assert!(regles.len() >= 5);
    }

    #[test]
    #[serial]
    fn msvcp_absente_est_expliquee_et_reparable() {
        crate::i18n::init_langue(Some("fr"));
        let texte =
            "err:module:import_dll Library MSVCP140.dll (needed by \"C:\\\\x.dll\") not found";
        let (message, probleme) = expliquer(texte).unwrap();
        assert_eq!(probleme, Some(Probleme::Msvcp140Absent));
        assert!(
            message.contains("msvcp140") && message.contains("Réparer"),
            "{message}"
        );
        crate::i18n::init_langue(Some("en"));
    }

    #[test]
    fn texte_sans_motif_donne_none() {
        assert_eq!(expliquer("all good"), None);
    }

    #[test]
    fn chaque_regle_initiale_donne_son_probleme() {
        let cas = [
            ("VCRUNTIME140.dll not found", Probleme::Msvcp140Absent),
            (
                "unimplemented function msvcp140.dll.??4 aborting",
                Probleme::Msvcp140Absent,
            ),
            ("could not load mscoree.dll", Probleme::MonoAbsent),
            ("d3d11 device creation failed", Probleme::DxvkAbsent),
            ("DXGI_ERROR_UNSUPPORTED", Probleme::DxvkAbsent),
            (
                "Error 0x80070643 : Fatal error during installation",
                Probleme::MonoAbsent,
            ),
        ];
        for (texte, attendu) in cas {
            let (_, probleme) = expliquer(texte).unwrap_or_else(|| panic!("{texte}"));
            assert_eq!(probleme, Some(attendu), "{texte}");
        }
    }

    #[test]
    fn les_traces_d_un_mono_installe_ne_sont_pas_une_absence() {
        // Traces ordinaires d'un Wine Mono en place (relevées dans des journaux réels).
        for texte in [
            "err:mscoree:LoadLibraryShim error reading registry key for installroot",
            "fixme:mscoree:ICLRRuntimeInfo_GetDefaultStartupFlags (0x123): stub",
            "Microsoft .NET Framework 4.8 Setup",
        ] {
            let probleme = expliquer(texte).and_then(|(_, p)| p);
            assert_ne!(probleme, Some(Probleme::MonoAbsent), "{texte}");
        }
        // Mono vraiment absent : toujours reconnu.
        for texte in [
            "err:mscoree:CLRRuntimeInfo_GetRuntimeHost Wine Mono is not installed",
            "This application requires .NET Framework 4.0, which is not installed",
        ] {
            let probleme = expliquer(texte).and_then(|(_, p)| p);
            assert_eq!(probleme, Some(Probleme::MonoAbsent), "{texte}");
        }
    }

    #[test]
    #[serial]
    fn dossier_de_telechargement_de_native_access_est_explique() {
        crate::i18n::init_langue(Some("en"));
        let (message, probleme) = expliquer("ERROR_CODE_DOWNLOAD_DIR_MISSING").unwrap();
        assert!(message.contains("Native Access"), "{message}");
        // Pas de réparation propre : « Réparer » réapplique de toute façon les dossiers.
        assert_eq!(probleme, None);
    }

    #[test]
    #[serial]
    fn lien_de_connexion_perdu_est_explique() {
        crate::i18n::init_langue(Some("en"));
        let (message, probleme) = expliquer("Error: open EBADF").unwrap();
        assert!(message.contains("sign-in link"), "{message}");
        assert_eq!(probleme, None);
    }

    #[test]
    fn chaque_message_du_toml_est_declare() {
        let fichier: FichierRegles = toml::from_str(REGLES_TOML).unwrap();
        for regle in fichier.regle {
            assert!(
                MESSAGES.contains(&regle.message.as_str()),
                "message absent de MESSAGES (voir CONTRIBUTING.md) : {}",
                regle.message
            );
        }
    }

    #[test]
    fn reparation_inconnue_est_refusee() {
        let texte = "[[regle]]\nmotif = 'x'\nmessage = 'm'\nreparation = 'Inconnu'\n";
        assert!(charger(texte).unwrap_err().contains("Inconnu"));
        assert!(charger("[[regle]]\nmotif = '('\nmessage = 'm'\n").is_err());
    }
}
