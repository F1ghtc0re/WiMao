//! La fenêtre principale : barre d'en-tête, liste des plugins, travail en cours, toasts.
//!
//! Disposition (règles GNOME / libadwaita) :
//! - `adw::HeaderBar` : titre centré `adw::WindowTitle` (« WiMao » + compte des plugins), bouton
//!   « Add an installer » (`suggested-action`) à gauche, caché tant que la page vide montre déjà
//!   le grand bouton central ; à droite « Repair » et « Back up » (icônes seules, infobulles) et
//!   le menu principal ;
//! - contenu dans un `gtk::Stack` : chargement, page vide accueillante (`adw::StatusPage` avec un
//!   grand bouton) ou liste (`gtk::ListBox` `boxed-list` dans un `adw::Clamp` et un `ScrolledWindow`) ;
//!   la section « Managers » (`gestionnaires.rs`) s'affiche au-dessus de la liste, et sous le
//!   grand bouton de la page vide ;
//! - pendant un travail : `adw::Banner` (message d'étape) + barre de progression en pulsation,
//!   actions désactivées (les boutons suivent leurs actions), fermeture confirmée ;
//! - pendant une session de gestionnaire (`session_fenetre.rs`) : seconde `adw::Banner`, teinte
//!   d'accent, avec « I'm done » puis « Stop anyway » ; actions coupées avec la raison en
//!   infobulle (en-tête, grand bouton de la page vide, lignes des gestionnaires), fermeture
//!   confirmée (le gestionnaire reste ouvert) ;
//! - le tout est la page racine (`principale`) d'un `adw::NavigationView` : les écrans suivants
//!   s'y empilent avec [`Fenetre::pousser`] ;
//! - avant une action qui arrête Wine (installeur, ouverture d'un gestionnaire, sauvegarde),
//!   [`Fenetre::avant_d_arreter_wine`] nomme les programmes ouverts et demande l'accord ;
//! - premier relevé en échec : page « erreur » (« Try again », « Copy the report ») ;
//! - « Repair » et la proposition de ponter au démarrage sont dans `reparation.rs` ;
//! - un travail qui panique remet l'interface en état et affiche un dialogue (`travail.rs`) ;
//! - succès par `adw::Toast`, dialogues pour les erreurs et les décisions seulement
//!   (`dialogues.rs`) ; glisser-déposer d'un installeur = bouton « Add an installer ».
//!
//! Le module est découpé : `actions.rs` (actions `win.*`, raccourcis, langue) et `entete.rs`
//! (barre d'en-tête, menu principal) sont des sous-modules qui ajoutent des méthodes à
//! [`Fenetre`] et lisent ses champs privés.
//!
//! La logique métier est dans `operations.rs` (fil de travail) ; ici il n'y a que l'interface.
//! Utilise `travail.rs`, `liste.rs`, `gestionnaires.rs`, `session_fenetre.rs`,
//! `reparation.rs`, `dialogues.rs`. Utilisé par `main.rs` ; `gestionnaires.rs`,
//! `session_fenetre.rs` et `reparation.rs` ajoutent leurs propres méthodes à [`Fenetre`] (d'où
//! les champs `pub(crate)`).

