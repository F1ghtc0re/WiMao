//! La fenêtre de l'assistant de premier lancement (640 × 560).
//!
//! Une `adw::NavigationPage` par page de l'automate (`assistant.rs`), dans un
//! `adw::NavigationView` :
//! - **Bienvenue** : présentation, « Start » et le lien « How does it work? » (mode d'emploi) ;
//! - **Environnement** : « Looking for an existing environment… » tant que la détection n'a pas
//!   répondu, puis soit deux lignes (« Import this environment (recommended) » avec sa taille
//!   et son nombre de plugins, « Create a new environment »), soit le seul bouton « Create the
//!   environment » ;
//! - **Opération** : sablier, étape courante donnée par le noyau et barre pulsée ;
//! - **Échec** : le message de l'erreur, « Try again » et « Copy the report » ;
//! - **Premiers pas** : « Add an installer », « Open a publisher manager » ou « Later », qui
//!   ferment l'assistant et ouvrent la fenêtre principale.
//!
//! Utilise `assistant.rs` (l'automate), `travail.rs` (travail en arrière-plan),
//! `operations.rs`, `dialogues.rs`, `aide_fenetre.rs` et `fenetre/` (la fenêtre principale,
//! ouverte à la fin). Utilisé par `main.rs` (`ouvrir_une_fenetre`).
//!
//! Choix non évidents :
//! - **la détection tourne en arrière-plan** : `import::detecter` parcourt tout
//!   `~/.wine-mao` (plus de 100 Go chez certains musiciens). Elle est lancée une seule fois, à
//!   l'ouverture ; le résultat est gardé, et la page « Environnement » montre un sablier tant
//!   qu'il manque ;
//! - **pas de bouton retour automatique** : chaque page a `can-pop = false` et ses propres
//!   boutons, qui passent par l'automate ; sans cela libadwaita dépilerait une page sans que
//!   l'automate le sache. Les pages sont mises en phase avec `replace_with_tags` ;
//! - **avant un import**, les programmes Windows ouverts sont nommés et la personne donne son
//!   accord (`dialogues::confirmer_arret_wine`) ; un refus laisse l'assistant où il est ;
//! - **fermeture** : pendant l'opération elle demande confirmation (`dialogues::confirmer_sortie`) ;
//!   à toute autre page, fermer l'assistant quitte l'application sans rien écrire ;
//! - les messages d'erreur sont échappés avant d'aller dans une `adw::StatusPage` (sa description
//!   est du balisage Pango).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4::{glib, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{tr, trf, trn};
use wimao_core::import::{self, EnvironnementExistant};
use wimao_core::outils::taille_lisible;
use wimao_core::{creer_environnement, expliquer, journal, Chemins, Erreur};

use crate::aide_fenetre::ouvrir_aide;
use crate::assistant::{Assistant, ChoixEnvironnement, Evenement, PageAssistant};
use crate::dialogues::{self, ID_APPLICATION};
use crate::fenetre::Fenetre;
use crate::operations;
use crate::travail::en_arriere_plan;

/// Largeur de la fenêtre (px).
const LARGEUR: i32 = 640;
/// Hauteur de la fenêtre (px).
const HAUTEUR: i32 = 560;
/// Largeur maximale du contenu des pages (px) : les lignes et boutons ne s'étirent pas.
const LARGEUR_CONTENU: i32 = 460;
/// Intervalle de pulsation de la barre de progression.
const PULSATION: Duration = Duration::from_millis(120);
/// Durée d'un toast (secondes).
const DUREE_TOAST: u32 = 4;
/// Chapitre du mode d'emploi ouvert par « How does it work? ».
const CHAPITRE_AIDE: &str = "premiers-pas";
/// Icônes de l'assistant, livrées avec WiMao (`data/icons/hicolor/symbolic/apps/`) : celles du
/// thème changent d'un thème à l'autre et certaines manquent à taille de page d'état.
const ICONE_PRET: &str = "wimao-pret-symbolic";
const ICONE_ERREUR: &str = "wimao-erreur-symbolic";
/// Classe libadwaita d'une `adw::StatusPage` resserrée (icône et marges réduites) : les pages
/// qui portent une liste doivent tenir dans 560 px de haut.
const CLASSE_COMPACTE: &str = "compact";
/// Espace vertical entre les éléments d'une page (px).
const ESPACE: i32 = 18;

