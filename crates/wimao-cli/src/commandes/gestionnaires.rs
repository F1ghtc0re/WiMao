//! Sous-commandes `recettes` (avec l'état installé des gestionnaires), `gestionnaire
//! installer|ouvrir` (bilan de session : plugins, relances du pilote, erreurs reconnues dans
//! le journal du gestionnaire) et `relayer-lien`.
//!
//! Utilise `wimao_recettes` (catalogue, `installer_recette`, session de gestionnaire,
//! `relayer_lien`, consignes) et `installation::apres_installation` (lien de connexion et
//! consigne d'activation, communs à toutes les installations). Utilisé par
//! `commandes::executer`.
//!
//! `gestionnaire ouvrir` tourne dans une boucle qui prend la même décision, à chaque tour,
//! que l'interface (`wimao_recettes::suite`) : fin à la fermeture du gestionnaire quand aucun
//! installeur ne tourne ; le premier Ctrl+C vaut « J'ai fini » (on attend les installeurs en
//! cours), le second « Arrêter quand même » (voir [`ControleC`]).

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use wimao_core::i18n::{langue_courante, tr, trf};
use wimao_core::{Chemins, Erreur, Resultat};
use wimao_recettes::{
    consigne, est_installe, installer_recette, ouvrir, relayer_lien, suite, texte_evenement,
    BilanSession, Commande, EvenementSession, OptionsInstallation, Phase, Recette, Session, Suite,
    TypeRecette, CONSIGNE_CONNEXION, INTERVALLE_PILOTE,
};

use super::installation::{afficher_plugins, apres_installation, charger_catalogue};
use super::{accord_avant_arret, Issue};
use crate::cli::ActionGestionnaire;
use crate::progression_terminal::ProgressionTerminal;
use crate::sortie::afficher;

/// Pas d'attente entre deux vérifications de Ctrl+C : l'arrêt reste réactif même si le
/// travail périodique ([`INTERVALLE_PILOTE`]) est bien plus espacé.
const PAS_D_ATTENTE: Duration = Duration::from_millis(200);
/// Nombre de Ctrl+C qui vaut « Arrêter quand même » (n'attend plus les installeurs) ; le
/// premier vaut « J'ai fini » (voir [`commande_ctrl_c`]).
const CTRL_C_FORCE: usize = 2;

/// Nombre de Ctrl+C reçus depuis le début de la session.
///
/// Le gestionnaire de signal ne fait que l'incrémenter : c'est une opération sûre dans un
/// gestionnaire de signal (async-signal-safe), contrairement à écrire, allouer ou prendre un
/// verrou, qui peuvent bloquer ou corrompre l'état si le signal interrompt le même code.
/// Toute la suite (messages, décisions) se passe dans la boucle principale, qui lit ce
/// compteur.
static NOMBRE_CTRL_C: AtomicUsize = AtomicUsize::new(0);

/// Le gestionnaire de signal de `SIGINT` : incrémente le compteur, rien d'autre.
extern "C" fn compter_ctrl_c(_signal: libc::c_int) {
    NOMBRE_CTRL_C.fetch_add(1, Ordering::SeqCst);
}

/// Prise en main de Ctrl+C pendant l'ouverture et la surveillance d'une session : remplace
/// l'arrêt immédiat du programme par le comptage de [`NOMBRE_CTRL_C`].
///
/// C'est une garde : [`Drop`] remet le gestionnaire de `SIGINT` d'avant. La garde est posée
/// **avant** `ouvrir` (un Ctrl+C pendant l'ouverture est compté, la surveillance s'arrête alors
/// aussitôt) et relâchée **avant** `fermer`, dont l'arrêt des processus, le pontage et les tests
/// peuvent durer longtemps : un Ctrl+C y doit interrompre normalement le programme, pas être
/// compté puis ignoré.
struct ControleC {
    /// Gestionnaire de `SIGINT` remplacé ; `None` si `signal` a échoué (alors rien n'a changé,
    /// Ctrl+C garde son effet habituel et il n'y a rien à restaurer).
    ancien: Option<libc::sighandler_t>,
}

