//! La barre d'en-tête de la fenêtre et le menu principal.
//!
//! - [`Fenetre::barre_en_tete`] : titre centré, « Add an installer » à gauche, « Repair »,
//!   « Back up », « History » (horloge) et le menu à droite ;
//! - [`menu_principal`] : rapport et langue (choix unique parmi [`LANGUES`]), restauration et
//!   annulation de l'import, mode d'emploi, à propos, quitter ;
//! - [`bouton_texte`] et [`bouton_icone`] : les deux sortes de boutons de l'en-tête.
//!
//! Les boutons sont reliés aux actions `win.*` créées par `fenetre/actions.rs` : ils se grisent
//! avec elles. Utilisé par `fenetre/mod.rs` (construction et assemblage) ; `actions.rs` y lit
//! [`LANGUE_SYSTEME`]. Pour ajouter une langue, voir `CONTRIBUTING.md`.

use gtk4::{gio, prelude::*};
use libadwaita as adw;
use wimao_core::i18n::tr;

use super::Fenetre;

/// Langues proposées : (code des réglages, libellé). « systeme » = suivre le système ;
/// les noms de langues ne sont pas traduits (chacun s'écrit dans sa langue).
pub(super) const LANGUES: [(&str, &str); 4] = [
    ("systeme", ""),
    ("fr", "Français"),
    ("en", "English"),
    ("es", "Español"),
];
/// Valeur de la clé `langue` quand on suit le système.
pub(super) const LANGUE_SYSTEME: &str = "systeme";

impl Fenetre {
    /// Barre d'en-tête : boutons, titre, menu principal.
    pub(super) fn barre_en_tete(&self) -> adw::HeaderBar {
        let barre = adw::HeaderBar::new();
        barre.set_title_widget(Some(&self.titre));
        self.bouton_ajouter.add_css_class("suggested-action");
        barre.pack_start(&self.bouton_ajouter);
        let menu = gtk4::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text(tr("Main menu"))
            .menu_model(&menu_principal(&self.menu_import))
            .primary(true)
            .build();
        menu.update_property(&[gtk4::accessible::Property::Label(&tr("Main menu"))]);
        barre.pack_end(&menu);
        // `pack_end` empile de droite à gauche : sauvegarde puis réparation à gauche du menu.
        barre.pack_end(&self.bouton_sauvegarder);
        barre.pack_end(&self.bouton_reparer);
        // Historique tout à gauche du groupe de droite ; Ctrl+H, comme son infobulle le dit.
        barre.pack_end(&bouton_icone(
            "document-open-recent-symbolic",
            &tr("History"),
            "win.historique",
        ));
        barre
    }
}

/// Bouton de l'en-tête avec icône et libellé (action principale).
pub(super) fn bouton_texte(icone: &str, libelle: &str, action: &str) -> gtk4::Button {
    let contenu = adw::ButtonContent::builder()
        .icon_name(icone)
        .label(libelle)
        .build();
    let bouton = gtk4::Button::builder()
        .child(&contenu)
        .action_name(action)
        .tooltip_text(libelle)
        .visible(false)
        .build();
    bouton.update_property(&[gtk4::accessible::Property::Label(libelle)]);
    bouton
}

/// Bouton de l'en-tête à icône seule : l'infobulle et le nom accessible portent le libellé.
pub(super) fn bouton_icone(icone: &str, libelle: &str, action: &str) -> gtk4::Button {
    let bouton = gtk4::Button::builder()
        .icon_name(icone)
        .action_name(action)
        .tooltip_text(libelle)
        .build();
    bouton.update_property(&[gtk4::accessible::Property::Label(libelle)]);
    bouton
}

/// Menu principal : rapport, sauvegarde/import, langue (choix unique), mode d'emploi, à propos,
/// quitter. `menu_import` est la section « Restore a backup… » / « Cancel the import », que
/// `Fenetre::actualiser_menu_import` remplit et met à jour (`restaurer.rs`).
pub(super) fn menu_principal(menu_import: &gio::Menu) -> gio::Menu {
    let menu = gio::Menu::new();
    // Chaque groupe est une vraie section : libadwaita trace un séparateur entre elles.
    let reglages = gio::Menu::new();
    reglages.append(Some(&tr("_Copy the report")), Some("win.rapport"));
    let langues = gio::Menu::new();
    for (code, nom) in LANGUES {
        let libelle = if code == LANGUE_SYSTEME {
            tr("System")
        } else {
            nom.to_owned()
        };
        langues.append(Some(&libelle), Some(&format!("win.langue::{code}")));
    }
    reglages.append_submenu(Some(&tr("_Language")), &langues);
    menu.append_section(None, &reglages);
    menu.append_section(None, menu_import);
    let fin = gio::Menu::new();
    fin.append(Some(&tr("_User guide")), Some("win.aide"));
    fin.append(Some(&tr("_About WiMao")), Some("win.a-propos"));
    fin.append(Some(&tr("_Quit")), Some("app.quitter"));
    menu.append_section(None, &fin);
    menu
}