/// Étiquettes des pages du `NavigationView` (une par [`PageAssistant`]).
const TAG_BIENVENUE: &str = "bienvenue";
const TAG_ENVIRONNEMENT: &str = "environnement";
const TAG_OPERATION: &str = "operation";
const TAG_ECHEC: &str = "echec";
const TAG_PREMIER_PAS: &str = "premier-pas";
/// Pages de la pile « Environnement » : détection en cours, ou choix.
const PILE_RECHERCHE: &str = "recherche";
const PILE_CHOIX: &str = "choix";

/// Ce que la personne fait en sortant de l'assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Suite {
    /// Ouvrir le sélecteur d'installeur de la fenêtre principale.
    Installeur,
    /// Ouvrir la fenêtre principale, dont la page vide montre la section des gestionnaires.
    Gestionnaire,
    /// Ouvrir la fenêtre principale, sans rien d'autre.
    PlusTard,
}

/// La fenêtre de l'assistant et ce que ses rappels doivent modifier. Se clone à bas prix (tout
/// est compté par référence), comme [`Fenetre`].
#[derive(Clone)]
struct FenetreAssistant {
    app: adw::Application,
    chemins: Rc<Chemins>,
    fenetre: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    navigation: adw::NavigationView,
    /// L'automate : la page courante et le choix.
    automate: Rc<RefCell<Assistant>>,
    /// Page « Environnement » : sablier de détection ou choix.
    pile_environnement: gtk4::Stack,
    /// Emplacement du choix, rempli quand la détection a répondu.
    emplacement_choix: adw::Bin,
    /// Page « Opération » : titre, étape courante et barre.
    titre_operation: gtk4::Label,
    etape: gtk4::Label,
    barre: gtk4::ProgressBar,
    /// Minuteur qui fait pulser la barre pendant une opération (un seul à la fois, retiré à la
    /// fin de l'opération).
    pulsation: Rc<RefCell<Option<glib::SourceId>>>,
    /// Page « Échec » : son titre est fixe, sa description reçoit le message de l'erreur.
    page_echec: adw::StatusPage,
    /// Résultat de la détection : `None` tant qu'elle n'a pas répondu, puis `Some(None)` (rien à
    /// importer) ou `Some(Some(..))`.
    detection: Rc<RefCell<Option<Option<EnvironnementExistant>>>>,
    /// Le dialogue de fermeture affiché, s'il y en a un (jamais deux).
    dialogue_fermeture: Rc<RefCell<Option<adw::AlertDialog>>>,
    /// Vrai quand fermer la fenêtre ne doit plus rien demander.
    sortie_confirmee: Rc<Cell<bool>>,
}

/// Ouvre l'assistant et lance la détection d'un environnement existant en arrière-plan.
///
/// `avertissement` : un message à montrer en toast (réglages abîmés, par exemple). Rend la
/// fenêtre pour que l'appelant puisse y attacher des dialogues.
pub fn ouvrir(
    app: &adw::Application,
    chemins: Rc<Chemins>,
    avertissement: Option<String>,
) -> adw::ApplicationWindow {
    let fenetre = adw::ApplicationWindow::builder()
        .application(app)
        .title("WiMao")
        .default_width(LARGEUR)
        .default_height(HAUTEUR)
        .build();
    let moi = FenetreAssistant {
        app: app.clone(),
        chemins,
        fenetre: fenetre.clone(),
        toasts: adw::ToastOverlay::new(),
        navigation: adw::NavigationView::new(),
        automate: Rc::new(RefCell::new(Assistant::nouveau(false))),
        pile_environnement: gtk4::Stack::new(),
        emplacement_choix: adw::Bin::new(),
        titre_operation: gtk4::Label::new(None),
        etape: gtk4::Label::new(None),
        barre: gtk4::ProgressBar::new(),
        pulsation: Rc::new(RefCell::new(None)),
        page_echec: adw::StatusPage::new(),
        detection: Rc::new(RefCell::new(None)),
        dialogue_fermeture: Rc::new(RefCell::new(None)),
        sortie_confirmee: Rc::new(Cell::new(false)),
    };
    moi.construire();
    if let Some(message) = avertissement {
        moi.toast(&message);
    }
    moi.lancer_detection();
    moi.brancher_fermeture();
    fenetre.present();
    fenetre
}