mod actions;
mod entete;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk4::{gdk, gio, glib, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{tr, trf};
use wimao_core::{journal, processus, Chemins};

use self::entete::{bouton_icone, bouton_texte};
use crate::dialogues::{self, est_installeur, ID_APPLICATION};
use crate::gestionnaires::{self, LigneGestionnaire};
use crate::liste;
use crate::operations::{self, Avertissements, Releve};
use crate::session_fenetre::SessionEnCours;
use crate::travail::en_arriere_plan;

/// Taille par défaut de la fenêtre.
const LARGEUR: i32 = 900;
/// Hauteur par défaut de la fenêtre.
const HAUTEUR: i32 = 640;
/// Largeur maximale de la liste (px) : au-delà, des lignes trop longues se lisent mal.
const LARGEUR_LISTE: i32 = 720;
/// Intervalle de pulsation de la barre de progression.
const PULSATION: Duration = Duration::from_millis(120);
/// Durée d'affichage d'un toast (secondes).
const DUREE_TOAST: u32 = 4;
/// Durée d'un toast qui porte un bouton (secondes) : le temps de le lire et de cliquer.
const DUREE_TOAST_ACTION: u32 = 15;
/// Classe CSS de la bannière de session (teinte d'accent, voir `style.css`) : elle se
/// distingue du bandeau de travail, gris.
const CLASSE_BANNIERE_SESSION: &str = "banniere-session";
/// Espace entre la section « Managers » et la liste des plugins, et sur la page vide entre le
/// grand bouton et la section (px, multiples de 6 comme dans GNOME).
const ESPACE_SECTIONS: i32 = 24;
/// Espace au-dessus de la section « Managers » sur la page vide (px).
const ESPACE_PAGE_VIDE: i32 = 36;
/// Largeur maximale de la section « Managers » sur la page vide (px).
const LARGEUR_SECTION_VIDE: i32 = 560;
/// Classe libadwaita d'une `adw::StatusPage` resserrée (icône et marges réduites).
const CLASSE_COMPACTE: &str = "compact";

/// Pages du `gtk::Stack`.
const PAGE_CHARGEMENT: &str = "chargement";
const PAGE_VIDE: &str = "vide";
const PAGE_LISTE: &str = "liste";
/// Le premier relevé a échoué : explication, « Try again » et « Copy the report ».
const PAGE_ERREUR: &str = "erreur";
/// Étiquette de la page racine du `adw::NavigationView` (la fenêtre principale).
pub(crate) const PAGE_PRINCIPALE: &str = "principale";

/// La fenêtre et les widgets que les actions doivent modifier. Se clone à bas prix (tout est
/// compté par référence), ce qui permet d'en donner une copie à chaque rappel.
#[derive(Clone)]
pub struct Fenetre {
    pub(crate) fenetre: adw::ApplicationWindow,
    pub(crate) chemins: Rc<Chemins>,
    titre: adw::WindowTitle,
    /// Bandeau du travail en cours (message d'étape).
    pub(crate) bandeau: adw::Banner,
    /// Bannière d'une session de gestionnaire, avec le bouton « I'm done » ou « Stop anyway »
    /// (`session_fenetre.rs`).
    pub(crate) banniere_session: adw::Banner,
    barre: gtk4::ProgressBar,
    pile: gtk4::Stack,
    liste: gtk4::ListBox,
    /// Emplacement de la section « Managers » sur la page vide.
    emplacement_vide: gtk4::Box,
    /// Emplacement de la section « Managers » au-dessus de la liste des plugins.
    emplacement_liste: gtk4::Box,
    /// Lignes de gestionnaires affichées (pour savoir si une recette se télécharge).
    gestionnaires: Rc<RefCell<Vec<LigneGestionnaire>>>,
    /// Bouton « Add an installer » de l'en-tête (caché sur la page vide).
    bouton_ajouter: gtk4::Button,
    /// Grand bouton « Add an installer » de la page vide (infobulle pendant une session).
    bouton_page_vide: gtk4::Button,
    /// Bouton « Repair » de l'en-tête (infobulle changée pendant une session).
    bouton_reparer: gtk4::Button,
    /// Bouton « Back up » de l'en-tête (infobulle changée pendant une session).
    bouton_sauvegarder: gtk4::Button,
    /// Section du menu principal « Restore a backup… » / « Cancel the import », reconstruite
    /// par `actualiser_menu_import` (`restaurer.rs`).
    pub(crate) menu_import: gio::Menu,
    toasts: adw::ToastOverlay,
    /// Navigation de la fenêtre : la page racine (étiquette `principale`) contient la barre
    /// d'en-tête et la pile des pages ; les écrans suivants (historique, points de restauration…)
    /// s'y empilent avec [`Fenetre::pousser`], avec le bouton retour d'libadwaita.
    pub(crate) navigation: adw::NavigationView,
    /// Le dialogue de fermeture actuellement affiché, s'il y en a un : un nouveau clic sur la
    /// croix le remet au premier plan au lieu d'en empiler un second.
    dialogue_fermeture: Rc<RefCell<Option<adw::AlertDialog>>>,
    /// Vrai pendant un travail ou une session : refuse d'en lancer un second.
    pub(crate) occupe: Rc<Cell<bool>>,
    /// Session de gestionnaire en cours, s'il y en a une (`session_fenetre.rs`).
    pub(crate) session: Rc<RefCell<Option<SessionEnCours>>>,
    /// Minuteur de pulsation de la barre (un seul à la fois).
    pulsation: Rc<RefCell<Option<glib::SourceId>>>,
    /// Vrai quand l'utilisateur a confirmé « Quitter quand même » : la fermeture n'est plus interceptée.
    sortie_confirmee: Rc<Cell<bool>>,
    /// Actions à couper pendant un travail (leurs boutons se grisent avec elles).
    actions_travail: Rc<RefCell<Vec<gio::SimpleAction>>>,
    /// Toast « N plugins are not bridged yet — Repair » tant qu'il est affiché
    /// (`reparation.rs`) : retiré dès qu'un travail ou une session commence, car son bouton
    /// proposerait alors une action indisponible.
    pub(crate) toast_pontage: Rc<RefCell<Option<adw::Toast>>>,
    /// Plugins mis à jour depuis l'ouverture de la fenêtre : ils portent la marque « Updated ».
    /// Rempli après une installation ou une session ([`Fenetre::noter_mis_a_jour`]), vidé à la
    /// fermeture de la fenêtre.
    mis_a_jour: Rc<RefCell<BTreeSet<PathBuf>>>,
}

impl Fenetre {
    /// Construit la fenêtre, branche les actions et lance le premier relevé des plugins.
    ///
    /// `avertissement_reglages` : le message rendu par `Reglages::charger` si `reglages.toml`
    /// était abîmé ; il est montré en toast dès l'ouverture (`None` après un changement de langue).
    /// `mis_a_jour` : plugins déjà marqués « Updated » à reprendre. Vide à l'ouverture de
    /// WiMao ; rempli quand la fenêtre est reconstruite au changement de langue, pour que la
    /// marque survive. Il est posé avant le premier relevé, qui en prend une copie.
    pub fn nouvelle(
        app: &adw::Application,
        chemins: Rc<Chemins>,
        avertissement_reglages: Option<String>,
        mis_a_jour: BTreeSet<PathBuf>,
    ) -> Fenetre {
        let titre = adw::WindowTitle::new("WiMao", "");
        let bandeau = adw::Banner::builder().revealed(false).build();
        let banniere_session = adw::Banner::builder()
            .revealed(false)
            .css_classes([CLASSE_BANNIERE_SESSION])
            .build();
        let barre = gtk4::ProgressBar::builder().visible(false).build();
        barre.add_css_class("osd");
        let pile = gtk4::Stack::builder()
            .transition_type(gtk4::StackTransitionType::Crossfade)
            .vexpand(true)
            .build();
        let liste = gtk4::ListBox::builder()
            .selection_mode(gtk4::SelectionMode::None)
            .css_classes(["boxed-list"])
            // Sans cela la liste s'étirerait sur toute la hauteur, même avec une seule ligne.
            .valign(gtk4::Align::Start)
            .build();
        let toasts = adw::ToastOverlay::new();
        let fenetre = adw::ApplicationWindow::builder()
            .application(app)
            .title("WiMao")
            .default_width(LARGEUR)
            .default_height(HAUTEUR)
            .build();
        let moi = Fenetre {
            fenetre,
            chemins,
            titre,
            bandeau,
            banniere_session,
            barre,
            pile,
            liste,
            emplacement_vide: gtk4::Box::new(gtk4::Orientation::Vertical, 0),
            emplacement_liste: gtk4::Box::new(gtk4::Orientation::Vertical, 0),
            gestionnaires: Rc::new(RefCell::new(Vec::new())),
            bouton_ajouter: bouton_texte(
                "list-add-symbolic",
                &tr("Add an installer"),
                "win.ajouter",
            ),
            bouton_page_vide: gtk4::Button::builder()
                .label(tr("Add an installer"))
                .action_name("win.ajouter")
                .halign(gtk4::Align::Center)
                .css_classes(["suggested-action", "pill"])
                .build(),
            bouton_reparer: bouton_icone("emblem-system-symbolic", &tr("Repair"), "win.reparer"),
            bouton_sauvegarder: bouton_icone(
                "drive-harddisk-symbolic",
                &tr("Back up"),
                "win.sauvegarder",
            ),
            menu_import: gio::Menu::new(),
            toasts,
            navigation: adw::NavigationView::new(),
            dialogue_fermeture: Rc::new(RefCell::new(None)),
            occupe: Rc::new(Cell::new(false)),
            session: Rc::new(RefCell::new(None)),
            pulsation: Rc::new(RefCell::new(None)),
            sortie_confirmee: Rc::new(Cell::new(false)),
            toast_pontage: Rc::new(RefCell::new(None)),
            mis_a_jour: Rc::new(RefCell::new(mis_a_jour)),
            actions_travail: Rc::new(RefCell::new(Vec::new())),
        };
        moi.construire_pages();
        moi.actualiser_menu_import();
        moi.creer_actions(app);
        moi.assembler();
        if let Some(message) = avertissement_reglages {
            moi.toast(&message);
        }
        moi
    }

