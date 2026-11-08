//! Lignes du tableau des plugins : `nom | format | ✅/⚠️/❌ | détail`.
//!
//! Utilise `i18n`, `inventaire` (formats), `pontage` (états de pontage) et `test_plugin`
//! (résultats de test). Utilisé par la CLI (`verifier` et `installer`) et par l'interface
//! GTK, pour que les deux disent exactement la même chose d'un plugin ; c'est pourquoi
//! ce module vit dans le noyau et non dans l'une des deux.

use crate::i18n::{tr, tr_noop};
use crate::inventaire::Format;
use crate::pontage::EtatPontage;
use crate::test_plugin::ResultatTest;

/// Verdict d'une ligne du tableau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statut {
    /// ✅ : tout va bien.
    Bon,
    /// ⚠️ : à surveiller, sans être une panne.
    Avertissement,
    /// ❌ : le plugin ne fonctionne pas.
    Echec,
}

impl Statut {
    /// Le symbole affiché.
    pub fn symbole(self) -> &'static str {
        match self {
            Statut::Bon => "✅",
            Statut::Avertissement => "⚠️",
            Statut::Echec => "❌",
        }
    }
}

/// Une ligne du tableau.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ligne {
    /// Nom du plugin.
    pub nom: String,
    /// Format du plugin.
    pub format: Format,
    /// Verdict.
    pub statut: Statut,
    /// Explication, déjà traduite.
    pub detail: String,
}

impl Ligne {
    /// Le texte de la ligne : `nom | format | symbole | détail`.
    pub fn texte(&self) -> String {
        format!(
            "{} | {} | {} | {}",
            self.nom,
            nom_format(self.format),
            self.statut.symbole(),
            self.detail
        )
    }
}

/// Nom court d'un format (noms propres : non traduits).
pub fn nom_format(format: Format) -> &'static str {
    match format {
        Format::Vst2 => "VST2",
        Format::Vst3 => "VST3",
        Format::Clap => "CLAP",
    }
}

/// Détail d'un verdict **avant** traduction : soit un `msgid` connu (qu'on peut mémoriser et
/// traduire plus tard, dans la langue du moment), soit un texte libre déjà traduit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetailBrut {
    /// Un `msgid` anglais de `po/` (une des constantes `DETAIL_*`).
    Cle(&'static str),
    /// Texte venu d'ailleurs (message d'un test de plugin), déjà traduit : affiché tel quel.
    Libre(String),
}

impl DetailBrut {
    /// Le texte à afficher, dans la langue courante.
    pub fn traduire(&self) -> String {
        match self {
            DetailBrut::Cle(msgid) => tr(msgid),
            DetailBrut::Libre(texte) => texte.clone(),
        }
    }
}

/// Détail d'un plugin qui s'ouvre.
pub const DETAIL_OUVRE: &str = tr_noop("The plugin opens");
/// Détail d'un plugin chargé dont l'interface n'a pas pu être vérifiée par le test.
pub const DETAIL_CHARGE: &str =
    tr_noop("Loads correctly; open it in your music software to check its interface");
/// Détail d'un VST2 écarté au profit du VST3.
pub const DETAIL_DOUBLON: &str = tr_noop("VST2 skipped: the same plugin is available as VST3");
/// Détail d'un plugin déjà ponté depuis un autre environnement.
pub const DETAIL_AILLEURS: &str = tr_noop("already bridged from another environment");
/// Détail d'un plugin ponté mais pas testé.
pub const DETAIL_NON_TESTE: &str = tr_noop("bridged, not tested");

/// Verdict et détail brut d'un test d'ouverture (`Remarque` donne ⚠️, `Probleme` donne ❌).
pub fn brut_depuis_test(test: &ResultatTest) -> (Statut, DetailBrut) {
    match test {
        ResultatTest::Pret => (Statut::Bon, DetailBrut::Cle(DETAIL_OUVRE)),
        ResultatTest::Charge => (Statut::Bon, DetailBrut::Cle(DETAIL_CHARGE)),
        ResultatTest::Remarque(texte) => (Statut::Avertissement, DetailBrut::Libre(texte.clone())),
        ResultatTest::Probleme(texte) => (Statut::Echec, DetailBrut::Libre(texte.clone())),
    }
}

/// Verdict et détail brut d'un plugin qui vient d'être installé : l'état de pontage d'abord
/// (un plugin écarté ou déjà ponté ailleurs n'a pas été testé), puis le test s'il a eu lieu.
pub fn brut_depuis_installation(
    etat: &EtatPontage,
    test: Option<&ResultatTest>,
) -> (Statut, DetailBrut) {
    match (etat, test) {
        (EtatPontage::Doublon, _) => (Statut::Avertissement, DetailBrut::Cle(DETAIL_DOUBLON)),
        (EtatPontage::DejaPonteAilleurs(_), _) => {
            (Statut::Avertissement, DetailBrut::Cle(DETAIL_AILLEURS))
        }
        (EtatPontage::Ponte, Some(test)) => brut_depuis_test(test),
        (EtatPontage::Ponte, None) => (Statut::Bon, DetailBrut::Cle(DETAIL_NON_TESTE)),
    }
}

/// Verdict et détail traduit d'un test d'ouverture.
pub fn depuis_test(test: &ResultatTest) -> (Statut, String) {
    let (statut, detail) = brut_depuis_test(test);
    (statut, detail.traduire())
}

/// Verdict et détail traduit d'un plugin qui vient d'être installé.
pub fn depuis_installation(etat: &EtatPontage, test: Option<&ResultatTest>) -> (Statut, String) {
    let (statut, detail) = brut_depuis_installation(etat, test);
    (statut, detail.traduire())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_ligne_suit_le_format_nom_format_statut_detail() {
        let ligne = Ligne {
            nom: "Xpand!2".into(),
            format: Format::Vst3,
            statut: Statut::Avertissement,
            detail: "d".into(),
        };
        assert_eq!(ligne.texte(), "Xpand!2 | VST3 | ⚠️ | d");
    }

    #[test]
    fn un_probleme_de_test_est_un_echec() {
        let (statut, _) = depuis_test(&ResultatTest::Probleme("x".into()));
        assert_eq!(statut, Statut::Echec);
        let (statut, _) = depuis_test(&ResultatTest::Remarque("x".into()));
        assert_eq!(statut, Statut::Avertissement);
    }

    #[test]
    // Exclusif des tests de langue (clé par défaut) : ce test lit la langue globale deux fois.
    #[serial_test::serial]
    fn charge_donne_bon_avec_le_detail() {
        crate::i18n::init_langue(Some("en"));
        assert_eq!(
            brut_depuis_test(&ResultatTest::Charge),
            (Statut::Bon, DetailBrut::Cle(DETAIL_CHARGE))
        );
    }

    #[test]
    fn un_doublon_n_est_pas_un_succes() {
        let (statut, _) = depuis_installation(&EtatPontage::Doublon, None);
        assert_eq!(statut, Statut::Avertissement);
    }
}
