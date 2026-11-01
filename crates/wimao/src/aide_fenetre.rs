//! La fenêtre du mode d'emploi : liste des chapitres à gauche, texte du chapitre à droite.
//!
//! - [`ouvrir_aide`] : ouvre la fenêtre (ou ramène celle qui existe), éventuellement sur un
//!   chapitre donné ;
//! - la barre latérale est une `GtkListBox` des titres traduits (`tr`) ; le contenu est un
//!   `ScrolledWindow > AdwClamp(720)` rempli de `GtkLabel` construits depuis les
//!   `Bloc` de `aide.rs` ;
//! - un clic sur un lien `aide:<clé>` change de chapitre, un lien `https://` s'ouvre dans le
//!   navigateur (`gtk4::UriLauncher`) ; tout autre lien est ignoré.
//!
//! Choix non évidents :
//! - **une seule fenêtre à la fois** : sa référence faible et celle de sa liste sont gardées
//!   dans un `thread_local` (tout le code GTK tourne dans le même fil) ; un nouvel appel
//!   présente la fenêtre existante et change son chapitre si on le demande ;
//! - la langue du texte est celle de l'interface à la construction de la fenêtre ; si elle a
//!   changé quand on rouvre l'aide, la fenêtre est détruite et reconstruite ;
//! - les rappels (changement de ligne, liens) ne gardent que des références **faibles** sur la
//!   liste, pour ne pas créer de cycle de références avec leurs propres widgets.
//!
//! Utilise `aide.rs`. Utilisé par `fenetre/actions.rs` (F1, menu) et `dialogues.rs` (lien
//! « Common problems » de la boîte d'erreur).

use std::cell::RefCell;

