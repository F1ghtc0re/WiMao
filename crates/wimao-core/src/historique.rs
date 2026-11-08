//! Historique des actions de l'utilisateur (`<données>/historique.jsonl`).
//!
//! - Une action = une ligne JSON (format « JSON Lines ») : date, nature, sujet, résultat et,
//!   si besoin, le point de restauration associé et un détail. À ne pas confondre avec le
//!   [`journal`], qui trace les *commandes* lancées pour le diagnostic technique.
//! - [`ajouter`] écrit à la fin du fichier ; [`lire`] relit tout, la plus récente en premier.
//! - [`ajouter_neutre`] écrit en plus le sujet et les détails sous forme de [`TexteNeutre`]
//!   (clé anglaise + valeurs) : [`Entree::sujet_affiche`] et [`Entree::details_affiches`] les
//!   traduisent **au moment de l'affichage**, dans la langue alors choisie (spec 5).
//!
//! # Choix non évidents
//!
//! - **Écrire ne fait jamais échouer l'action** : une erreur d'écriture est seulement notée au
//!   journal technique (spec 6.2). L'historique est une aide, pas une condition de réussite.
//! - **Une ligne illisible est ignorée** à la lecture (coupure de courant en pleine écriture,
//!   édition à la main…) : le reste de l'historique reste consultable.
//! - Si le fichier ne se termine pas par un saut de ligne (écriture interrompue), [`ajouter`]
//!   en commence un, pour que la nouvelle entrée ne soit pas collée à la ligne abîmée.
//! - `sujet` et `details` restent écrits (dans la langue du moment) en plus du texte neutre :
//!   les anciennes versions et la lecture directe du fichier n'ont ainsi rien à savoir du neutre.
//! - Une clé neutre doit être **déclarée** dans [`CLES_CONNUES`] ; une clé inconnue (fichier
//!   édité à la main, version plus récente) ou une valeur manquante fait retomber l'affichage
//!   sur le texte brut, jamais sur un `{name}` laissé tel quel.
//! - Les causes d'échec (messages d'erreur) restent du texte libre, traduit à l'écriture : limite
//!   assumée.
//! - La date est écrite avec son décalage horaire (`+01:00`) pour rester non ambiguë.
//!
//! Utilise `chemins`, `journal` et `outils::heure_locale` ; utilisé par le moteur
//! (installation, réparation, sauvegarde, import…) et par l'interface (page Historique).

use crate::chemins::Chemins;
use crate::erreur::Resultat;
use crate::i18n::{tr, tr_noop, trc, trn};
use crate::journal;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::LazyLock;

/// Nom du fichier d'historique, dans le dossier de données.
pub const FICHIER_HISTORIQUE: &str = "historique.jsonl";

/// Nature d'une action enregistrée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    /// Installation d'un plugin.
    Installation,
    /// Installation ou mise à jour d'un gestionnaire (Native Access, etc.).
    Gestionnaire,
    /// Réparation de l'environnement.
    Reparation,
    /// Retour à un point de restauration.
    Retour,
    /// Sauvegarde de l'environnement.
    Sauvegarde,
    /// Restauration d'une sauvegarde.
    Restauration,
    /// Import de plugins ou de réglages.
    Import,
    /// Annulation d'un import.
    AnnulationImport,
}

/// Issue d'une action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResultatAction {
    /// L'action est allée au bout.
    Reussi,
    /// L'action a échoué.
    Echec,
    /// L'utilisateur a annulé l'action.
    Annule,
}

