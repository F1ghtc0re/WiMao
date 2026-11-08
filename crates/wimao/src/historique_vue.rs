//! La page « History » : ce que WiMao a fait, jour par jour, et « Go back to before this ».
//!
//! Deux parties :
//! - la **partie pure**, testée sans écran : [`grouper_par_jour`] (titres « Today »,
//!   « Yesterday », sinon la date), [`point_disponible`] (une entrée n'offre le retour que si son
//!   point est dans `ListePoints::points`, jamais un point périmé ou disparu), et les petits
//!   formateurs d'heure et de date ;
//! - la **page** : [`Fenetre::ouvrir_historique`] charge les données dans un fil (lire le
//!   journal et lister les points touche le disque), puis empile une `adw::NavigationPage` (un
//!   `AdwPreferencesGroup` par jour, une `AdwActionRow` par entrée avec sa pastille d'issue).
//!
//! Retour en arrière : le bouton d'une ligne active l'action `win.revenir` (paramètre = identifiant
//! du point). Cette action fait partie de celles que `fenetre/actions.rs` coupe pendant un
//! travail ou une session : les boutons se grisent donc d'eux-mêmes et leur infobulle dit
//! pourquoi, alors que la page reste consultable. Ensuite : aperçu dans un fil de travail,
//! `adw::AlertDialog` de confirmation (motif, date, nombre de fichiers), question habituelle
//! avant d'arrêter Wine si des programmes sont ouverts, retour dans un fil de travail, toast de
//! bilan, puis rechargement de la page et de la liste des plugins. Le moteur
//! (`restauration::revenir`) écrit lui-même l'entrée d'historique du retour.
//!
//! Les libellés suivent le mode d'emploi (`aide/*/06-historique.md`) : ce sont les mêmes mots.
//!
//! Ajoute des méthodes à `Fenetre` (même motif que `reparation.rs`). Utilise `operations.rs`,
//! `liste.rs` (pastilles), `travail.rs` et le moteur (`historique`, `restauration`). Utilisé par
//! `fenetre/actions.rs` (actions `win.historique` et `win.revenir`) et `fenetre/entete.rs`
//! (bouton en forme d'horloge).

