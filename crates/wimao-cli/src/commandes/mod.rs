//! Exécution des sous-commandes : une fonction par commande, rangées par thème.
//!
//! - [`installation`] : `init`, `installer`, `detecter` ;
//! - [`verification`] : `verifier` ;
//! - [`entretien`] : `diagnostiquer`, `reparer`, `rapport` ;
//! - [`gestionnaires`] : `recettes`, `gestionnaire installer|ouvrir`, `relayer-lien` ;
//! - [`applications`] : `application lister|ouvrir|retirer` ;
//! - [`historique`] : `historique` ;
//! - [`import`] : `importer` et `importer --annuler` ;
//! - [`restauration`] : `restauration lister|revenir` ;
//! - [`sauvegarde`] : `sauvegarder`, `restaurer`.
//!
//! Utilise `wimao-core` et `wimao-recettes`. Utilisé par `main.rs`, qui n'a ensuite plus
//! qu'à traduire le résultat en code de sortie.

pub mod applications;
pub mod entretien;
pub mod gestionnaires;
pub mod historique;
pub mod import;
pub mod installation;
pub mod restauration;
pub mod sauvegarde;
pub mod verification;

use wimao_core::{Chemins, Resultat};

use crate::cli::Commande;

/// Comment une commande réussie s'est terminée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Issue {
    /// Tout s'est bien passé (code de sortie 0).
    Succes,
    /// Au moins un plugin est en échec ❌ (code de sortie 2).
    PluginEnEchec,
}

/// Lance la sous-commande demandée.
///
/// `chemins` est calculé une fois par `main` ; une erreur de calcul (`HOME` absent) n'est
/// rendue que par les commandes qui en ont besoin : `detecter` n'en a pas.
pub fn executer(commande: Commande, chemins: Resultat<Chemins>) -> Resultat<Issue> {
    match commande {
        Commande::Detecter { fichier } => installation::detecter(&fichier, chemins.as_ref().ok()),
        autre => executer_avec_chemins(autre, &chemins?),
    }
}

/// Termine ou abandonne un import de `~/.wine-mao` coupé net (coupure de courant), avant toute
/// sous-commande : voir `wimao_core::import::reprendre_interruption`. Le message de réparation,
/// ou l'erreur d'un cas ambigu, est écrit sur la sortie d'erreur (la sortie standard reste celle
/// de la commande) ; une erreur n'empêche pas la commande de s'exécuter.
///
/// Sauf `relayer-lien` (lancé par le bureau à chaque lien, doit rester instantané et sans effet
/// de bord) et `verifier --prefixe` (ne doit rien écrire dans les dossiers de WiMao).
fn reprendre_import_interrompu(commande: &Commande, c: &Chemins) {
    if matches!(
        commande,
        Commande::Verifier { prefixe: Some(_) } | Commande::RelayerLien { .. }
    ) {
        return;
    }
    match wimao_core::import::reprendre_interruption(c) {
        Ok(Some(message)) => eprintln!("{message}"),
        Ok(None) => {}
        Err(e) => eprintln!("{e}"),
    }
}

/// Avant une action qui arrête Wine : nomme les programmes Windows ouverts dans l'environnement
/// de WiMao et demande l'accord (sauf `oui`). Vrai s'il faut continuer.
///
/// Même mécanisme que `reparer` (`confirmation`), mais avec un texte qui ne parle pas de
/// réparation. Sur refus, écrit « Cancelled: nothing was changed » et rend faux : l'appelant
/// termine alors avec le code 0, comme toute annulation. Sans programme ouvert, ne demande rien.
///
/// Erreur : [`wimao_core::Erreur::Io`] si `/proc` est illisible.
pub(crate) fn accord_avant_arret(c: &Chemins, oui: bool) -> Resultat<bool> {
    let programmes = wimao_core::processus::programmes_ouverts(c)?;
    Ok(confirmer_arret_wine(&programmes, oui))
}

/// Pose la question de [`accord_avant_arret`] pour une liste déjà connue de `programmes`
/// (vide : pas de question). Écrit le message d'annulation sur refus.
fn confirmer_arret_wine(programmes: &[String], oui: bool) -> bool {
    if programmes.is_empty() {
        return true;
    }
    let accepte = crate::confirmation::confirmer_fermeture(
        programmes,
        oui,
        &mut std::io::stdin().lock(),
        &mut std::io::stdout(),
    );
    if !accepte {
        crate::sortie::afficher!("{}", wimao_core::i18n::tr("Cancelled: nothing was changed"));
    }
    accepte
}

/// Les sous-commandes qui ont besoin des emplacements de WiMao.
fn executer_avec_chemins(commande: Commande, c: &Chemins) -> Resultat<Issue> {
    reprendre_import_interrompu(&commande, c);
    match commande {
        Commande::Init => installation::init(c),
        Commande::Installer(args) => installation::installer(c, &args),
        Commande::Verifier { prefixe } => verification::verifier(c, prefixe),
        Commande::Diagnostiquer => entretien::diagnostiquer(c),
        Commande::Reparer { oui } => entretien::reparer(c, oui),
        Commande::Restauration(action) => restauration::executer(c, action),
        Commande::Sauvegarder { destination, oui } => sauvegarde::sauvegarder(c, &destination, oui),
        Commande::Importer { annuler, oui } => import::importer(c, annuler, oui),
        Commande::Restaurer(args) => sauvegarde::restaurer(c, &args),
        Commande::Rapport => entretien::rapport(c),
        Commande::Recettes => gestionnaires::recettes(c),
        Commande::Gestionnaire(action) => gestionnaires::gestionnaire(c, action),
        Commande::Application(action) => applications::application(c, action),
        Commande::Historique { tout } => historique::afficher(c, tout),
        Commande::RelayerLien { url } => gestionnaires::relayer(c, &url),
        // `executer` traite `detecter` avant d'appeler cette fonction (elle n'a pas besoin des
        // chemins) : arriver ici serait une erreur de programmation, pas d'utilisation.
        Commande::Detecter { .. } => unreachable!("detecter est traitée par `executer`"),
    }
}