impl FenetreAssistant {
    /// Fabrique les cinq pages, les ajoute au `NavigationView` et assemble la fenêtre. Les pages
    /// sont créées une fois pour toutes ; seul leur empilement change (voir
    /// [`FenetreAssistant::synchroniser`]).
    fn construire(&self) {
        let pages = [
            self.page_bienvenue(),
            self.page_environnement(),
            self.page_operation(),
            self.page_echec(),
            self.page_premier_pas(),
        ];
        for page in &pages {
            self.navigation.add(page);
        }
        self.toasts.set_child(Some(&self.navigation));
        self.fenetre.set_content(Some(&self.toasts));
        self.synchroniser();
    }

    // --- Pages ---------------------------------------------------------------------------

    /// Habille `contenu` d'une barre d'en-tête transparente. `retour` : un bouton qui envoie
    /// [`Evenement::Retour`] à l'automate.
    fn habiller(
        &self,
        tag: &str,
        contenu: &impl IsA<gtk4::Widget>,
        retour: bool,
    ) -> adw::NavigationPage {
        let entete = adw::HeaderBar::builder()
            .show_title(false)
            .css_classes(["flat"])
            .build();
        if retour {
            let bouton = gtk4::Button::builder()
                .icon_name("go-previous-symbolic")
                .tooltip_text(tr("Back"))
                .build();
            let moi = self.clone();
            bouton.connect_clicked(move |_| moi.appliquer(Evenement::Retour));
            entete.pack_start(&bouton);
        }
        let vue = adw::ToolbarView::new();
        vue.add_top_bar(&entete);
        vue.set_content(Some(contenu));
        adw::NavigationPage::builder()
            .tag(tag)
            .title("WiMao")
            .can_pop(false)
            .child(&vue)
            .build()
    }

    /// Resserre `enfant` dans une colonne de [`LARGEUR_CONTENU`].
    fn resserrer(enfant: &impl IsA<gtk4::Widget>) -> adw::Clamp {
        adw::Clamp::builder()
            .maximum_size(LARGEUR_CONTENU)
            .child(enfant)
            .build()
    }