    /// Affiche la fenêtre.
    pub fn presenter(&self) {
        self.fenetre.present();
    }

    /// Empile `page` sur la page principale (bouton retour automatique). Les écrans de
    /// l'historique et des points de restauration passent par ici.
    pub(crate) fn pousser(&self, page: &adw::NavigationPage) {
        self.navigation.push(page);
    }

    /// Lance `puis` quand on peut arrêter Wine : tout de suite s'il n'y a aucun programme
    /// Windows ouvert dans l'environnement de WiMao ; sinon après que la personne a accepté de
    /// les fermer (`dialogues::confirmer_arret_wine`). Un refus n'appelle pas `puis` : un toast
    /// dit que rien n'a changé.
    ///
    /// Même règle que la CLI (`accord_avant_arret`). La liste vient de `/proc` : lecture
    /// instantanée, sans danger pour le fil de GTK. Si `/proc` est illisible, l'erreur est
    /// montrée et `puis` n'est pas appelé (on ne sait pas ce qu'on fermerait).
    pub(crate) fn avant_d_arreter_wine(&self, puis: impl FnOnce() + 'static) {
        match processus::programmes_ouverts(&self.chemins) {
            Ok(programmes) if programmes.is_empty() => puis(),
            Ok(programmes) => {
                let moi = self.clone();
                dialogues::confirmer_arret_wine(&self.fenetre, &programmes, move |oui| {
                    if oui {
                        puis();
                    } else {
                        moi.toast(&tr("Cancelled: nothing was changed"));
                    }
                });
            }
            Err(erreur) => self.erreur(&erreur, false),
        }
    }

