//! Recettes WiMao : réglages connus par plugin.
//!
//! - [`recette`] : le format d'une recette (fichier TOML) et sa lecture validée ;
//! - [`catalogue`] : l'ensemble des recettes trouvées sur le disque et la recherche
//!   de la recette qui correspond à un installeur ;
//! - [`chemins_windows`] : variables des recettes et traduction des chemins Windows vers Linux ;
//! - [`detection`] : reconnaissance du type d'un installeur et options silencieuses ;
//! - [`archive`] : extraction d'une archive et tri de son contenu (installeur ou plugins) ;
//! - [`extraction`] : méthodes sans Wine `extraire-7z` et `squirrel` ;
//! - [`obtention`] : téléchargement vérifié de l'installeur d'une recette ;
//! - [`pilote`] : relance en silencieux les installeurs qu'un gestionnaire ouvre avec une interface ;
//! - [`gestionnaire`] : session d'un gestionnaire d'éditeur (ouverture, pilote, pontage, fermeture, liens) ;
//! - [`installation`] : parcours complet d'installation d'un plugin (orchestrateur) ;
//! - [`apres_installation`] : suite commune d'une installation (lien de connexion).
//!
//! Utilise `wimao-core` (erreurs, chemins, traductions, processus, liens) ; utilisée par
//! `wimao-cli` et `wimao`. Voir `ARCHITECTURE.md` et `docs/recettes.md`.
#![warn(missing_docs)]

pub mod apres_installation;
pub mod archive;
pub mod catalogue;
pub mod chemins_windows;
pub mod detection;
pub mod extraction;
pub mod gestionnaire;
pub mod installation;
pub mod obtention;
pub mod pilote;
pub mod recette;
#[cfg(test)]
mod test_utils;

pub use apres_installation::{declarer_lien_wimao, lien_non_declare, traiter_le_lien, SuiteLien};
pub use archive::{installer_archive, ContenuArchive};
pub use catalogue::{dossiers_recettes, Catalogue};
pub use chemins_windows::utilisateur_windows;
pub use detection::{arguments_silencieux, detecter, TypeInstalleur};
pub use gestionnaire::{
    ouvrir, relayer_lien, suite, texte_evenement, BilanSession, Commande, EvenementSession, Phase,
    Session, Suite, INTERVALLE_PILOTE,
};
pub use installation::{
    est_installe, installer, installer_recette, programmes_a_nommer_avant_reparation,
    reparer_gestionnaires, OptionsInstallation, RapportInstallation,
};
pub use pilote::IssueRelance;
pub use recette::{
    charger, consigne, Correctifs, Dependance, EntreeRegistre, EtapeExtraction, Gestionnaire,
    Installation, Methode, Recette, TypeRecette, CONSIGNE_ACTIVATION, CONSIGNE_CONNEXION,
};
