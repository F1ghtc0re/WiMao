//! Session d'un gestionnaire, côté fenêtre : bannière, dialogues, toasts, actions coupées.
//!
//! Ce module ajoute à `Fenetre` l'ouverture d'une session (le fil vit dans `session.rs`) et le
//! traitement de chaque [`MessageSession`]. Une session passe par les états de [`EtatSession`] ;
//! la bannière `adw::Banner` de la session (distincte du bandeau de travail : teinte d'accent,
//! `.banniere-session` dans `style.css`) suit cet état :
//!
//! | état | titre de la bannière | bouton |
//! |---|---|---|
//! | `Ouverte` | « `<nom> is open — WiMao bridges new plugins` » | « I'm done » |
//! | `FinDemandee` | « Finishing the session — a silent installation may still be running… » | aucun |
//! | `Attente` | « Waiting for the installers to finish… » | « Stop anyway » |
//!
//! Pendant `Ouverture` et `Fermeture`, la bannière est cachée et le bandeau de travail montre
//! les étapes. Pendant la session, les étapes d'un tour (pontage…) prennent un moment la place
//! du titre de la bannière, qui revient à la fin du tour ([`MessageSession::FinDesEtapes`]).
//!
//! « I'm done » n'est lu par le fil qu'à la fin du tour en cours ; une relance synchrone du
//! pilote peut durer jusqu'à 30 minutes : d'où le texte de l'état `FinDemandee`.
//!
//! Le dialogue d'attente des installeurs (« Keep waiting » / « Stop anyway ») peut être fermé ;
//! « Stop anyway » reste alors dans la bannière (spec 4.4, étape 1). Il est oublié dès qu'il
//! est fermé, pour ne jamais fermer deux fois un dialogue.
//!
//! Utilise `session.rs`, `operations.rs`, `dialogues.rs`, `fenetre/mod.rs`. Utilisé par
//! `fenetre/mod.rs` (bouton de la bannière, fermeture) et `gestionnaires.rs` (action « Open »).

use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{tr, trf};
use wimao_core::Resultat;
use wimao_recettes::{texte_evenement, BilanSession, EvenementSession};

use crate::dialogues;
use crate::fenetre::Fenetre;
use crate::operations::{self, Avertissements};
use crate::session::{demarrer, MessageSession, Telecommande};
use wimao_recettes::Commande;

/// Où en est la session, vue de la fenêtre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EtatSession {
    /// `ouvrir` tourne (bandeau de travail).
    Ouverture,
    /// Le gestionnaire est ouvert, WiMao surveille.
    Ouverte,
    /// « I'm done » envoyé, le fil ne l'a pas encore traité.
    FinDemandee,
    /// Des installeurs tournent : le fil attend qu'ils finissent.
    Attente,
    /// `fermer` tourne (bandeau de travail) : quitter n'abandonne plus la session.
    Fermeture,
}

/// Session en cours, vue de la fenêtre.
pub struct SessionEnCours {
    /// Nom du gestionnaire (messages, infobulles).
    pub nom: String,
    /// Pour parler au fil de session.
    pub telecommande: Telecommande,
    /// État courant.
    pub etat: EtatSession,
    /// Dialogue d'attente des installeurs **tant qu'il est affiché** (oublié à sa fermeture).
    pub attente: Option<adw::AlertDialog>,
}

impl Fenetre {
    /// « Open » : ouvre une session du gestionnaire `nom` dans un fil dédié.
    pub(crate) fn ouvrir_gestionnaire(&self, nom: String) {
        if self.occupe.replace(true) {
            return; // un travail est déjà en cours
        }
        self.retirer_toast_pontage();
        self.basculer_occupe(true, &trf("Opening {name}…", &[("name", &nom)]));
        let moi = self.clone();
        let telecommande = demarrer((*self.chemins).clone(), nom.clone(), move |message| {
            moi.sur_message_session(message)
        });
        *self.session.borrow_mut() = Some(SessionEnCours {
            nom: nom.clone(),
            telecommande,
            etat: EtatSession::Ouverture,
            attente: None,
        });
        self.expliquer_indisponibles(Some(&nom));
        self.redessiner_gestionnaires();
    }