use gtk4::{gio, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::historique::{
    jour_local_iso, libelle_action, libelle_resultat, Action, Entree, ResultatAction,
};
use wimao_core::i18n::{tr, trf, trn};
use wimao_core::restauration::{libelle_motif, BilanRetour, ListePoints, PointRestauration};

use crate::fenetre::{Fenetre, PAGE_PRINCIPALE};
use crate::liste::creer_badge_libre;
use crate::operations::{self, ApercuRetour};
use crate::travail::en_arriere_plan;

/// Largeur maximale (px) des pages de contenu empilées (historique) : colonne de 640 px, à
/// l'image de la fenêtre de l'assistant.
const LARGEUR_CONTENU: i32 = 640;
/// Étiquette de la page dans le `NavigationView`.
const PAGE_HISTORIQUE: &str = "historique";

/// Faut-il pousser la page Historique ? Non si elle est déjà la page visible : un second
/// Ctrl+H n'empile pas de doublon. `page_visible` est l'étiquette de la page à l'écran.
pub(crate) fn doit_ouvrir_historique(page_visible: Option<&str>) -> bool {
    page_visible != Some(PAGE_HISTORIQUE)
}
/// Nom (sans `win.`) de l'action « Go back to before this » ; paramètre : identifiant du point.
pub const ACTION_REVENIR: &str = "revenir";
/// Réponses du dialogue de confirmation.
const REPONSE_ANNULER: &str = "annuler";
const REPONSE_REVENIR: &str = "revenir";

// --- Partie pure ---------------------------------------------------------------------------

/// Les entrées d'un même jour, sous un titre.
pub struct Jour {
    /// « Today », « Yesterday » (traduits) ou la date `AAAA-MM-JJ`.
    pub titre: String,
    /// Entrées du jour, dans l'ordre reçu (la plus récente d'abord, comme `historique::lire`).
    pub entrees: Vec<Entree>,
}

/// Jour d'une entrée : les dix premiers caractères de sa date (`AAAA-MM-JJ`), ou la date
/// entière si elle est plus courte (ligne écrite à la main).
fn jour_de(entree: &Entree) -> &str {
    entree.date.get(..10).unwrap_or(&entree.date)
}

/// Regroupe `entrees` par jour, en gardant l'ordre. `aujourd_hui` et `hier` sont au format
/// `AAAA-MM-JJ` (donnés par l'appelant : la fonction reste testable sans horloge).
///
/// Seules les entrées **consécutives** d'un même jour se regroupent : le journal est déjà trié
/// du plus récent au plus ancien, donc un jour n'apparaît qu'une fois.
pub fn grouper_par_jour(entrees: &[Entree], aujourd_hui: &str, hier: &str) -> Vec<Jour> {
    let mut jours: Vec<(String, Jour)> = Vec::new();
    for entree in entrees {
        let cle = jour_de(entree);
        match jours.last_mut() {
            Some((derniere, jour)) if derniere == cle => jour.entrees.push(entree.clone()),
            _ => {
                let titre = if cle == aujourd_hui {
                    tr("Today")
                } else if cle == hier {
                    tr("Yesterday")
                } else {
                    cle.to_owned()
                };
                jours.push((
                    cle.to_owned(),
                    Jour {
                        titre,
                        entrees: vec![entree.clone()],
                    },
                ));
            }
        }
    }
    jours.into_iter().map(|(_, jour)| jour).collect()
}

/// Le point de restauration que `entree` permet de retrouver, s'il est **utilisable** : dans
/// `liste.points`. Un point périmé (une sauvegarde a été restaurée depuis), incomplet ou
/// supprimé (trop ancien) n'est pas proposé, et une entrée sans point non plus.
pub fn point_disponible(entree: &Entree, liste: &ListePoints) -> Option<PointRestauration> {
    let id = entree.point.as_deref()?;
    liste.points.iter().find(|p| p.id == id).cloned()
}

/// Heure `HH:MM` d'une date `AAAA-MM-JJTHH:MM:SS+HH:MM` ; vide si la date est plus courte.
fn heure_de(date: &str) -> &str {
    date.get(11..16).unwrap_or("")
}

/// Date lisible d'un identifiant de point `AAAA-MM-JJ-HHhMMmSS` : `AAAA-MM-JJ HH:MM`. Un
/// identifiant d'une autre forme est rendu tel quel.
fn date_du_point(id: &str) -> String {
    match (id.get(..10), id.get(11..13), id.get(14..16)) {
        (Some(jour), Some(heures), Some(minutes)) if id.get(13..14) == Some("h") => {
            format!("{jour} {heures}:{minutes}")
        }
        _ => id.to_owned(),
    }
}

/// Icône symbolique d'une action (toutes présentes dans Adwaita et Yaru).
fn icone_action(action: Action) -> &'static str {
    match action {
        Action::Installation => "system-software-install-symbolic",
        Action::Gestionnaire => "view-app-grid-symbolic",
        Action::Reparation => "emblem-system-symbolic",
        Action::Retour => "edit-undo-symbolic",
        Action::Sauvegarde => "document-save-symbolic",
        Action::Restauration => "document-revert-symbolic",
        Action::Import => "folder-download-symbolic",
        Action::AnnulationImport => "edit-undo-symbolic",
    }
}

/// Icône et classe CSS (voir `style.css`) de la pastille d'une issue. Comme pour les plugins,
/// l'icône et le mot disent la même chose : la couleur seule ne suffit pas.
fn apparence_resultat(resultat: ResultatAction) -> (&'static str, &'static str) {
    match resultat {
        ResultatAction::Reussi => ("object-select-symbolic", "success"),
        ResultatAction::Echec => ("dialog-error-symbolic", "error"),
        ResultatAction::Annule => ("process-stop-symbolic", "dim-label"),
    }
}

/// Sous-titre d'une ligne : « 14:03 · Installation · précision » (la précision est omise si
/// elle est vide).
fn sous_titre(entree: &Entree) -> String {
    let mut morceaux = vec![
        heure_de(&entree.date).to_owned(),
        libelle_action(entree.action),
    ];
    let details = entree.details_affiches();
    if !details.is_empty() {
        morceaux.push(details);
    }
    morceaux.retain(|m| !m.is_empty());
    morceaux.join(" · ")
}

// --- Widgets -------------------------------------------------------------------------------