    /// Fabrique les pages du contenu (chargement, vide, liste).
    fn construire_pages(&self) {
        let chargement = gtk4::Spinner::builder()
            .spinning(true)
            .halign(gtk4::Align::Center)
            .valign(gtk4::Align::Center)
            .width_request(32)
            .height_request(32)
            .build();
        self.pile.add_named(&chargement, Some(PAGE_CHARGEMENT));
        self.pile.add_named(&self.page_vide(), Some(PAGE_VIDE));
        self.pile.add_named(&self.page_liste(), Some(PAGE_LISTE));
        self.pile.add_named(&self.page_erreur(), Some(PAGE_ERREUR));
        self.pile.set_visible_child_name(PAGE_CHARGEMENT);
    }

    /// Page « Aucun plugin » : icône, explication, grand bouton pour l'action principale, puis
    /// la section « Managers » (souvent le vrai point de départ : Native Access…).
    fn page_vide(&self) -> adw::StatusPage {
        // La section reste plus étroite que la page : une ligne de 900 px se lit mal.
        let section = adw::Clamp::builder()
            .maximum_size(LARGEUR_SECTION_VIDE)
            .child(&self.emplacement_vide)
            .build();
        let contenu = gtk4::Box::new(gtk4::Orientation::Vertical, ESPACE_PAGE_VIDE);
        contenu.append(&self.bouton_page_vide);
        contenu.append(&section);
        adw::StatusPage::builder()
            .icon_name(ID_APPLICATION)
            .title(tr("No plugin"))
            .description(tr(
                "Choose the installer of a Windows plugin, or drop it on this window.",
            ))
            .child(&contenu)
            .build()
    }