/// Une ligne de l'historique.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entree {
    /// Date locale avec décalage, par exemple `2026-11-01T14:03:12+01:00`.
    pub date: String,
    /// Nature de l'action.
    pub action: Action,
    /// Ce sur quoi elle porte (nom du plugin, de la sauvegarde…).
    pub sujet: String,
    /// Issue de l'action.
    pub resultat: ResultatAction,
    /// Point de restauration lié à l'action, s'il y en a un.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    /// Précision libre (cause d'un échec, par exemple) ; absente si vide.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub details: String,
    /// Sujet sous forme neutre (clé anglaise + valeurs), traduit à l'affichage ; absent pour les
    /// anciennes entrées et pour les sujets qui ne sont qu'un nom propre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sujet_neutre: Option<TexteNeutre>,
    /// Précision sous forme neutre, traduite à l'affichage ; absente si la précision est vide
    /// ou est un texte libre (cause d'une erreur).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details_neutre: Option<TexteNeutre>,
}

impl Entree {
    /// Sujet à afficher : le texte neutre traduit dans la langue **courante** s'il existe et
    /// si sa clé est connue, sinon le `sujet` brut (ancienne entrée, clé inconnue).
    pub fn sujet_affiche(&self) -> String {
        self.sujet_neutre
            .as_ref()
            .and_then(TexteNeutre::traduit)
            .unwrap_or_else(|| self.sujet.clone())
    }

    /// Précision à afficher (même règle que [`Entree::sujet_affiche`], avec `details`).
    pub fn details_affiches(&self) -> String {
        self.details_neutre
            .as_ref()
            .and_then(TexteNeutre::traduit)
            .unwrap_or_else(|| self.details.clone())
    }
}

// --- Clés des textes neutres ---------------------------------------------------------------
//
// Chaque clé est un msgid anglais. Les clés simples passent par `tr_noop` pour que `xgettext`
// les extraie ; les clés **à pluriel** sont de simples littéraux, car leur msgid est déjà extrait
// par l'appel `trn` de `TexteNeutre::traduit` (un `tr_noop` en plus donnerait deux entrées
// contradictoires pour le même msgid).

/// Sujet : réparation (valeur : aucune).
pub const CLE_REPARATION: &str = tr_noop("Repair");
/// Sujet : retour vers le point créé avant une réparation.
pub const CLE_AVANT_REPARATION: &str = tr_noop("before a repair");
/// Sujet : retour vers le point d'une session de gestionnaire (valeur `name`).
pub const CLE_AVANT_SESSION: &str = tr_noop("before the {name} session");
/// Sujet : retour vers le point d'une installation (valeur `file`).
pub const CLE_AVANT_INSTALLATION: &str = tr_noop("before installing {file}");
/// Détail à pluriel : fichiers retirés par un retour en arrière (valeur `n`).
pub const CLE_FICHIERS_RETIRES: &str = "{n} file removed";
/// Détail à pluriel : plugins traités par une installation (valeur `n`).
pub const CLE_PLUGINS_TRAITES: &str = "{n} plugin processed";
/// Détail : import terminé après une interruption (valeur `path`).
pub const CLE_IMPORT_REPRIS: &str =
    tr_noop("The import of {path} had been interrupted: it has been completed");
/// Détail : annulation d'import terminée après une interruption (valeurs `path`, `command`).
pub const CLE_ANNULATION_REPRISE: &str = tr_noop(
    "Cancelling the import of {path} had been interrupted: it has been completed. If you use yabridgectl yourself, run \"{command}\"",
);
/// Détail : pas de réglages yabridgectl à mettre à jour (valeurs `path`, `command`).
pub const CLE_SANS_REGLAGES_YABRIDGECTL: &str = tr_noop(
    "No yabridgectl settings in {path}: if you use yabridgectl yourself, run \"{command}\" to update your plugin links",
);
/// Détail : liens yabridgectl non mis à jour (valeurs `error`, `command`).
pub const CLE_LIENS_NON_MIS_A_JOUR: &str =
    tr_noop("Your plugin links could not be updated ({error}): run \"{command}\" yourself");

