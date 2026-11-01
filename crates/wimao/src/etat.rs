//! Mémoire des résultats de test, gardée dans `<donnees>/etat.toml`.
//!
//! Le noyau ne conserve pas le verdict d'un test d'ouverture : la CLI l'affiche puis
//! l'oublie. L'interface, elle, montre la liste des plugins à chaque démarrage sans
//! relancer Carla (plusieurs secondes par plugin) ; elle garde donc le dernier verdict
//! connu de chaque plugin dans ce fichier. Format :
//!
//! ```toml
//! [plugins."/chemin/du/prefixe/drive_c/Program Files/Common Files/VST3/WiMaoTest.vst3"]
//! resultat = "pret"            # pret | remarque | probleme
//! cle = "The plugin opens"     # facultatif : msgid anglais, traduit à l'affichage
//! detail = ""                  # texte libre, affiché tel quel quand il n'y a pas de `cle`
//! ```
//!
//! `cle` permet d'afficher le détail dans la langue **du moment** et non celle de l'installation ;
//! les anciens fichiers (sans `cle`, `detail` libre) restent lus et affichés tels quels.
//!
//! Utilise `wimao_core::tableau` (verdict, détail brut) et `fichier_toml.rs` (lecture et écriture
//! atomique). Utilisé par `operations.rs` (lecture à chaque relevé des plugins, écriture après une
//! installation) et `liste.rs` (pastilles de la liste).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use wimao_core::i18n::tr;
use wimao_core::tableau::{DetailBrut, Statut};
use wimao_core::Resultat;

use crate::fichier_toml;

/// Nom du fichier, dans le dossier de données.
pub const FICHIER_ETAT: &str = "etat.toml";

/// Verdict mémorisé, écrit en minuscules dans le fichier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// ✅ : le plugin s'ouvre.
    Pret,
    /// ⚠️ : à surveiller.
    Remarque,
    /// ❌ : le plugin ne fonctionne pas.
    Probleme,
}

impl Verdict {
    /// Le verdict mémorisé correspondant à un [`Statut`] du noyau.
    pub fn depuis_statut(statut: Statut) -> Verdict {
        match statut {
            Statut::Bon => Verdict::Pret,
            Statut::Avertissement => Verdict::Remarque,
            Statut::Echec => Verdict::Probleme,
        }
    }

    /// Le [`Statut`] du noyau, qui sait donner le symbole à afficher.
    pub fn statut(self) -> Statut {
        match self {
            Verdict::Pret => Statut::Bon,
            Verdict::Remarque => Statut::Avertissement,
            Verdict::Probleme => Statut::Echec,
        }
    }
}

/// Dernier résultat connu d'un plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntreePlugin {
    /// Le verdict.
    pub resultat: Verdict,
    /// `msgid` anglais du détail, traduit à l'affichage ; absent dans les anciens fichiers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cle: Option<String>,
    /// Détail libre (texte déjà traduit), utilisé quand il n'y a pas de `cle`.
    #[serde(default)]
    pub detail: String,
}

impl EntreePlugin {
    /// Le détail à afficher, dans la langue courante.
    pub fn texte(&self) -> String {
        match &self.cle {
            Some(msgid) => tr(msgid),
            None => self.detail.clone(),
        }
    }
}

/// Contenu de `etat.toml` : un résultat par chemin de plugin Windows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Etat {
    /// Chemin du plugin (texte, car clé TOML) vers son dernier résultat.
    #[serde(default)]
    pub plugins: BTreeMap<String, EntreePlugin>,
}

impl Etat {
    /// Lit `<donnees>/etat.toml`. Absent : état vide. Illisible ou mal formé : état vide aussi
    /// (ce n'est qu'une mémoire d'affichage) **avec** un avertissement à montrer.
    pub fn charger(donnees: &Path) -> (Etat, Option<String>) {
        fichier_toml::lire(&donnees.join(FICHIER_ETAT))
    }