    /// Page « Impossible de lire les plugins » : montrée quand le premier relevé échoue (sans
    /// elle, le sablier tournerait indéfiniment derrière le dialogue d'erreur). « Try again »
    /// relance le relevé ; « Copy the report » est l'action `win.rapport` (grisée pendant un
    /// travail, comme le menu).
    fn page_erreur(&self) -> adw::StatusPage {
        let reessayer = gtk4::Button::builder()
            .label(tr("Try again"))
            .halign(gtk4::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        let moi = self.clone();
        reessayer.connect_clicked(move |_| {
            // Le sablier revient pendant le nouveau relevé.
            moi.pile.set_visible_child_name(PAGE_CHARGEMENT);
            moi.rafraichir();
        });
        let rapport = gtk4::Button::builder()
            .label(tr("Copy the report"))
            .action_name("win.rapport")
            .halign(gtk4::Align::Center)
            .css_classes(["pill"])
            .build();
        let boutons = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
        boutons.append(&reessayer);
        boutons.append(&rapport);
        adw::StatusPage::builder()
            .icon_name("dialog-error-symbolic")
            .title(tr("Could not read the plugins"))
            .description(tr("Try again, or copy the report to ask for help."))
            .child(&boutons)
            .build()
    }

    /// Page de la liste : section « Managers » puis groupe « Plugins », dans une colonne
    /// bornée à [`LARGEUR_LISTE`], défilante si besoin.
    fn page_liste(&self) -> gtk4::ScrolledWindow {
        let plugins = adw::PreferencesGroup::builder()
            .title(tr("Plugins"))
            .build();
        plugins.add(&self.liste);
        let sections = gtk4::Box::new(gtk4::Orientation::Vertical, ESPACE_SECTIONS);
        sections.append(&self.emplacement_liste);
        sections.append(&plugins);
        let colonne = adw::Clamp::builder()
            .maximum_size(LARGEUR_LISTE)
            .tightening_threshold(LARGEUR_LISTE)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .child(&sections)
            .build();
        gtk4::ScrolledWindow::builder()
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .child(&colonne)
            .build()
    }

    /// Assemble les widgets dans la fenêtre et branche le glisser-déposer.
    fn assembler(&self) {
        let haut = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        haut.append(&self.barre_en_tete());
        haut.append(&self.banniere_session);
        haut.append(&self.bandeau);
        haut.append(&self.barre);
        let vue = adw::ToolbarView::new();
        vue.add_top_bar(&haut);
        vue.set_content(Some(&self.pile));
        // La fenêtre principale est la page racine ; les autres écrans s'empilent dessus.
        let principale = adw::NavigationPage::builder()
            .tag(PAGE_PRINCIPALE)
            .title("WiMao")
            .child(&vue)
            .build();
        self.navigation.add(&principale);
        self.toasts.set_child(Some(&self.navigation));
        self.fenetre.set_content(Some(&self.toasts));
        self.brancher_depot();
        self.brancher_fermeture();
        let moi = self.clone();
        self.banniere_session
            .connect_button_clicked(move |_| moi.bouton_banniere_session());
        self.rafraichir();
        self.verifier_pontage();
    }

    /// Glisser-déposer : lâcher un installeur sur la fenêtre = « Add an installer ».
    fn brancher_depot(&self) {
        let cible = gtk4::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        let moi = self.clone();
        cible.connect_enter(move |_, _, _| {
            moi.pile.add_css_class("zone-depot");
            gdk::DragAction::COPY
        });
        let moi = self.clone();
        cible.connect_leave(move |_| moi.pile.remove_css_class("zone-depot"));
        let moi = self.clone();
        cible.connect_drop(move |_, valeur, _, _| {
            moi.pile.remove_css_class("zone-depot");
            match valeur.get::<gio::File>().ok().and_then(|f| f.path()) {
                _ if moi.occupe.get() => moi.toast(&tr("A task is already running")),
                Some(chemin) if est_installeur(&chemin) => {
                    let suite = moi.clone();
                    moi.avant_d_arreter_wine(move || suite.installer(chemin, false));
                }
                _ => moi.toast(&tr("This file is not an installer or an archive")),
            }
            true
        });
        self.fenetre.add_controller(cible);
    }

    /// Pendant un travail, fermer la fenêtre demande confirmation (fichiers incomplets sinon).
    /// Pendant une session de gestionnaire, la question dit ce qui se passe : le gestionnaire
    /// reste ouvert, mais WiMao cesse de ponter (spec 4.4).
    fn brancher_fermeture(&self) {
        let moi = self.clone();
        self.fenetre.connect_close_request(move |_| {
            if !moi.occupe.get() || moi.sortie_confirmee.get() {
                // La fenêtre se ferme : les marques « Updated » ne durent pas plus longtemps.
                moi.mis_a_jour.borrow_mut().clear();
                return glib::Propagation::Proceed;
            }
            let confirme = moi.clone();
            let quitter = move || {
                confirme.abandonner_session();
                confirme.sortie_confirmee.set(true);
                confirme.fenetre.close();
            };
            // Un seul dialogue à la fois : un second clic sur la croix remet le premier devant.
            let ouvert = moi.dialogue_fermeture.borrow().clone();
            if let Some(dialogue) = ouvert {
                dialogue.present(Some(&moi.fenetre));
                return glib::Propagation::Stop;
            }
            let dialogue = match moi.session_a_abandonner() {
                Some(nom) => dialogues::confirmer_sortie_session(&moi.fenetre, &nom, quitter),
                None => dialogues::confirmer_sortie(&moi.fenetre, quitter),
            };
            // Fermé (quelle que soit la réponse) : on l'oublie, la croix pourra en rouvrir un.
            let oubli = moi.dialogue_fermeture.clone();
            dialogue.connect_closed(move |_| {
                oubli.borrow_mut().take();
            });
            *moi.dialogue_fermeture.borrow_mut() = Some(dialogue);
            glib::Propagation::Stop
        });
    }

    // --- Relevé et affichage -------------------------------------------------------------

    /// Relit les plugins et les gestionnaires (dans un fil : lire les DLL VST2 peut prendre un
    /// moment) et redessine.
    pub(crate) fn rafraichir(&self) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        // Copie pour le fil de travail : l'ensemble vit dans le fil de GTK.
        let mis_a_jour = self.mis_a_jour.borrow().clone();
        en_arriere_plan(
            move |_| operations::lister(&c, &mis_a_jour),
            |_| {},
            move |resultat| match resultat {
                Ok((releve, avertissements)) => {
                    moi.actualiser_menu_import();
                    moi.afficher(releve);
                    moi.avertir(&avertissements);
                }
                // Pendant un travail ou une session, « Repair » serait refusé : pas de bouton.
                Err(erreur) => {
                    // Premier relevé en échec : la page d'erreur remplace le sablier. Un
                    // relevé ultérieur raté laisse la liste précédente à l'écran.
                    if moi.pile.visible_child_name().as_deref() == Some(PAGE_CHARGEMENT) {
                        moi.pile.set_visible_child_name(PAGE_ERREUR);
                    }
                    moi.erreur(&erreur, !moi.occupe.get());
                }
            },
            {
                let moi = self.clone();
                move |cause| moi.panique(&cause)
            },
        );
    }

