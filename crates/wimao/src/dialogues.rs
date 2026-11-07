//! Boîtes de dialogue de WiMao : erreur, questions, consigne, « À propos », sélecteurs de
//! fichier.
//!
//! Règle de l'interface : un dialogue seulement pour une erreur, une décision ou une consigne à
//! suivre ; les succès passent par un toast (`fenetre/mod.rs`). Utilise `wimao_core::{regles,
//! i18n}` pour les textes. Utilisé par `fenetre/mod.rs`, `gestionnaires.rs` et `session.rs`. Aucune
//! de ces fonctions ne bloque : le résultat revient dans un rappel, dans le fil de GTK.
//!
//! Questions propres aux gestionnaires d'éditeurs (sous-projet 2) : remplacer le lien de
//! connexion d'un autre programme ([`confirmer_lien`]), fermer des programmes ouverts avant de
//! réparer ([`confirmer_fermeture_programmes`]) ou avant une autre action qui arrête Wine
//! ([`confirmer_arret_wine`]), attendre les installeurs en fin de session
//! ([`attendre_installeurs`]), quitter pendant une session ([`confirmer_sortie_session`]).

use std::path::{Path, PathBuf};

use gtk4::{gio, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{tr, trf};
use wimao_core::{expliquer, Erreur, Probleme};

use crate::aide_fenetre;

/// Extensions d'installeurs acceptées (sélecteur de fichier et glisser-déposer).
pub const EXTENSIONS_INSTALLEUR: [&str; 4] = ["exe", "msi", "zip", "7z"];
/// Adresse du dépôt, affichée dans « À propos ».
const SITE_WEB: &str = "https://github.com/F1ghtc0re/WiMao";
/// Identifiant d'application, aussi nom de l'icône.
pub const ID_APPLICATION: &str = "fr.wimao.WiMao";

/// Identifiant de la réponse « fermer ».
/// Réponse « Copy the command » du dialogue du lien yabridge.
const REPONSE_COPIER: &str = "copier";
const REPONSE_FERMER: &str = "fermer";
/// Identifiant de la réponse « ouvrir le chapitre des problèmes fréquents ».
const REPONSE_PROBLEMES: &str = "problemes";
/// Identifiant de la réponse « réparer ».
const REPONSE_REPARER: &str = "reparer";
/// Identifiant de la réponse « continuer le travail » (sortie refusée).
const REPONSE_CONTINUER: &str = "continuer";
/// Identifiant de la réponse « quitter quand même ».
const REPONSE_QUITTER: &str = "quitter";
/// Identifiant de la réponse « ouvrir l'installeur ».
const REPONSE_OUVRIR: &str = "ouvrir";
/// Identifiant de la réponse « annuler ».
const REPONSE_ANNULER: &str = "annuler";
/// Identifiant de la réponse « remplacer le lien par WiMao ».
const REPONSE_REMPLACER: &str = "remplacer";
/// Identifiant de la réponse « fermer les programmes et réparer ».
const REPONSE_FERMER_REPARER: &str = "fermer-reparer";
/// Identifiant de la réponse « fermer les programmes et continuer ».
const REPONSE_FERMER_CONTINUER: &str = "fermer-continuer";
/// Identifiant de la réponse « continuer d'attendre les installeurs ».
const REPONSE_ATTENDRE: &str = "attendre";
/// Identifiant de la réponse « arrêter quand même ».
const REPONSE_ARRETER: &str = "arreter";

/// Dit si `fichier` a l'extension d'un installeur ou d'une archive prise en charge
/// (sans tenir compte de la casse : `SETUP.EXE` est courant sous Windows).
pub fn est_installeur(fichier: &Path) -> bool {
    fichier
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS_INSTALLEUR.contains(&e.to_lowercase().as_str()))
}

