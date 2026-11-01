//! Exécution d'un travail long dans un fil, sans figer la fenêtre.
//!
//! Les fonctions du noyau (installation, sauvegarde, réparation…) peuvent durer des minutes
//! et bloquent : appelées dans le fil de GTK, elles gèleraient la fenêtre. [`en_arriere_plan`]
//! les lance dans un fil séparé et ramène les messages de progression puis le résultat dans
//! le fil de GTK, seul autorisé à toucher aux widgets, par un canal `async-channel` lu par
//! `glib::spawn_future_local`.
//!
//! Un travail qui panique est détecté (`catch_unwind`) et signalé à `sur_panique` : l'interface
//! ne reste jamais bloquée en « travail en cours ».
//!
//! Utilise `wimao_core::Progression`. Utilisé par `fenetre/mod.rs` pour chaque action.
//! Le test ci-dessous tourne sans affichage : il n'emploie que la boucle d'événements de
//! GLib, pas GTK (voir `CONTRIBUTING.md`).

use std::panic::{catch_unwind, AssertUnwindSafe};

use gtk4::glib;
use wimao_core::Progression;

/// Ce que le fil de travail envoie au fil de GTK.
enum Message<T> {
    /// Une nouvelle étape a commencé.
    Etape(String),
    /// Le travail est fini et rend ce résultat (toujours le dernier message).
    Fin(T),
    /// Le travail a paniqué ; le texte décrit la panique (dernier message aussi).
    Panique(String),
}

/// [`Progression`] qui poste chaque étape dans le canal du fil de GTK.
struct ProgressionCanal<T> {
    /// Extrémité d'envoi du canal (clonable, utilisable depuis plusieurs fils).
    envoi: async_channel::Sender<Message<T>>,
}

impl<T: Send> Progression for ProgressionCanal<T> {
    fn etape(&self, message: &str) {
        // Si la fenêtre a disparu, plus personne n'écoute : on perd le message, sans plus.
        let _ = self.envoi.send_blocking(Message::Etape(message.to_owned()));
    }
}

/// Texte lisible d'une charge de panique (`&str` ou `String` dans presque tous les cas).
/// Aussi utilisé par le fil de session (`session.rs`).
pub(crate) fn texte_de_panique(charge: &(dyn std::any::Any + Send)) -> String {
    charge
        .downcast_ref::<&str>()
        .map(|t| (*t).to_owned())
        .or_else(|| charge.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_owned())
}

/// Exécute `travail` dans un nouveau fil, puis rappelle `sur_etape` et `fin` dans le fil de GTK.
///
/// - `travail` reçoit une [`Progression`] ; chaque `etape(message)` arrive à `sur_etape`,
///   dans l'ordre ;
/// - `fin` reçoit le résultat de `travail`, après toutes les étapes ;
/// - si `travail` **panique** (ou si le fil disparaît sans résultat), `sur_panique` reçoit un
///   texte décrivant la cause et `fin` n'est pas appelée : l'appelant remet l'interface en état
///   et prévient l'utilisateur, au lieu de rester bloqué en « travail en cours » ;
/// - doit être appelée depuis le fil de GTK (celui qui fait tourner la boucle principale).
///
/// Ajouts par rapport à une simple paire `travail`/`fin` : `sur_etape` (message courant du
/// bandeau) et `sur_panique`.
pub fn en_arriere_plan<T: Send + 'static>(
    travail: impl FnOnce(&dyn Progression) -> T + Send + 'static,
    sur_etape: impl Fn(String) + 'static,
    fin: impl FnOnce(T) + 'static,
    sur_panique: impl FnOnce(String) + 'static,
) {
    let (envoi, reception) = async_channel::unbounded();
    std::thread::spawn(move || {
        let progression = ProgressionCanal {
            envoi: envoi.clone(),
        };
        // `AssertUnwindSafe` : on ne réutilise rien du travail après une panique, tout est jeté.
        let issue = catch_unwind(AssertUnwindSafe(|| travail(&progression)));
        let message = match issue {
            Ok(resultat) => Message::Fin(resultat),
            Err(charge) => Message::Panique(texte_de_panique(charge.as_ref())),
        };
        let _ = envoi.send_blocking(message);
    });
    glib::spawn_future_local(async move {
        while let Ok(message) = reception.recv().await {
            match message {
                Message::Etape(texte) => sur_etape(texte),
                Message::Fin(resultat) => return fin(resultat),
                Message::Panique(texte) => return sur_panique(texte),
            }
        }
        // Canal fermé sans « fin » : le fil a disparu sans rien dire.
        sur_panique("the worker thread ended without a result".to_owned());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    /// Délai maximal d'un test : au-delà la boucle est arrêtée et les assertions échouent,
    /// au lieu de tourner à l'infini.
    const DELAI_MAX: Duration = Duration::from_secs(10);

    /// Lance `en_arriere_plan` dans un contexte GLib neuf et attend `fin` ou `sur_panique`
    /// (au plus [`DELAI_MAX`]). Rend les étapes reçues et l'issue (`Ok(valeur)` ou `Err(panique)`).
    fn executer(
        travail: impl FnOnce(&dyn Progression) -> i32 + Send + 'static,
    ) -> (Vec<String>, Option<Result<i32, String>>) {
        // Un contexte neuf, déclaré « par défaut » pour ce fil : `spawn_future_local` y poste.
        let contexte = glib::MainContext::new();
        let boucle = glib::MainLoop::new(Some(&contexte), false);
        let etapes = Rc::new(RefCell::new(Vec::new()));
        let issue = Rc::new(RefCell::new(None));
        contexte
            .with_thread_default(|| {
                let (e, i1, i2) = (etapes.clone(), issue.clone(), issue.clone());
                let (b1, b2, b3) = (boucle.clone(), boucle.clone(), boucle.clone());
                en_arriere_plan(
                    travail,
                    move |texte| e.borrow_mut().push(texte),
                    move |valeur| {
                        *i1.borrow_mut() = Some(Ok(valeur));
                        b1.quit();
                    },
                    move |panique| {
                        *i2.borrow_mut() = Some(Err(panique));
                        b2.quit();
                    },
                );
                // Source attachée à NOTRE contexte (et non au contexte global, que les autres tests,
                // lancés en parallèle, peuvent occuper).
                let garde =
                    glib::timeout_source_new(DELAI_MAX, None, glib::Priority::DEFAULT, move || {
                        b3.quit();
                        glib::ControlFlow::Break
                    });
                garde.attach(Some(&contexte));
                boucle.run();
            })
            .unwrap();
        let etapes = etapes.borrow().clone();
        let issue = issue.borrow().clone();
        (etapes, issue)
    }

    #[test]
    fn les_etapes_arrivent_dans_l_ordre_puis_le_resultat() {
        let (etapes, issue) = executer(|p| {
            p.etape("un");
            p.etape("deux");
            p.etape("trois");
            42
        });
        assert_eq!(etapes, ["un", "deux", "trois"]);
        assert_eq!(issue, Some(Ok(42)));
    }

    #[test]
    fn un_travail_qui_panique_appelle_sur_panique() {
        let (etapes, issue) = executer(|p| {
            p.etape("avant");
            panic!("boum");
        });
        assert_eq!(etapes, ["avant"]);
        assert_eq!(issue, Some(Err("boum".to_owned())));
    }
}