    /// Retient des plugins mis à jour (rapport d'installation ou bilan de session) pour leur
    /// marque « Updated » ; à appeler **avant** [`Fenetre::rafraichir`], qui la dessine.
    pub(crate) fn noter_mis_a_jour(&self, chemins: &BTreeSet<PathBuf>) {
        self.mis_a_jour.borrow_mut().extend(chemins.iter().cloned());
    }

    /// Remplit la liste et la section « Managers », le sous-titre, et choisit la page à montrer.
    fn afficher(&self, releve: Releve) {
        let lignes = &releve.plugins;
        self.liste.remove_all();
        for ligne in lignes {
            self.liste.append(&liste::creer_rangee(ligne));
        }
        self.titre.set_subtitle(&liste::sous_titre(lignes));
        // Page vide : le grand bouton central suffit, celui de l'en-tête ferait doublon.
        self.bouton_ajouter.set_visible(!lignes.is_empty());
        let page = if lignes.is_empty() {
            PAGE_VIDE
        } else {
            PAGE_LISTE
        };
        self.pile.set_visible_child_name(page);
        *self.gestionnaires.borrow_mut() = releve.gestionnaires;
        self.redessiner_gestionnaires();
    }

    /// Place la section « Managers » dans la page visible (et la retire de l'autre), d'après
    /// les lignes mémorisées et la session en cours. Aucune recette de gestionnaire : pas de
    /// section du tout, plutôt qu'un groupe vide. Rappelée au début et à la fin d'une session.
    pub(crate) fn redessiner_gestionnaires(&self) {
        for vieux in [&self.emplacement_vide, &self.emplacement_liste] {
            while let Some(enfant) = vieux.first_child() {
                vieux.remove(&enfant);
            }
        }
        let emplacement = match self.pile.visible_child_name().as_deref() {
            Some(PAGE_VIDE) => &self.emplacement_vide,
            Some(PAGE_LISTE) => &self.emplacement_liste,
            _ => return, // encore en chargement : le premier relevé redessinera
        };
        let lignes = self.gestionnaires.borrow();
        if !lignes.is_empty() {
            let session = self.nom_session();
            emplacement.append(&gestionnaires::creer_section(&lignes, session.as_deref()));
        }
        // Page vide avec la section : version resserrée, pour que tout tienne sans défiler.
        if let Some(page) = self
            .emplacement_vide
            .ancestor(adw::StatusPage::static_type())
        {
            if lignes.is_empty() {
                page.remove_css_class(CLASSE_COMPACTE);
            } else {
                page.add_css_class(CLASSE_COMPACTE);
            }
        }
    }

    /// La ligne affichée du gestionnaire `nom`, si elle existe.
    pub(crate) fn ligne_gestionnaire(&self, nom: &str) -> Option<LigneGestionnaire> {
        self.gestionnaires
            .borrow()
            .iter()
            .find(|l| l.nom == nom)
            .cloned()
    }

    /// Montre un toast (message court qui disparaît seul).
    pub(crate) fn toast(&self, message: &str) {
        let toast = adw::Toast::builder()
            .title(message)
            .timeout(DUREE_TOAST)
            .build();
        self.ajouter_toast(toast);
    }

    /// Montre un toast déjà construit (par exemple avec un bouton, `reparation.rs`).
    pub(crate) fn ajouter_toast(&self, toast: adw::Toast) {
        self.toasts.add_toast(toast);
    }

