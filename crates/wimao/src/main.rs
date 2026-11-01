//! Point d'entrée de l'interface graphique de WiMao (GTK4 + libadwaita).
//!
//! Démarrage, dans l'ordre : réglages (langue choisie) puis `init_langue` ; chemins et
//! journal (`<données>/journal.log`, comme la CLI) ; application `fr.wimao.WiMao` ; feuille
//! de style ; première fenêtre (assistant de premier lancement ou fenêtre principale). Le travail
//! est rangé dans :
//! - `aide.rs` : mode d'emploi (chapitres Markdown, convertisseur) ; `aide_fenetre.rs` : sa fenêtre (F1) ;
//! - `assistant.rs` : l'automate de l'assistant de premier lancement ; `assistant_fenetre.rs` : sa fenêtre ;
//! - `fenetre/` : la fenêtre (`mod.rs`), ses actions (`actions.rs`) et son en-tête (`entete.rs`) ; `dialogues.rs` : boîtes de dialogue ;
//! - `fichier_toml.rs` : lecture/écriture atomique de TOML ; `liste.rs` : lignes et pastilles de la liste ; `travail.rs` : fil de travail ;
//! - `historique_vue.rs` : page « History » et retour en arrière ;
//! - `gestionnaires.rs` : section « Managers » et installation d'un gestionnaire ;
//!   `session.rs` : fil d'une session de gestionnaire ; `session_fenetre.rs` : la session dans
//!   la fenêtre (bannière, dialogues, bilan) ; `reparation.rs` : « Repair » et proposition de
//!   ponter au démarrage ; `restaurer.rs` : « Restore a backup… » et « Cancel the import » ;
//! - `operations.rs` : appels au noyau (sans widget) ; `etat.rs` : résultats mémorisés
//!   (`etat.toml`) ; `reglages.rs` : `reglages.toml` (langue).
//!
//! Utilise `wimao-core` et `wimao-recettes`. Voir `ARCHITECTURE.md`.

mod aide;
mod aide_fenetre;
mod assistant;
mod assistant_fenetre;
mod dialogues;
mod etat;
mod fenetre;
mod fichier_toml;
mod gestionnaires;
mod historique_vue;
mod liste;
mod operations;
mod reglages;
mod reparation;
mod restaurer;
mod session;
mod session_fenetre;
mod travail;

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk4::{gdk, gio, glib, prelude::*};
use libadwaita as adw;
use wimao_core::i18n::{init_langue, tr};
use wimao_core::import;
use wimao_core::{journal, Chemins};

use fenetre::Fenetre;
use reglages::Reglages;

/// Nom du fichier journal, dans le dossier de données.
const FICHIER_JOURNAL: &str = "journal.log";
/// Feuille de style (variables libadwaita uniquement), embarquée dans le binaire.
const STYLE: &str = include_str!("../data/style.css");
/// Icônes de l'application (disposition `hicolor/scalable/apps/`), cherchées dans les sources
/// quand WiMao n'est pas installé : `cargo run` montre alors la vraie icône. Une fois
/// installée (`/usr/share/icons/hicolor/scalable/apps/`), le thème la trouve seul.
///
/// Absent des builds de paquet (cfg `wimao_paquet`, voir `build.rs`) : ce chemin est celui de la
/// machine de fabrication et ne doit pas se retrouver dans le binaire livré.
#[cfg(not(wimao_paquet))]
const ICONES_DEVELOPPEMENT: Option<&str> = Some(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
#[cfg(wimao_paquet)]
const ICONES_DEVELOPPEMENT: Option<&str> = None;

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(dialogues::ID_APPLICATION)
        .build();
    app.connect_startup(|app| {
        charger_style();
        ajouter_icones_developpement();
        ajouter_action_quitter(app);
    });
    match Chemins::depuis_env() {
        Ok(chemins) => demarrer(&app, chemins),
        // Pas de chemins (HOME absent) : une fenêtre explique, au lieu d'un échec muet.
        Err(erreur) => {
            init_langue(None);
            app.connect_activate(move |app| dialogues::fenetre_erreur_demarrage(app, &erreur));
        }
    }
    // Aucun argument de ligne de commande : GTK ne doit pas les analyser.
    app.run_with_args::<&str>(&[])
}

/// Langue, journal, puis ouverture de la fenêtre à l'activation de l'application.
fn demarrer(app: &adw::Application, chemins: Chemins) {
    // La langue choisie dans le menu, sinon celle du système (`None`). Un `reglages.toml`
    // abîmé est mis de côté et signalé : l'avertissement est gardé pour la fenêtre.
    let (reglages, avertissement) = Reglages::charger(&chemins.config);
    init_langue(reglages.langue.as_deref());
    journal::initialiser(chemins.donnees.join(FICHIER_JOURNAL));
    if let Some(message) = &avertissement {
        journal::noter("réglages", message);
    }
    let chemins = Rc::new(chemins);
    // L'avertissement n'est montré qu'à la première fenêtre : `take` le consomme.
    let avertissement = Rc::new(RefCell::new(avertissement));
    app.connect_activate(move |app| match app.active_window() {
        Some(fenetre) => fenetre.present(),
        None => {
            let message = avertissement.borrow_mut().take();
            ouvrir_une_fenetre(app, &chemins, message);
        }
    });
}

