//! Progression en texte simple pour le terminal.
//!
//! Implémente `wimao_core::Progression`. Utilisé par les sous-commandes longues
//! (`init`, `installer`, `reparer`, `sauvegarder`, `restaurer`).

use wimao_core::Progression;

/// Préfixe des lignes de progression, pour les distinguer des résultats.
const PREFIXE: &str = "-> ";

/// Écrit chaque étape sur la **sortie d'erreur** : la sortie standard reste réservée aux
/// résultats (tableaux, rapport), donc `wimao-cli rapport > fichier.txt` ou un tube
/// ne reçoivent pas les messages d'avancement.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProgressionTerminal;

impl Progression for ProgressionTerminal {
    fn etape(&self, message: &str) {
        eprintln!("{PREFIXE}{message}");
    }
}