/// Affiche une erreur : le texte traduit de l'erreur, puis l'explication d'une règle si une
/// s'applique. Si la règle désigne un problème réparable et que `reparable` est vrai, un
/// bouton « Réparer » appelle `reparer` avec ce problème. Un bouton « Common problems » ouvre
/// le mode d'emploi au chapitre des problèmes fréquents.
pub fn afficher_erreur(
    parent: &adw::ApplicationWindow,
    erreur: &Erreur,
    reparable: bool,
    reparer: impl FnOnce(Probleme) + 'static,
) {
    let texte = erreur.to_string();
    let (corps, probleme) = match expliquer(&texte) {
        Some((explication, probleme)) => (format!("{texte}\n\n{explication}"), probleme),
        None => (texte, None),
    };
    let dialogue = adw::AlertDialog::new(Some(&tr("Something went wrong")), Some(&corps));
    dialogue.add_response(REPONSE_FERMER, &tr("Close"));
    dialogue.add_response(REPONSE_PROBLEMES, &tr("Common problems"));
    dialogue.set_close_response(REPONSE_FERMER);
    let proposition = probleme.filter(|_| reparable);
    if proposition.is_some() {
        dialogue.add_response(REPONSE_REPARER, &tr("Repair"));
        dialogue.set_response_appearance(REPONSE_REPARER, adw::ResponseAppearance::Suggested);
        dialogue.set_default_response(Some(REPONSE_REPARER));
    }
    let fenetre = parent.clone();
    dialogue.choose(
        Some(parent),
        None::<&gio::Cancellable>,
        move |reponse| match (reponse.as_str(), proposition) {
            (REPONSE_REPARER, Some(probleme)) => reparer(probleme),
            (REPONSE_PROBLEMES, _) => aide_fenetre::ouvrir_aide(&fenetre, Some("problemes")),
            _ => {}
        },
    );
}

/// Dialogue d'une panique du fil de travail : message traduit, cause technique et fichier
/// journal (`journal`, s'il est actif) où elle a été notée.
pub fn afficher_panique(parent: &adw::ApplicationWindow, cause: &str, journal: Option<&Path>) {
    let mut corps = trf("An unexpected error occurred: {cause}", &[("cause", cause)]);
    if let Some(fichier) = journal {
        corps.push_str("\n\n");
        corps.push_str(&trf(
            "Details were written to the log: {path}",
            &[("path", &fichier.display().to_string())],
        ));
    }
    let dialogue = adw::AlertDialog::new(Some(&tr("Something went wrong")), Some(&corps));
    dialogue.add_response(REPONSE_FERMER, &tr("Close"));
    dialogue.set_close_response(REPONSE_FERMER);
    dialogue.present(Some(parent));
}

/// Demande confirmation de quitter pendant un travail ; `quitter` n'est appelé que si
/// l'utilisateur choisit « Quitter quand même » (le choix par défaut est de continuer).
/// Rend le dialogue, pour que la fenêtre n'en ouvre jamais deux (`Fenetre::dialogue_fermeture`).
pub fn confirmer_sortie(
    parent: &adw::ApplicationWindow,
    quitter: impl FnOnce() + 'static,
) -> adw::AlertDialog {
    let dialogue = adw::AlertDialog::new(
        None,
        Some(&tr(
            "A task is in progress (installation or backup). Quitting now may leave incomplete files.",
        )),
    );
    dialogue.add_response(REPONSE_CONTINUER, &tr("Keep working"));
    dialogue.add_response(REPONSE_QUITTER, &tr("Quit anyway"));
    dialogue.set_response_appearance(REPONSE_QUITTER, adw::ResponseAppearance::Destructive);
    dialogue.set_default_response(Some(REPONSE_CONTINUER));
    dialogue.set_close_response(REPONSE_CONTINUER);
    // `choose` consomme son dialogue : on en garde un exemplaire (même objet GTK) à rendre.
    dialogue
        .clone()
        .choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
            if reponse == REPONSE_QUITTER {
                quitter();
            }
        });
    dialogue
}

/// Demande confirmation de quitter pendant la session du gestionnaire `nom` : le gestionnaire
/// reste ouvert, mais WiMao cesse de ponter ; ce qui manque sera proposé au prochain démarrage
/// (spec 4.4). `quitter` n'est appelé que sur « Quit anyway ». Rend le dialogue (voir
/// [`confirmer_sortie`]).
pub fn confirmer_sortie_session(
    parent: &adw::ApplicationWindow,
    nom: &str,
    quitter: impl FnOnce() + 'static,
) -> adw::AlertDialog {
    let dialogue = adw::AlertDialog::new(
        Some(&trf("{name} is still open", &[("name", nom)])),
        Some(&trf(
            "If you quit, {name} stays open but WiMao stops bridging the plugins it installs. WiMao will offer to bridge them the next time it starts.",
            &[("name", nom)],
        )),
    );
    dialogue.add_response(REPONSE_CONTINUER, &tr("Keep WiMao open"));
    dialogue.add_response(REPONSE_QUITTER, &tr("Quit anyway"));
    dialogue.set_response_appearance(REPONSE_QUITTER, adw::ResponseAppearance::Destructive);
    dialogue.set_default_response(Some(REPONSE_CONTINUER));
    dialogue.set_close_response(REPONSE_CONTINUER);
    // `choose` consomme son dialogue : on en garde un exemplaire (même objet GTK) à rendre.
    dialogue
        .clone()
        .choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
            if reponse == REPONSE_QUITTER {
                quitter();
            }
        });
    dialogue
}