    /// Accueil : icône de l'application, titre, une phrase, « Start » et le lien d'aide.
    fn page_bienvenue(&self) -> adw::NavigationPage {
        let commencer = gtk4::Button::builder()
            .label(tr("Start"))
            .halign(gtk4::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        let moi = self.clone();
        commencer.connect_clicked(move |_| moi.appliquer(Evenement::Commencer));
        let aide = gtk4::Button::builder()
            .label(tr("How does it work?"))
            .halign(gtk4::Align::Center)
            .css_classes(["flat"])
            .build();
        let fenetre = self.fenetre.clone();
        aide.connect_clicked(move |_| ouvrir_aide(&fenetre, Some(CHAPITRE_AIDE)));
        let boutons = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        boutons.append(&commencer);
        boutons.append(&aide);
        // Entrée = « Start » : le bouton principal est le widget par défaut de la fenêtre.
        self.fenetre.set_default_widget(Some(&commencer));
        let statut = adw::StatusPage::builder()
            .icon_name(ID_APPLICATION)
            .title(tr("Welcome to WiMao"))
            .description(tr(
                "WiMao installs your Windows audio plugins and makes them work in your music software (Bitwig, Ardour, REAPER…), on Linux.",
            ))
            .child(&boutons)
            .build();
        self.habiller(TAG_BIENVENUE, &statut, false)
    }

    /// Choix de l'environnement : sablier de détection, puis (voir
    /// [`FenetreAssistant::afficher_choix`]) les choix.
    fn page_environnement(&self) -> adw::NavigationPage {
        let sablier = gtk4::Spinner::builder()
            .spinning(true)
            .halign(gtk4::Align::Center)
            .width_request(32)
            .height_request(32)
            .build();
        let message = gtk4::Label::builder()
            .label(tr("Looking for an existing environment…"))
            .css_classes(["dim-label"])
            .build();
        let recherche = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        recherche.set_valign(gtk4::Align::Center);
        recherche.append(&sablier);
        recherche.append(&message);
        self.pile_environnement
            .add_named(&recherche, Some(PILE_RECHERCHE));
        self.pile_environnement
            .add_named(&self.emplacement_choix, Some(PILE_CHOIX));
        self.pile_environnement
            .set_visible_child_name(PILE_RECHERCHE);
        self.habiller(TAG_ENVIRONNEMENT, &self.pile_environnement, true)
    }

    /// Opération en cours : sablier, titre, étape courante et barre pulsée.
    fn page_operation(&self) -> adw::NavigationPage {
        let sablier = gtk4::Spinner::builder()
            .spinning(true)
            .halign(gtk4::Align::Center)
            .width_request(48)
            .height_request(48)
            .build();
        self.titre_operation.add_css_class("title-2");
        self.titre_operation.set_wrap(true);
        self.titre_operation
            .set_justify(gtk4::Justification::Center);
        self.etape.add_css_class("dim-label");
        self.etape.set_wrap(true);
        self.etape.set_justify(gtk4::Justification::Center);
        let conseil = gtk4::Label::builder()
            .label(tr("This can take a few minutes. Please keep WiMao open."))
            .css_classes(["caption", "dim-label"])
            .wrap(true)
            .justify(gtk4::Justification::Center)
            .build();
        let colonne = gtk4::Box::new(gtk4::Orientation::Vertical, ESPACE);
        colonne.set_valign(gtk4::Align::Center);
        colonne.append(&sablier);
        colonne.append(&self.titre_operation);
        colonne.append(&self.etape);
        colonne.append(&self.barre);
        colonne.append(&conseil);
        colonne.set_margin_start(24);
        colonne.set_margin_end(24);
        self.habiller(TAG_OPERATION, &Self::resserrer(&colonne), false)
    }

    /// Échec : icône d'erreur, message (rempli à l'échec), « Try again » et « Copy the report ».
    fn page_echec(&self) -> adw::NavigationPage {
        let reessayer = gtk4::Button::builder()
            .label(tr("Try again"))
            .halign(gtk4::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        let moi = self.clone();
        reessayer.connect_clicked(move |_| moi.reessayer());
        let rapport = gtk4::Button::builder()
            .label(tr("Copy the report"))
            .halign(gtk4::Align::Center)
            .css_classes(["pill"])
            .build();
        let moi = self.clone();
        rapport.connect_clicked(move |_| moi.copier_rapport());
        let boutons = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        boutons.append(&reessayer);
        boutons.append(&rapport);
        self.page_echec.set_icon_name(Some(ICONE_ERREUR));
        self.page_echec
            .set_title(&tr("The environment could not be set up"));
        self.page_echec.set_child(Some(&boutons));
        self.habiller(TAG_ECHEC, &self.page_echec, true)
    }

    /// Premiers pas : trois lignes en carte, qui ferment l'assistant.
    fn page_premier_pas(&self) -> adw::NavigationPage {
        let liste = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let lignes = [
            (
                "list-add-symbolic",
                tr("Add an installer"),
                tr("Install a Windows plugin from its installer file."),
                Suite::Installeur,
            ),
            (
                "view-grid-symbolic",
                tr("Open a publisher manager"),
                tr("For plugins that come with a publisher's app, such as Native Access."),
                Suite::Gestionnaire,
            ),
            (
                "document-open-recent-symbolic",
                tr("Later"),
                tr("Go to the main window."),
                Suite::PlusTard,
            ),
        ];
        for (icone, titre, sous_titre, suite) in lignes {
            let moi = self.clone();
            liste.append(&ligne_activable(icone, &titre, &sous_titre, move || {
                moi.terminer(suite);
            }));
        }
        let statut = adw::StatusPage::builder()
            .icon_name(ICONE_PRET)
            .css_classes([CLASSE_COMPACTE])
            .title(tr("Your environment is ready"))
            .description(tr("What would you like to do first?"))
            .child(&Self::resserrer(&liste))
            .build();
        self.habiller(TAG_PREMIER_PAS, &statut, false)
    }

    // --- Détection de l'environnement existant -------------------------------------------

    /// Lance `import::detecter` dans un fil (elle parcourt tout le dossier). Appelée une seule
    /// fois : le résultat est gardé dans `detection`.
    fn lancer_detection(&self) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        let sur_panique = self.clone();
        en_arriere_plan(
            move |_| import::detecter(&c),
            |_| {},
            move |trouve| moi.detection_terminee(trouve),
            move |cause| {
                // Pas de détection : on propose seulement de créer, sans bloquer l'assistant.
                journal::noter("panique de la détection de l'environnement", &cause);
                sur_panique.detection_terminee(None);
            },
        );
    }

    /// La détection a répondu : l'automate apprend s'il y a un environnement à importer et la
    /// page « Environnement » montre les choix.
    fn detection_terminee(&self, trouve: Option<EnvironnementExistant>) {
        self.automate
            .borrow_mut()
            .definir_existant(trouve.is_some());
        *self.detection.borrow_mut() = Some(trouve);
        self.afficher_choix();
    }

    /// Remplit la page « Environnement » : deux lignes si un environnement existe, sinon un
    /// seul bouton.
    fn afficher_choix(&self) {
        let detection = self.detection.borrow();
        let Some(trouve) = detection.as_ref() else {
            return;
        };
        let contenu: gtk4::Widget = match trouve {
            Some(existant) => {
                let liste = gtk4::ListBox::builder()
                    .selection_mode(gtk4::SelectionMode::None)
                    .css_classes(["boxed-list"])
                    .build();
                let moi = self.clone();
                liste.append(&ligne_activable(
                    "folder-download-symbolic",
                    &tr("Import this environment (recommended)"),
                    &description_import(existant),
                    move || moi.choisir(ChoixEnvironnement::Importer),
                ));
                let moi = self.clone();
                liste.append(&ligne_activable(
                    "list-add-symbolic",
                    &tr("Create a new environment"),
                    &tr("Starts from scratch. Your old environment stays where it is."),
                    move || moi.choisir(ChoixEnvironnement::Creer),
                ));
                Self::resserrer(&liste).upcast()
            }
            None => {
                let creer = gtk4::Button::builder()
                    .label(tr("Create the environment"))
                    .halign(gtk4::Align::Center)
                    .css_classes(["suggested-action", "pill"])
                    .build();
                let moi = self.clone();
                creer.connect_clicked(move |_| moi.choisir(ChoixEnvironnement::Creer));
                creer.upcast()
            }
        };
        let (description, icone) = if trouve.is_some() {
            (
                tr("An environment from a previous installation was found. WiMao can take it over, or start from scratch."),
                "folder-symbolic",
            )
        } else {
            (
                tr("WiMao will create the Wine environment where your plugins are installed."),
                "applications-system-symbolic",
            )
        };
        let statut = adw::StatusPage::builder()
            .icon_name(icone)
            .css_classes([CLASSE_COMPACTE])
            .title(tr("Your Wine environment"))
            .description(glib::markup_escape_text(&description))
            .child(&contenu)
            .build();
        self.emplacement_choix.set_child(Some(&statut));
        self.pile_environnement.set_visible_child_name(PILE_CHOIX);
    }

    // --- Parcours ------------------------------------------------------------------------

    /// Passe `e` à l'automate puis met les pages en phase.
    ///
    /// Au retour d'un échec, un environnement peut avoir été posé par l'essai (le préfixe
    /// existe) : l'import échouerait alors en `DejaPresent`, on ne le propose plus.
    fn appliquer(&self, e: Evenement) {
        let avant = self.automate.borrow().page();
        self.automate.borrow_mut().suivant(e);
        let apres = self.automate.borrow().page();
        if avant == PageAssistant::Echec
            && apres == PageAssistant::Environnement
            && self.chemins.prefixe.symlink_metadata().is_ok()
        {
            self.detection_terminee(None);
        }
        self.synchroniser();
    }

    /// Empile les pages de l'automate : Accueil, puis Choix, puis Opération (ou Échec à la
    /// place d'Opération). Premiers pas remplace tout : on ne revient pas en arrière.
    fn synchroniser(&self) {
        let page = self.automate.borrow().page();
        let pile: &[&str] = match page {
            PageAssistant::Bienvenue => &[TAG_BIENVENUE],
            PageAssistant::Environnement => &[TAG_BIENVENUE, TAG_ENVIRONNEMENT],
            PageAssistant::Operation => &[TAG_BIENVENUE, TAG_ENVIRONNEMENT, TAG_OPERATION],
            PageAssistant::Echec => &[TAG_BIENVENUE, TAG_ENVIRONNEMENT, TAG_ECHEC],
            PageAssistant::PremierPas => &[TAG_PREMIER_PAS],
        };
        self.navigation.replace_with_tags(pile);
    }

    /// Un clic sur un choix de la page « Environnement ».
    fn choisir(&self, choix: ChoixEnvironnement) {
        self.demarrer(choix, Evenement::Choisir(choix));
    }

    /// « Try again » : refait l'opération choisie.
    fn reessayer(&self) {
        let choix = self.automate.borrow().choix();
        if let Some(choix) = choix {
            self.demarrer(choix, Evenement::Reessayer);
        }
    }

    /// Lance `choix`. Avant un import, les programmes Windows ouverts sont nommés et la personne
    /// donne son accord ; un refus, ou une liste illisible, laisse l'assistant à sa page.
    fn demarrer(&self, choix: ChoixEnvironnement, evenement: Evenement) {
        // Double clic, ou « Try again » pressé deux fois : on ne pose pas la question deux fois.
        if !self.automate.borrow().lance_operation(evenement) {
            return;
        }
        if choix == ChoixEnvironnement::Creer {
            return self.engager(choix, evenement);
        }
        match import::programmes_ouverts(&self.chemins) {
            Ok(programmes) if programmes.is_empty() => self.engager(choix, evenement),
            Ok(programmes) => {
                let moi = self.clone();
                dialogues::confirmer_arret_wine(&self.fenetre, &programmes, move |oui| {
                    if oui {
                        moi.engager(choix, evenement);
                    }
                });
            }
            Err(erreur) => self.montrer_erreur(&erreur),
        }
    }

    /// Passe à la page « Opération » puis lance le travail.
    ///
    /// L'automate est relu ici : pendant la question « arrêter Wine ? », un autre clic a pu
    /// lancer l'opération. Si l'événement n'ouvre plus d'opération, on ne fait rien (jamais
    /// deux imports ou créations en même temps).
    fn engager(&self, choix: ChoixEnvironnement, evenement: Evenement) {
        if !self.automate.borrow().lance_operation(evenement) {
            return;
        }
        self.titre_operation.set_label(&match choix {
            ChoixEnvironnement::Importer => tr("Importing the environment…"),
            ChoixEnvironnement::Creer => tr("Creating the environment…"),
        });
        self.etape.set_label("");
        self.appliquer(evenement);
        self.executer(choix);
    }

    /// Import ou création, dans un fil de travail ; l'issue devient [`Evenement::Reussite`] ou
    /// [`Evenement::Echec`].
    fn executer(&self, choix: ChoixEnvironnement) {
        let barre = self.barre.clone();
        let minuteur = glib::timeout_add_local(PULSATION, move || {
            barre.pulse();
            glib::ControlFlow::Continue
        });
        if let Some(ancien) = self.pulsation.borrow_mut().replace(minuteur) {
            ancien.remove();
        }
        let c = (*self.chemins).clone();
        let (pour_etape, pour_fin, pour_panique) = (self.clone(), self.clone(), self.clone());
        en_arriere_plan(
            move |p| match choix {
                ChoixEnvironnement::Importer => import::importer(&c, p).map(|_| ()),
                ChoixEnvironnement::Creer => creer_environnement(&c, p),
            },
            move |etape| pour_etape.etape.set_label(&etape),
            move |resultat| match resultat {
                Ok(()) => pour_fin.operation_terminee(None),
                Err(erreur) => pour_fin.operation_terminee(Some(texte_echec(&erreur))),
            },
            move |cause| {
                journal::noter("panique du fil de travail", &cause);
                let message = trf(
                    "An unexpected error occurred: {cause}",
                    &[("cause", &cause)],
                );
                pour_panique.operation_terminee(Some(message));
            },
        );
    }

    /// Fin du travail : `None` = réussite ; `Some(message)` = échec (le message va sur la page
    /// d'échec).
    fn operation_terminee(&self, echec: Option<String>) {
        if let Some(minuteur) = self.pulsation.borrow_mut().take() {
            minuteur.remove();
        }
        match echec {
            None => self.appliquer(Evenement::Reussite),
            Some(message) => {
                self.page_echec
                    .set_description(Some(&glib::markup_escape_text(&message)));
                self.appliquer(Evenement::Echec);
            }
        }
    }

    /// « Copy the report » : le rapport anonymisé va dans le presse-papiers.
    fn copier_rapport(&self) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        let sur_panique = self.clone();
        en_arriere_plan(
            move |_| operations::rapport(&c),
            |_| {},
            move |resultat| match resultat {
                Ok(texte) => {
                    dialogues::copier(&moi.fenetre, &texte);
                    moi.toast(&tr("Report copied"));
                }
                Err(erreur) => moi.montrer_erreur(&erreur),
            },
            move |cause| {
                journal::noter("panique du fil de travail", &cause);
                dialogues::afficher_panique(&sur_panique.fenetre, &cause, journal::fichier_actif());
            },
        );
    }

