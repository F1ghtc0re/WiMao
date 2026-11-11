//! Sous-commandes `application lister|ouvrir|retirer` : les programmes ajoutés avec un
//! installeur (liste enregistrée dans `applications.toml`, voir
//! `wimao_recettes::applications`).
//!
//! `ouvrir` passe par la même session que `gestionnaire ouvrir`
//! (`gestionnaires::ouvrir_recette`), avec la recette que fabrique
//! `Application::vers_recette` : mêmes messages, même surveillance, même bilan. Utilisé par
//! `commandes::executer`.

use wimao_core::i18n::{tr, trf};
use wimao_core::{Chemins, Erreur, Resultat};
use wimao_recettes::applications::{applications_presentes, retirer_application, Application};

use super::gestionnaires::ouvrir_recette;
use super::Issue;
use crate::cli::ActionApplication;
use crate::sortie::afficher;

/// Aiguille vers `application lister`, `ouvrir` ou `retirer`.
pub fn application(c: &Chemins, action: ActionApplication) -> Resultat<Issue> {
    match action {
        ActionApplication::Lister => lister(c),
        ActionApplication::Ouvrir {
            nom,
            sans_test,
            oui,
        } => {
            let appli = application_nommee(c, &nom)?;
            ouvrir_recette(c, &appli.vers_recette(), sans_test, oui)
        }
        ActionApplication::Retirer { nom } => retirer(c, &nom),
    }
}

/// Erreur « application introuvable » (message traduit).
fn introuvable(nom: &str) -> Erreur {
    Erreur::Introuvable(trf("an application named {name}", &[("name", nom)]))
}

/// Cherche, parmi les applications présentes, celle qui s'appelle exactement `nom`.
fn application_nommee(c: &Chemins, nom: &str) -> Resultat<Application> {
    applications_presentes(c)
        .into_iter()
        .find(|a| a.nom == nom)
        .ok_or_else(|| introuvable(nom))
}

/// Ligne de `lister` pour une application : `nom — programme`.
fn ligne_application(a: &Application) -> String {
    format!("{} — {}", a.nom, a.programme)
}

/// `application lister` : une ligne par application présente, ou « Aucune application ».
fn lister(c: &Chemins) -> Resultat<Issue> {
    let liste = applications_presentes(c);
    if liste.is_empty() {
        afficher!("{}", tr("No application added"));
    }
    for a in &liste {
        afficher!("{}", ligne_application(a));
    }
    Ok(Issue::Succes)
}

/// `application retirer` : enlève de la liste, sans rien désinstaller. Nom inconnu : erreur.
fn retirer(c: &Chemins, nom: &str) -> Resultat<Issue> {
    if !retirer_application(c, nom)? {
        return Err(introuvable(nom));
    }
    afficher!(
        "{}",
        trf(
            "{name} removed from the list; nothing was uninstalled",
            &[("name", nom)]
        )
    );
    Ok(Issue::Succes)
}