    /// Écrit `<donnees>/etat.toml` (atomiquement, voir `fichier_toml::ecrire`).
    ///
    /// # Erreurs
    /// [`wimao_core::Erreur::Io`] si le disque refuse l'écriture.
    pub fn enregistrer(&self, donnees: &Path) -> Resultat<()> {
        fichier_toml::ecrire(&donnees.join(FICHIER_ETAT), self)
    }

    /// Mémorise le résultat d'un plugin (remplace le précédent).
    pub fn noter(&mut self, chemin: &Path, statut: Statut, detail: &DetailBrut) {
        let (cle, texte) = match detail {
            DetailBrut::Cle(msgid) => (Some((*msgid).to_owned()), String::new()),
            DetailBrut::Libre(texte) => (None, texte.clone()),
        };
        self.plugins.insert(
            chemin.display().to_string(),
            EntreePlugin {
                resultat: Verdict::depuis_statut(statut),
                cle,
                detail: texte,
            },
        );
    }

    /// Le dernier résultat connu du plugin installé à `chemin`.
    pub fn trouver(&self, chemin: &Path) -> Option<&EntreePlugin> {
        self.plugins.get(&chemin.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn libre(t: &str) -> DetailBrut {
        DetailBrut::Libre(t.into())
    }

    #[test]
    fn l_ecriture_puis_la_relecture_sont_identiques() {
        let t = TempDir::new().unwrap();
        let mut etat = Etat::default();
        // Un chemin avec espaces, `!` et accent : la clé TOML doit être correctement citée.
        etat.noter(
            Path::new("/p/Program Files/Électrique/Xpand!2.vst3"),
            Statut::Avertissement,
            &libre("interface \"non vérifiée\""),
        );
        etat.noter(
            Path::new("/p/B.vst3"),
            Statut::Bon,
            &DetailBrut::Cle("The plugin opens"),
        );
        etat.enregistrer(t.path()).unwrap();
        assert_eq!(Etat::charger(t.path()), (etat, None));
    }

    #[test]
    fn le_fichier_suit_le_format_documente() {
        let t = TempDir::new().unwrap();
        let mut etat = Etat::default();
        etat.noter(Path::new("/p/A.vst3"), Statut::Echec, &libre("d"));
        etat.enregistrer(t.path()).unwrap();
        let texte = std::fs::read_to_string(t.path().join(FICHIER_ETAT)).unwrap();
        assert!(texte.contains("[plugins.\"/p/A.vst3\"]"), "{texte}");
        assert!(texte.contains("resultat = \"probleme\""), "{texte}");
    }

    #[test]
    fn un_ancien_fichier_sans_cle_est_lu_tel_quel() {
        let t = TempDir::new().unwrap();
        std::fs::write(
            t.path().join(FICHIER_ETAT),
            "[plugins.\"/p/A.vst3\"]\nresultat = \"pret\"\ndetail = \"Le plugin s'ouvre\"\n",
        )
        .unwrap();
        let (etat, avertissement) = Etat::charger(t.path());
        assert!(avertissement.is_none());
        assert_eq!(
            etat.trouver(Path::new("/p/A.vst3")).unwrap().texte(),
            "Le plugin s'ouvre"
        );
    }

    #[test]
    fn un_fichier_abime_donne_un_etat_vide_et_un_avertissement() {
        let t = TempDir::new().unwrap();
        assert_eq!(Etat::charger(t.path()), (Etat::default(), None));
        std::fs::write(t.path().join(FICHIER_ETAT), "ceci n'est pas du TOML [").unwrap();
        let (etat, avertissement) = Etat::charger(t.path());
        assert_eq!(etat, Etat::default());
        assert!(avertissement.is_some());
    }

    #[test]
    fn noter_remplace_le_resultat_precedent() {
        let mut etat = Etat::default();
        let chemin = Path::new("/p/A.vst3");
        etat.noter(chemin, Statut::Echec, &libre("avant"));
        etat.noter(chemin, Statut::Bon, &libre("après"));
        assert_eq!(etat.plugins.len(), 1);
        assert_eq!(etat.trouver(chemin).unwrap().resultat, Verdict::Pret);
    }
}
