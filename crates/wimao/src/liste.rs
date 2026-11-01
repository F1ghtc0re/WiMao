//! Données et apparence de la liste des plugins.
//!
//! Deux parties : [`LignePlugin`] (données pures, triées et testables sans écran) et les
//! fonctions qui fabriquent les widgets (`creer_rangee`, `creer_badge`, `creer_badge_libre`) d'après ces données.
//! Utilise `etat.rs` (résultats mémorisés) et `wimao_core::tableau` (symboles, noms de
//! formats, communs avec la CLI). Utilisé par `operations.rs` (calcul), `fenetre/mod.rs` (affichage) et `historique_vue.rs`
//! (pastilles d'issue).

use std::collections::BTreeSet;
use std::path::PathBuf;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{tr, trn};
use wimao_core::tableau::{nom_format, Statut};
use wimao_core::{Format, Inventaire};

use crate::etat::Etat;

/// Icône de préfixe d'un format (icônes symboliques présentes dans Adwaita et Yaru).
fn icone_format(format: Format) -> &'static str {
    match format {
        Format::Vst3 => "audio-x-generic-symbolic",
        Format::Vst2 => "application-x-addon-symbolic",
        Format::Clap => "applications-multimedia-symbolic",
    }
}

/// Un plugin tel que la liste l'affiche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LignePlugin {
    /// Nom du plugin.
    pub nom: String,
    /// Format du plugin.
    pub format: Format,
    /// Chemin dans l'environnement Wine.
    pub chemin: PathBuf,
    /// Dernier verdict connu ; `None` si le plugin n'a pas encore été testé.
    pub statut: Option<Statut>,
    /// Explication du dernier verdict (vide si aucun).
    pub detail: String,
    /// Vrai si le plugin a été mis à jour depuis l'ouverture de la fenêtre : la rangée porte
    /// alors la marque « Updated » (ensemble tenu par `Fenetre`, vidé à sa fermeture).
    pub mis_a_jour: bool,
}

impl LignePlugin {
    /// Une ligne par plugin de l'inventaire, avec son résultat mémorisé, **triée** :
    /// les problèmes d'abord, puis les remarques, les plugins pas encore testés, enfin les
    /// plugins prêts ; à égalité, ordre alphabétique sans casse. Ce qui demande une action
    /// de l'utilisateur doit se voir sans défiler.
    ///
    /// `mis_a_jour` : chemins des plugins mis à jour depuis l'ouverture de la fenêtre (rapports
    /// d'installation et bilans de session) ; ils portent la marque « Updated », sans effet sur
    /// le tri.
    pub fn depuis_inventaire(
        inventaire: &Inventaire,
        etat: &Etat,
        mis_a_jour: &BTreeSet<PathBuf>,
    ) -> Vec<LignePlugin> {
        let mut lignes: Vec<LignePlugin> = inventaire
            .0
            .iter()
            .map(|plugin| {
                let connu = etat.trouver(&plugin.chemin);
                LignePlugin {
                    nom: plugin.nom.clone(),
                    format: plugin.format,
                    chemin: plugin.chemin.clone(),
                    statut: connu.map(|e| e.resultat.statut()),
                    detail: connu.map(|e| e.texte()).unwrap_or_default(),
                    mis_a_jour: mis_a_jour.contains(&plugin.chemin),
                }
            })
            .collect();
        lignes.sort_by_key(|l| (rang(l.statut), l.nom.to_lowercase()));
        lignes
    }

    /// Nombre de plugins prêts (✅) dans `lignes`.
    pub fn nombre_prets(lignes: &[LignePlugin]) -> usize {
        lignes
            .iter()
            .filter(|l| l.statut == Some(Statut::Bon))
            .count()
    }
}

/// Rang de tri : plus petit = plus haut dans la liste.
fn rang(statut: Option<Statut>) -> u8 {
    match statut {
        Some(Statut::Echec) => 0,
        Some(Statut::Avertissement) => 1,
        None => 2,
        Some(Statut::Bon) => 3,
    }
}

/// Texte de l'en-tête sous « WiMao » : « 3 plugins · 1 prêt » (le second morceau est omis
/// quand aucun plugin n'est prêt). Les pluriels viennent des `.mo` (`trn`), sans « (s) ».
pub fn sous_titre(lignes: &[LignePlugin]) -> String {
    if lignes.is_empty() {
        return String::new();
    }
    let total = trn("{n} plugin", "{n} plugins", lignes.len() as u64, &[]);
    match LignePlugin::nombre_prets(lignes) {
        0 => total,
        prets => format!(
            "{total} · {}",
            trn("{n} ready", "{n} ready", prets as u64, &[])
        ),
    }
}

/// Icône symbolique, classe CSS (voir `style.css`) et libellé court d'un verdict.
pub fn apparence(statut: Option<Statut>) -> (&'static str, &'static str, String) {
    match statut {
        Some(Statut::Bon) => ("object-select-symbolic", "success", tr("Ready")),
        Some(Statut::Avertissement) => ("dialog-warning-symbolic", "warning", tr("Check")),
        Some(Statut::Echec) => ("dialog-error-symbolic", "error", tr("Problem")),
        None => ("content-loading-symbolic", "dim-label", tr("Not tested")),
    }
}

