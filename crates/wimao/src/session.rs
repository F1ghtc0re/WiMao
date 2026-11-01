//! Session d'un gestionnaire : le fil qui la fait vivre, du côté du travail (sans widget).
//!
//! Une session (spec, sections 4.2 à 4.4) dure aussi longtemps que le gestionnaire est ouvert,
//! souvent de longues minutes. Elle vit donc dans un **fil dédié** ([`demarrer`]) et non dans
//! le fil de travail ordinaire (`travail.rs`), qui rend un seul résultat :
//! - le fil ouvre la session (`wimao_recettes::ouvrir`), puis appelle `Session::tick` toutes
//!   les [`INTERVALLE_PILOTE`] (2 s) et envoie chaque événement (relance du pilote, plugin
//!   ponté) au fil de GTK par un canal `async-channel`, lu par `glib::spawn_future_local`
//!   (comme `travail.rs`) ;
//! - le fil de GTK lui parle par une [`Telecommande`] (canal `std::sync::mpsc`) : « I'm done »
//!   ([`Commande::Terminer`]), « Stop anyway » ([`Commande::ArreterQuandMeme`]) ou fenêtre
//!   fermée ([`Commande::Abandonner`]) ; ces commandes viennent de `wimao_recettes`.
//!   L'attente de 2 s entre deux tours est un `recv_timeout` : une commande réveille le fil
//!   aussitôt, **sauf** pendant un tour : une relance synchrone du pilote dans `tick` peut
//!   durer jusqu'à 30 minutes, et la commande n'est lue qu'après (la fenêtre l'annonce, voir `session_fenetre.rs`) ;
//! - la décision de chaque tour est la fonction pure `wimao_recettes::suite`, la même que
//!   celle de la CLI (`gestionnaire ouvrir`), testée sans Wine ;
//! - à la fin, `Session::fermer` arrête le gestionnaire, ponte et teste ce qui reste ; les
//!   verdicts sont mémorisés (`etat.toml`) et le bilan revient à la fenêtre.
//!
//! Abandon (fenêtre fermée en pleine session, spec 4.4) : la session est détruite **sans**
//! `fermer` ; le gestionnaire reste ouvert et le pilote s'arrête. Le verrou d'opération n'est
//! libéré (`Drop`) que si le fil arrive jusque-là avant la fin du processus ; sinon le fichier
//! verrou reste, mais son processus est mort et il est ignoré à la prochaine opération. Au
//! prochain démarrage, la fenêtre propose de ponter ce qui manque (`reparation.rs`).
//!
//! Le côté fenêtre (bannière, dialogues, toasts) est dans `session_fenetre.rs`.
//! Utilise `wimao_recettes` (session), `operations.rs`, `travail.rs` (texte d'une panique).

use std::collections::HashSet;
use std::mem::Discriminant;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Instant;

use gtk4::glib;
use wimao_core::i18n::langue_courante;
use wimao_core::{journal, Chemins, Erreur, Progression, Resultat};
use wimao_recettes::{
    consigne, ouvrir, suite, BilanSession, Commande, EvenementSession, Phase, Session, Suite,
    CONSIGNE_CONNEXION, INTERVALLE_PILOTE,
};

use crate::operations::{self, Avertissements};
use crate::travail::texte_de_panique;

/// Titre des erreurs d'un tour de session dans le journal (elles ne coupent pas la session :
/// la prochaine échéance réessaie, comme dans la CLI).
const EVENEMENT_ERREUR_TOUR: &str = "session (interface) : erreur d'un tour";
/// Nom des opérations d'un tour, pour le journal (une erreur n'y est écrite qu'une fois).
const OPERATION_TICK: &str = "tick";
/// Voir [`OPERATION_TICK`].
const OPERATION_FERME: &str = "gestionnaire_ferme";
/// Voir [`OPERATION_TICK`].
const OPERATION_INSTALLEURS: &str = "installeurs_en_cours";