/// Toutes les clés qu'un texte neutre peut porter. Une clé absente d'ici n'est jamais traduite.
pub const CLES_CONNUES: &[&str] = &[
    CLE_REPARATION,
    CLE_AVANT_REPARATION,
    CLE_AVANT_SESSION,
    CLE_AVANT_INSTALLATION,
    CLE_FICHIERS_RETIRES,
    CLE_PLUGINS_TRAITES,
    CLE_IMPORT_REPRIS,
    CLE_ANNULATION_REPRISE,
    CLE_SANS_REGLAGES_YABRIDGECTL,
    CLE_LIENS_NON_MIS_A_JOUR,
];

/// Un `{nom}` à remplacer dans un modèle de phrase.
static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([A-Za-z_][A-Za-z0-9_]*)\}").expect("regex statique valide"));

/// Texte neutre : une clé (msgid anglais de [`CLES_CONNUES`]) et les valeurs de ses `{nom}`.
/// Les valeurs ne sont jamais traduites (noms de plugins, nombres, chemins).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct TexteNeutre {
    /// Msgid anglais, par exemple `before installing {file}`.
    pub cle: String,
    /// Valeur de chaque `{nom}` de la clé.
    #[serde(default)]
    pub valeurs: BTreeMap<String, String>,
}

