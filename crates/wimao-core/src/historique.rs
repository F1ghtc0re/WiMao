//! Historique des actions de l'utilisateur (`<données>/historique.jsonl`).
//!
//! - Une action = une ligne JSON (format « JSON Lines ») : date, nature, sujet, résultat et,
//!   si besoin, le point de restauration associé et un détail. À ne pas confondre avec le
//!   [`journal`], qui trace les *commandes* lancées pour le diagnostic technique.
//! - [`ajouter`] écrit à la fin du fichier ; [`lire`] relit tout, la plus récente en premier.
//!
//! # Choix non évidents
//!
//! - **Écrire ne fait jamais échouer l'action** : une erreur d'écriture est seulement notée au
//!   journal technique (spec 6.2). L'historique est une aide, pas une condition de réussite.
//! - **Une ligne illisible est ignorée** à la lecture (coupure de courant en pleine écriture,
//!   édition à la main…) : le reste de l'historique reste consultable.
//! - Si le fichier ne se termine pas par un saut de ligne (écriture interrompue), [`ajouter`]
//!   en commence un, pour que la nouvelle entrée ne soit pas collée à la ligne abîmée.
//! - La date est écrite avec son décalage horaire (`+01:00`) pour rester non ambiguë.
//!
//! Utilise `chemins`, `journal` et `outils::heure_locale` ; utilisé par le moteur
//! (installation, réparation, sauvegarde, import…) et par l'interface (page Historique).

use crate::chemins::Chemins;
use crate::erreur::Resultat;
use crate::i18n::{tr, trc};
use crate::journal;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

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
pub fn ajouter(
    c: &Chemins,
    action: Action,
    sujet: &str,
    resultat: ResultatAction,
    point: Option<&str>,
    details: &str,
) {
    let entree = Entree {
        date: date_locale_iso(),
        action,
        sujet: sujet.to_owned(),
        resultat,
        point: point.map(str::to_owned),
        details: details.to_owned(),
    };
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
}
