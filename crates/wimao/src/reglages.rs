//! Réglages de l'utilisateur, gardés dans `<config>/reglages.toml`
//! (`~/.config/wimao/reglages.toml`).
//!
//! Un seul réglage pour l'instant, la clé `langue` (`"fr"`, `"en"` ou `"es"`) ; sans la
//! clé, WiMao suit la langue du système. Utilise `fichier_toml.rs` (lecture tolérante,
//! écriture atomique, partagées avec `etat.rs`) ; utilisé par `main.rs` (langue au démarrage)
//! et `fenetre/actions.rs` (menu « Langue »).

use std::path::Path;

use serde::{Deserialize, Serialize};
use wimao_core::i18n::trf;
use wimao_core::Resultat;

use crate::fichier_toml;

/// Nom du fichier, dans le dossier des réglages.
pub const FICHIER_REGLAGES: &str = "reglages.toml";

/// Contenu de `reglages.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reglages {
    /// Langue choisie ; `None` : celle du système.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub langue: Option<String>,
}

impl Reglages {
    /// Lit `<config>/reglages.toml`. Absent : réglages par défaut (langue du système), sans
    /// avertissement. Illisible ou mal formé : réglages par défaut aussi, pour qu'un fichier
    /// abîmé n'empêche jamais de démarrer, mais le fichier est **mis de côté** sous le nom
    /// `reglages.toml.abime-<identifiant>` (la personne peut le récupérer ; sans cela, le
    /// prochain choix de langue l'écraserait en silence) et un avertissement traduit est rendu,
    /// à montrer et à journaliser par l'appelant.
    ///
    /// L'avertissement est traduit dans la langue **du moment** : au démarrage, la langue
    /// choisie n'est pas encore connue (c'est ce fichier qui la porte), donc c'est celle du
    /// système.
    pub fn charger(config: &Path) -> (Reglages, Option<String>) {
        let fichier = config.join(FICHIER_REGLAGES);
        let (reglages, avertissement) = fichier_toml::lire(&fichier);
        let Some(avertissement) = avertissement else {
            return (reglages, None);
        };
        let avertissement = match mettre_de_cote(&fichier) {
            Some(nouveau_nom) => trf(
                "{warning} (the file was kept as {name})",
                &[("warning", &avertissement), ("name", &nouveau_nom)],
            ),
            // Mise de côté impossible (dossier en lecture seule…) : l'avertissement seul.
            None => avertissement,
        };
        (reglages, Some(avertissement))
    }

    /// Écrit `<config>/reglages.toml` (atomiquement ; le dossier est créé au besoin).
    ///
    /// # Erreurs
    /// [`wimao_core::Erreur::Io`] si le disque refuse l'écriture.
    pub fn enregistrer(&self, config: &Path) -> Resultat<()> {
        fichier_toml::ecrire(&config.join(FICHIER_REGLAGES), self)
    }
}

/// Renomme `fichier` en `<fichier>.abime-<identifiant>` et rend le nouveau nom (sans dossier).
/// `None` si le renommage échoue : l'appelant se contente alors de l'avertissement.
///
/// L'identifiant est la date locale de [`wimao_core::historique::date_locale_iso`], dont les
/// caractères peu sûrs dans un nom de fichier (`:` et `+`) sont remplacés par `-`. Deux fichiers
/// abîmés mis de côté dans la même seconde se partageraient le même nom : le second remplacerait
/// le premier, ce qui est sans conséquence ici (on ne met de côté qu'une fois par démarrage).
fn mettre_de_cote(fichier: &Path) -> Option<String> {
    let identifiant = wimao_core::historique::date_locale_iso().replace([':', '+'], "-");
    let nom = format!("{FICHIER_REGLAGES}.abime-{identifiant}");
    std::fs::rename(fichier, fichier.with_file_name(&nom)).ok()?;
    Some(nom)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn la_langue_choisie_est_relue() {
        let t = TempDir::new().unwrap();
        let reglages = Reglages {
            langue: Some("es".into()),
        };
        reglages.enregistrer(t.path()).unwrap();
        assert_eq!(Reglages::charger(t.path()), (reglages, None));
        let texte = std::fs::read_to_string(t.path().join(FICHIER_REGLAGES)).unwrap();
        assert_eq!(texte.trim(), "langue = \"es\"");
        // Écriture atomique : aucun fichier temporaire ne reste.
        assert!(!t.path().join("reglages.toml.tmp").exists());
    }

    #[test]
    fn sans_fichier_la_langue_est_celle_du_systeme() {
        let t = TempDir::new().unwrap();
        assert_eq!(Reglages::charger(t.path()), (Reglages::default(), None));
    }

    #[test]
    fn fichier_abime_est_mis_de_cote_avec_un_avertissement() {
        let t = TempDir::new().unwrap();
        std::fs::write(t.path().join(FICHIER_REGLAGES), "langue = [").unwrap();
        let (reglages, avertissement) = Reglages::charger(t.path());
        // Réglages par défaut, un avertissement, et le fichier abîmé est conservé à côté.
        assert_eq!(reglages, Reglages::default());
        let avertissement = avertissement.expect("un avertissement est attendu");
        assert!(
            avertissement.contains("reglages.toml.abime-"),
            "{avertissement}"
        );
        assert!(!t.path().join(FICHIER_REGLAGES).exists());
        let mis_de_cote: Vec<_> = std::fs::read_dir(t.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|nom| nom.starts_with("reglages.toml.abime-"))
            .collect();
        assert_eq!(mis_de_cote.len(), 1, "{mis_de_cote:?}");
        // Le contenu d'origine est intact, pour que la personne puisse le récupérer.
        let copie = std::fs::read_to_string(t.path().join(&mis_de_cote[0])).unwrap();
        assert_eq!(copie, "langue = [");
        // Le démarrage suivant repart d'un dossier propre, sans nouvel avertissement.
        assert_eq!(Reglages::charger(t.path()), (Reglages::default(), None));
    }
}