    /// Nom du gestionnaire de la session en cours, quel que soit son état.
    pub(crate) fn nom_session(&self) -> Option<String> {
        self.session.borrow().as_ref().map(|s| s.nom.clone())
    }

    /// État de la session en cours.
    fn etat_session(&self) -> Option<EtatSession> {
        self.session.borrow().as_ref().map(|s| s.etat)
    }

    /// Nom du gestionnaire si une session est ouverte et pas encore en fermeture : fermer la
    /// fenêtre l'abandonnerait (voir [`Fenetre::abandonner_session`]).
    pub(crate) fn session_a_abandonner(&self) -> Option<String> {
        let session = self.session.borrow();
        session
            .as_ref()
            .filter(|s| s.etat != EtatSession::Fermeture)
            .map(|s| s.nom.clone())
    }

    /// Fenêtre fermée pendant la session : le pilote s'arrête, le gestionnaire reste ouvert.
    pub(crate) fn abandonner_session(&self) {
        if let Some(session) = self.session.borrow_mut().take() {
            session.telecommande.envoyer(Commande::Abandonner);
        }
    }

    /// Envoie `commande` au fil de session et passe la fenêtre à l'état `etat`.
    fn commander(&self, commande: Commande, etat: EtatSession) {
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.telecommande.envoyer(commande);
            session.etat = etat;
        }
        self.mettre_a_jour_banniere();
    }

    /// Bouton de la bannière : « I'm done » ou « Stop anyway » selon l'état.
    pub(crate) fn bouton_banniere_session(&self) {
        match self.etat_session() {
            Some(EtatSession::Ouverte) => {
                self.commander(Commande::Terminer, EtatSession::FinDemandee);
            }
            Some(EtatSession::Attente) => {
                // Le dialogue d'attente pose la même question : il n'a plus d'objet.
                self.fermer_dialogue_attente();
                self.commander(Commande::ArreterQuandMeme, EtatSession::FinDemandee);
            }
            _ => {}
        }
    }

    /// Titre et bouton de la bannière d'après l'état (voir le tableau du module).
    fn mettre_a_jour_banniere(&self) {
        let Some((nom, etat)) = self
            .session
            .borrow()
            .as_ref()
            .map(|s| (s.nom.clone(), s.etat))
        else {
            return;
        };
        let (titre, bouton) = match etat {
            EtatSession::Ouverte => (
                trf(
                    "{name} is open — WiMao bridges new plugins",
                    &[("name", &nom)],
                ),
                Some(tr("I'm done")),
            ),
            EtatSession::FinDemandee => (
                tr("Finishing the session — a silent installation may still be running…"),
                None,
            ),
            EtatSession::Attente => (
                tr("Waiting for the installers to finish…"),
                Some(tr("Stop anyway")),
            ),
            EtatSession::Ouverture | EtatSession::Fermeture => {
                self.banniere_session.set_revealed(false);
                return;
            }
        };
        self.banniere_session.set_title(&titre);
        self.banniere_session.set_button_label(bouton.as_deref());
        self.banniere_session.set_revealed(true);
    }

    /// Traite un message du fil de session (dans le fil de GTK).
    fn sur_message_session(&self, message: MessageSession) {
        match message {
            MessageSession::Etape(texte) => self.afficher_etape(&texte),
            MessageSession::FinDesEtapes => self.mettre_a_jour_banniere(),
            MessageSession::Ouverte { consigne } => self.session_ouverte(consigne),
            MessageSession::Evenement(evenement @ EvenementSession::Relance(_)) => {
                self.toast(&texte_evenement(&evenement));
            }
            // Le plugin ponté apparaît dans la liste, avec la pastille « Not tested ».
            MessageSession::Evenement(EvenementSession::Ponte(_)) => self.rafraichir(),
            MessageSession::Attente(installeurs) => self.attendre_installeurs(&installeurs),
            MessageSession::Fermeture => self.session_en_fermeture(),
            MessageSession::Fin(resultat) => self.fin_de_session(resultat),
            MessageSession::Panique(cause) => {
                self.terminer_session();
                self.panique(&cause);
            }
        }
    }

    /// Une étape : dans le bandeau de travail à l'ouverture et à la fermeture, dans la
    /// bannière pendant la session (le bandeau y est caché).
    fn afficher_etape(&self, texte: &str) {
        match self.etat_session() {
            Some(EtatSession::Ouverture | EtatSession::Fermeture) | None => {
                self.bandeau.set_title(texte);
            }
            Some(_) => self.banniere_session.set_title(texte),
        }
    }

    /// Session ouverte : bandeau de travail caché, bannière de session, consigne.
    fn session_ouverte(&self, consigne: Option<String>) {
        let Some(nom) = self.nom_session() else {
            return;
        };
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.etat = EtatSession::Ouverte;
        }
        self.montrer_indicateur(false, "");
        self.mettre_a_jour_banniere();
        if let Some(texte) = consigne {
            let titre = trf("Sign in to {name}", &[("name", &nom)]);
            dialogues::afficher_consigne(&self.fenetre, &titre, &texte);
        }
    }

    /// Des installeurs tournent encore : bannière avec « Stop anyway », et dialogue d'attente.
    fn attendre_installeurs(&self, installeurs: &[String]) {
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.etat = EtatSession::Attente;
        }
        self.mettre_a_jour_banniere();
        let moi = self.clone();
        let dialogue =
            dialogues::attendre_installeurs(&self.fenetre, installeurs, move |arreter| {
                // Le dialogue est fermé : on l'oublie (il ne doit plus être fermé par la fenêtre).
                if let Some(session) = moi.session.borrow_mut().as_mut() {
                    session.attente = None;
                }
                if arreter {
                    moi.commander(Commande::ArreterQuandMeme, EtatSession::FinDemandee);
                }
            });
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.attente = Some(dialogue);
        }
    }

    /// Ferme le dialogue d'attente s'il est encore affiché. Il est sorti de la session
    /// **avant** `force_close`, dont le rappel reprend la session.
    fn fermer_dialogue_attente(&self) {
        let dialogue = self
            .session
            .borrow_mut()
            .as_mut()
            .and_then(|s| s.attente.take());
        if let Some(dialogue) = dialogue {
            dialogue.force_close();
        }
    }

    /// La fermeture commence : bannière cachée, bandeau de travail (étapes de `fermer`).
    fn session_en_fermeture(&self) {
        self.fermer_dialogue_attente();
        let nom = self.nom_session().unwrap_or_default();
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.etat = EtatSession::Fermeture;
        }
        self.mettre_a_jour_banniere();
        self.montrer_indicateur(true, &trf("Closing {name}…", &[("name", &nom)]));
    }

    /// Fin de session : interface remise en état, puis bilan ou erreur.
    fn fin_de_session(&self, resultat: Resultat<(BilanSession, Avertissements)>) {
        let nom = self.terminer_session();
        match resultat {
            Err(erreur) => self.erreur(&erreur, true),
            Ok((bilan, avertissements)) => {
                let message = match operations::bilan_session(&bilan) {
                    Some(texte) => trf(
                        "{name} closed: {summary}",
                        &[("name", &nom), ("summary", &texte)],
                    ),
                    None => trf("{name} closed: no new or updated plugin", &[("name", &nom)]),
                };
                self.toast(&message);
                self.noter_mis_a_jour(&bilan.mis_a_jour);
                self.avertir(&avertissements);
            }
        }
        // Après une erreur aussi : un gestionnaire a pu s'installer ou des plugins s'ajouter.
        self.rafraichir();
    }

    /// Oublie la session et remet l'interface au repos ; rend le nom du gestionnaire.
    fn terminer_session(&self) -> String {
        self.fermer_dialogue_attente();
        let session = self.session.borrow_mut().take();
        self.banniere_session.set_revealed(false);
        self.expliquer_indisponibles(None);
        self.fin_de_travail();
        self.redessiner_gestionnaires();
        session.map(|s| s.nom).unwrap_or_default()
    }
}
