//! Suite commune d'une installation réussie : lien de connexion et consigne d'activation.
//!
//! Toutes les installations passent par ici, quel que soit leur point de départ : « Add an
//! installer » et « Install » (gestionnaire) dans l'interface, `wimao-cli installer` et
//! `wimao-cli gestionnaire installer`. Sans ce point commun, une recette au `lien-connexion`
//! installée par « Add an installer » ne déclarait jamais son lien.
//!
//! - [`traiter_le_lien`] : si la recette a un `lien-connexion`, applique la décision commune
//!   `wimao_core::liens::lien_demande_confirmation` : lien libre ou déjà à WiMao, il est
//!   déclaré tout de suite ; géré par un autre programme, l'appelant pose la question (dialogue
//!   ou « o/N ») puis appelle [`declarer_lien_wimao`]. Une erreur après une installation
//!   réussie n'est **jamais** une erreur de l'installation : elle devient un avertissement
//!   ([`lien_non_declare`]) ;
//! - la consigne `activation` est déjà dans le rapport
//!   ([`crate::RapportInstallation::consigne_activation`]) : chaque interface l'affiche.
//!
//! Utilise `wimao_core::liens` et [`crate::installation`]. Utilisé par `wimao-cli` et `wimao`.

use wimao_core::i18n::trf;
use wimao_core::liens::{commande_relais, declarer_lien, etat_lien, lien_demande_confirmation};
use wimao_core::{Chemins, Erreur, Resultat};

use crate::installation::RapportInstallation;

/// Suite à donner au lien de connexion après une installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuiteLien {
    /// Rien à faire : pas de `lien-connexion`, rien d'installé, ou erreur (alors signalée
    /// en avertissement par [`traiter_le_lien`]).
    Aucune,
    /// Le lien `schema://` est maintenant relayé à WiMao (il était libre, ou déjà à WiMao).
    Declare(String),
    /// Un autre programme (`actuel`, nom de son `.desktop`) gère `schema://` : l'appelant
    /// demande l'accord avant de le remplacer ([`declarer_lien_wimao`]).
    AConfirmer {
        /// Schéma du lien (`native-access`).
        schema: String,
        /// `.desktop` qui le gère aujourd'hui.
        actuel: String,
    },
}

/// S'occupe du lien de connexion de `rapport` (voir le module). Rend la suite et, si quelque
/// chose a échoué, l'avertissement à montrer (le lien n'est alors pas relayé, mais
/// l'installation reste réussie).
///
/// Rien n'est fait quand l'installation n'a rien lancé
/// ([`RapportInstallation::interactif_necessaire`]) : le produit n'est pas installé.
pub fn traiter_le_lien(c: &Chemins, rapport: &RapportInstallation) -> (SuiteLien, Option<String>) {
    let Some(schema) = rapport
        .lien_a_declarer
        .as_deref()
        .filter(|_| !rapport.interactif_necessaire)
    else {
        return (SuiteLien::Aucune, None);
    };
    match suite_du_lien(c, schema) {
        Ok(suite) => (suite, None),
        Err(erreur) => (SuiteLien::Aucune, Some(lien_non_declare(&erreur))),
    }
}

/// Regarde qui gère `schema://` et le déclare pour WiMao si personne d'autre ne le gère.
fn suite_du_lien(c: &Chemins, schema: &str) -> Resultat<SuiteLien> {
    match lien_demande_confirmation(&etat_lien(c, schema)?) {
        None => {
            declarer_lien_wimao(c, schema)?;
            Ok(SuiteLien::Declare(schema.to_owned()))
        }
        Some(actuel) => Ok(SuiteLien::AConfirmer {
            schema: schema.to_owned(),
            actuel,
        }),
    }
}

/// Déclare WiMao comme gestionnaire du lien `schema://` (le relais est `wimao-cli`).
///
/// Erreurs : celles de [`commande_relais`] et de [`declarer_lien`].
pub fn declarer_lien_wimao(c: &Chemins, schema: &str) -> Resultat<()> {
    declarer_lien(c, schema, &commande_relais()?)
}

/// Texte d'un lien qui n'a pas pu être déclaré après une installation réussie (CLI et
/// interface) : il dit que le produit est installé, pour qu'on ne refasse pas tout.
pub fn lien_non_declare(erreur: &Erreur) -> String {
    trf(
        "installed, but the sign-in link could not be declared: {error}",
        &[("error", &erreur.to_string())],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::TypeInstalleur;
    use std::collections::BTreeSet;

    fn rapport(lien: Option<&str>, interactif_necessaire: bool) -> RapportInstallation {
        RapportInstallation {
            point: None,
            recette: Some("Essai".into()),
            methode: TypeInstalleur::Inno,
            plugins: Vec::new(),
            mis_a_jour: BTreeSet::new(),
            interactif_necessaire,
            code_installeur: None,
            gestionnaire: true,
            lien_a_declarer: lien.map(str::to_owned),
            consigne_activation: None,
        }
    }

    #[test]
    fn sans_lien_ou_sans_installation_rien_n_est_fait() {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        assert_eq!(
            traiter_le_lien(&c, &rapport(None, false)),
            (SuiteLien::Aucune, None)
        );
        // Rien n'a été installé : le lien n'est pas déclaré (aucun outil lancé).
        assert_eq!(
            traiter_le_lien(&c, &rapport(Some("essai"), true)),
            (SuiteLien::Aucune, None)
        );
        assert!(!c.xdg_data_home().join("applications").exists());
    }

    // Aucun test de cette crate ne change la langue : sans catalogue, les textes sont en anglais.
    #[test]
    fn l_avertissement_dit_que_le_produit_est_installe() {
        let texte = lien_non_declare(&Erreur::Introuvable("xdg-mime".into()));
        assert!(texte.starts_with("installed, but"), "{texte}");
        assert!(texte.contains("xdg-mime"), "{texte}");
    }
}
