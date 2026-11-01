//! Sortie du programme : écriture sur stdout sans panique, codes de sortie, affichage des erreurs.
//!
//! Utilise `wimao_core` (erreurs traduites, règles d'erreurs). Utilisé par `main.rs` et par
//! toutes les sous-commandes (macro [`afficher`]).

use std::io::{ErrorKind, Write};

use wimao_core::i18n::tr;
use wimao_core::{expliquer, Erreur, Resultat};

use crate::commandes::Issue;

/// Code de sortie d'un échec (message sur stderr).
pub const CODE_ERREUR: u8 = 1;
/// Code de sortie quand au moins un plugin est en échec.
pub const CODE_PLUGIN_EN_ECHEC: u8 = 2;

/// Code de sortie d'une commande : 0 succès, 1 erreur, 2 au moins un plugin ❌.
///
/// Fonction pure (testée seule) : `main` n'a plus qu'à l'appliquer. Les erreurs d'usage
/// de `clap` sortent en 1 aussi (voir `cli::analyser`) : le code 2 reste réservé aux plugins ❌.
pub fn code_de_sortie(resultat: &Resultat<Issue>) -> u8 {
    match resultat {
        Ok(Issue::Succes) => 0,
        Ok(Issue::PluginEnEchec) => CODE_PLUGIN_EN_ECHEC,
        Err(_) => CODE_ERREUR,
    }
}

/// Écrit l'erreur (déjà traduite) sur stderr, suivie de l'explication d'une règle d'erreur
/// si une s'applique.
pub fn afficher_erreur(erreur: &Erreur) {
    let texte = erreur.to_string();
    eprintln!("{} {texte}", tr("Error:"));
    if let Some((explication, _)) = expliquer(&texte) {
        eprintln!("{explication}");
    }
}

/// Écrit `texte` et un saut de ligne sur la sortie standard.
///
/// Contrairement à `println!`, ne panique pas quand la sortie est fermée
/// (`wimao-cli rapport | head`) : le lecteur n'écoute plus (`BrokenPipe`), on s'arrête
/// donc en silence avec le code 0. Toute autre erreur d'écriture est ignorée : il n'y a
/// nulle part où la signaler.
pub fn ecrire(texte: &str) {
    let mut sortie = std::io::stdout().lock();
    if let Err(e) = writeln!(sortie, "{texte}") {
        if e.kind() == ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
    }
}

/// Comme `println!`, via [`ecrire`] (pas de panique si la sortie est fermée).
macro_rules! afficher {
    ($($arg:tt)*) => { $crate::sortie::ecrire(&format!($($arg)*)) };
}
pub(crate) use afficher;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_code_de_sortie_suit_le_resultat() {
        assert_eq!(code_de_sortie(&Ok(Issue::Succes)), 0);
        assert_eq!(code_de_sortie(&Err(Erreur::PluginsEnCours)), 1);
        assert_eq!(code_de_sortie(&Ok(Issue::PluginEnEchec)), 2);
    }
}
