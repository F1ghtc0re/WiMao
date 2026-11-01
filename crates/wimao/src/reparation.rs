//! « Repair » dans la fenêtre, et la proposition de ponter au démarrage.
//!
//! - [`Fenetre::reparer`] : diagnostic dans le fil de travail, puis, si la réparation
//!   fermerait des programmes ouverts (toute réparation arrête Wine), la question qui les nomme
//!   (même décision que la CLI : `programmes_a_nommer_avant_reparation`, spec 4.5) : fermer et
//!   réparer, ou annuler **toute** la réparation. La réparation réapplique ensuite toujours
//!   les réglages des gestionnaires installés et redémarre leurs services ;
//! - [`Fenetre::verifier_pontage`] : au démarrage, si le diagnostic trouve des plugins non
//!   pontés (par exemple WiMao fermé pendant une session, spec 4.4), un toast propose
//!   « Repair », qui les ponte (après la même question si des programmes sont ouverts : un
//!   gestionnaire resté ouvert serait fermé). Ce toast est retiré dès qu'un travail ou une session commence
//!   ([`Fenetre::retirer_toast_pontage`], appelé par `lancer` et `ouvrir_gestionnaire`).
//!
//! Ajoute des méthodes à `Fenetre` (même motif que `gestionnaires.rs` et `session_fenetre.rs`).
//! Utilise `operations.rs`, `dialogues.rs`, `travail.rs`. Utilisé par `fenetre/mod.rs` (action
//! `win.reparer`, démarrage, bouton « Repair » des dialogues d'erreur).

use libadwaita as adw;
use wimao_core::i18n::{tr, trn};
use wimao_core::{journal, Probleme};

use crate::dialogues;
use crate::fenetre::Fenetre;
use crate::operations::{self, ReparationPrevue};
use crate::travail::en_arriere_plan;

/// Durée d'affichage d'un toast qui propose une action (secondes) : le temps de la lire.
const DUREE_TOAST_ACTION: u32 = 15;

impl Fenetre {
    /// « Repair » : répare tout, ou seulement `problemes` (dialogue d'erreur, toast de
    /// pontage). Diagnostique d'abord, puis demande confirmation si la réparation fermerait des
    /// programmes ouverts (même décision que la CLI, spec 4.5).
    pub(crate) fn reparer(&self, problemes: Option<Vec<Probleme>>) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        self.lancer(
            &tr("Looking for problems…"),
            move |_| operations::preparer_reparation(&c, problemes),
            move |resultat| match resultat {
                // Pas de « Repair » dans l'erreur d'une réparation (voir `Fenetre::erreur`).
                Err(erreur) => moi.erreur(&erreur, false),
                Ok(prevue) => moi.confirmer_reparation(prevue),
            },
        );
    }

    /// Suite de « Repair » : réparation directe, ou question sur les programmes ouverts.
    /// Refuser de les fermer annule toute la réparation (rien n'est modifié). Sans problème
    /// trouvé, la réparation est quand même lancée : elle réapplique les réglages des
    /// gestionnaires et redémarre leurs services.
    fn confirmer_reparation(&self, prevue: ReparationPrevue) {
        if prevue.programmes.is_empty() {
            return self.lancer_reparation(prevue.problemes);
        }
        let moi = self.clone();
        let problemes = prevue.problemes;
        dialogues::confirmer_fermeture_programmes(&self.fenetre, &prevue.programmes, move |oui| {
            if oui {
                moi.lancer_reparation(problemes);
            } else {
                moi.toast(&tr("Cancelled: nothing was changed"));
            }
        });
    }

    /// Répare `problemes` dans le fil de travail, puis réapplique les réglages des
    /// gestionnaires et redémarre leurs services.
    fn lancer_reparation(&self, problemes: Vec<Probleme>) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        // Un simple avertissement (lien yabridge) ne se répare pas : il ne compte pas comme un problème
        // réparé (son texte, lui, est montré par « Diagnose » de la ligne de commande).
        let aucun_probleme = !problemes.iter().any(Probleme::est_reparable);
        self.lancer(
            &tr("Repairing…"),
            move |p| operations::reparer(&c, &problemes, p),
            move |resultat| match resultat {
                Err(erreur) => moi.erreur(&erreur, false),
                Ok(avertissements) => {
                    moi.toast(&if aucun_probleme {
                        tr("No problem found")
                    } else {
                        tr("Repair finished")
                    });
                    moi.avertir(&avertissements);
                    // Une réparation du pontage ajoute des plugins à la liste.
                    moi.rafraichir();
                }
            },
        );
    }

    /// Au démarrage : si des plugins ne sont pas pontés (par exemple WiMao fermé pendant une
    /// session, spec 4.4), un toast propose « Repair », qui les ponte. Une erreur du
    /// diagnostic est seulement notée : « Repair » la montrera si on le demande.
    pub(crate) fn verifier_pontage(&self) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        en_arriere_plan(
            move |_| operations::plugins_non_pontes(&c),
            |_| {},
            move |resultat| match resultat {
                Ok(Some(nombre)) => moi.proposer_pontage(nombre),
                Ok(None) => {}
                Err(erreur) => journal::noter("diagnostic au démarrage", &erreur.to_string()),
            },
            |cause| journal::noter("panique du diagnostic au démarrage", &cause),
        );
    }

    /// Toast « N plugins are not bridged yet » avec le bouton « Repair » (plugins non pontés,
    /// par exemple après une session interrompue par la fermeture de WiMao).
    fn proposer_pontage(&self, nombre: usize) {
        let toast = adw::Toast::builder()
            .title(trn(
                "{n} plugin is not bridged yet",
                "{n} plugins are not bridged yet",
                nombre as u64,
                &[],
            ))
            .button_label(tr("Repair"))
            .timeout(DUREE_TOAST_ACTION)
            .build();
        let moi = self.clone();
        toast.connect_button_clicked(move |_| {
            // Filet de sécurité : le toast est retiré quand un travail commence
            // (`retirer_toast_pontage`), mais `lancer` refuserait sans rien dire.
            if moi.occupe.get() {
                moi.toast(&tr("A task is already running"));
            } else {
                moi.reparer(Some(vec![Probleme::PontageIncomplet(nombre)]));
            }
        });
        // Fermé (délai, bouton, retrait) : on l'oublie, pour ne jamais le retirer deux fois.
        let oubli = self.toast_pontage.clone();
        toast.connect_dismissed(move |_| {
            oubli.borrow_mut().take();
        });
        *self.toast_pontage.borrow_mut() = Some(toast.clone());
        self.ajouter_toast(toast);
    }

    /// Retire le toast de pontage s'il est affiché : appelé au début de tout travail et de
    /// toute session, pendant lesquels « Repair » n'est pas disponible. Le toast est sorti de
    /// la `RefCell` **avant** `dismiss`, dont le signal `dismissed` la reprend.
    pub(crate) fn retirer_toast_pontage(&self) {
        let toast = self.toast_pontage.borrow_mut().take();
        if let Some(toast) = toast {
            toast.dismiss();
        }
    }
}