/// Le lien `schema://` est géré par un autre programme (`actuel`) : faut-il le confier à
/// WiMao ? `choix` reçoit `true` pour « Use WiMao ». Le choix par défaut, et celui de la
/// touche Échap, est de ne rien remplacer : la personne a peut-être un relais qui marche.
pub fn confirmer_lien(
    parent: &adw::ApplicationWindow,
    nom: &str,
    schema: &str,
    actuel: &str,
    choix: impl FnOnce(bool) + 'static,
) {
    let corps = format!(
        "{}\n\n{}",
        trf(
            "The link {scheme}:// is handled by {current}. Replace it with WiMao?",
            &[("scheme", schema), ("current", actuel)],
        ),
        trf(
            "{name} uses this link to finish signing in from your web browser.",
            &[("name", nom)],
        ),
    );
    let dialogue = adw::AlertDialog::new(Some(&tr("Sign-in link")), Some(&corps));
    dialogue.add_response(REPONSE_ANNULER, &tr("Keep the current one"));
    dialogue.add_response(REPONSE_REMPLACER, &tr("Use WiMao"));
    dialogue.set_response_appearance(REPONSE_REMPLACER, adw::ResponseAppearance::Suggested);
    dialogue.set_default_response(Some(REPONSE_ANNULER));
    dialogue.set_close_response(REPONSE_ANNULER);
    dialogue.choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
        choix(reponse == REPONSE_REMPLACER);
    });
}

/// « Repair » arrête Wine, donc fermerait ces `programmes` ouverts : la personne confirme.
/// `choix` reçoit `true` pour « Close and repair » ; « Cancel » (et Échap) annule **toute**
/// la réparation : il n'y a pas de réparation partielle qui laisserait Wine ouvert.
pub fn confirmer_fermeture_programmes(
    parent: &adw::ApplicationWindow,
    programmes: &[String],
    choix: impl FnOnce(bool) + 'static,
) {
    let dialogue = adw::AlertDialog::new(
        Some(&tr("Close the open programs?")),
        Some(&trf(
            "Repair stops Wine, so it will close these open programs: {programs}.",
            &[("programs", &programmes.join(", "))],
        )),
    );
    dialogue.add_response(REPONSE_ANNULER, &tr("Cancel"));
    dialogue.add_response(REPONSE_FERMER_REPARER, &tr("Close and repair"));
    dialogue.set_response_appearance(REPONSE_FERMER_REPARER, adw::ResponseAppearance::Destructive);
    // Par défaut, l'action sans danger : Entrée ne ferme pas des programmes par mégarde.
    dialogue.set_default_response(Some(REPONSE_ANNULER));
    dialogue.set_close_response(REPONSE_ANNULER);
    dialogue.choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
        choix(reponse == REPONSE_FERMER_REPARER);
    });
}

/// Une action de WiMao (ajouter un installeur, ouvrir un gestionnaire, sauvegarder) arrête Wine,
/// donc fermerait ces `programmes` ouverts : la personne confirme. `choix` reçoit `true` pour
/// « Close and continue » ; « Cancel » (et Échap) annule l'action, sans rien changer.
///
/// Même question que [`confirmer_fermeture_programmes`], avec un texte qui ne parle pas de
/// réparation (même phrase que la CLI).
pub fn confirmer_arret_wine(
    parent: &adw::ApplicationWindow,
    programmes: &[String],
    choix: impl FnOnce(bool) + 'static,
) {
    let dialogue = adw::AlertDialog::new(
        Some(&tr("Close the open programs?")),
        Some(&trf(
            "Wine will be stopped and these open programs will be closed: {programs}.",
            &[("programs", &programmes.join(", "))],
        )),
    );
    dialogue.add_response(REPONSE_ANNULER, &tr("Cancel"));
    dialogue.add_response(REPONSE_FERMER_CONTINUER, &tr("Close and continue"));
    dialogue.set_response_appearance(
        REPONSE_FERMER_CONTINUER,
        adw::ResponseAppearance::Destructive,
    );
    // Par défaut, l'action sans danger : Entrée ne ferme pas des programmes par mégarde.
    dialogue.set_default_response(Some(REPONSE_ANNULER));
    dialogue.set_close_response(REPONSE_ANNULER);
    dialogue.choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
        choix(reponse == REPONSE_FERMER_CONTINUER);
    });
}

