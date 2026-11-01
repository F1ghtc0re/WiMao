//! Section « Gestionnaires » de la fenêtre : une ligne par gestionnaire d'éditeur (Native
//! Access, Arturia Software Center…) avec son bouton « Install » ou « Open » (spec, section 6).
//!
//! Trois parties :
//! - [`LigneGestionnaire`] : données pures d'une ligne (calculées par
//!   `operations::lignes_gestionnaires`, testables sans écran) ;
//! - [`creer_section`] : le groupe `adw::PreferencesGroup` affiché au-dessus de la liste des
//!   plugins, et aussi sur la page vide (un premier lancement commence souvent par Native
//!   Access). Pendant une session, la ligne du gestionnaire ouvert dit « In use » et les
//!   infobulles des boutons grisés disent pourquoi ;
//! - l'action « Install » de la fenêtre (`Fenetre::installer_gestionnaire`) : installation
//!   dans le fil de travail, puis la suite commune à toute installation
//!   ([`Fenetre::suite_commune`] : question sur le lien de connexion si un autre programme le
//!   gère déjà, consigne d'activation). « Open » ouvre une session : voir `session.rs`.
//!
//! Les boutons des lignes déclenchent les actions `win.installer-gestionnaire` et
//! `win.ouvrir-gestionnaire` (paramètre : le nom de la recette). Couper ces actions pendant
//! un travail grise donc les boutons sans rien suivre ici.
//!
//! Utilise `operations.rs` (installation, lien), `dialogues.rs` et `fenetre/mod.rs`. Utilisé par
//! `fenetre/mod.rs` (affichage de la section, actions).

use std::path::PathBuf;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{tr, trf};

use crate::dialogues;
use crate::fenetre::Fenetre;
use wimao_recettes::{declarer_lien_wimao, SuiteLien};

use crate::operations::{self, GestionnaireInstalle, Installe};

/// Action de la fenêtre qui installe le gestionnaire nommé par son paramètre.
pub const ACTION_INSTALLER: &str = "installer-gestionnaire";
/// Action de la fenêtre qui ouvre une session du gestionnaire nommé par son paramètre.
pub const ACTION_OUVRIR: &str = "ouvrir-gestionnaire";
/// Icône des lignes : une boîte, le « paquet » d'un éditeur (présente dans Adwaita et Yaru).
const ICONE_GESTIONNAIRE: &str = "package-x-generic-symbolic";
/// Taille de l'icône dans sa tuile (px) : celle des grandes icônes de lignes GNOME.
const TAILLE_ICONE: i32 = 20;

/// Un gestionnaire tel que la section l'affiche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LigneGestionnaire {
    /// Nom de la recette (« Native Access 2 »), aussi paramètre des actions.
    pub nom: String,
    /// Éditeur (« Native Instruments ») ; vide si la recette ne le dit pas.
    pub editeur: String,
    /// Vrai si son programme est présent dans l'environnement Wine.
    pub installe: bool,
    /// Vrai si la recette donne une adresse de téléchargement ; sinon « Install » demande
    /// d'abord le fichier de l'installeur.
    pub telechargeable: bool,
}

impl LigneGestionnaire {
    /// Sous-titre de la ligne : « éditeur · Installé » (ou l'état seul sans éditeur).
    /// `ouvert` : ce gestionnaire est celui de la session en cours (« In use »).
    pub fn sous_titre(&self, ouvert: bool) -> String {
        let etat = if ouvert {
            tr("In use")
        } else if self.installe {
            tr("Installed")
        } else {
            tr("Not installed")
        };
        if self.editeur.is_empty() {
            etat
        } else {
            format!("{} · {etat}", self.editeur)
        }
    }
}

/// Le groupe « Managers » : titre, explication courte et une ligne par gestionnaire.
/// `session` : nom du gestionnaire ouvert, s'il y a une session (ses boutons sont alors
/// grisés par leurs actions, et les infobulles disent pourquoi).
pub fn creer_section(lignes: &[LigneGestionnaire], session: Option<&str>) -> adw::PreferencesGroup {
    let groupe = adw::PreferencesGroup::builder()
        .title(tr("Managers"))
        .description(tr(
            "Publisher apps that download your plugins. WiMao bridges what they install.",
        ))
        .build();
    for ligne in lignes {
        groupe.add(&creer_rangee(ligne, session));
    }
    groupe
}

/// Une ligne : tuile d'icône, nom, « éditeur · état », bouton « Install » ou « Open ».
fn creer_rangee(ligne: &LigneGestionnaire, session: Option<&str>) -> adw::ActionRow {
    let ouvert = session == Some(ligne.nom.as_str());
    let rangee = adw::ActionRow::builder()
        .title(&ligne.nom)
        .subtitle(ligne.sous_titre(ouvert))
        // Un nom de recette peut contenir « & » : pas de balisage Pango.
        .use_markup(false)
        .build();
    rangee.add_prefix(&tuile_icone(ligne.installe));
    let bouton = bouton_action(ligne, session);
    rangee.add_suffix(&bouton);
    // Toute la ligne réagit au clic, comme dans les réglages de GNOME.
    rangee.set_activatable_widget(Some(&bouton));
    rangee
}

/// Tuile arrondie qui porte l'icône : teinte d'accent quand le gestionnaire est installé,
/// neutre sinon (voir `.tuile-gestionnaire` dans `style.css`).
fn tuile_icone(installe: bool) -> gtk4::Image {
    let image = gtk4::Image::builder()
        .icon_name(ICONE_GESTIONNAIRE)
        .pixel_size(TAILLE_ICONE)
        .valign(gtk4::Align::Center)
        .css_classes(["tuile-gestionnaire"])
        .build();
    if installe {
        image.add_css_class("installe");
    }
    image
}