    /// Sortie de l'assistant : la fenêtre principale s'ouvre (avant la fermeture de l'assistant,
    /// pour que l'application ne se termine pas entre les deux) ; « Add an installer » ouvre en
    /// plus le sélecteur d'installeur.
    fn terminer(&self, suite: Suite) {
        // Un second clic (ou une double activation) n'ouvre pas une seconde fenêtre principale.
        if self.sortie_confirmee.replace(true) {
            return;
        }
        let principale = Fenetre::nouvelle(&self.app, self.chemins.clone(), None);
        principale.presenter();
        self.fenetre.close();
        if suite == Suite::Installeur {
            principale.demander_installeur();
        }
    }

    // --- Fenêtre -------------------------------------------------------------------------

    /// Montre un toast.
    fn toast(&self, message: &str) {
        self.toasts.add_toast(
            adw::Toast::builder()
                .title(message)
                .timeout(DUREE_TOAST)
                .build(),
        );
    }

    /// Dialogue d'erreur (sans proposition de réparer : il n'y a encore rien à réparer).
    fn montrer_erreur(&self, erreur: &Erreur) {
        dialogues::afficher_erreur(&self.fenetre, erreur, false, |_| {});
    }

    /// Pendant l'opération, fermer la fenêtre demande confirmation ; ailleurs, elle se ferme et
    /// l'application se termine sans rien écrire.
    fn brancher_fermeture(&self) {
        let moi = self.clone();
        self.fenetre.connect_close_request(move |_| {
            let en_operation = moi.automate.borrow().page() == PageAssistant::Operation;
            if !en_operation || moi.sortie_confirmee.get() {
                return glib::Propagation::Proceed;
            }
            // Un seul dialogue à la fois : un second clic sur la croix remet le premier devant.
            let ouvert = moi.dialogue_fermeture.borrow().clone();
            if let Some(dialogue) = ouvert {
                dialogue.present(Some(&moi.fenetre));
                return glib::Propagation::Stop;
            }
            let confirme = moi.clone();
            let dialogue = dialogues::confirmer_sortie(&moi.fenetre, move || {
                confirme.sortie_confirmee.set(true);
                confirme.fenetre.close();
            });
            let oubli = moi.dialogue_fermeture.clone();
            dialogue.connect_closed(move |_| {
                oubli.borrow_mut().take();
            });
            *moi.dialogue_fermeture.borrow_mut() = Some(dialogue);
            glib::Propagation::Stop
        });
    }
}

