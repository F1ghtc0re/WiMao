//! Annonce de l'avancement des longues opérations (installation, sauvegarde…).
//!
//! Défini ici pour que le cœur ne dépende ni de la console ni de GTK : la CLI
//! écrit dans le terminal, l'interface met à jour une barre, les tests utilisent
//! [`ProgressionMuette`].

/// Reçoit les messages d'étape d'une opération longue.
///
/// `Send + Sync` car l'opération tourne souvent dans un fil séparé de l'interface.
pub trait Progression: Send + Sync {
    /// Annonce qu'une nouvelle étape commence, avec un message lisible.
    fn etape(&self, message: &str);
}

/// Progression qui ignore tout : pour les tests et les usages sans affichage.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProgressionMuette;

impl Progression for ProgressionMuette {
    fn etape(&self, _message: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_progression_muette_accepte_les_messages() {
        let p: &dyn Progression = &ProgressionMuette;
        p.etape("test");
    }
}
