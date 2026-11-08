//! Les actions `win.*` de la fenêtre et leurs raccourcis clavier.
//!
//! - [`Fenetre::creer_actions`] crée toutes les actions (ajouter un installeur, réparer,
//!   sauvegarder, restaurer, annuler l'import, copier le rapport, historique, retour en arrière,
//!   mode d'emploi, à propos, gestionnaires, langue) et dit lesquelles se coupent pendant un travail (pas l'historique) ;
//! - [`Fenetre::action`] et [`Fenetre::action_nommee`] sont les deux fabriques d'actions
//!   (sans paramètre, ou avec un nom de recette en paramètre) ;
//! - [`Fenetre::action_langue`] et [`Fenetre::changer_langue`] portent le choix de la langue.
//!
//! Ajoute des méthodes à `Fenetre` (même motif que `gestionnaires.rs`). Utilise `dialogues.rs`,
//! `gestionnaires.rs` et `reglages.rs`, et les méthodes de `fenetre/mod.rs` que les actions
//! déclenchent. Utilisé par `fenetre/mod.rs` (`nouvelle`) ; le menu qui les affiche est dans
//! `fenetre/entete.rs`, qui fournit aussi [`LANGUE_SYSTEME`].

use gtk4::{gio, glib, prelude::*};
use libadwaita as adw;
use wimao_core::i18n::init_langue;

use super::entete::LANGUE_SYSTEME;
use super::Fenetre;
use crate::aide_fenetre;
use crate::dialogues;
use crate::gestionnaires;
use crate::historique_vue::ACTION_REVENIR;
use crate::reglages::Reglages;
use crate::restaurer::{ACTION_ANNULER_IMPORT, ACTION_RESTAURER};

impl Fenetre {
    /// Crée les actions `win.*` et les raccourcis clavier.
    pub(super) fn creer_actions(&self, app: &adw::Application) {
        let mut a_couper = Vec::new();
        let ajouter = self.action("ajouter", |f| f.demander_installeur());
        let reparer = self.action("reparer", |f| f.reparer(None));
        let sauvegarder = self.action("sauvegarder", |f| f.demander_sauvegarde());
        let rapport = self.action("rapport", |f| f.copier_rapport());
        // Consulter l'historique reste possible pendant un travail : cette action n'est pas
        // coupée. Seuls les boutons « Go back to before this » (`win.revenir`) le sont.
        self.action("historique", |f| f.ouvrir_historique());
        let restaurer = self.action(ACTION_RESTAURER, |f| f.demander_restauration());
        let annuler_import = self.action(ACTION_ANNULER_IMPORT, |f| f.demander_annulation_import());
        let revenir = self.action_nommee(ACTION_REVENIR, |f, id| f.demander_retour(id));
        self.action("a-propos", |f| dialogues::a_propos(&f.fenetre));
        self.action("aide", |f| aide_fenetre::ouvrir_aide(&f.fenetre, None));
        let installer = self.action_nommee(gestionnaires::ACTION_INSTALLER, |f, nom| {
            f.installer_gestionnaire(nom)
        });
        // Ouvrir un gestionnaire (re)démarre Wine : même question que pour un installeur.
        let ouvrir = self.action_nommee(gestionnaires::ACTION_OUVRIR, |f, nom| {
            let suite = f.clone();
            f.avant_d_arreter_wine(move || suite.ouvrir_gestionnaire(nom));
        });
        a_couper.extend([
            ajouter,
            reparer,
            sauvegarder,
            rapport,
            installer,
            ouvrir,
            revenir,
            restaurer,
            annuler_import,
        ]);
        a_couper.push(self.action_langue());
        // Quitter pendant un travail passe aussi par la confirmation de `close-request`.
        if let Some(quitter) = app
            .lookup_action("quitter")
            .and_downcast::<gio::SimpleAction>()
        {
            a_couper.push(quitter);
        }
        *self.actions_travail.borrow_mut() = a_couper;
        app.set_accels_for_action("win.ajouter", &["<Ctrl>o"]);
        app.set_accels_for_action("win.historique", &["<Ctrl>h"]);
        app.set_accels_for_action("app.quitter", &["<Ctrl>q"]);
        // « À propos » n'a pas de raccourci : F1 est réservé au mode d'emploi.
        app.set_accels_for_action("win.aide", &["F1"]);
    }

    /// Crée l'action `win.<nom>` : appelle `f` avec la fenêtre.
    pub(super) fn action(&self, nom: &str, f: impl Fn(&Fenetre) + 'static) -> gio::SimpleAction {
        let action = gio::SimpleAction::new(nom, None);
        let moi = self.clone();
        action.connect_activate(move |_, _| f(&moi));
        self.fenetre.add_action(&action);
        action
    }

    /// Crée l'action `win.<nom>` à paramètre texte (le nom d'une recette) : appelle `f` avec
    /// la fenêtre et ce texte.
    fn action_nommee(
        &self,
        nom: &str,
        f: impl Fn(&Fenetre, String) + 'static,
    ) -> gio::SimpleAction {
        let action = gio::SimpleAction::new(nom, Some(glib::VariantTy::STRING));
        let moi = self.clone();
        action.connect_activate(move |_, valeur| {
            if let Some(texte) = valeur.and_then(|v| v.get::<String>()) {
                f(&moi, texte);
            }
        });
        self.fenetre.add_action(&action);
        action
    }

    /// Action `win.langue` : menu à choix unique ; enregistre le choix et reconstruit la fenêtre.
    fn action_langue(&self) -> gio::SimpleAction {
        let actuelle = Reglages::charger(&self.chemins.config)
            .0
            .langue
            .unwrap_or_else(|| LANGUE_SYSTEME.to_owned());
        let action = gio::SimpleAction::new_stateful(
            "langue",
            Some(glib::VariantTy::STRING),
            &actuelle.to_variant(),
        );
        let moi = self.clone();
        action.connect_activate(move |action, valeur| {
            if let Some(code) = valeur.and_then(|v| v.get::<String>()) {
                // Le choix affiché ne change qu'une fois le réglage vraiment enregistré.
                if moi.changer_langue(&code) {
                    action.set_state(&code.to_variant());
                }
            }
        });
        self.fenetre.add_action(&action);
        action
    }

    /// Enregistre la langue, l'applique, et remplace la fenêtre par une neuve dans cette langue
    /// (les textes d'une fenêtre ouverte ne se retraduisent pas).
    ///
    /// Rend `false` (sans rien changer) si le réglage n'a pas pu être enregistré.
    fn changer_langue(&self, code: &str) -> bool {
        let reglages = Reglages {
            langue: (code != LANGUE_SYSTEME).then(|| code.to_owned()),
        };
        if let Err(erreur) = reglages.enregistrer(&self.chemins.config) {
            self.erreur(&erreur, false);
            return false;
        }
        init_langue(reglages.langue.as_deref());
        if let Some(app) = self
            .fenetre
            .application()
            .and_downcast::<adw::Application>()
        {
            // Les marques « Updated » passent à la nouvelle fenêtre : copie prise avant la
            // fermeture de l'ancienne, qui vide l'ensemble.
            let mis_a_jour = self.mis_a_jour.borrow().clone();
            Fenetre::nouvelle(&app, self.chemins.clone(), None, mis_a_jour).presenter();
        }
        self.fenetre.close();
        true
    }
}