/// Pastille arrondie : icône et libellé court, colorés selon le verdict.
///
/// La couleur seule ne suffit pas (daltonisme) : l'icône et le mot disent la même chose.
pub fn creer_badge(statut: Option<Statut>) -> gtk4::Box {
    let (icone, classe, libelle) = apparence(statut);
    creer_badge_libre(icone, classe, &libelle)
}

/// Pastille de même style avec une icône, une classe CSS (`success`, `warning`, `error` ou
/// `dim-label`, voir `style.css`) et un libellé au choix : la page Historique y met l'issue
/// d'une action (« Succeeded », « Failed »…), qui n'est pas un verdict de plugin.
pub fn creer_badge_libre(icone: &str, classe: &str, libelle: &str) -> gtk4::Box {
    let badge = gtk4::Box::builder()
        .orientation(gtk4::Orientation::Horizontal)
        .spacing(6)
        .valign(gtk4::Align::Center)
        .css_classes(["badge", classe])
        .build();
    badge.append(&gtk4::Image::from_icon_name(icone));
    badge.append(&gtk4::Label::new(Some(libelle)));
    badge
}

/// Icône, classe CSS et libellé de la marque « Updated » (plugin mis à jour pendant cette
/// session de la fenêtre). La classe `accent` est définie dans `style.css`.
pub fn apparence_mis_a_jour() -> (&'static str, &'static str, String) {
    (
        "software-update-available-symbolic",
        "accent",
        tr("Updated"),
    )
}

/// Une rangée de la liste : icône, nom, « format · détail », puis à droite la marque « Updated »
/// (si le plugin vient d'être mis à jour) et la pastille d'état.
pub fn creer_rangee(ligne: &LignePlugin) -> adw::ActionRow {
    let sous_titre = if ligne.detail.is_empty() {
        nom_format(ligne.format).to_owned()
    } else {
        format!("{} · {}", nom_format(ligne.format), ligne.detail)
    };
    let rangee = adw::ActionRow::builder()
        .title(&ligne.nom)
        .subtitle(sous_titre)
        // Un nom de plugin peut contenir « & » : surtout pas de balisage Pango.
        .use_markup(false)
        .build();
    rangee.add_prefix(&gtk4::Image::from_icon_name(icone_format(ligne.format)));
    if ligne.mis_a_jour {
        let (icone, classe, libelle) = apparence_mis_a_jour();
        rangee.add_suffix(&creer_badge_libre(icone, classe, &libelle));
    }
    rangee.add_suffix(&creer_badge(ligne.statut));
    rangee
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::etat::Etat;
    use std::path::Path;
    use wimao_core::tableau::DetailBrut;
    use wimao_core::PluginWindows;

    fn plugin(nom: &str) -> PluginWindows {
        PluginWindows {
            chemin: Path::new("/p").join(format!("{nom}.vst3")),
            format: Format::Vst3,
            nom: nom.into(),
        }
    }

    #[test]
    fn les_problemes_passent_avant_les_plugins_prets() {
        let mut inventaire = Inventaire::default();
        for nom in ["beta", "Alpha", "Gamma", "delta"] {
            inventaire.0.insert(plugin(nom));
        }
        let mut etat = Etat::default();
        etat.noter(
            &plugin("Alpha").chemin,
            Statut::Bon,
            &DetailBrut::Libre("ok".into()),
        );
        etat.noter(
            &plugin("Gamma").chemin,
            Statut::Echec,
            &DetailBrut::Libre("KO".into()),
        );
        etat.noter(
            &plugin("beta").chemin,
            Statut::Avertissement,
            &DetailBrut::Libre("?".into()),
        );
        let noms: Vec<String> =
            LignePlugin::depuis_inventaire(&inventaire, &etat, &BTreeSet::new())
                .into_iter()
                .map(|l| l.nom)
                .collect();
        // Problème, remarque, pas testé, prêt.
        assert_eq!(noms, ["Gamma", "beta", "delta", "Alpha"]);
    }

    #[test]
    fn plugin_mis_a_jour_porte_la_marque() {
        let mut inventaire = Inventaire::default();
        for nom in ["Alpha", "Beta"] {
            inventaire.0.insert(plugin(nom));
        }
        let etat = Etat::default();
        // L'ensemble contient Alpha : seul Alpha porte la marque.
        let ensemble = BTreeSet::from([plugin("Alpha").chemin]);
        let lignes = LignePlugin::depuis_inventaire(&inventaire, &etat, &ensemble);
        let marque = |nom: &str| lignes.iter().find(|l| l.nom == nom).unwrap().mis_a_jour;
        assert!(marque("Alpha"));
        assert!(!marque("Beta"));
        // Ensemble vide : aucune marque.
        let sans = LignePlugin::depuis_inventaire(&inventaire, &etat, &BTreeSet::new());
        assert!(sans.iter().all(|l| !l.mis_a_jour));
    }

    #[test]
    fn le_sous_titre_compte_les_plugins_prets() {
        assert_eq!(sous_titre(&[]), "");
        let ligne = |statut| LignePlugin {
            nom: "x".into(),
            format: Format::Clap,
            chemin: PathBuf::new(),
            statut,
            detail: String::new(),
            mis_a_jour: false,
        };
        let lignes = [ligne(Some(Statut::Bon)), ligne(None)];
        assert_eq!(LignePlugin::nombre_prets(&lignes), 1);
        assert_eq!(sous_titre(&lignes), "2 plugins · 1 ready");
        assert_eq!(sous_titre(&lignes[1..]), "1 plugin");
    }
}
