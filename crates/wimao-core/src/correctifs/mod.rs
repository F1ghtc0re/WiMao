//! Correctifs appliqués à l'environnement Wine après sa création.
//!
//! Un correctif répare un manque connu de Wine pour les plugins audio (Wine Mono
//! pour les installeurs .NET, DXVK pour l'affichage, msvcp140 de Microsoft pour Maschine 3). Utilise [`Chemins`] et
//! [`Progression`] ; utilisé par `environnement::creer_environnement` (qui applique
//! chaque correctif manquant) et par la commande « Réparer » (`diagnostic::reparer`, qui
//! appelle [`Correctif::retirer`] puis [`Correctif::appliquer`]).

mod dll;
pub mod dxvk;
pub mod mono;
pub mod msvcp;

use crate::chemins::Chemins;
use crate::erreur::Resultat;
use crate::progression::Progression;

/// Une modification de l'environnement Wine, vérifiable et réversible.
pub trait Correctif {
    /// Nom stable du correctif, affiché dans les messages (« Wine Mono »…).
    fn nom(&self) -> &'static str;

    /// Vrai si le correctif est déjà en place. Doit être peu coûteux et sans
    /// effet de bord (ne lance pas Wine : le registre est lu dans les fichiers `.reg`,
    /// voir `registre_fichier`) : sert à rendre la création de l'environnement idempotente
    /// et au diagnostic. Seul un « absent » attendu donne `false` ; une erreur d'entrée/sortie
    /// inattendue est renvoyée.
    fn est_applique(&self, c: &Chemins) -> Resultat<bool>;

    /// Applique le correctif (téléchargement, installation…) en annonçant ses
    /// étapes à `p`.
    fn appliquer(&self, c: &Chemins, p: &dyn Progression) -> Resultat<()>;

    /// Retire ce que le correctif a posé, pour pouvoir le ré-appliquer proprement
    /// (« Réparer »). Par défaut ne fait rien : Wine Mono n'a rien à retirer, son
    /// installateur remplace lui-même une version existante.
    fn retirer(&self, _c: &Chemins) -> Resultat<()> {
        Ok(())
    }
}

/// Les correctifs dans l'ordre d'application : Mono, DXVK, puis msvcp140. Mono
/// d'abord car il n'a besoin de rien ; msvcp140 en dernier car, faute de cache
/// Microsoft, il lance des installeurs dans l'environnement déjà complet.
pub fn correctifs() -> Vec<Box<dyn Correctif>> {
    vec![
        Box::new(mono::Mono),
        Box::new(dxvk::Dxvk),
        Box::new(msvcp::Msvcp140),
    ]
}