impl TexteNeutre {
    /// Construit un texte neutre à partir d'une clé et de ses valeurs `(nom, valeur)`.
    pub fn nouveau(cle: &str, valeurs: &[(&str, &str)]) -> Self {
        Self {
            cle: cle.to_owned(),
            valeurs: valeurs
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    /// Texte traduit dans la langue courante ; `None` si la clé est inconnue ou si une valeur
    /// manque (l'appelant retombe alors sur le texte brut).
    pub fn traduit(&self) -> Option<String> {
        if !CLES_CONNUES.contains(&self.cle.as_str()) {
            return None;
        }
        // Modèle traduit. Pour les clés à pluriel, `trn(…, &[])` remplace lui-même `{n}` à
        // partir de son argument `n` (d'où la lecture de la valeur « n » ci-dessous).
        let modele = match self.cle.as_str() {
            CLE_FICHIERS_RETIRES => {
                let n = self.valeurs.get("n")?.parse::<u64>().ok()?;
                trn("{n} file removed", "{n} files removed", n, &[])
            }
            CLE_PLUGINS_TRAITES => {
                let n = self.valeurs.get("n")?.parse::<u64>().ok()?;
                trn("{n} plugin processed", "{n} plugins processed", n, &[])
            }
            cle => tr(cle),
        };
        // Une seule passe sur le modèle : une valeur qui contient elle-même `{x}` n'est pas
        // substituée une seconde fois. Un `{nom}` sans valeur fait renoncer (`None`) : on ne
        // montre jamais un texte troué.
        let mut manquante = false;
        let texte = PLACEHOLDER.replace_all(&modele, |m: &regex::Captures| {
            match self.valeurs.get(&m[1]) {
                Some(valeur) => valeur.clone(),
                None => {
                    manquante = true;
                    String::new()
                }
            }
        });
        if manquante {
            return None;
        }
        Some(texte.into_owned())
    }

    /// Texte à écrire dans le champ brut : le texte traduit, ou à défaut la clé elle-même.
    pub fn en_clair(&self) -> String {
        self.traduit().unwrap_or_else(|| self.cle.clone())
    }
}

/// Nom de l'action dans la langue courante (« Installation », « Sauvegarde »…).
///
/// Dans le moteur plutôt que dans l'interface : la ligne de commande (`historique`) et la
/// page Historique de l'application affichent ainsi les mêmes mots.
///
/// Ce sont des **noms** (« Réparation », « Retour en arrière »), pas des verbes d'action comme
/// sur les boutons : d'où le contexte `"history"`, qui garde les msgids `Repair`, `Undo`… des
/// boutons intacts. Le contexte est un littéral : `xgettext` (`--keyword=trc:1c,2`) ne
/// l'extrait pas d'une constante.
pub fn libelle_action(a: Action) -> String {
    match a {
        Action::Installation => trc("history", "Installation"),
        Action::Gestionnaire => trc("history", "Manager session"),
        Action::Reparation => trc("history", "Repair"),
        Action::Retour => trc("history", "Undo"),
        Action::Sauvegarde => trc("history", "Backup"),
        Action::Restauration => trc("history", "Restore"),
        Action::Import => trc("history", "Import"),
        Action::AnnulationImport => trc("history", "Import cancelled"),
    }
}

/// Issue de l'action dans la langue courante (« Réussi », « Échec », « Annulé »).
pub fn libelle_resultat(r: ResultatAction) -> String {
    match r {
        ResultatAction::Reussi => tr("Succeeded"),
        ResultatAction::Echec => tr("Failed"),
        ResultatAction::Annule => tr("Cancelled"),
    }
}

/// Date de l'instant présent, à l'heure locale, au format `AAAA-MM-JJTHH:MM:SS+HH:MM`.
/// Si l'heure locale est illisible, on retombe sur l'époque en UTC plutôt que d'échouer.
pub fn date_locale_iso() -> String {
    match crate::outils::heure_locale() {
        Ok(tm) => {
            // `tm_gmtoff` : décalage à l'est de UTC, en secondes (négatif à l'ouest).
            let decalage: i64 = tm.tm_gmtoff;
            let signe = if decalage < 0 { '-' } else { '+' };
            let minutes = decalage.abs() / 60;
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{signe}{:02}:{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec,
                minutes / 60,
                minutes % 60
            )
        }
        Err(_) => "1970-01-01T00:00:00+00:00".to_string(),
    }
}

/// Jour local voisin d'aujourd'hui au format `AAAA-MM-JJ` (`0` = aujourd'hui, `-1` = hier).
///
/// Sert à la page Historique pour titrer « Today » et « Yesterday » : même format que le
/// début de [`Entree::date`]. Si l'heure locale est illisible, retombe sur le 1er janvier 1970
/// (comme [`date_locale_iso`]).
pub fn jour_local_iso(jours: i32) -> String {
    match crate::outils::jour_local_decale(jours) {
        Ok(tm) => format!(
            "{:04}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        ),
        Err(_) => "1970-01-01".to_string(),
    }
}

/// Ajoute une action à l'historique. Ne renvoie rien : en cas d'erreur d'écriture, elle est
/// notée au journal technique et l'action elle-même n'est pas affectée.
///
/// Pour un sujet qui n'est qu'un nom propre (plugin, gestionnaire, archive) : rien à traduire,
/// donc pas de texte neutre. Pour une phrase, voir [`ajouter_neutre`].
pub fn ajouter(
    c: &Chemins,
    action: Action,
    sujet: &str,
    resultat: ResultatAction,
    point: Option<&str>,
    details: &str,
) {
    noter_entree(
        c,
        Entree {
            date: date_locale_iso(),
            action,
            sujet: sujet.to_owned(),
            resultat,
            point: point.map(str::to_owned),
            details: details.to_owned(),
            sujet_neutre: None,
            details_neutre: None,
        },
    );
}

/// Comme [`ajouter`], pour un sujet (et éventuellement des détails) qui sont des phrases
/// traduisibles : ils sont écrits sous forme neutre, et aussi, traduits dans la langue du moment,
/// dans `sujet` / `details`.
///
/// `details` (neutre) l'emporte sur `details_libre` (cause d'erreur déjà traduite, texte libre) :
/// l'appelant en fournit un seul des deux, l'autre vaut `None` / `""`.
pub fn ajouter_neutre(
    c: &Chemins,
    action: Action,
    sujet: &TexteNeutre,
    resultat: ResultatAction,
    point: Option<&str>,
    details: Option<&TexteNeutre>,
    details_libre: &str,
) {
    noter_entree(
        c,
        Entree {
            date: date_locale_iso(),
            action,
            sujet: sujet.en_clair(),
            resultat,
            point: point.map(str::to_owned),
            details: details.map_or_else(|| details_libre.to_owned(), TexteNeutre::en_clair),
            sujet_neutre: Some(sujet.clone()),
            details_neutre: details.cloned(),
        },
    );
}

/// Comme [`ajouter`], pour un sujet qui est un nom propre (rien à traduire) mais dont les
/// détails sont une phrase traduisible (« 2 plugins processed »). Les détails sont écrits sous
/// forme neutre et, traduits, dans `details`.
pub fn ajouter_details_neutres(
    c: &Chemins,
    action: Action,
    sujet: &str,
    resultat: ResultatAction,
    point: Option<&str>,
    details: &TexteNeutre,
) {
    noter_entree(
        c,
        Entree {
            date: date_locale_iso(),
            action,
            sujet: sujet.to_owned(),
            resultat,
            point: point.map(str::to_owned),
            details: details.en_clair(),
            sujet_neutre: None,
            details_neutre: Some(details.clone()),
        },
    );
}

/// Écrit `entree` ; une erreur d'écriture est seulement notée au journal technique.
fn noter_entree(c: &Chemins, entree: Entree) {
    if let Err(e) = ecrire(c, &entree) {
        journal::noter("historique", &format!("écriture impossible : {e}"));
    }
}

/// Écrit `entree` en fin de fichier (créé, avec son dossier, au besoin) et force l'écriture
/// sur disque.
fn ecrire(c: &Chemins, entree: &Entree) -> std::io::Result<()> {
    std::fs::create_dir_all(&c.donnees)?;
    let mut f = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(c.donnees.join(FICHIER_HISTORIQUE))?;
    // Le dernier octet doit être un saut de ligne ; sinon une écriture précédente a été
    // interrompue et on ouvre une nouvelle ligne pour ne pas coller les deux.
    let mut ligne = String::new();
    if f.metadata()?.len() > 0 {
        let mut dernier = [0u8; 1];
        f.seek(SeekFrom::End(-1))?;
        f.read_exact(&mut dernier)?;
        if dernier[0] != b'\n' {
            ligne.push('\n');
        }
    }
    let json = serde_json::to_string(entree).map_err(std::io::Error::other)?;
    ligne.push_str(&json);
    ligne.push('\n');
    // Une seule écriture (mode « append » : toujours en fin de fichier).
    f.write_all(ligne.as_bytes())?;
    f.sync_data()
}

/// Relit l'historique, **la plus récente entrée d'abord**. Liste vide si le fichier n'existe
/// pas ; une ligne illisible est ignorée et notée au journal technique.
pub fn lire(c: &Chemins) -> Resultat<Vec<Entree>> {
    let octets = match std::fs::read(c.donnees.join(FICHIER_HISTORIQUE)) {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let texte = String::from_utf8_lossy(&octets);
    let mut entrees = Vec::new();
    for ligne in texte.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str::<Entree>(ligne) {
            Ok(e) => entrees.push(e),
            Err(e) => journal::noter("historique", &format!("ligne ignorée ({e}) : {ligne}")),
        }
    }
    // Le fichier est dans l'ordre chronologique : on inverse pour la plus récente d'abord.
    entrees.reverse();
    Ok(entrees)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::init_langue;
    use regex::Regex;
    use serial_test::serial;

    #[test]
    #[serial]
    fn libelles_d_action_sont_des_noms() {
        use Action::*;
        let attendu = [
            (
                "fr",
                [
                    (Reparation, "Réparation"),
                    (Retour, "Retour en arrière"),
                    (Sauvegarde, "Sauvegarde"),
                    (Restauration, "Restauration"),
                    (Installation, "Installation"),
                    (Gestionnaire, "Session de gestionnaire"),
                    (Import, "Import"),
                    (AnnulationImport, "Import annulé"),
                ],
            ),
            (
                "es",
                [
                    (Reparation, "Reparación"),
                    (Retour, "Vuelta atrás"),
                    (Sauvegarde, "Copia de seguridad"),
                    (Restauration, "Restauración"),
                    (Installation, "Instalación"),
                    (Gestionnaire, "Sesión del gestor"),
                    (Import, "Importación"),
                    (AnnulationImport, "Importación cancelada"),
                ],
            ),
        ];
        for (langue, paires) in attendu {
            init_langue(Some(langue));
            for (action, texte) in paires {
                assert_eq!(libelle_action(action), texte, "{langue} {action:?}");
            }
        }
        // Remet la langue par défaut : l'état est global et partagé avec les autres tests.
        init_langue(Some("en"));
    }

    /// Chemins de test dans un dossier temporaire (à garder en vie pendant le test).
    fn chemins_de_test() -> (tempfile::TempDir, Chemins) {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        (d, c)
    }

    #[test]
    fn jour_local_iso_aujourd_hui_et_hier() {
        let aujourdhui = jour_local_iso(0);
        // Même jour que l'horodatage complet (sauf si minuit passe entre les deux appels).
        assert!(date_locale_iso().starts_with(&aujourdhui) || jour_local_iso(0) != aujourdhui);
        let hier = jour_local_iso(-1);
        let re = Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap();
        assert!(re.is_match(&hier));
        // Le format AAAA-MM-JJ se compare comme les dates : hier précède aujourd'hui.
        assert!(hier < aujourdhui);
        assert!(jour_local_iso(1) > aujourdhui);
    }

    #[test]
    fn ajout_puis_lecture_plus_recente_d_abord() {
        let (_d, c) = chemins_de_test();
        ajouter(
            &c,
            Action::Installation,
            "premier",
            ResultatAction::Reussi,
            None,
            "",
        );
        ajouter(
            &c,
            Action::Sauvegarde,
            "second",
            ResultatAction::Echec,
            Some("p1"),
            "x",
        );
        let l = lire(&c).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].sujet, "second");
        assert_eq!(l[0].point.as_deref(), Some("p1"));
        assert_eq!(l[1].sujet, "premier");
    }

    #[test]
    fn fichier_absent_donne_une_liste_vide() {
        let (_d, c) = chemins_de_test();
        assert!(lire(&c).unwrap().is_empty());
    }

    #[test]
    fn ligne_abimee_est_ignoree() {
        let (_d, c) = chemins_de_test();
        ajouter(&c, Action::Import, "ok", ResultatAction::Annule, None, "");
        let f = c.donnees.join(FICHIER_HISTORIQUE);
        let mut t = std::fs::read_to_string(&f).unwrap();
        t.push_str("{\"date\":\"2026");
        std::fs::write(&f, t).unwrap();
        assert_eq!(lire(&c).unwrap().len(), 1);
        // Une nouvelle entrée n'est pas collée à la ligne abîmée.
        ajouter(
            &c,
            Action::Retour,
            "suite",
            ResultatAction::Reussi,
            None,
            "",
        );
        let l = lire(&c).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].sujet, "suite");
    }

    #[test]
    fn format_d_une_ligne() {
        let (_d, c) = chemins_de_test();
        ajouter(
            &c,
            Action::AnnulationImport,
            "x",
            ResultatAction::Reussi,
            None,
            "",
        );
        let t = std::fs::read_to_string(c.donnees.join(FICHIER_HISTORIQUE)).unwrap();
        assert!(t.contains("\"action\":\"annulation-import\""), "{t}");
        assert!(t.contains("\"resultat\":\"reussi\""), "{t}");
        assert!(!t.contains("point") && !t.contains("details"), "{t}");
    }

    #[test]
    fn date_iso_a_le_bon_format() {
        let re = Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d$").unwrap();
        let d = date_locale_iso();
        assert!(re.is_match(&d), "{d}");
    }

    /// Entrée minimale pour les tests d'affichage.
    fn entree_de_test(sujet_neutre: Option<TexteNeutre>) -> Entree {
        Entree {
            date: "2026-11-07T10:00:00+01:00".to_owned(),
            action: Action::Retour,
            sujet: "before installing Serum 2".to_owned(),
            resultat: ResultatAction::Reussi,
            point: None,
            details: String::new(),
            sujet_neutre,
            details_neutre: None,
        }
    }

    #[test]
    fn ancienne_entree_sans_neutre_affiche_le_brut() {
        // Une ligne écrite avant le sous-projet 7 : aucun champ neutre.
        let json =
            r#"{"date":"d","action":"retour","sujet":"brut","resultat":"reussi","details":"x"}"#;
        let e: Entree = serde_json::from_str(json).unwrap();
        assert!(e.sujet_neutre.is_none() && e.details_neutre.is_none());
        assert_eq!(e.sujet_affiche(), "brut");
        assert_eq!(e.details_affiches(), "x");
    }

    #[test]
    #[serial]
    fn entree_neutre_traduite_en_fr_puis_es() {
        let e = entree_de_test(Some(TexteNeutre::nouveau(
            CLE_AVANT_INSTALLATION,
            &[("file", "Serum 2")],
        )));
        init_langue(Some("fr"));
        let fr = e.sujet_affiche();
        init_langue(Some("es"));
        let es = e.sujet_affiche();
        init_langue(Some("en"));
        assert_eq!(fr, "avant l'installation de Serum 2");
        assert_eq!(es, "antes de instalar Serum 2");
    }

    #[test]
    #[serial]
    fn pluriel_suit_la_langue_et_le_nombre() {
        let un = TexteNeutre::nouveau(CLE_FICHIERS_RETIRES, &[("n", "1")]);
        let trois = TexteNeutre::nouveau(CLE_FICHIERS_RETIRES, &[("n", "3")]);
        init_langue(Some("en"));
        assert_eq!(un.traduit().unwrap(), "1 file removed");
        assert_eq!(trois.traduit().unwrap(), "3 files removed");
        init_langue(Some("fr"));
        let fr = trois.traduit().unwrap();
        init_langue(Some("en"));
        assert!(fr.contains('3') && fr != "3 files removed", "{fr}");
    }

    #[test]
    #[serial]
    fn pluriels_en_francais_et_en_espagnol() {
        let texte = |n: &str| TexteNeutre::nouveau(CLE_FICHIERS_RETIRES, &[("n", n)]).traduit();
        init_langue(Some("fr"));
        let fr: Vec<_> = ["0", "1", "2"].map(texte).into_iter().collect();
        init_langue(Some("es"));
        let es: Vec<_> = ["1", "2"].map(texte).into_iter().collect();
        init_langue(Some("en"));
        // En français, 0 et 1 sont au singulier.
        assert_eq!(fr[0].as_deref(), Some("0 fichier supprimé"));
        assert_eq!(fr[1].as_deref(), Some("1 fichier supprimé"));
        assert_eq!(fr[2].as_deref(), Some("2 fichiers supprimés"));
        assert_eq!(es[0].as_deref(), Some("1 archivo eliminado"));
        assert_eq!(es[1].as_deref(), Some("2 archivos eliminados"));
    }

    #[test]
    #[serial]
    fn valeur_contenant_un_placeholder_n_est_pas_resubstituee() {
        // Le texte attendu est l'anglais : la langue globale est fixée, `serial` l'y protège.
        init_langue(Some("en"));
        // `{file}` reçoit une valeur qui ressemble à un placeholder : elle reste telle quelle.
        let t = TexteNeutre::nouveau(CLE_AVANT_INSTALLATION, &[("file", "{name}")]);
        assert_eq!(t.traduit().as_deref(), Some("before installing {name}"));
        // Même avec une autre valeur fournie pour `name`.
        let t = TexteNeutre::nouveau(CLE_AVANT_INSTALLATION, &[("file", "{name}"), ("name", "X")]);
        assert_eq!(t.traduit().as_deref(), Some("before installing {name}"));
        // Valeur manquante : toujours `None`.
        assert!(TexteNeutre::nouveau(CLE_AVANT_INSTALLATION, &[])
            .traduit()
            .is_none());
    }

    #[test]
    fn cle_inconnue_retombe_sur_le_brut() {
        let e = entree_de_test(Some(TexteNeutre::nouveau("not a known key", &[])));
        assert_eq!(e.sujet_affiche(), "before installing Serum 2");
    }

    #[test]
    fn valeur_manquante_retombe_sur_le_brut() {
        // `{file}` n'a pas de valeur : aucun `{…}` ne doit s'afficher.
        let e = entree_de_test(Some(TexteNeutre::nouveau(CLE_AVANT_INSTALLATION, &[])));
        let affiche = e.sujet_affiche();
        assert_eq!(affiche, "before installing Serum 2");
        assert!(!affiche.contains('{'));
        // Pluriel sans `n` ou avec un `n` illisible : même repli.
        assert!(TexteNeutre::nouveau(CLE_FICHIERS_RETIRES, &[])
            .traduit()
            .is_none());
        assert!(TexteNeutre::nouveau(CLE_FICHIERS_RETIRES, &[("n", "x")])
            .traduit()
            .is_none());
    }

    #[test]
    fn aller_retour_json() {
        let (_d, c) = chemins_de_test();
        // Sans neutre : les champs sont absents du fichier.
        ajouter(&c, Action::Import, "x", ResultatAction::Reussi, None, "");
        let t = std::fs::read_to_string(c.donnees.join(FICHIER_HISTORIQUE)).unwrap();
        assert!(!t.contains("neutre"), "{t}");
        // Avec neutre : relu à l'identique, brut écrit en clair.
        let sujet = TexteNeutre::nouveau(CLE_AVANT_SESSION, &[("name", "Native Access")]);
        let details = TexteNeutre::nouveau(CLE_PLUGINS_TRAITES, &[("n", "2")]);
        ajouter_neutre(
            &c,
            Action::Retour,
            &sujet,
            ResultatAction::Reussi,
            None,
            Some(&details),
            "ignoré",
        );
        let l = lire(&c).unwrap();
        assert_eq!(l[0].sujet_neutre.as_ref(), Some(&sujet));
        assert_eq!(l[0].details_neutre.as_ref(), Some(&details));
        assert!(l[0].sujet.contains("Native Access"), "{:?}", l[0]);
        assert!(l[0].details.starts_with('2'), "{:?}", l[0]);
    }

    #[test]
    fn details_libres_sans_neutre() {
        let (_d, c) = chemins_de_test();
        let sujet = TexteNeutre::nouveau(CLE_REPARATION, &[]);
        ajouter_neutre(
            &c,
            Action::Reparation,
            &sujet,
            ResultatAction::Echec,
            None,
            None,
            "cause",
        );
        let e = &lire(&c).unwrap()[0];
        assert!(e.details_neutre.is_none());
        assert_eq!(e.details_affiches(), "cause");
    }

    #[test]
    fn toutes_les_cles_se_traduisent_avec_leurs_valeurs() {
        // Garde-fou : chaque clé déclarée fonctionne avec les valeurs que ses placeholders
        // réclament (une faute de frappe dans un appelant ferait retomber sur le brut).
        for cle in CLES_CONNUES {
            let noms: Vec<String> = PLACEHOLDER
                .captures_iter(cle)
                .map(|m| m[1].to_owned())
                .collect();
            let valeurs: Vec<(&str, &str)> = noms.iter().map(|n| (n.as_str(), "1")).collect();
            let t = TexteNeutre::nouveau(cle, &valeurs).traduit();
            assert!(t.is_some_and(|t| !t.contains('{')), "{cle}");
        }
    }
}
