//! Sous-commande `restauration lister|revenir`.
//!
//! Utilise `wimao_core::restauration` (points, aperçu, retour) et `confirmation`.
//! Utilisé par `commandes::executer`.

use std::io::{stdin, stdout};

use wimao_core::i18n::{tr, trf, trn};
use wimao_core::restauration::{
    apercu_retour, libelle_motif, lister, revenir, FilePoints, PointRestauration,
};
use wimao_core::{Chemins, Erreur, Resultat};

use super::Issue;
use crate::cli::ActionRestauration;
use crate::confirmation::confirmer;
use crate::sortie::afficher;

/// Aiguille vers `lister` ou `revenir`.
pub fn executer(c: &Chemins, action: ActionRestauration) -> Resultat<Issue> {
    match action {
        ActionRestauration::Lister => lister_points(c),
        ActionRestauration::Revenir { id, oui } => revenir_au_point(c, &id, oui),
    }
}

/// Nom traduit de la file d'un point (colonne « file » de `restauration lister`).
fn nom_de_file(point: &PointRestauration) -> String {
    match point.file() {
        FilePoints::Installation => tr("installation"),
        FilePoints::Reparation => tr("repair"),
    }
}

/// Un point par ligne (`identifiant  file  motif`), du plus récent au plus ancien ; les points
/// périmés (ils ne correspondent plus à l'environnement, voir
/// `restauration::perimer_tous_les_points`) suivent, marqués « outdated ».
fn lister_points(c: &Chemins) -> Resultat<Issue> {
    let liste = lister(c)?;
    if liste.points.is_empty() && liste.perimes.is_empty() {
        afficher!("{}", tr("No restore point"));
    }
    for point in &liste.points {
        afficher!(
            "{}  {}  {}",
            point.id,
            nom_de_file(point),
            libelle_motif(&point.motif)
        );
    }
    for point in &liste.perimes {
        afficher!(
            "{}  {}  {} ({})",
            point.id,
            nom_de_file(point),
            libelle_motif(&point.motif),
            tr("outdated")
        );
    }
    for dossier in &liste.incomplets {
        eprintln!(
            "{}",
            trf(
                "Unusable restore point (interrupted creation): {path}",
                &[("path", &dossier.display().to_string())]
            )
        );
    }
    Ok(Issue::Succes)
}

/// Montre ce qui sera retiré, demande confirmation, puis revient au point.
///
/// Seuls les chemins de l'aperçu **confirmé** sont donnés à `revenir` : un fichier apparu
/// entre-temps ne peut pas être supprimé sans que l'utilisateur l'ait vu.
fn revenir_au_point(c: &Chemins, id: &str, oui: bool) -> Resultat<Issue> {
    let liste = lister(c)?;
    // Un point périmé est refusé tout de suite : ni aperçu ni question pour une action impossible.
    if liste.perimes.iter().any(|p| p.id == id) {
        return Err(Erreur::PointPerime(id.to_string()));
    }
    let point = liste
        .points
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Erreur::Introuvable(trf("restore point {id}", &[("id", id)])))?;
    let apercu = apercu_retour(c, &point)?;
    afficher!(
        "{}",
        trn(
            "{n} file added since this point will be removed:",
            "{n} files added since this point will be removed:",
            apercu.len() as u64,
            &[]
        )
    );
    for chemin in &apercu {
        afficher!("  {}", chemin.display());
    }
    let question = tr("Put the environment back in this state?");
    if !confirmer(&question, oui, &mut stdin().lock(), &mut stdout()) {
        afficher!("{}", tr("Cancelled: nothing was changed"));
        return Ok(Issue::Succes);
    }
    let bilan = revenir(c, &point, &apercu)?;
    afficher!(
        "{}",
        trn(
            "Done: {n} file removed",
            "Done: {n} files removed",
            bilan.fichiers_supprimes as u64,
            &[]
        )
    );
    if bilan.liens_retires > 0 {
        afficher!(
            "{}",
            trn(
                "{n} broken plugin link removed",
                "{n} broken plugin links removed",
                bilan.liens_retires as u64,
                &[]
            )
        );
    }
    Ok(Issue::Succes)
}