/// Choisit et ouvre la première fenêtre : l'assistant si WiMao n'a pas encore d'environnement
/// (`system.reg` absent), la fenêtre principale sinon.
///
/// Cette décision est prise **à chaque activation sans fenêtre** et non une seule fois au
/// démarrage : après « Cancel the import », WiMao n'a plus d'environnement et `restaurer.rs`
/// rappelle `activate`, ce qui doit ramener l'assistant.
///
/// Avant de décider, [`import::reprendre_interruption`] répare un import ou une annulation
/// interrompus par un plantage ou une coupure de courant (elle peut remettre l'environnement en
/// place). Elle est rapide, sans parcours du dossier : on peut l'appeler dans le fil de GTK.
/// Son message, ou son erreur, est montré dans un dialogue sur la fenêtre ouverte ; l'erreur
/// n'empêche pas de démarrer.
fn ouvrir_une_fenetre(
    app: &adw::Application,
    chemins: &Rc<Chemins>,
    avertissement: Option<String>,
) {
    let reprise = import::reprendre_interruption(chemins);
    let fenetre = if chemins.prefixe.join("system.reg").exists() {
        let principale = Fenetre::nouvelle(app, chemins.clone(), avertissement);
        principale.presenter();
        principale.fenetre.clone()
    } else {
        assistant_fenetre::ouvrir(app, chemins.clone(), avertissement)
    };
    match reprise {
        Ok(Some(message)) => {
            journal::noter("reprise après interruption", &message);
            dialogues::afficher_consigne(
                &fenetre,
                &tr("An interrupted operation was recovered"),
                &message,
            );
        }
        Ok(None) => {}
        Err(erreur) => dialogues::afficher_erreur(&fenetre, &erreur, false, |_| {}),
    }
}

/// Action `app.quitter` (Ctrl+Q) : **ferme** la fenêtre au lieu d'appeler `quit()` tout de suite,
/// pour que la confirmation « un travail est en cours » (`close-request`) s'applique.
fn ajouter_action_quitter(app: &adw::Application) {
    let quitter = gio::SimpleAction::new("quitter", None);
    let app_suivie = app.clone();
    quitter.connect_activate(move |_, _| match app_suivie.active_window() {
        Some(fenetre) => fenetre.close(),
        None => app_suivie.quit(),
    });
    app.add_action(&quitter);
}

/// Le dossier d'icônes des sources (`repli`), seulement s'il existe (`existe` est injecté pour
/// les tests). `None` quand il n'y a pas de repli (build de paquet).
///
/// Le chemin est figé à la compilation : sur une machine où WiMao est installé depuis un
/// paquet, les sources n'y sont pas et ce dossier n'existe pas ; le thème d'icônes système
/// suffit alors, inutile de lui ajouter un chemin mort.
fn dossier_icones_developpement(
    existe: impl Fn(&Path) -> bool,
    repli: Option<&Path>,
) -> Option<&Path> {
    repli.filter(|dossier| existe(dossier))
}

/// Ajoute le dossier d'icônes des sources au thème d'icônes, s'il existe
/// (voir [`ICONES_DEVELOPPEMENT`]). Sans effet dans un paquet.
fn ajouter_icones_developpement() {
    let Some(ecran) = gdk::Display::default() else {
        return;
    };
    let repli = ICONES_DEVELOPPEMENT.map(Path::new);
    if let Some(dossier) = dossier_icones_developpement(Path::exists, repli) {
        gtk4::IconTheme::for_display(&ecran).add_search_path(dossier);
    }
}

/// Charge `style.css` pour tout l'écran.
fn charger_style() {
    let Some(ecran) = gdk::Display::default() else {
        return;
    };
    let fournisseur = gtk4::CssProvider::new();
    fournisseur.load_from_string(STYLE);
    gtk4::style_context_add_provider_for_display(
        &ecran,
        &fournisseur,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un binaire installé (paquet `.deb`) n'a plus les sources à côté de lui : le dossier
    /// d'icônes de développement ne doit être proposé au thème que s'il existe vraiment, et
    /// jamais dans un build de paquet (aucun repli compilé).
    #[test]
    fn icones_de_developpement_seulement_si_le_dossier_existe() {
        let repli = Some(Path::new("/depot/icons"));
        assert_eq!(dossier_icones_developpement(|_| false, repli), None);
        assert_eq!(dossier_icones_developpement(|_| true, repli), repli);
        // Paquet : pas de repli, donc rien, même si « tout existe ».
        assert_eq!(dossier_icones_developpement(|_| true, None), None);
    }
}