/// Consigne d'une recette, sous le titre `titre` (déjà traduit) : connexion à l'ouverture
/// d'un gestionnaire (« Sign in to … »), activation après une installation (« Activate … »).
/// Affichée une fois, avec un seul bouton.
pub fn afficher_consigne(parent: &adw::ApplicationWindow, titre: &str, consigne: &str) {
    let dialogue = adw::AlertDialog::new(Some(titre), Some(consigne));
    dialogue.add_response(REPONSE_FERMER, &tr("OK"));
    dialogue.set_close_response(REPONSE_FERMER);
    dialogue.present(Some(parent));
}

/// Fin de session demandée alors que des `installeurs` tournent (liste vide : pas encore
/// identifiés) : WiMao les attend. `choix`
/// est appelé à la fermeture du dialogue, quelle qu'en soit la cause, avec `true` pour
/// « Stop anyway ». Rend le dialogue, pour que la fenêtre le ferme d'elle-même quand les
/// installeurs ont fini (`force_close`, seulement tant que `choix` n'a pas été appelé).
pub fn attendre_installeurs(
    parent: &adw::ApplicationWindow,
    installeurs: &[String],
    choix: impl FnOnce(bool) + 'static,
) -> adw::AlertDialog {
    let corps = if installeurs.is_empty() {
        // Installeurs pas encore identifiés (voir `MessageSession::Attente`).
        tr("An installer may still be running. WiMao waits for it to finish, then bridges the new plugins.")
    } else {
        trf(
            "Still running: {programs}. WiMao waits for them to finish, then bridges the new plugins.",
            &[("programs", &installeurs.join(", "))],
        )
    };
    let dialogue = adw::AlertDialog::new(Some(&tr("Installations in progress")), Some(&corps));
    dialogue.add_response(REPONSE_ATTENDRE, &tr("Keep waiting"));
    dialogue.add_response(REPONSE_ARRETER, &tr("Stop anyway"));
    dialogue.set_response_appearance(REPONSE_ARRETER, adw::ResponseAppearance::Destructive);
    dialogue.set_default_response(Some(REPONSE_ATTENDRE));
    dialogue.set_close_response(REPONSE_ATTENDRE);
    // `choose` consomme le dialogue : on en garde une référence (objet GObject compté).
    dialogue
        .clone()
        .choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
            choix(reponse == REPONSE_ARRETER);
        });
    dialogue
}

/// Fenêtre affichée quand WiMao ne peut pas démarrer (par exemple `HOME` absent) : le message
/// d'erreur, lisible, plutôt qu'un échec muet dans un terminal que le musicien ne regarde pas.
pub fn fenetre_erreur_demarrage(app: &adw::Application, erreur: &Erreur) {
    let page = adw::StatusPage::builder()
        .icon_name("dialog-error-symbolic")
        .title(tr("WiMao cannot start"))
        .description(erreur.to_string())
        .build();
    let vue = adw::ToolbarView::new();
    vue.add_top_bar(&adw::HeaderBar::new());
    vue.set_content(Some(&page));
    let fenetre = adw::ApplicationWindow::builder()
        .application(app)
        .title("WiMao")
        .default_width(560)
        .default_height(360)
        .content(&vue)
        .build();
    fenetre.present();
}

/// Propose d'ouvrir l'installeur normalement après une installation silencieuse sans effet ;
/// `question` vient de `operations::message_aucun_plugin` (qui cite le code de l'installeur).
pub fn proposer_installation_normale(
    parent: &adw::ApplicationWindow,
    question: &str,
    ouvrir: impl FnOnce() + 'static,
) {
    let dialogue = adw::AlertDialog::new(None, Some(question));
    dialogue.add_response(REPONSE_FERMER, &tr("Cancel"));
    dialogue.add_response(REPONSE_OUVRIR, &tr("Open installer"));
    dialogue.set_response_appearance(REPONSE_OUVRIR, adw::ResponseAppearance::Suggested);
    dialogue.set_default_response(Some(REPONSE_OUVRIR));
    dialogue.set_close_response(REPONSE_FERMER);
    dialogue.choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
        if reponse == REPONSE_OUVRIR {
            ouvrir();
        }
    });
}