/// Une ligne de l'historique : icône de l'action, sujet, sous-titre, pastille d'issue et, si
/// `point` est utilisable, le bouton « Go back to before this ».
fn creer_rangee(entree: &Entree, point: Option<&PointRestauration>) -> adw::ActionRow {
    let rangee = adw::ActionRow::builder()
        .title(entree.sujet_affiche())
        .subtitle(sous_titre(entree))
        // Un sujet ou une précision peut contenir « & » ou « < » : jamais de balisage Pango.
        .use_markup(false)
        .build();
    rangee.add_prefix(&gtk4::Image::from_icon_name(icone_action(entree.action)));
    let (icone, classe) = apparence_resultat(entree.resultat);
    rangee.add_suffix(&creer_badge_libre(
        icone,
        classe,
        &libelle_resultat(entree.resultat),
    ));
    if let Some(point) = point {
        rangee.add_suffix(&creer_bouton_retour(&point.id));
    }
    rangee
}

/// Bouton « Go back to before this » du point `id`. Il est relié à l'action `win.revenir` :
/// quand l'action est coupée (travail ou session en cours), il se grise et son infobulle dit
/// d'attendre.
fn creer_bouton_retour(id: &str) -> gtk4::Button {
    let bouton = gtk4::Button::builder()
        .label(tr("Go back to before this"))
        .valign(gtk4::Align::Center)
        .action_name(format!("win.{ACTION_REVENIR}"))
        .action_target(&id.to_variant())
        .build();
    // GTK grise le bouton d'une action coupée ; on suit ce changement pour l'infobulle.
    let mettre_infobulle = |b: &gtk4::Button| {
        b.set_tooltip_text(
            (!b.is_sensitive())
                .then(|| tr("Wait until the current task is finished"))
                .as_deref(),
        );
    };
    mettre_infobulle(&bouton);
    bouton.connect_sensitive_notify(mettre_infobulle);
    bouton
}

/// Contenu de la page : un groupe par jour, ou la page « Nothing yet » s'il n'y a rien.
fn creer_contenu(entrees: &[Entree], liste: &ListePoints) -> gtk4::Widget {
    if entrees.is_empty() {
        let vide = adw::StatusPage::builder()
            .icon_name("document-open-recent-symbolic")
            .title(tr("Nothing yet"))
            .description(tr("What WiMao does will show up here."))
            .vexpand(true)
            .build();
        return vide.upcast();
    }
    let boite = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Vertical)
        .spacing(18)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(12)
        .margin_end(12)
        .build();
    for jour in grouper_par_jour(entrees, &jour_local_iso(0), &jour_local_iso(-1)) {
        let groupe = adw::PreferencesGroup::builder().title(&jour.titre).build();
        for entree in &jour.entrees {
            groupe.add(&creer_rangee(
                entree,
                point_disponible(entree, liste).as_ref(),
            ));
        }
        boite.append(&groupe);
    }
    let clamp = adw::Clamp::builder()
        .maximum_size(LARGEUR_CONTENU)
        .child(&boite)
        .build();
    gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build()
        .upcast()
}

/// La page complète : barre d'en-tête (bouton retour automatique) et contenu.
fn creer_page(entrees: &[Entree], liste: &ListePoints) -> adw::NavigationPage {
    let vue = adw::ToolbarView::new();
    vue.add_top_bar(&adw::HeaderBar::new());
    vue.set_content(Some(&creer_contenu(entrees, liste)));
    adw::NavigationPage::builder()
        .tag(PAGE_HISTORIQUE)
        .title(tr("History"))
        .child(&vue)
        .build()
}

/// Texte du dialogue de confirmation : le point utilisé et sa date, le nombre de fichiers qui
/// seront supprimés, puis la mise en garde du mode d'emploi.
fn texte_confirmation(apercu: &ApercuRetour) -> String {
    let fichiers = trn(
        "{n} file will be removed",
        "{n} files will be removed",
        apercu.fichiers.len() as u64,
        &[],
    );
    format!(
        "{}\n{}\n\n{}",
        trf(
            "Point used: {reason} ({date})",
            &[
                ("reason", &libelle_motif(&apercu.point.motif)),
                ("date", &date_du_point(&apercu.point.id)),
            ],
        ),
        fichiers,
        tr("Everything done after this point will be undone too."),
    )
}

/// Phrase du toast de bilan : fichiers puis liens retirés.
fn texte_bilan(bilan: &BilanRetour) -> String {
    let resume = format!(
        "{} · {}",
        trn(
            "{n} file removed",
            "{n} files removed",
            bilan.fichiers_supprimes as u64,
            &[]
        ),
        trn(
            "{n} link removed",
            "{n} links removed",
            bilan.liens_retires as u64,
            &[]
        ),
    );
    trf("Rolled back: {summary}", &[("summary", &resume)])
}