    /// Toast d'avertissement pour le premier avertissement (tous sont déjà au journal).
    pub(crate) fn avertir(&self, avertissements: &Avertissements) {
        if let Some(premier) = avertissements.first() {
            self.toast(&trf("Warning: {message}", &[("message", premier)]));
        }
    }

    /// Une panique du fil de travail : on la note au journal, on remet l'interface en état
    /// (au cas où un travail était en cours) et on prévient l'utilisateur.
    pub(crate) fn panique(&self, cause: &str) {
        journal::noter("panique du fil de travail", cause);
        self.fin_de_travail();
        dialogues::afficher_panique(&self.fenetre, cause, journal::fichier_actif());
    }

    /// Dialogue d'erreur. `reparable` : proposer « Repair » si une règle d'erreur désigne un
    /// problème réparable. Toutes les opérations l'autorisent **sauf la réparation elle-même** :
    /// un échec de réparation qui reproposerait « Repair » ferait tourner l'utilisateur en rond.
    pub(crate) fn erreur(&self, erreur: &wimao_core::Erreur, reparable: bool) {
        let moi = self.clone();
        dialogues::afficher_erreur(&self.fenetre, erreur, reparable, move |probleme| {
            moi.reparer(Some(vec![probleme]));
        });
    }

    // --- Travail en arrière-plan ---------------------------------------------------------

    /// Lance `travail` dans un fil avec bandeau et barre visibles et actions coupées ;
    /// `fin` s'exécute ensuite, interface déjà remise en état.
    pub(crate) fn lancer<T: Send + 'static>(
        &self,
        message: &str,
        travail: impl FnOnce(&dyn wimao_core::Progression) -> T + Send + 'static,
        fin: impl FnOnce(T) + 'static,
    ) {
        if self.occupe.replace(true) {
            return; // un travail est déjà en cours (raccourci clavier, dépôt…)
        }
        self.retirer_toast_pontage();
        self.basculer_occupe(true, message);
        let (pour_etape, pour_fin, pour_panique) = (self.clone(), self.clone(), self.clone());
        en_arriere_plan(
            travail,
            move |etape| pour_etape.bandeau.set_title(&etape),
            move |resultat| {
                pour_fin.fin_de_travail();
                fin(resultat);
            },
            move |cause| pour_panique.panique(&cause),
        );
    }

    /// Remet l'interface en état de repos (actions rendues, bandeau et barre cachés).
    pub(crate) fn fin_de_travail(&self) {
        self.occupe.set(false);
        self.basculer_occupe(false, "");
    }

    /// Montre ou cache l'indicateur de travail et (dés)active les actions.
    pub(crate) fn basculer_occupe(&self, occupe: bool, message: &str) {
        for action in self.actions_travail.borrow().iter() {
            action.set_enabled(!occupe);
        }
        self.montrer_indicateur(occupe, message);
    }

    /// Infobulles des boutons de travail : pendant la session du gestionnaire `session`, elles
    /// disent pourquoi le bouton est grisé (spec 4.2) ; `None` remet les libellés. Le grand
    /// bouton de la page vide n'a pas d'infobulle hors session (son libellé est visible).
    /// Les boutons des lignes de gestionnaires suivent `redessiner_gestionnaires`.
    pub(crate) fn expliquer_indisponibles(&self, session: Option<&str>) {
        let boutons = [
            (&self.bouton_ajouter, tr("Add an installer"), true),
            (&self.bouton_page_vide, tr("Add an installer"), false),
            (&self.bouton_reparer, tr("Repair"), true),
            (&self.bouton_sauvegarder, tr("Back up"), true),
        ];
        for (bouton, libelle, infobulle_au_repos) in boutons {
            let texte = match session {
                Some(nom) => Some(trf(
                    "{action} is not available while {name} is open",
                    &[("action", &libelle), ("name", nom)],
                )),
                None => infobulle_au_repos.then_some(libelle),
            };
            bouton.set_tooltip_text(texte.as_deref());
        }
    }

    /// Montre ou cache le bandeau de travail et la barre de progression en pulsation.
    pub(crate) fn montrer_indicateur(&self, occupe: bool, message: &str) {
        self.bandeau.set_title(message);
        self.bandeau.set_revealed(occupe);
        self.barre.set_visible(occupe);
        let mut minuteur = self.pulsation.borrow_mut();
        if occupe && minuteur.is_none() {
            let barre = self.barre.clone();
            *minuteur = Some(glib::timeout_add_local(PULSATION, move || {
                barre.pulse();
                glib::ControlFlow::Continue
            }));
        } else if !occupe {
            if let Some(source) = minuteur.take() {
                source.remove();
            }
        }
    }

