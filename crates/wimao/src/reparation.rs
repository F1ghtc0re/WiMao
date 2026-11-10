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
use wimao_core::{commande_lien_yabridge, journal, message_probleme, Probleme};

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
        // Un simple avertissement (lien yabridge) ne se répare pas : il ne compte pas comme un
        // problème réparé. Son information (message + commande à copier) vient dans une boîte.
        let reparables = problemes.iter().any(Probleme::est_reparable);
        let lien = problemes
            .iter()
            .find(|p| {
                matches!(
                    p,
                    Probleme::LienYabridge { .. } | Probleme::FlatpakSansAcces { .. }
                )
            })
            .cloned();
        let (toast, boite) = issue_reparation(reparables, lien.is_some());
        self.lancer(
            &tr("Repairing…"),
            move |p| operations::reparer(&c, &problemes, p),
            move |resultat| match resultat {
                Err(erreur) => moi.erreur(&erreur, false),
                Ok(avertissements) => {
                    if toast {
                        moi.toast(&if reparables {
                            tr("Repair finished")
                        } else {
                            tr("No problem found")
                        });
                    }
                    moi.avertir(&avertissements);
                    if let (true, Some(lien)) = (boite, lien) {
                        moi.informer_lien(&lien);
                    }
                    // Une réparation du pontage ajoute des plugins à la liste.
                    moi.rafraichir();
                }
            },
        );
    }

    /// Boîte « Information » d'un avertissement (lien yabridge ou permissions Flatpak) : message du diagnostic, bouton qui copie la
    /// commande (toast « Command copied »). WiMao ne lance jamais cette commande.
    fn informer_lien(&self, probleme: &Probleme) {
        // La commande à copier : celle du lien, ou celle de `flatpak override`.
        let commande = match probleme {
            Probleme::LienYabridge {
                lien,
                attendu,
                etat,
            } => commande_lien_yabridge(lien, attendu, etat),
            Probleme::FlatpakSansAcces { commande, .. } => commande.clone(),
            _ => return,
        };
        let moi = self.clone();
        dialogues::informer_lien_yabridge(&self.fenetre, &message_probleme(probleme), move || {
            dialogues::copier(&moi.fenetre, &commande);
            moi.toast(&tr("Command copied"));
        });
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

/// Ce que montre la fin d'une réparation : `(toast, boîte)`.
///
/// - `reparables` : au moins un vrai problème a été réparé ;
/// - `avertissement_lien` : le diagnostic a signalé le lien yabridge (avertissement).
///
/// Le toast dit « Repair finished » (réparé) ou « No problem found » (rien du tout). Avec le
/// seul avertissement de lien, il est omis : « No problem found » serait faux, puisque la boîte
/// d'information s'ouvre, et elle s'ouvre dès qu'il y a cet avertissement, réparation ou non.
fn issue_reparation(reparables: bool, avertissement_lien: bool) -> (bool, bool) {
    // Rien du tout (ni réparé, ni lien) : le toast « rien à réparer » reste utile.
    (reparables || !avertissement_lien, avertissement_lien)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_quatre_issues_de_la_reparation() {
        // (réparables, avertissement de lien) -> (toast, boîte)
        assert_eq!(issue_reparation(true, false), (true, false));
        assert_eq!(issue_reparation(true, true), (true, true));
        assert_eq!(issue_reparation(false, true), (false, true));
        assert_eq!(issue_reparation(false, false), (true, false));
    }
}