// --- La page dans la fenêtre ---------------------------------------------------------------

impl Fenetre {
    /// Ouvre la page Historique (ou la recharge si elle est déjà affichée). Les données sont
    /// lues dans un fil : consulter l'historique reste possible pendant un travail.
    pub(crate) fn ouvrir_historique(&self) {
        // Déjà à l'écran : un second Ctrl+H ne fait rien (le rechargement après un retour en
        // arrière passe par `charger_et_montrer_historique`).
        let visible = self.navigation.visible_page().and_then(|p| p.tag());
        if !doit_ouvrir_historique(visible.as_deref()) {
            return;
        }
        self.charger_et_montrer_historique();
    }

    /// Charge les données de l'historique puis les montre (empile la page ou remplace celle
    /// qui est visible).
    fn charger_et_montrer_historique(&self) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        en_arriere_plan(
            move |_| operations::charger_historique(&c),
            |_| {},
            move |resultat| match resultat {
                Ok((entrees, liste)) => moi.montrer_historique(&entrees, &liste),
                Err(erreur) => moi.erreur(&erreur, false),
            },
            {
                let moi = self.clone();
                move |cause| moi.panique(&cause)
            },
        );
    }

    /// Empile la page ; si l'historique est déjà la page visible, la remplace sur place
    /// (rechargement après un retour en arrière, sans animation ni doublon).
    fn montrer_historique(&self, entrees: &[Entree], liste: &ListePoints) {
        let page = creer_page(entrees, liste);
        let visible = self.navigation.visible_page();
        let deja_ouverte = visible.and_then(|p| p.tag()).as_deref() == Some(PAGE_HISTORIQUE);
        match (deja_ouverte, self.navigation.find_page(PAGE_PRINCIPALE)) {
            (true, Some(racine)) => self.navigation.replace(&[racine, page]),
            _ => self.pousser(&page),
        }
    }

    /// « Go back to before this » pour le point `id` : calcule l'aperçu (dans le fil de
    /// travail, comme tout travail), puis demande confirmation.
    pub(crate) fn demander_retour(&self, id: String) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        self.lancer(
            &tr("Looking at what will be removed…"),
            move |_| operations::apercu_du_retour(&c, &id),
            move |resultat| match resultat {
                Ok(apercu) => moi.confirmer_retour(apercu),
                Err(erreur) => moi.erreur(&erreur, false),
            },
        );
    }

    /// Dialogue de confirmation ; « Annuler » (et Échap) ne change rien.
    fn confirmer_retour(&self, apercu: ApercuRetour) {
        let dialogue = adw::AlertDialog::new(
            Some(&tr("Go back to before this?")),
            Some(&texte_confirmation(&apercu)),
        );
        dialogue.add_response(REPONSE_ANNULER, &tr("Cancel"));
        dialogue.add_response(REPONSE_REVENIR, &tr("Go back"));
        dialogue.set_response_appearance(REPONSE_REVENIR, adw::ResponseAppearance::Destructive);
        // Par défaut, l'issue sans danger : Entrée ne supprime rien par mégarde.
        dialogue.set_default_response(Some(REPONSE_ANNULER));
        dialogue.set_close_response(REPONSE_ANNULER);
        let moi = self.clone();
        dialogue.choose(
            Some(&self.fenetre),
            None::<&gio::Cancellable>,
            move |reponse| {
                if reponse != REPONSE_REVENIR {
                    return;
                }
                // Le retour arrête Wine : même question que pour un installeur.
                let suite = moi.clone();
                moi.avant_d_arreter_wine(move || suite.faire_retour(apercu));
            },
        );
    }

    /// Fait le retour dans le fil de travail, puis toast de bilan (ou erreur), rechargement de
    /// la page et de la liste des plugins. Après un échec aussi : le moteur a noté l'échec dans
    /// l'historique, et un retour à moitié fait a pu changer des fichiers.
    fn faire_retour(&self, apercu: ApercuRetour) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        self.lancer(
            &tr("Going back…"),
            move |_| operations::retour_en_arriere(&c, &apercu),
            move |resultat| {
                match resultat {
                    Ok(bilan) => moi.toast(&texte_bilan(&bilan)),
                    Err(erreur) => moi.erreur(&erreur, false),
                }
                moi.rafraichir();
                // Seulement si l'historique est encore à l'écran : sinon rien à recharger.
                let visible = moi.navigation.visible_page().and_then(|p| p.tag());
                if visible.as_deref() == Some(PAGE_HISTORIQUE) {
                    moi.charger_et_montrer_historique();
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn double_ctrl_h_ne_pousse_pas_de_seconde_page() {
        assert!(doit_ouvrir_historique(None));
        assert!(doit_ouvrir_historique(Some("principale")));
        assert!(!doit_ouvrir_historique(Some("historique")));
    }

    fn entree(date: &str, point: Option<&str>) -> Entree {
        Entree {
            date: date.to_owned(),
            action: Action::Installation,
            sujet: "Serum".to_owned(),
            resultat: ResultatAction::Reussi,
            point: point.map(str::to_owned),
            details: String::new(),
            sujet_neutre: None,
            details_neutre: None,
        }
    }

    fn point(id: &str) -> PointRestauration {
        PointRestauration {
            id: id.to_owned(),
            motif: "repair".to_owned(),
            dossier: PathBuf::from("/inexistant").join(id),
        }
    }

    #[test]
    fn groupes_aujourd_hui_hier_puis_date() {
        let entrees = [
            entree("2026-11-01T14:03:12+01:00", None),
            entree("2026-10-31T09:00:00+01:00", None),
            entree("2026-10-30T18:30:00+01:00", None),
        ];
        let jours = grouper_par_jour(&entrees, "2026-11-01", "2026-10-31");
        let titres: Vec<&str> = jours.iter().map(|j| j.titre.as_str()).collect();
        // `tr` plutôt que du texte en dur : un autre test change la langue globale en parallèle.
        assert_eq!(
            titres,
            [tr("Today"), tr("Yesterday"), "2026-10-30".to_owned()]
        );
    }

    #[test]
    fn plusieurs_entrees_d_un_jour_forment_un_seul_groupe() {
        let entrees = [
            entree("2026-11-01T14:03:12+01:00", None),
            entree("2026-11-01T09:00:00+01:00", None),
            entree("2026-10-30T18:30:00+01:00", None),
        ];
        let jours = grouper_par_jour(&entrees, "2026-11-01", "2026-10-31");
        assert_eq!(jours.len(), 2);
        assert_eq!(jours[0].entrees.len(), 2);
        assert!(grouper_par_jour(&[], "2026-11-01", "2026-10-31").is_empty());
    }

    #[test]
    fn point_perime_ou_absent_n_est_pas_propose() {
        let liste = ListePoints {
            points: vec![point("p-ok")],
            perimes: vec![point("p-vieux")],
            incomplets: vec![],
        };
        let ok = point_disponible(&entree("2026-11-01T10:00:00+01:00", Some("p-ok")), &liste);
        assert_eq!(ok.map(|p| p.id), Some("p-ok".to_owned()));
        let perime = entree("2026-11-01T10:00:00+01:00", Some("p-vieux"));
        assert!(point_disponible(&perime, &liste).is_none());
        let inconnu = entree("2026-11-01T10:00:00+01:00", Some("p-disparu"));
        assert!(point_disponible(&inconnu, &liste).is_none());
        assert!(point_disponible(&entree("2026-11-01T10:00:00+01:00", None), &liste).is_none());
    }

    #[test]
    fn heure_et_date_du_point_sont_lisibles() {
        assert_eq!(heure_de("2026-11-01T14:03:12+01:00"), "14:03");
        assert_eq!(heure_de("2026-11-01"), "");
        assert_eq!(date_du_point("2026-11-01-14h03m12"), "2026-11-01 14:03");
        assert_eq!(date_du_point("2026-11-01-14h03m12-2"), "2026-11-01 14:03");
        assert_eq!(date_du_point("bizarre"), "bizarre");
    }

    #[test]
    fn sous_titre_omet_les_morceaux_vides() {
        let mut e = entree("2026-11-01T14:03:12+01:00", None);
        let action = libelle_action(Action::Installation);
        assert_eq!(sous_titre(&e), format!("14:03 · {action}"));
        e.details = "2 files".to_owned();
        assert_eq!(sous_titre(&e), format!("14:03 · {action} · 2 files"));
    }
}
