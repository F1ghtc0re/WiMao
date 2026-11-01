//! Sous-commandes `init`, `installer` et `detecter`.
//!
//! Utilise `wimao_core::creer_environnement` et `wimao_recettes` (catalogue, détection,
//! orchestrateur d'installation, suite commune : lien de connexion), et `confirmation` (la
//! question « o/N » ; la décision de la poser vient de la bibliothèque, pour que l'interface
//! pense pareil). Utilisé par `commandes::executer`.

use std::collections::BTreeSet;
use std::io::{stdin, stdout};
use std::path::{Path, PathBuf};

use wimao_core::i18n::{tr, trf};
use wimao_core::pontage::EtatPontage;
use wimao_core::{creer_environnement, Chemins, Erreur, PluginWindows, Resultat, ResultatTest};
use wimao_recettes::{
    declarer_lien_wimao, detecter as detecter_type, dossiers_recettes,
    installer as installer_plugin, lien_non_declare, traiter_le_lien, Catalogue,
    OptionsInstallation, RapportInstallation, SuiteLien, TypeInstalleur,
};

use super::{accord_avant_arret, Issue};
use crate::cli::ArgsInstaller;
use crate::confirmation::confirmer;
use crate::progression_terminal::ProgressionTerminal;
use crate::sortie::afficher;
use wimao_core::tableau::{depuis_installation, Ligne, Statut};

/// `init` : crée l'environnement Wine (sans effet s'il existe déjà).
pub fn init(c: &Chemins) -> Resultat<Issue> {
    creer_environnement(c, &ProgressionTerminal)?;
    afficher!("{}", tr("The Wine environment is ready"));
    Ok(Issue::Succes)
}

/// `installer` : installe un plugin et imprime le tableau des plugins installés.
///
/// Les recettes invalides sont signalées sur la sortie d'erreur sans bloquer l'installation
/// (une recette cassée ne doit pas empêcher d'installer un autre plugin). Rend
/// [`Issue::PluginEnEchec`] si un plugin testé est en échec.
pub fn installer(c: &Chemins, args: &ArgsInstaller) -> Resultat<Issue> {
    if !args.fichier.exists() {
        return Err(Erreur::Introuvable(trf(
            "the file {path} does not exist",
            &[("path", &args.fichier.display().to_string())],
        )));
    }
    // Installer arrête Wine : on nomme d'abord les programmes ouverts (refus : rien n'est fait).
    if !accord_avant_arret(c, args.oui)? {
        return Ok(Issue::Succes);
    }
    let catalogue = charger_catalogue(c);
    let options = OptionsInstallation {
        interactif: args.interactif,
        tester: !args.sans_test,
    };
    let rapport = installer_plugin(c, &args.fichier, &catalogue, options, &ProgressionTerminal)?;

    let issue = afficher_plugins(&rapport.plugins, &rapport.mis_a_jour);
    if rapport.interactif_necessaire {
        afficher!("{}", message_aucun_plugin(rapport.code_installeur));
    }
    // `--oui` vaut aussi pour la question du lien : si un autre programme le gère, on la saute.
    apres_installation(c, &rapport, args.oui);
    Ok(issue)
}

/// Suite commune à toute installation réussie (`installer` et `gestionnaire installer`, comme
/// l'interface) : lien de connexion de la recette (`wimao_recettes::traiter_le_lien` ; si un
/// autre programme le gère, la question « o/N », sautée par `oui`), puis consigne
/// `activation` de la recette.
///
/// Une erreur du lien (outil `xdg-mime` absent, relais introuvable…) n'est qu'un
/// **avertissement** sur la sortie d'erreur : le produit est installé, le code de sortie
/// reste celui de l'installation.
pub(super) fn apres_installation(c: &Chemins, rapport: &RapportInstallation, oui: bool) {
    let (lien, probleme) = traiter_le_lien(c, rapport);
    if let Some(texte) = probleme {
        avertir(&texte);
    }
    match lien {
        SuiteLien::Aucune => {}
        SuiteLien::Declare(schema) => afficher!("{}", texte_lien_relaye(&schema)),
        SuiteLien::AConfirmer { schema, actuel } => {
            let question = trf(
                "The link {scheme}:// is handled by {current}. Replace it with WiMao?",
                &[("scheme", &schema), ("current", &actuel)],
            );
            if !confirmer(&question, oui, &mut stdin().lock(), &mut stdout()) {
                afficher!(
                    "{}",
                    trf(
                        "The link {scheme}:// is not relayed to WiMao",
                        &[("scheme", &schema)]
                    )
                );
            } else {
                match declarer_lien_wimao(c, &schema) {
                    Ok(()) => afficher!("{}", texte_lien_relaye(&schema)),
                    Err(erreur) => avertir(&lien_non_declare(&erreur)),
                }
            }
        }
    }
    if let Some(texte) = &rapport.consigne_activation {
        afficher!("{texte}");
    }
}

