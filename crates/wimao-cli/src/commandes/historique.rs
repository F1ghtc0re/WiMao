//! Sous-commande `historique [--tout]`.
//!
//! Affiche les actions enregistrées par le moteur (`<données>/historique.jsonl`), la plus
//! récente d'abord, une par ligne : `date  résultat  action  sujet  [point]`. Les 20 dernières
//! seulement, sauf avec `--tout`.
//!
//! Utilise `wimao_core::historique` (lecture, libellés traduits partagés avec l'interface).
//! Utilisé par `commandes::executer`.

use wimao_core::historique::{libelle_action, libelle_resultat, lire, Entree};
use wimao_core::i18n::tr;
use wimao_core::{Chemins, Resultat};

use super::Issue;
use crate::sortie::afficher;

/// Nombre d'entrées montrées sans `--tout`.
const ENTREES_PAR_DEFAUT: usize = 20;

/// Une entrée sur une ligne : `date  résultat  action  sujet  [point]` (le point est omis
/// quand l'action n'en a pas créé).
fn ligne(e: &Entree) -> String {
    let mut texte = format!(
        "{}  {}  {}  {}",
        e.date,
        libelle_resultat(e.resultat),
        libelle_action(e.action),
        e.sujet
    );
    if let Some(point) = &e.point {
        texte.push_str(&format!("  [{point}]"));
    }
    texte
}

/// Écrit l'historique sur la sortie standard. Sans aucune entrée : « No action yet », code 0.
pub fn afficher(c: &Chemins, tout: bool) -> Resultat<Issue> {
    let entrees = lire(c)?;
    if entrees.is_empty() {
        afficher!("{}", tr("No action yet"));
        return Ok(Issue::Succes);
    }
    // `lire` rend déjà la plus récente d'abord : garder les 20 premières = les 20 dernières.
    let limite = if tout {
        entrees.len()
    } else {
        ENTREES_PAR_DEFAUT
    };
    for e in entrees.iter().take(limite) {
        afficher!("{}", ligne(e));
    }
    Ok(Issue::Succes)
}
