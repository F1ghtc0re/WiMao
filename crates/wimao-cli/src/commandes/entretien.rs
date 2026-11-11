//! Sous-commandes `diagnostiquer`, `reparer` et `rapport`.
//!
//! Utilise `wimao_core::{diagnostic, rapport}` et `wimao_recettes` (`reparer_gestionnaires`,
//! `programmes_a_nommer_avant_reparation`). Utilisé par `commandes::executer`.

use std::io::{stdin, stdout, BufRead, Write};

use wimao_core::i18n::{tr, trf};
use wimao_core::rapport::rapport as construire_rapport;
use wimao_core::{diagnostiquer as chercher_problemes, message_probleme, reparer as reparer_tout};
use wimao_core::{Chemins, Probleme, Resultat};

use wimao_recettes::{programmes_a_nommer_avant_reparation, reparer_gestionnaires};

use super::installation::charger_catalogue;
use super::Issue;
use crate::confirmation::confirmer;
use crate::progression_terminal::ProgressionTerminal;
use crate::sortie::afficher;

/// `diagnostiquer` : liste les défauts de l'environnement (sans lancer Wine).
pub fn diagnostiquer(c: &Chemins) -> Resultat<Issue> {
    let problemes = chercher_problemes(c)?;
    afficher(&problemes);
    Ok(Issue::Succes)
}

/// `reparer` : diagnostique, demande confirmation si des programmes ouverts seraient fermés,
/// répare, puis réapplique les réglages des gestionnaires installés et redémarre leurs
/// services.
///
/// Toute réparation arrête Wine, donc tout programme ouvert dans l'environnement : la liste
/// des programmes à nommer vient de `wimao_recettes::programmes_a_nommer_avant_reparation`
/// (même décision que l'interface). Si la personne refuse, **rien** n'est fait (code 0, comme
/// une restauration annulée). Les réglages des gestionnaires sont réappliqués même sans
/// problème trouvé : « Réparer » remet aussi en place un registre ou un service perdu, que le
/// diagnostic ne voit pas.
pub fn reparer(c: &Chemins, oui: bool) -> Resultat<Issue> {
    let problemes = chercher_problemes(c)?;
    afficher(&problemes);
    let catalogue = charger_catalogue(c);
    let programmes = programmes_a_nommer_avant_reparation(c, &catalogue, &problemes)?;
    if !programmes.is_empty()
        && !confirmer_fermeture(&programmes, oui, &mut stdin().lock(), &mut stdout())
    {
        afficher!("{}", tr("Cancelled: nothing was changed"));
        return Ok(Issue::Succes);
    }
    // Sans problème réparable, `reparer_tout` pose seulement les liens yabridge manquants
    // (Flatpak compris) : ni point de restauration ni arrêt de Wine.
    let reparation = reparer_tout(c, &problemes, &ProgressionTerminal);
    // Même après une réparation partielle (erreur), des réglages ont pu se perdre : on les
    // réapplique, puis on rend l'erreur de la réparation (la première rencontrée).
    let reglages = reparer_gestionnaires(c, &catalogue, &ProgressionTerminal);
    reparation?;
    reglages?;
    afficher!("{}", tr("Repair finished"));
    Ok(Issue::Succes)
}

/// Nomme les `programmes` ouverts que la réparation fermerait et demande si on continue.
/// Seule la question est ici : la liste vient de
/// `wimao_recettes::programmes_a_nommer_avant_reparation`.
fn confirmer_fermeture(
    programmes: &[String],
    oui: bool,
    entree: &mut impl BufRead,
    sortie: &mut impl Write,
) -> bool {
    let question = trf(
        "Repairing will close these open programs: {programs}. Continue?",
        &[("programs", &programmes.join(", "))],
    );
    confirmer(&question, oui, entree, sortie)
}

/// `rapport` : imprime le rapport anonymisé, à coller dans un message de support.
pub fn rapport(c: &Chemins) -> Resultat<Issue> {
    afficher!("{}", construire_rapport(c)?);
    Ok(Issue::Succes)
}

/// Écrit un problème par ligne, ou le constat qu'il n'y en a pas.
fn afficher(problemes: &[Probleme]) {
    if problemes.is_empty() {
        afficher!("{}", tr("No problem found"));
    }
    for probleme in problemes {
        afficher!("- {}", message_probleme(probleme));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_question_nomme_les_programmes_et_oui_la_saute() {
        let programmes = vec!["Native Access.exe".to_string(), "Serum.exe".to_string()];
        let mut sortie = Vec::new();
        assert!(!confirmer_fermeture(
            &programmes,
            false,
            &mut "n\n".as_bytes(),
            &mut sortie
        ));
        let texte = String::from_utf8(sortie).unwrap();
        assert!(texte.contains("Native Access.exe, Serum.exe"), "{texte}");
        let mut muette = Vec::new();
        assert!(confirmer_fermeture(
            &programmes,
            true,
            &mut "".as_bytes(),
            &mut muette
        ));
        assert!(muette.is_empty());
        assert!(confirmer_fermeture(
            &programmes,
            false,
            &mut "o\n".as_bytes(),
            &mut Vec::new()
        ));
    }
}