/// Sélecteur de fichier d'installeur (`.exe .msi .zip .7z`) ; `choisi` reçoit le chemin.
pub fn choisir_installeur(parent: &adw::ApplicationWindow, choisi: impl FnOnce(PathBuf) + 'static) {
    let filtre = gtk4::FileFilter::new();
    filtre.set_name(Some(&tr("Installers and archives")));
    for extension in EXTENSIONS_INSTALLEUR {
        // `add_suffix` ignore la casse, contrairement à un motif « *.exe ».
        filtre.add_suffix(extension);
    }
    let filtres = gio::ListStore::new::<gtk4::FileFilter>();
    filtres.append(&filtre);
    let selecteur = gtk4::FileDialog::builder()
        .title(tr("Add an installer"))
        .filters(&filtres)
        .build();
    selecteur.open(Some(parent), None::<&gio::Cancellable>, move |resultat| {
        // Annuler le sélecteur est une réponse normale, pas une erreur à montrer.
        if let Some(chemin) = resultat.ok().and_then(|f| f.path()) {
            choisi(chemin);
        }
    });
}

/// Sélecteur de dossier pour la sauvegarde ; `choisi` reçoit le chemin.
pub fn choisir_dossier(parent: &adw::ApplicationWindow, choisi: impl FnOnce(PathBuf) + 'static) {
    let selecteur = gtk4::FileDialog::builder()
        .title(tr("Choose the backup folder"))
        .build();
    selecteur.select_folder(Some(parent), None::<&gio::Cancellable>, move |resultat| {
        if let Some(chemin) = resultat.ok().and_then(|f| f.path()) {
            choisi(chemin);
        }
    });
}

/// Sélecteur du `.sha256` d'une sauvegarde à restaurer ; `choisi` reçoit le chemin.
pub fn choisir_sauvegarde(parent: &adw::ApplicationWindow, choisi: impl FnOnce(PathBuf) + 'static) {
    let filtre = gtk4::FileFilter::new();
    filtre.set_name(Some(&tr("Backup checksums (*.sha256)")));
    filtre.add_suffix("sha256");
    let filtres = gio::ListStore::new::<gtk4::FileFilter>();
    filtres.append(&filtre);
    let selecteur = gtk4::FileDialog::builder()
        .title(tr("Choose the backup to restore"))
        .filters(&filtres)
        .build();
    selecteur.open(Some(parent), None::<&gio::Cancellable>, move |resultat| {
        if let Some(chemin) = resultat.ok().and_then(|f| f.path()) {
            choisi(chemin);
        }
    });
}

/// Fenêtre « À propos » : nom, version, licence GPL et dépôt.
pub fn a_propos(parent: &adw::ApplicationWindow) {
    let dialogue = adw::AboutDialog::builder()
        .application_name("WiMao")
        .application_icon(ID_APPLICATION)
        .version(env!("CARGO_PKG_VERSION"))
        .website(SITE_WEB)
        .issue_url(format!("{SITE_WEB}/issues"))
        .license_type(gtk4::License::Gpl30)
        .comments(tr("Windows audio plugins on Linux"))
        .build();
    dialogue.present(Some(parent));
}

/// Information sur le lien `~/.local/share/yabridge` (avertissement, jamais réparé par WiMao) :
/// `message` explique la situation, « Copy the command » appelle `copie` (qui place `commande`
/// dans le presse-papiers) et « Close » ferme. WiMao ne lance jamais la commande.
pub fn informer_lien_yabridge(
    parent: &adw::ApplicationWindow,
    message: &str,
    copie: impl FnOnce() + 'static,
) {
    let dialogue = adw::AlertDialog::new(Some(&tr("Information")), Some(message));
    dialogue.add_response(REPONSE_FERMER, &tr("Close"));
    dialogue.add_response(REPONSE_COPIER, &tr("Copy the command"));
    dialogue.set_response_appearance(REPONSE_COPIER, adw::ResponseAppearance::Suggested);
    dialogue.set_default_response(Some(REPONSE_COPIER));
    dialogue.set_close_response(REPONSE_FERMER);
    dialogue.choose(Some(parent), None::<&gio::Cancellable>, move |reponse| {
        if reponse == REPONSE_COPIER {
            copie();
        }
    });
}

/// Place `texte` dans le presse-papiers de l'écran de `fenetre`.
pub fn copier(fenetre: &adw::ApplicationWindow, texte: &str) {
    fenetre.clipboard().set_text(texte);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_extensions_sont_reconnues_sans_tenir_compte_de_la_casse() {
        for bon in ["a/Setup.exe", "x.MSI", "p.Zip", "z.7z"] {
            assert!(est_installeur(Path::new(bon)), "{bon}");
        }
        for mauvais in ["notes.txt", "sans_extension", "dossier/"] {
            assert!(!est_installeur(Path::new(mauvais)), "{mauvais}");
        }
    }
}