/// Texte d'un lien de connexion confié à WiMao.
fn texte_lien_relaye(schema: &str) -> String {
    trf(
        "The link {scheme}:// is now relayed to WiMao",
        &[("scheme", schema)],
    )
}

/// Écrit un avertissement sur la sortie d'erreur (sans changer le code de sortie).
fn avertir(texte: &str) {
    eprintln!("{} {texte}", tr("Warning:"));
}

/// Charge les recettes. Les recettes invalides sont signalées sur la sortie d'erreur sans
/// rien bloquer : une recette cassée ne doit pas empêcher d'utiliser les autres.
pub(super) fn charger_catalogue(c: &Chemins) -> Catalogue {
    let (catalogue, erreurs) = Catalogue::charger(&dossiers_recettes(c));
    for erreur in erreurs {
        eprintln!("{} {erreur}", tr("Warning, ignored recipe:"));
    }
    catalogue
}

/// Écrit une ligne par plugin (`nom | format | ✅/⚠️/❌ | détail`) et rend
/// [`Issue::PluginEnEchec`] si l'un est en échec. Commun à `installer` et à la fermeture
/// d'une session de gestionnaire. `mis_a_jour` : chemins des plugins qui existaient déjà.
pub(super) fn afficher_plugins(
    plugins: &[(PluginWindows, EtatPontage, Option<ResultatTest>)],
    mis_a_jour: &BTreeSet<PathBuf>,
) -> Issue {
    let mut issue = Issue::Succes;
    for (plugin, etat, test) in plugins {
        let (statut, detail) = depuis_installation(etat, test.as_ref());
        if statut == Statut::Echec {
            issue = Issue::PluginEnEchec;
        }
        // Une mise à jour est signalée à côté du nom : sinon on croirait à un doublon.
        let nom = if mis_a_jour.contains(&plugin.chemin) {
            trf("{name} (updated)", &[("name", &plugin.nom)])
        } else {
            plugin.nom.clone()
        };
        let ligne = Ligne {
            nom,
            format: plugin.format,
            statut,
            detail,
        };
        afficher!("{}", ligne.texte());
    }
    issue
}

/// Conseil quand rien n'a été installé ; cite le code de sortie de l'installeur s'il est
/// non nul (il aide à comprendre pourquoi, voir le journal pour le détail).
fn message_aucun_plugin(code: Option<i32>) -> String {
    match code {
        Some(code) if code != 0 => trf(
            "No new or updated plugin was found (installer exit code {code}): run the command again with --interactif to open the installer normally",
            &[("code", &code.to_string())],
        ),
        _ => tr("No new or updated plugin was found: run the command again with --interactif to open the installer normally"),
    }
}

/// `detecter` : dit de quel type est l'installeur, et quelle recette s'y applique.
///
/// `chemins` est facultatif : sans lui (`HOME` absent) on détecte quand même, seule la
/// recherche de recette est omise.
pub fn detecter(fichier: &Path, chemins: Option<&Chemins>) -> Resultat<Issue> {
    let type_ = detecter_type(fichier)?;
    afficher!(
        "{}",
        trf(
            "{file}: {type}",
            &[
                ("file", &fichier.display().to_string()),
                ("type", &nom_type(type_))
            ]
        )
    );
    if let Some(c) = chemins {
        let (catalogue, _) = Catalogue::charger(&dossiers_recettes(c));
        if let Some(recette) = catalogue.trouver(fichier) {
            afficher!("{}", trf("Recipe: {name}", &[("name", &recette.nom)]));
        }
    }
    Ok(Issue::Succes)
}

/// Nom lisible d'un type d'installeur (les marques ne sont pas traduites).
fn nom_type(type_: TypeInstalleur) -> String {
    match type_ {
        TypeInstalleur::Inno => "Inno Setup".to_owned(),
        TypeInstalleur::Msi => "Windows Installer (MSI)".to_owned(),
        TypeInstalleur::WixBurn => "WiX Burn".to_owned(),
        TypeInstalleur::Nsis => "NSIS".to_owned(),
        TypeInstalleur::InstallAware => "InstallAware".to_owned(),
        TypeInstalleur::Archive => tr("Archive (to extract)"),
        TypeInstalleur::Inconnu => tr("Unknown installer"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_marques_ne_sont_pas_traduites() {
        assert_eq!(nom_type(TypeInstalleur::Inno), "Inno Setup");
        assert_eq!(nom_type(TypeInstalleur::Nsis), "NSIS");
    }
}