/// Une ligne en carte : icône, titre, sous-titre et flèche ; `action` s'exécute à l'activation.
fn ligne_activable(
    icone: &str,
    titre: &str,
    sous_titre: &str,
    action: impl Fn() + 'static,
) -> adw::ActionRow {
    let ligne = adw::ActionRow::builder()
        .title(glib::markup_escape_text(titre))
        .subtitle(glib::markup_escape_text(sous_titre))
        .subtitle_lines(0)
        .activatable(true)
        .build();
    ligne.add_prefix(&gtk4::Image::from_icon_name(icone));
    ligne.add_suffix(&gtk4::Image::from_icon_name("go-next-symbolic"));
    ligne.connect_activated(move |_| action());
    ligne
}

/// Sous-titre de la ligne d'import : taille lisible et nombre de plugins
/// (`114.3 GiB · 12 plugins found`).
fn description_import(existant: &EnvironnementExistant) -> String {
    let plugins = trn(
        "{count} plugin found",
        "{count} plugins found",
        existant.plugins as u64,
        &[("count", &existant.plugins.to_string())],
    );
    format!("{} · {plugins}", taille_lisible(existant.taille))
}

/// Texte d'une erreur pour la page d'échec : le message, puis l'explication d'une règle si une
/// s'applique (comme le dialogue d'erreur de la fenêtre principale).
fn texte_echec(erreur: &Erreur) -> String {
    let texte = erreur.to_string();
    match expliquer(&texte) {
        Some((explication, _)) => format!("{texte}\n\n{explication}"),
        None => texte,
    }
}
