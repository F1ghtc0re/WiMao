//! Sous-commande `importer` : reprendre l'environnement Wine existant (`~/.wine-mao`) dans
//! WiMao, ou annuler cet import.
//!
//! Utilise `wimao_core::import` (détection, import, annulation), `confirmation` (la question
//! « o/N ») et `processus` (programmes ouverts dans l'environnement de WiMao, pour l'annulation).
//! Utilisé par `commandes::executer`.
//!
//! Choix non évidents :
//! - l'aperçu (chemin, taille, plugins, destination, lien laissé) est écrit **avant** toute
//!   question : la personne sait ce qui va se passer ;
//! - les programmes ouverts sont nommés dans la même question que « Continuer ? », pour ne
//!   pas en poser deux ;
//! - un refus n'est pas une erreur (code 0), comme pour `reparer` et `restaurer` ;
//! - quand il n'y a rien à importer, l'erreur rendue est celle que `wimao_core::import::importer`
//!   rendrait (mêmes textes déjà traduits), mais sans lancer l'import : `detecter` a déjà
//!   parcouru le dossier, inutile de le refaire pour échouer.

use std::io::{stdin, stdout};

use wimao_core::i18n::{tr, trf, trn};
use wimao_core::import::{
    ancien_emplacement, annulation_possible, annuler, detecter, importer as importer_existant,
    programmes_ouverts, EnvironnementExistant,
};
use wimao_core::outils::taille_lisible;
use wimao_core::{Chemins, Erreur, Resultat};

use super::Issue;
use crate::confirmation::{confirmer, confirmer_fermeture};
use crate::progression_terminal::ProgressionTerminal;
use crate::sortie::afficher;

/// Nom du dossier de l'ancien environnement dans le dossier personnel (même valeur que
/// `wimao_core::import`, qui ne l'expose pas : il ne sert ici qu'à écrire un message d'erreur).
const NOM_ANCIEN: &str = ".wine-mao";

/// `importer` ou, avec `annuler`, `importer --annuler`.
pub fn importer(c: &Chemins, annuler: bool, oui: bool) -> Resultat<Issue> {
    if annuler {
        annuler_import(c, oui)
    } else {
        importer_environnement(c, oui)
    }
}

/// Importe `~/.wine-mao` : aperçu, question, import.
fn importer_environnement(c: &Chemins, oui: bool) -> Resultat<Issue> {
    let Some(existant) = detecter(c) else {
        return Err(rien_a_importer(c));
    };
    afficher_apercu(c, &existant);
    let programmes = programmes_ouverts(c)?;
    if !demander_accord(&programmes, oui) {
        afficher!("{}", tr("Cancelled: nothing was changed"));
        return Ok(Issue::Succes);
    }
    let importe = importer_existant(c, &ProgressionTerminal)?;
    afficher!(
        "{}",
        trf(
            "Import finished: {path} is now a link to the environment of WiMao",
            &[("path", &importe.ancien.display().to_string())]
        )
    );
    afficher!(
        "{}",
        trf(
            "A copy of the registry and of the yabridgectl settings is kept in {path}",
            &[("path", &importe.sauvegarde.display().to_string())]
        )
    );
    Ok(Issue::Succes)
}

/// Annule le dernier import : aperçu, question, retour du dossier dans `~/.wine-mao`.
fn annuler_import(c: &Chemins, oui: bool) -> Resultat<Issue> {
    if !annulation_possible(c) {
        return Err(Erreur::Introuvable(tr("an import to cancel")));
    }
    // Cas normal après `annulation_possible` ; sinon (trace illisible) le lien porte le nom
    // habituel.
    let ancien = ancien_emplacement(c).unwrap_or_else(|| c.home.join(NOM_ANCIEN));
    afficher!(
        "{}",
        trf(
            "The import will be cancelled: the environment goes back to {path} and the link is removed.",
            &[("path", &ancien.display().to_string())]
        )
    );
    // Annuler arrête le `wineserver` de l'environnement de WiMao.
    let programmes = wimao_core::processus::programmes_ouverts(c)?;
    if !demander_accord(&programmes, oui) {
        afficher!("{}", tr("Cancelled: nothing was changed"));
        return Ok(Issue::Succes);
    }
    annuler(c, &ProgressionTerminal)?;
    afficher!("{}", tr("Import cancelled"));
    Ok(Issue::Succes)
}

/// Écrit ce que l'import va faire : l'environnement trouvé, sa taille, ses plugins, sa
/// destination et le lien laissé à sa place.
fn afficher_apercu(c: &Chemins, existant: &EnvironnementExistant) {
    let chemin = existant.chemin.display().to_string();
    afficher!(
        "{}",
        trf(
            "Existing Wine environment found: {path}",
            &[("path", &chemin)]
        )
    );
    afficher!(
        "{}",
        trf(
            "Size: {size}",
            &[("size", &taille_lisible(existant.taille))]
        )
    );
    afficher!(
        "{}",
        trn(
            "{count} plugin found",
            "{count} plugins found",
            existant.plugins as u64,
            &[("count", &existant.plugins.to_string())]
        )
    );
    afficher!(
        "{}",
        trf(
            "It will be moved to {path} (nothing is copied).",
            &[("path", &c.prefixe.display().to_string())]
        )
    );
    afficher!(
        "{}",
        trf(
            "A link will be left at {path}, pointing to the new location.",
            &[("path", &chemin)]
        )
    );
}

/// Pose la question finale : si des `programmes` sont ouverts, les nomme (ils seront fermés) ;
/// sinon demande simplement de continuer. `oui` saute la question.
fn demander_accord(programmes: &[String], oui: bool) -> bool {
    if programmes.is_empty() {
        confirmer(&tr("Continue?"), oui, &mut stdin().lock(), &mut stdout())
    } else {
        confirmer_fermeture(programmes, oui, &mut stdin().lock(), &mut stdout())
    }
}

/// Erreur quand `detecter` ne trouve rien : un environnement de WiMao déjà là
/// ([`Erreur::DejaPresent`]) ou pas d'environnement à reprendre ([`Erreur::Introuvable`]).
fn rien_a_importer(c: &Chemins) -> Erreur {
    if c.prefixe.symlink_metadata().is_ok() {
        return Erreur::DejaPresent(c.prefixe.clone());
    }
    Erreur::Introuvable(trf(
        "Wine environment in {path} (a folder with system.reg, user.reg and drive_c)",
        &[("path", &c.home.join(NOM_ANCIEN).display().to_string())],
    ))
}