    // --- Actions -------------------------------------------------------------------------

    /// « Add an installer » : choisit un fichier puis l'installe.
    pub(crate) fn demander_installeur(&self) {
        let moi = self.clone();
        dialogues::choisir_installeur(&self.fenetre, move |chemin| {
            let suite = moi.clone();
            moi.avant_d_arreter_wine(move || suite.installer(chemin, false));
        });
    }

    /// Installe `fichier` ; `interactif` ouvre l'installeur au lieu de le lancer en silence.
    fn installer(&self, fichier: PathBuf, interactif: bool) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        let fichier_suite = fichier.clone();
        self.lancer(
            &tr("Installing…"),
            move |p| operations::installer(&c, &fichier, interactif, p),
            move |resultat| match resultat {
                Err(erreur) => moi.erreur(&erreur, true),
                Ok((installe, avertissements)) => {
                    moi.apres_installation(installe, interactif, fichier_suite);
                    moi.avertir(&avertissements);
                }
            },
        );
    }

    /// Suite d'une installation réussie : proposition d'installation normale, ou toast adapté
    /// puis la suite commune à toute installation (lien de connexion, consigne d'activation :
    /// [`Fenetre::suite_commune`]).
    fn apres_installation(
        &self,
        installe: operations::Installe,
        interactif: bool,
        fichier: PathBuf,
    ) {
        let rapport = &installe.rapport;
        if rapport.interactif_necessaire && !interactif {
            let relance = self.clone();
            let question = operations::message_aucun_plugin(rapport.code_installeur);
            dialogues::proposer_installation_normale(&self.fenetre, &question, move || {
                relance.installer(fichier, true);
            });
            return;
        }
        let message = match operations::bilan_installation(rapport) {
            Some(bilan) => bilan,
            None if interactif => tr("No new or updated plugin found"),
            None => tr("Installation finished"),
        };
        self.toast(&message);
        self.noter_mis_a_jour(&rapport.mis_a_jour);
        self.rafraichir();
        // Nom du produit pour les titres : celui de la recette (seules les recettes ont un
        // lien ou une consigne), sinon le fichier.
        let nom = rapport.recette.clone().unwrap_or_else(|| {
            fichier
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        });
        self.suite_commune(&nom, installe);
    }

    /// « Back up » : choisit un dossier puis y écrit la sauvegarde.
    fn demander_sauvegarde(&self) {
        let moi = self.clone();
        dialogues::choisir_dossier(&self.fenetre, move |dossier| {
            let suite = moi.clone();
            moi.avant_d_arreter_wine(move || suite.sauvegarder(dossier));
        });
    }

    /// Écrit la sauvegarde dans `dossier`.
    fn sauvegarder(&self, dossier: PathBuf) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        let nom_dossier = dossier.file_name().map_or_else(
            || dossier.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        self.lancer(
            &tr("Backing up…"),
            move |p| operations::sauvegarder(&c, &dossier, p),
            move |resultat| match resultat {
                Err(erreur) => moi.erreur(&erreur, true),
                Ok(sha256) => moi.toast_sauvegarde(&nom_dossier, sha256),
            },
        );
    }

    /// Toast de fin de sauvegarde : où elle est, et « Show » qui ouvre le dossier dans le
    /// gestionnaire de fichiers (portail `FileLauncher`, sélectionne le `.sha256`).
    fn toast_sauvegarde(&self, nom_dossier: &str, sha256: PathBuf) {
        let toast = adw::Toast::builder()
            .title(trf("Backup saved in {folder}", &[("folder", nom_dossier)]))
            .button_label(tr("Show"))
            .timeout(DUREE_TOAST_ACTION)
            .build();
        let fenetre = self.fenetre.clone();
        toast.connect_button_clicked(move |_| {
            let lanceur = gtk4::FileLauncher::new(Some(&gio::File::for_path(&sha256)));
            lanceur.open_containing_folder(Some(&fenetre), None::<&gio::Cancellable>, |r| {
                if let Err(erreur) = r {
                    journal::noter("ouverture du dossier de la sauvegarde", &erreur.to_string());
                }
            });
        });
        self.ajouter_toast(toast);
    }

    /// « Copy the report » : met le rapport anonymisé dans le presse-papiers.
    fn copier_rapport(&self) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        self.lancer(
            &tr("Preparing the report…"),
            move |_| operations::rapport(&c),
            move |resultat| match resultat {
                Err(erreur) => moi.erreur(&erreur, true),
                Ok(texte) => {
                    dialogues::copier(&moi.fenetre, &texte);
                    moi.toast(&tr("Report copied"));
                }
            },
        );
    }
}