use gtk4::{glib, glib::prelude::*, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{langue_courante, tr};

use crate::aide::{self, Bloc, Lien, CHAPITRES};

/// Largeur maximale du texte (pixels) : au-delà, les lignes deviennent pénibles à lire.
const LARGEUR_TEXTE: i32 = 720;

/// La fenêtre d'aide ouverte : références faibles (elles n'empêchent pas la fermeture) et
/// langue dans laquelle elle a été construite.
struct Ouverte {
    fenetre: glib::WeakRef<adw::Window>,
    liste: glib::WeakRef<gtk4::ListBox>,
    langue: String,
}

thread_local! {
    /// La fenêtre d'aide ouverte, s'il y en a une.
    static OUVERTE: RefCell<Option<Ouverte>> = const { RefCell::new(None) };
}

/// Ouvre le mode d'emploi, transitoire pour `parent`, sur le chapitre `chapitre` (sa clé ; le
/// premier chapitre si `None` ou inconnu). Si la fenêtre d'aide existe déjà, elle est
/// présentée (et change de chapitre si `chapitre` est donné) : jamais deux fenêtres.
pub fn ouvrir_aide(parent: &impl IsA<gtk4::Window>, chapitre: Option<&str>) {
    let rang = chapitre
        .and_then(|cle| CHAPITRES.iter().position(|c| c.cle == cle))
        .unwrap_or(0);
    let langue = langue_courante();
    let existante = OUVERTE.with(|o| {
        o.borrow()
            .as_ref()
            .and_then(|o| Some((o.fenetre.upgrade()?, o.liste.upgrade()?, o.langue.clone())))
    });
    if let Some((fenetre, liste, langue_ouverte)) = existante {
        if langue_ouverte == langue {
            if chapitre.is_some() {
                choisir(&liste, rang);
            }
            fenetre.present();
            return;
        }
        // La langue de l'interface a changé depuis l'ouverture : on repart d'une fenêtre neuve.
        fenetre.destroy();
    }
    let (fenetre, liste) = construire(parent, rang, &langue);
    OUVERTE.with(|o| {
        *o.borrow_mut() = Some(Ouverte {
            fenetre: fenetre.downgrade(),
            liste: liste.downgrade(),
            langue,
        });
    });
    fenetre.present();
}

/// Sélectionne le chapitre de rang `rang` dans la liste (ce qui affiche son texte).
fn choisir(liste: &gtk4::ListBox, rang: usize) {
    if let Some(ligne) = i32::try_from(rang).ok().and_then(|i| liste.row_at_index(i)) {
        liste.select_row(Some(&ligne));
    }
}

/// Construit la fenêtre d'aide, sélectionnée sur le chapitre `rang`.
fn construire(
    parent: &impl IsA<gtk4::Window>,
    rang: usize,
    langue: &str,
) -> (adw::Window, gtk4::ListBox) {
    // Barre latérale : un titre traduit par chapitre.
    let liste = gtk4::ListBox::builder()
        .selection_mode(gtk4::SelectionMode::Single)
        .css_classes(["navigation-sidebar"])
        .build();
    for chapitre in &CHAPITRES {
        let titre = gtk4::Label::builder()
            .label(tr(chapitre.titre_msgid))
            .xalign(0.0)
            .wrap(true)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(6)
            .margin_end(6)
            .build();
        liste.append(&gtk4::ListBoxRow::builder().child(&titre).build());
    }
    let defilement_liste = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .child(&liste)
        .build();
    let entete_liste = adw::HeaderBar::new();
    let vue_liste = adw::ToolbarView::new();
    vue_liste.add_top_bar(&entete_liste);
    vue_liste.set_content(Some(&defilement_liste));
    let page_liste = adw::NavigationPage::builder()
        .title(tr("User guide"))
        .child(&vue_liste)
        .build();

    // Contenu : les blocs du chapitre, dans une colonne de largeur limitée.
    let colonne = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(12)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(18)
        .margin_end(18)
        .build();
    let clamp = adw::Clamp::builder()
        .maximum_size(LARGEUR_TEXTE)
        .child(&colonne)
        .build();
    let defilement = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let vue_contenu = adw::ToolbarView::new();
    vue_contenu.add_top_bar(&adw::HeaderBar::new());
    vue_contenu.set_content(Some(&defilement));
    let page_contenu = adw::NavigationPage::builder()
        .title(tr("User guide"))
        .child(&vue_contenu)
        .build();

    let division = adw::NavigationSplitView::builder()
        .sidebar(&page_liste)
        .content(&page_contenu)
        .min_sidebar_width(200.0)
        .max_sidebar_width(280.0)
        .build();
    let fenetre = adw::Window::builder()
        .title(tr("User guide"))
        .default_width(960)
        .default_height(680)
        .transient_for(parent)
        .content(&division)
        .build();

    // Choisir une ligne affiche le chapitre correspondant.
    let (pour_colonne, pour_defilement, pour_page) =
        (colonne.clone(), defilement.clone(), page_contenu.clone());
    // Références faibles : la liste possède ses rappels, qui ne doivent pas la posséder en retour.
    let pour_liste = liste.downgrade();
    let langue = langue.to_owned();
    liste.connect_row_selected(move |_, ligne| {
        let Some(chapitre) = ligne
            .and_then(|l| usize::try_from(l.index()).ok())
            .and_then(|i| CHAPITRES.get(i))
        else {
            return;
        };
        pour_page.set_title(&tr(chapitre.titre_msgid));
        let markdown = aide::texte(chapitre.cle, &langue).unwrap_or_default();
        afficher(&pour_colonne, &pour_liste, &aide::convertir(markdown));
        // Un nouveau chapitre s'ouvre en haut.
        pour_defilement.vadjustment().set_value(0.0);
        // Sur un écran étroit, la liste se replie : montrer le texte.
        division_montrer_contenu(&pour_colonne);
    });
    choisir(&liste, rang);
    (fenetre, liste)
}

/// Sur un écran étroit (division repliée), passe de la liste au contenu après un choix.
fn division_montrer_contenu(colonne: &gtk4::Box) {
    if let Some(division) = colonne
        .ancestor(adw::NavigationSplitView::static_type())
        .and_downcast::<adw::NavigationSplitView>()
    {
        division.set_show_content(true);
    }
}

/// Remplace le contenu de `colonne` par les widgets des `blocs` ; `liste` sert aux liens
/// internes (changer de chapitre).
fn afficher(colonne: &gtk4::Box, liste: &glib::WeakRef<gtk4::ListBox>, blocs: &[Bloc]) {
    while let Some(enfant) = colonne.first_child() {
        colonne.remove(&enfant);
    }
    for bloc in blocs {
        match bloc {
            Bloc::Titre(niveau, texte) => {
                let classe = if *niveau == 1 { "title-1" } else { "title-2" };
                colonne.append(&etiquette(texte, classe, liste));
            }
            Bloc::Paragraphe(texte) => colonne.append(&etiquette(texte, "body", liste)),
            Bloc::Liste {
                numerotee,
                elements,
            } => {
                let boite = gtk4::Box::builder()
                    .orientation(gtk4::Orientation::Vertical)
                    .spacing(6)
                    .build();
                for (i, texte) in elements.iter().enumerate() {
                    let ligne = gtk4::Box::builder().spacing(8).build();
                    let puce = if *numerotee {
                        format!("{}.", i + 1)
                    } else {
                        "•".to_owned()
                    };
                    ligne.append(
                        &gtk4::Label::builder()
                            .label(puce)
                            .valign(gtk4::Align::Start)
                            .css_classes(["body"])
                            .build(),
                    );
                    ligne.append(&etiquette(texte, "body", liste));
                    boite.append(&ligne);
                }
                colonne.append(&boite);
            }
        }
    }
}

/// Un `GtkLabel` de texte en balisage Pango, avec la classe de style `classe`, dont les liens
/// sont traités par [`suivre_lien`].
fn etiquette(balisage: &str, classe: &str, liste: &glib::WeakRef<gtk4::ListBox>) -> gtk4::Label {
    let etiquette = gtk4::Label::builder()
        .use_markup(true)
        .label(balisage)
        .wrap(true)
        .selectable(true)
        .xalign(0.0)
        .hexpand(true)
        .css_classes([classe])
        .build();
    let liste = liste.clone(); // clone d'une référence faible
    etiquette.connect_activate_link(move |e, uri| suivre_lien(e, &liste, uri));
    etiquette
}

/// Traite un clic sur le lien `uri` : change de chapitre, ouvre le navigateur, ou ignore.
/// Rend toujours « traité » : GTK ne doit pas tenter d'ouvrir lui-même un lien refusé.
fn suivre_lien(
    etiquette: &gtk4::Label,
    liste: &glib::WeakRef<gtk4::ListBox>,
    uri: &str,
) -> glib::Propagation {
    match aide::lien(uri) {
        Some(Lien::Chapitre(cle)) => {
            if let (Some(liste), Some(rang)) =
                (liste.upgrade(), CHAPITRES.iter().position(|c| c.cle == cle))
            {
                choisir(&liste, rang);
            }
        }
        Some(Lien::Web(adresse)) => {
            let fenetre = etiquette.root().and_downcast::<gtk4::Window>();
            gtk4::UriLauncher::new(&adresse).launch(
                fenetre.as_ref(),
                None::<&gtk4::gio::Cancellable>,
                |_| {},
            );
        }
        None => {}
    }
    glib::Propagation::Stop
}