/// Bouton arrondi de la ligne : « Install » ou « Open ». Style ordinaire, pas
/// `suggested-action` : selon les règles GNOME, l'accent est réservé à l'action principale de
/// la vue (« Add an installer »).
fn bouton_action(ligne: &LigneGestionnaire, session: Option<&str>) -> gtk4::Button {
    let (libelle, action) = if ligne.installe {
        (tr("Open"), ACTION_OUVRIR)
    } else {
        (tr("Install"), ACTION_INSTALLER)
    };
    let infobulle = match session {
        Some(nom) if nom == ligne.nom => trf("{name} is already open", &[("name", nom)]),
        Some(nom) => trf(
            "{action} is not available while {name} is open",
            &[("action", &libelle), ("name", nom)],
        ),
        None if ligne.installe => trf(
            "Open {name}: WiMao bridges the plugins it installs",
            &[("name", &ligne.nom)],
        ),
        None => trf("Install {name}", &[("name", &ligne.nom)]),
    };
    gtk4::Button::builder()
        .label(libelle)
        .valign(gtk4::Align::Center)
        .css_classes(["pill", "bouton-ligne"])
        .tooltip_text(infobulle)
        .action_name(format!("win.{action}"))
        .action_target(&ligne.nom.to_variant())
        .build()
}

// --- Action « Install » ------------------------------------------------------------------

impl Fenetre {
    /// « Install » : demande d'abord l'installeur si la recette n'a pas d'adresse de
    /// téléchargement, puis installe.
    pub(crate) fn installer_gestionnaire(&self, nom: String) {
        let telechargeable = self
            .ligne_gestionnaire(&nom)
            .is_none_or(|ligne| ligne.telechargeable);
        if telechargeable {
            self.lancer_installation_gestionnaire(nom, None);
            return;
        }
        let moi = self.clone();
        dialogues::choisir_installeur(&self.fenetre, move |fichier| {
            moi.lancer_installation_gestionnaire(nom, Some(fichier));
        });
    }

    /// Installation dans le fil de travail (téléchargement compris), puis suite.
    fn lancer_installation_gestionnaire(&self, nom: String, fichier: Option<PathBuf>) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        let message = trf("Installing {name}…", &[("name", &nom)]);
        self.lancer(
            &message,
            move |p| operations::installer_gestionnaire(&c, &nom, fichier.as_deref(), p),
            move |resultat| match resultat {
                Err(erreur) => moi.erreur(&erreur, true),
                Ok((installe, avertissements)) => {
                    moi.apres_installation_gestionnaire(installe);
                    moi.avertir(&avertissements);
                }
            },
        );
    }

    /// Toast « `<nom>` est installé » (suivi du bilan si l'installation a aussi apporté des
    /// plugins), liste rafraîchie, puis la suite commune (lien, consigne d'activation).
    fn apres_installation_gestionnaire(&self, installe: GestionnaireInstalle) {
        let mut message = trf("{name} is installed", &[("name", &installe.nom)]);
        if let Some(bilan) = operations::bilan_installation(&installe.installe.rapport) {
            message = format!("{message} · {bilan}");
        }
        self.toast(&message);
        self.noter_mis_a_jour(&installe.installe.rapport.mis_a_jour);
        self.rafraichir();
        self.suite_commune(&installe.nom, installe.installe);
    }

    /// Suite commune à toute installation réussie (« Add an installer » comme « Install ») :
    /// toast du lien déclaré, ou question si un autre programme gère le lien, puis consigne
    /// `activation` de la recette (licence, numéro de série…) dans un dialogue. `nom` : nom du
    /// produit pour les titres.
    pub(crate) fn suite_commune(&self, nom: &str, installe: Installe) {
        match installe.lien {
            SuiteLien::Aucune => {}
            SuiteLien::Declare(schema) => self.toast(&texte_lien_relaye(&schema)),
            SuiteLien::AConfirmer { schema, actuel } => {
                let moi = self.clone();
                let schema_suite = schema.clone();
                dialogues::confirmer_lien(&self.fenetre, nom, &schema, &actuel, move |oui| {
                    if oui {
                        moi.declarer_lien(schema_suite);
                    } else {
                        moi.toast(&trf(
                            "The link {scheme}:// is not relayed to WiMao",
                            &[("scheme", &schema_suite)],
                        ));
                    }
                });
            }
        }
        if let Some(texte) = &installe.rapport.consigne_activation {
            let titre = trf("Activate {name}", &[("name", nom)]);
            dialogues::afficher_consigne(&self.fenetre, &titre, texte);
        }
    }

    /// Déclare le lien `schema://` pour WiMao (après accord), dans le fil de travail.
    fn declarer_lien(&self, schema: String) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        let schema_travail = schema.clone();
        self.lancer(
            &tr("Declaring the sign-in link…"),
            move |_| declarer_lien_wimao(&c, &schema_travail),
            move |resultat| match resultat {
                Ok(()) => moi.toast(&texte_lien_relaye(&schema)),
                Err(erreur) => moi.erreur(&erreur, false),
            },
        );
    }
}

/// Toast d'un lien de connexion confié à WiMao (déclaré d'emblée ou après accord).
fn texte_lien_relaye(schema: &str) -> String {
    trf(
        "The link {scheme}:// is now relayed to WiMao",
        &[("scheme", schema)],
    )
}