/// Ce que le fil de session envoie au fil de GTK.
#[derive(Debug)]
pub enum MessageSession {
    /// Une étape de l'ouverture, d'un tour ou de la fermeture (texte du bandeau de travail, ou
    /// de la bannière pendant la session).
    Etape(String),
    /// Le tour qui avait annoncé des étapes est fini : la bannière reprend son texte.
    FinDesEtapes,
    /// La session est ouverte ; `consigne` : consigne de connexion de la recette, à montrer
    /// une fois.
    Ouverte {
        /// Consigne `connexion` dans la langue de l'interface, s'il y en a une.
        consigne: Option<String>,
    },
    /// Un événement d'un tour (relance du pilote, plugin ponté).
    Evenement(EvenementSession),
    /// Fin demandée, mais ces installeurs tournent encore : la session les attend. Liste vide :
    /// installeurs pas encore identifiés (lecture de `/proc` en échec).
    Attente(Vec<String>),
    /// La fermeture commence (le dialogue d'attente, s'il est ouvert, n'a plus d'objet).
    Fermeture,
    /// Fin : bilan (et avertissements de la mémoire des verdicts) ou erreur d'ouverture ou de
    /// fermeture. Toujours le dernier message, sauf abandon ou panique.
    Fin(Resultat<(BilanSession, Avertissements)>),
    /// Le fil a paniqué ; texte de la panique (dernier message aussi).
    Panique(String),
}

/// Moyen de parler au fil de session. Se clone ; si le fil a fini, l'envoi est sans effet.
#[derive(Clone)]
pub struct Telecommande(mpsc::Sender<Commande>);

impl Telecommande {
    /// Envoie `commande` au fil de session (ignorée s'il est déjà terminé).
    pub fn envoyer(&self, commande: Commande) {
        let _ = self.0.send(commande);
    }
}

/// [`Progression`] qui poste chaque étape dans le canal du fil de GTK, et retient qu'une étape
/// a été annoncée (pour envoyer [`MessageSession::FinDesEtapes`] à la fin du tour).
struct ProgressionSession {
    /// Extrémité d'envoi du canal vers le fil de GTK.
    envoi: async_channel::Sender<MessageSession>,
    /// Vrai si une étape a été envoyée depuis la dernière [`ProgressionSession::fin_des_etapes`].
    etape_envoyee: AtomicBool,
}

impl ProgressionSession {
    /// Envoie `message` au fil de GTK ; fenêtre disparue : le message est perdu sans dommage.
    fn envoyer(&self, message: MessageSession) {
        let _ = self.envoi.send_blocking(message);
    }

    /// Si le tour a annoncé des étapes, prévient la fenêtre qu'elles sont finies.
    fn fin_des_etapes(&self) {
        if self.etape_envoyee.swap(false, Ordering::SeqCst) {
            self.envoyer(MessageSession::FinDesEtapes);
        }
    }
}

impl Progression for ProgressionSession {
    fn etape(&self, message: &str) {
        self.etape_envoyee.store(true, Ordering::SeqCst);
        self.envoyer(MessageSession::Etape(message.to_owned()));
    }
}

/// Mémoire d'un tour à l'autre : dernière liste d'installeurs lue avec succès, et erreurs déjà
/// écrites au journal (une seule fois par opération et par sorte d'erreur : la même erreur
/// reviendrait toutes les 2 s).
#[derive(Debug, Default)]
struct MemoireTours {
    /// Derniers installeurs en cours connus (gardés si la lecture suivante échoue : une erreur
    /// de `/proc` ne doit pas faire croire qu'ils ont fini et fermer la session). `None` tant
    /// qu'aucune lecture n'a réussi : inconnus, et non « aucun ».
    installeurs: Option<Vec<String>>,
    /// (opération, sorte d'erreur) déjà notées.
    erreurs_notees: HashSet<(&'static str, Discriminant<Erreur>)>,
}

impl MemoireTours {
    /// Note `erreur` de l'opération `operation` au journal si elle ne l'a pas déjà été ; rend
    /// vrai si elle vient d'être notée.
    fn noter(&mut self, c: &Chemins, operation: &'static str, erreur: &Erreur) -> bool {
        let nouvelle = self
            .erreurs_notees
            .insert((operation, std::mem::discriminant(erreur)));
        if nouvelle {
            journal::noter(
                EVENEMENT_ERREUR_TOUR,
                &format!("{} : {operation} : {erreur}", c.prefixe.display()),
            );
        }
        nouvelle
    }