impl ControleC {
    /// Remet le compteur à zéro et installe le gestionnaire de `SIGINT`.
    fn installer() -> ControleC {
        NOMBRE_CTRL_C.store(0, Ordering::SeqCst);
        // SAFETY : `compter_ctrl_c` est une fonction `extern "C"` qui n'utilise qu'un
        // atomique (async-signal-safe) ; `signal` ne lit que son adresse.
        let ancien = unsafe {
            libc::signal(
                libc::SIGINT,
                compter_ctrl_c as extern "C" fn(libc::c_int) as libc::sighandler_t,
            )
        };
        ControleC {
            ancien: (ancien != libc::SIG_ERR).then_some(ancien),
        }
    }

    /// Nombre de Ctrl+C reçus.
    fn nombre(&self) -> usize {
        NOMBRE_CTRL_C.load(Ordering::SeqCst)
    }
}

impl Drop for ControleC {
    fn drop(&mut self) {
        if let Some(ancien) = self.ancien {
            // SAFETY : on remet le gestionnaire que `signal` nous avait rendu.
            unsafe {
                libc::signal(libc::SIGINT, ancien);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------
// recettes
// ---------------------------------------------------------------------------------------

/// `recettes` : une ligne par recette (`nom  (type)  éditeur`), suivie pour un gestionnaire
/// de son état (`· Installé` ou `· Non installé`, comme la ligne de l'interface).
pub fn recettes(c: &Chemins) -> Resultat<Issue> {
    let catalogue = charger_catalogue(c);
    if catalogue.0.is_empty() {
        afficher!("{}", tr("No recipe"));
    }
    for recette in &catalogue.0 {
        let installe =
            (recette.type_ == TypeRecette::Gestionnaire).then(|| est_installe(c, recette));
        afficher!("{}", ligne_recette(recette, installe));
    }
    Ok(Issue::Succes)
}

/// Mot du type de recette tel qu'il s'écrit dans le fichier TOML (non traduit).
fn mot_type(type_: TypeRecette) -> &'static str {
    match type_ {
        TypeRecette::Gestionnaire => "gestionnaire",
        TypeRecette::Installeur => "installeur",
        TypeRecette::Archive => "archive",
    }
}

/// Ligne de [`recettes`] pour une recette. `installe` : état d'un gestionnaire
/// (`wimao_recettes::est_installe`), `None` pour les autres recettes (un plugin se reconnaît
/// à l'inventaire, pas à un fichier).
fn ligne_recette(r: &Recette, installe: Option<bool>) -> String {
    let mut ligne = format!("{}  ({})", r.nom, mot_type(r.type_));
    if !r.editeur.is_empty() {
        ligne.push_str("  ");
        ligne.push_str(&r.editeur);
    }
    if let Some(installe) = installe {
        let etat = if installe {
            tr("Installed")
        } else {
            tr("Not installed")
        };
        ligne.push_str(" · ");
        ligne.push_str(&etat);
    }
    ligne
}

/// Cherche la recette `nom` (son nom ou son fichier sans `.toml`, casse ignorée).
///
/// Erreur : [`Erreur::Introuvable`] (message traduit) si aucune ne correspond.
fn recette_nommee(c: &Chemins, nom: &str) -> Resultat<Recette> {
    charger_catalogue(c)
        .trouver_par_nom(nom)
        .cloned()
        .ok_or_else(|| Erreur::Introuvable(trf("a recipe named {name}", &[("name", nom)])))
}

// ---------------------------------------------------------------------------------------
// gestionnaire
// ---------------------------------------------------------------------------------------

/// Aiguille vers `gestionnaire installer` ou `gestionnaire ouvrir`.
pub fn gestionnaire(c: &Chemins, action: ActionGestionnaire) -> Resultat<Issue> {
    match action {
        ActionGestionnaire::Installer { nom, fichier, oui } => {
            installer_gestionnaire(c, &nom, fichier.as_deref(), oui)
        }
        ActionGestionnaire::Ouvrir {
            nom,
            sans_test,
            oui,
        } => ouvrir_gestionnaire(c, &nom, sans_test, oui),
    }
}

/// `gestionnaire installer` : installe la recette, puis fait la suite commune à toute
/// installation (`apres_installation` : lien de connexion, consigne d'activation).
fn installer_gestionnaire(
    c: &Chemins,
    nom: &str,
    fichier: Option<&Path>,
    oui: bool,
) -> Resultat<Issue> {
    let recette = recette_nommee(c, nom)?;
    // Une installation en silence : les gestionnaires n'ont pas d'installeur à ouvrir.
    let options = OptionsInstallation {
        interactif: false,
        tester: true,
    };
    let rapport = installer_recette(c, &recette, fichier, options, &ProgressionTerminal)?;
    afficher!("{}", trf("{name} is installed", &[("name", &recette.nom)]));
    let issue = afficher_plugins(&rapport.plugins, &rapport.mis_a_jour);
    apres_installation(c, &rapport, oui);
    Ok(issue)
}

/// `gestionnaire ouvrir` : ouvre la session, la surveille, puis la ferme et affiche le bilan.
///
/// Ouvrir le gestionnaire (re)démarre Wine : les programmes déjà ouverts sont nommés avant, et
/// un refus (sauf `--oui`) termine la commande sans rien faire.
fn ouvrir_gestionnaire(c: &Chemins, nom: &str, sans_test: bool, oui: bool) -> Resultat<Issue> {
    let recette = recette_nommee(c, nom)?;
    if !accord_avant_arret(c, oui)? {
        return Ok(Issue::Succes);
    }
    let session = {
        // Garde posée avant `ouvrir`, relâchée à la sortie du bloc donc avant `fermer`
        // (voir `ControleC`). Sur erreur d'`ouvrir`, `?` la relâche aussi. Un Ctrl+C reçu
        // pendant `ouvrir` est lu au premier tour (« J'ai fini »).
        let ctrl_c = ControleC::installer();
        let mut session = ouvrir(c, &recette, &ProgressionTerminal)?;
        afficher!(
            "{}",
            trf(
                "{name} is open: close it when you are done, or press Ctrl+C",
                &[("name", &recette.nom)]
            )
        );
        // Consigne de connexion de la recette (spec 4.2, étape 6), comme le dialogue de
        // l'interface.
        if let Some(texte) = consigne(&recette, CONSIGNE_CONNEXION, &langue_courante()) {
            afficher!("{texte}");
        }
        surveiller(&mut session, &ctrl_c);
        session
    };
    let bilan = session.fermer(!sans_test, &ProgressionTerminal)?;
    Ok(afficher_bilan(&bilan))
}

/// Bilan d'une session : tableau des plugins, puis une ligne par relance du pilote (même
/// texte qu'en direct, `texte_evenement`) et, sur la sortie d'erreur, les erreurs reconnues
/// dans le journal du gestionnaire. Rend l'issue du tableau (plugin en échec ou non).
fn afficher_bilan(bilan: &BilanSession) -> Issue {
    let issue = afficher_plugins(&bilan.plugins, &bilan.mis_a_jour);
    for relance in &bilan.relances {
        afficher_evenement(&EvenementSession::Relance(relance.clone()));
    }
    for texte in &bilan.avertissements {
        eprintln!("{} {texte}", tr("Warning:"));
    }
    issue
}

/// Commande que représentent les Ctrl+C reçus : `nombre` au total, dont `deja_lus` déjà
/// traduits en commande. `None` s'il n'y en a pas de nouveau ; sinon le premier Ctrl+C vaut
/// « J'ai fini » ([`Commande::Terminer`]) et tout Ctrl+C suivant « Arrêter quand même »
/// ([`Commande::ArreterQuandMeme`]), comme les boutons de l'interface.
fn commande_ctrl_c(nombre: usize, deja_lus: usize) -> Option<Commande> {
    if nombre <= deja_lus {
        None
    } else if nombre >= CTRL_C_FORCE {
        Some(Commande::ArreterQuandMeme)
    } else {
        Some(Commande::Terminer)
    }
}

/// Boucle de surveillance : un tour toutes les [`INTERVALLE_PILOTE`] (`tick`, événements
/// affichés, état du gestionnaire et installeurs en cours), puis la décision commune avec
/// l'interface, `wimao_recettes::suite`. Rend quand il faut fermer la session.
///
/// - pendant l'attente des installeurs (après le premier Ctrl+C), `tick` continue : le pilote
///   relance encore et les plugins installés sont pontés ;
/// - une erreur d'un appel (lecture de `/proc`, pontage…) est affichée et la boucle continue :
///   la prochaine échéance réessaie ;
/// - des installeurs illisibles gardent la dernière liste connue ; jamais lus, ils sont
///   « inconnus » (`None`) et non « aucun » : la session ne se ferme pas sans les attendre.
fn surveiller(session: &mut Session, ctrl_c: &ControleC) {
    let mut phase = Phase::Surveillance;
    let mut ctrl_c_lus = 0;
    let mut installeurs: Option<Vec<String>> = None;
    loop {
        match session.tick(Instant::now(), &ProgressionTerminal) {
            Ok(evenements) => evenements.iter().for_each(afficher_evenement),
            Err(erreur) => signaler(&erreur),
        }
        let ferme = session.gestionnaire_ferme().unwrap_or_else(|e| {
            signaler(&e);
            false // illisible : compte comme ouvert, pour ne pas finir à tort
        });
        match session.installeurs_en_cours() {
            Ok(liste) => installeurs = Some(liste),
            Err(e) => signaler(&e),
        }
        let nombre = ctrl_c.nombre();
        let commande = commande_ctrl_c(nombre, ctrl_c_lus);
        ctrl_c_lus = nombre;
        match suite(phase, commande, ferme, installeurs.as_deref()) {
            Suite::Continuer => {}
            Suite::Attendre => {
                phase = Phase::Attente;
                annoncer_l_attente(installeurs.as_deref());
            }
            // La CLI n'envoie jamais `Abandonner` ; s'il arrivait, fermer reste le plus sûr.
            Suite::Fermer | Suite::Abandonner => return,
        }
        dormir_jusqu_a_l_echeance(ctrl_c, ctrl_c_lus);
    }
}

/// Dort [`INTERVALLE_PILOTE`] par petits pas, ou moins si un nouveau Ctrl+C arrive (au-delà
/// des `deja_lus`) : la commande est alors traitée sans attendre la fin de l'intervalle.
fn dormir_jusqu_a_l_echeance(ctrl_c: &ControleC, deja_lus: usize) {
    let debut = Instant::now();
    while debut.elapsed() < INTERVALLE_PILOTE && ctrl_c.nombre() <= deja_lus {
        std::thread::sleep(PAS_D_ATTENTE);
    }
}

/// Dit que la session attend des installeurs avant de se fermer (liste `None` : pas encore
/// identifiés, lecture de `/proc` en échec).
fn annoncer_l_attente(installeurs: Option<&[String]>) {
    let texte = match installeurs.filter(|liste| !liste.is_empty()) {
        Some(liste) => trf(
            "Still running: {programs}. Waiting for them to finish (Ctrl+C again to stop anyway)",
            &[("programs", &liste.join(", "))],
        ),
        None => tr(
            "An installer may still be running. Waiting for it to finish (Ctrl+C again to stop anyway)",
        ),
    };
    afficher!("{texte}");
}

/// Affiche une erreur d'une étape de la boucle sans l'arrêter.
fn signaler(erreur: &Erreur) {
    eprintln!("{} {erreur}", tr("Error:"));
}

/// Affiche un événement de la session sur une ligne.
fn afficher_evenement(evenement: &EvenementSession) {
    afficher!("{}", texte_evenement(evenement));
}

// ---------------------------------------------------------------------------------------
// relayer-lien
// ---------------------------------------------------------------------------------------

/// `relayer-lien` : transmet le lien au gestionnaire. Rien sur stdout (le bureau lance cette
/// commande en arrière-plan) : seul le code de sortie compte.
pub fn relayer(c: &Chemins, url: &str) -> Resultat<Issue> {
    relayer_lien(c, &charger_catalogue(c), url)?;
    Ok(Issue::Succes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noms(liste: &[&str]) -> Vec<String> {
        liste.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn ctrl_c_est_compte_puis_l_ancien_gestionnaire_est_restaure() {
        /// Gestionnaire de `SIGINT` actuel, lu sans le modifier (`sigaction` avec un `act` nul).
        fn gestionnaire_actuel() -> libc::sighandler_t {
            // SAFETY : lecture seule ; `old` est une structure valide à remplir.
            unsafe {
                let mut old: libc::sigaction = std::mem::zeroed();
                libc::sigaction(libc::SIGINT, std::ptr::null(), &mut old);
                old.sa_sigaction
            }
        }
        let avant = gestionnaire_actuel();
        {
            let garde = ControleC::installer();
            assert_eq!(garde.nombre(), 0);
            // SAFETY : le gestionnaire installé ne fait qu'incrémenter un atomique.
            unsafe { libc::raise(libc::SIGINT) };
            assert_eq!(garde.nombre(), 1);
            assert_ne!(gestionnaire_actuel(), avant);
        }
        // Pas de nouveau SIGINT ici : l'ancien gestionnaire (qui tue le programme) est revenu.
        assert_eq!(gestionnaire_actuel(), avant);
    }

    #[test]
    fn premier_ctrl_c_j_ai_fini_second_arreter_quand_meme() {
        assert_eq!(commande_ctrl_c(0, 0), None);
        assert_eq!(commande_ctrl_c(1, 0), Some(Commande::Terminer));
        // Déjà lu : pas de nouvelle commande.
        assert_eq!(commande_ctrl_c(1, 1), None);
        assert_eq!(commande_ctrl_c(2, 1), Some(Commande::ArreterQuandMeme));
        // Deux Ctrl+C d'un coup (entre deux tours) : « Arrêter quand même ».
        assert_eq!(commande_ctrl_c(2, 0), Some(Commande::ArreterQuandMeme));
        assert_eq!(commande_ctrl_c(3, 2), Some(Commande::ArreterQuandMeme));
    }

    #[test]
    fn la_cli_decide_comme_l_interface() {
        // Même fonction que l'interface : on vérifie ici les cas propres au terminal.
        let installeurs = noms(&["Setup.exe"]);
        let s = Phase::Surveillance;
        // 1er Ctrl+C avec un installeur en cours : on attend (et `tick` continue).
        assert_eq!(
            suite(s, commande_ctrl_c(1, 0), false, Some(&installeurs)),
            Suite::Attendre
        );
        // Installeurs jamais lus (erreur de /proc) : « inconnus », on attend aussi.
        assert_eq!(
            suite(s, commande_ctrl_c(1, 0), false, None),
            Suite::Attendre
        );
        // 2e Ctrl+C : on ferme sans attendre.
        let a = Phase::Attente;
        assert_eq!(
            suite(a, commande_ctrl_c(2, 1), false, Some(&installeurs)),
            Suite::Fermer
        );
    }

    #[test]
    fn la_ligne_d_un_gestionnaire_dit_s_il_est_installe() {
        // La langue n'est pas fixée ici (pas de dépendance à l'ordre des tests) : on compare
        // au texte traduit dans la langue courante.
        let fichier = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../wimao-recettes/tests/recettes/valide.toml");
        let r = wimao_recettes::charger(&fichier).unwrap();
        assert_eq!(
            ligne_recette(&r, Some(false)),
            format!(
                "Native Access 2  (gestionnaire)  Native Instruments · {}",
                tr("Not installed")
            )
        );
        assert!(ligne_recette(&r, Some(true)).ends_with(&format!(" · {}", tr("Installed"))));
        assert!(!ligne_recette(&r, None).contains('·'));
    }

    #[test]
    fn le_type_est_ecrit_comme_dans_le_fichier_de_recette() {
        assert_eq!(mot_type(TypeRecette::Gestionnaire), "gestionnaire");
        assert_eq!(mot_type(TypeRecette::Installeur), "installeur");
    }
}