    /// Installeurs lus, ou en cas d'erreur la dernière liste connue (`None` si aucune lecture
    /// n'a encore réussi).
    fn installeurs(&mut self, c: &Chemins, lus: Resultat<Vec<String>>) -> Option<Vec<String>> {
        match lus {
            Ok(liste) => self.installeurs = Some(liste),
            Err(erreur) => {
                self.noter(c, OPERATION_INSTALLEURS, &erreur);
            }
        }
        self.installeurs.clone()
    }
}

/// Démarre le fil de session du gestionnaire `nom` et rend sa télécommande.
///
/// `recevoir` est appelé dans le fil de GTK pour chaque [`MessageSession`], dans l'ordre. Doit
/// être appelée depuis le fil de GTK.
pub fn demarrer(
    c: Chemins,
    nom: String,
    recevoir: impl Fn(MessageSession) + 'static,
) -> Telecommande {
    let (envoi, reception) = async_channel::unbounded();
    let (commandes, ecoute) = mpsc::channel();
    std::thread::spawn(move || {
        let progression = ProgressionSession {
            envoi: envoi.clone(),
            etape_envoyee: AtomicBool::new(false),
        };
        // `AssertUnwindSafe` : après une panique, rien de la session n'est réutilisé.
        let issue = catch_unwind(AssertUnwindSafe(|| {
            fil_de_session(&c, &nom, &ecoute, &progression)
        }));
        let dernier = match issue {
            Ok(Some(fin)) => MessageSession::Fin(fin),
            Ok(None) => return, // abandon : la fenêtre est fermée, personne n'attend
            Err(charge) => MessageSession::Panique(texte_de_panique(charge.as_ref())),
        };
        let _ = envoi.send_blocking(dernier);
    });
    glib::spawn_future_local(async move {
        while let Ok(message) = reception.recv().await {
            recevoir(message);
        }
    });
    Telecommande(commandes)
}

/// Corps du fil : ouvre, surveille, ferme. Rend `None` en cas d'abandon (pas de bilan).
fn fil_de_session(
    c: &Chemins,
    nom: &str,
    ecoute: &mpsc::Receiver<Commande>,
    p: &ProgressionSession,
) -> Option<Resultat<(BilanSession, Avertissements)>> {
    let mut avertissements = Vec::new();
    let ouverture = operations::recette_nommee(c, nom, &mut avertissements)
        .and_then(|recette| Ok((ouvrir(c, &recette, p)?, recette)));
    let (mut session, recette) = match ouverture {
        Ok(ouverte) => ouverte,
        Err(erreur) => return Some(Err(erreur)),
    };
    let consigne = consigne(&recette, CONSIGNE_CONNEXION, &langue_courante());
    // Les étapes de l'ouverture sont finies ; la fenêtre passe à la bannière.
    p.etape_envoyee.store(false, Ordering::SeqCst);
    p.envoyer(MessageSession::Ouverte { consigne });
    if !surveiller(c, &mut session, ecoute, p) {
        // Session détruite sans `fermer` : verrou libéré, gestionnaire laissé ouvert.
        return None;
    }
    p.envoyer(MessageSession::Fermeture);
    Some(session.fermer(true, p).map(|bilan| {
        operations::memoriser(c, &bilan.plugins, &mut avertissements);
        // Erreurs reconnues dans le journal du gestionnaire (« use Repair »…) : montrées comme
        // les autres avertissements, après le bilan.
        avertissements.extend(bilan.avertissements.iter().cloned());
        (bilan, avertissements)
    }))
}

/// Boucle de surveillance : un tour toutes les [`INTERVALLE_PILOTE`] jusqu'à la décision de
/// fermer (`true`) ou d'abandonner (`false`). Une fenêtre disparue (canal des commandes
/// fermé) vaut abandon.
fn surveiller(
    c: &Chemins,
    session: &mut Session,
    ecoute: &mpsc::Receiver<Commande>,
    p: &ProgressionSession,
) -> bool {
    let mut phase = Phase::Surveillance;
    let mut commande = None;
    let mut memoire = MemoireTours::default();
    loop {
        let (ferme, installeurs) = faire_un_tour(c, session, p, &mut memoire);
        p.fin_des_etapes();
        match suite(phase, commande.take(), ferme, installeurs.as_deref()) {
            Suite::Continuer => {}
            Suite::Attendre => {
                phase = Phase::Attente;
                // Liste inconnue : la fenêtre dit « un installeur » sans le nommer.
                p.envoyer(MessageSession::Attente(installeurs.unwrap_or_default()));
            }
            Suite::Fermer => return true,
            Suite::Abandonner => return false,
        }
        commande = match ecoute.recv_timeout(INTERVALLE_PILOTE) {
            Ok(recue) => Some(recue),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(Commande::Abandonner),
        };
    }
}

/// Un tour : `tick` (événements envoyés à la fenêtre), puis état du gestionnaire et
/// installeurs en cours. Une erreur est notée au journal (une fois, voir [`MemoireTours`]) et
/// le tour continue : la prochaine échéance réessaie. Un gestionnaire illisible compte comme
/// ouvert et des installeurs illisibles comme toujours en cours (dernière liste connue, ou
/// `None` si aucune lecture n'a réussi), pour ne pas finir à tort.
fn faire_un_tour(
    c: &Chemins,
    session: &mut Session,
    p: &ProgressionSession,
    memoire: &mut MemoireTours,
) -> (bool, Option<Vec<String>>) {
    match session.tick(Instant::now(), p) {
        Ok(evenements) => {
            for evenement in evenements {
                p.envoyer(MessageSession::Evenement(evenement));
            }
        }
        Err(erreur) => {
            memoire.noter(c, OPERATION_TICK, &erreur);
        }
    }
    let ferme = session.gestionnaire_ferme().unwrap_or_else(|erreur| {
        memoire.noter(c, OPERATION_FERME, &erreur);
        false
    });
    let installeurs = memoire.installeurs(c, session.installeurs_en_cours());
    (ferme, installeurs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noms(liste: &[&str]) -> Vec<String> {
        liste.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn une_erreur_n_est_notee_qu_une_fois_et_les_installeurs_sont_gardes() {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let mut memoire = MemoireTours::default();
        let erreur = || Erreur::Io(std::io::Error::other("proc"));
        assert!(memoire.noter(&c, OPERATION_TICK, &erreur()));
        assert!(!memoire.noter(&c, OPERATION_TICK, &erreur()));
        // Même sorte d'erreur, autre opération : notée.
        assert!(memoire.noter(&c, OPERATION_FERME, &erreur()));
        assert_eq!(
            memoire.installeurs(&c, Ok(noms(&["Setup.exe"]))),
            Some(noms(&["Setup.exe"]))
        );
        // Lecture en échec : la dernière liste connue reste (la session continue d'attendre).
        assert_eq!(
            memoire.installeurs(&c, Err(erreur())),
            Some(noms(&["Setup.exe"]))
        );
        assert_eq!(memoire.installeurs(&c, Ok(Vec::new())), Some(Vec::new()));
    }

    #[test]
    fn des_installeurs_jamais_lus_ne_sont_pas_une_liste_vide() {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let mut memoire = MemoireTours::default();
        let erreur = || Erreur::Io(std::io::Error::other("proc"));
        // Première lecture en échec : liste inconnue, et non vide.
        assert_eq!(memoire.installeurs(&c, Err(erreur())), None);
        assert_eq!(memoire.installeurs(&c, Err(erreur())), None);
        // Inconnus : la session ne finit pas seule et « I'm done » attend.
        let (s, a) = (Phase::Surveillance, Phase::Attente);
        assert_eq!(suite(s, None, true, None), Suite::Continuer);
        assert_eq!(
            suite(s, Some(Commande::Terminer), false, None),
            Suite::Attendre
        );
        assert_eq!(suite(a, None, true, None), Suite::Continuer);
        // « Stop anyway » permet toujours de sortir.
        assert_eq!(
            suite(a, Some(Commande::ArreterQuandMeme), false, None),
            Suite::Fermer
        );
        // Une lecture réussie, même vide, débloque la fin.
        assert_eq!(memoire.installeurs(&c, Ok(Vec::new())), Some(Vec::new()));
        assert_eq!(suite(a, None, false, Some(&[])), Suite::Fermer);
    }
}
