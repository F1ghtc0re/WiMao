//! Les actions de WiMao, écrites comme des fonctions ordinaires sans aucun widget.
//!
//! Chaque fonction ici est destinée à tourner dans un fil de travail (`travail.rs`, ou le fil
//! de session de `session.rs`) : elle appelle le noyau (`wimao-core`) et les recettes
//! (`wimao-recettes`) comme le fait la CLI (`wimao-cli`), et rend des données simples. Séparer
//! cela de `fenetre/mod.rs` garde la fenêtre mince et permet de tester l'essentiel sans affichage.
//!
//! Les **décisions** communes avec la CLI ne sont pas recopiées ici : elles viennent des
//! bibliothèques (`traiter_le_lien`, `programmes_a_nommer_avant_reparation`,
//! `Catalogue::trouver_par_nom`). Seule la façon de poser la question change (dialogue).
//!
//! Utilisé par `fenetre/mod.rs`, `gestionnaires.rs`, `reparation.rs` et `session.rs`. Utilise `etat.rs` (mémoire
//! des résultats de test), `liste.rs` et `gestionnaires.rs` (données des lignes).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use wimao_core::historique::{self, Entree};
use wimao_core::i18n::{tr, trf, trn};
use wimao_core::import::annuler as annuler_l_import;
use wimao_core::journal;
use wimao_core::pontage::EtatPontage;
use wimao_core::rapport::rapport as construire_rapport;
use wimao_core::restauration::{self, BilanRetour, ListePoints, PointRestauration};
use wimao_core::sauvegarde::restaurer as restaurer_sauvegarde;
use wimao_core::sauvegarde::sauvegarder as sauvegarder_dans;
use wimao_core::tableau::{brut_depuis_installation, Statut};
use wimao_core::{
    diagnostiquer, reparer as reparer_probleme, Chemins, Erreur, Inventaire, PluginWindows,
    Probleme, Progression, Resultat, ResultatTest,
};
use wimao_recettes::traiter_le_lien;
use wimao_recettes::{
    dossiers_recettes, est_installe, installer as installer_plugin, installer_recette,
    programmes_a_nommer_avant_reparation, reparer_gestionnaires, BilanSession, Catalogue,
    IssueRelance, OptionsInstallation, RapportInstallation, Recette, SuiteLien, TypeRecette,
};

use crate::etat::Etat;
use crate::gestionnaires::LigneGestionnaire;
use crate::liste::LignePlugin;

/// Dossier des disques de Windows dans l'environnement Wine.
const DOSSIER_DRIVE_C: &str = "drive_c";

/// Ce que le fil de travail ramène avec son résultat : des avertissements non bloquants
/// (recette ignorée, `etat.toml` inutilisable…), déjà écrits au journal, que la fenêtre montre
/// dans un toast.
pub type Avertissements = Vec<String>;

/// Plugins d'une installation ou d'une session : le plugin, son état de pontage et le résultat
/// de son test d'ouverture (`None` : pas testé).
type PluginsTraites = [(PluginWindows, EtatPontage, Option<ResultatTest>)];

/// Écrit un avertissement au journal et sur stderr, puis le range dans `avertissements`.
fn avertir(avertissements: &mut Avertissements, texte: String) {
    eprintln!("{texte}");
    journal::noter("avertissement de l'interface", &texte);
    avertissements.push(texte);
}

/// Charge le catalogue des recettes ; chaque recette invalide devient un avertissement, pour
/// qu'une recette cassée n'empêche ni d'installer un plugin ni d'afficher la fenêtre.
fn charger_catalogue(c: &Chemins, avertissements: &mut Avertissements) -> Catalogue {
    let (catalogue, erreurs) = Catalogue::charger(&dossiers_recettes(c));
    for erreur in erreurs {
        // Pas de « Warning » ici : la fenêtre l'ajoute déjà devant chaque avertissement.
        let texte = format!("{} {erreur}", tr("Ignored recipe:"));
        avertir(avertissements, texte);
    }
    catalogue
}

/// Ce qu'affiche la fenêtre : les plugins installés et les gestionnaires d'éditeurs connus.
#[derive(Debug, Clone, Default)]
pub struct Releve {
    /// Une ligne par plugin, triée (voir [`LignePlugin::depuis_inventaire`]).
    pub plugins: Vec<LignePlugin>,
    /// Une ligne par recette de gestionnaire, triée par nom.
    pub gestionnaires: Vec<LigneGestionnaire>,
}

/// Relève les plugins installés (avec leur dernier résultat de test connu) et l'état des
/// gestionnaires du catalogue.
///
/// `mis_a_jour` : plugins mis à jour depuis l'ouverture de la fenêtre (ils portent la marque
/// « Updated », voir [`LignePlugin::depuis_inventaire`]).
///
/// # Erreurs
/// Celles de [`Inventaire::relever`] (dossier illisible).
pub fn lister(c: &Chemins, mis_a_jour: &BTreeSet<PathBuf>) -> Resultat<(Releve, Avertissements)> {
    let mut avertissements = Vec::new();
    let inventaire = Inventaire::relever(&c.prefixe.join(DOSSIER_DRIVE_C))?;
    let (etat, probleme) = Etat::charger(&c.donnees);
    if let Some(texte) = probleme {
        avertir(&mut avertissements, texte);
    }
    let catalogue = charger_catalogue(c, &mut avertissements);
    let releve = Releve {
        plugins: LignePlugin::depuis_inventaire(&inventaire, &etat, mis_a_jour),
        gestionnaires: lignes_gestionnaires(c, &catalogue),
    };
    Ok((releve, avertissements))
}

/// Une ligne par recette `type = "gestionnaire"` du catalogue, triée par nom (sans casse).
/// « Installé » quand son `programme` existe dans l'environnement ([`est_installe`]).
pub fn lignes_gestionnaires(c: &Chemins, catalogue: &Catalogue) -> Vec<LigneGestionnaire> {
    let mut lignes: Vec<LigneGestionnaire> = catalogue
        .0
        .iter()
        .filter(|r| r.type_ == TypeRecette::Gestionnaire)
        .map(|r| LigneGestionnaire {
            nom: r.nom.clone(),
            editeur: r.editeur.clone(),
            installe: est_installe(c, r),
            telechargeable: r.telecharger.is_some(),
        })
        .collect();
    lignes.sort_by_key(|l| l.nom.to_lowercase());
    lignes
}

/// Dit combien de plugins ne sont pas pontés (`Probleme::PontageIncomplet`), d'après le
/// diagnostic (lecture seule, sans lancer Wine). `None` : rien à ponter.
///
/// Sert au démarrage (spec 4.4) : si WiMao a été fermé pendant une session, ce que le
/// gestionnaire a installé ensuite attend d'être ponté.
///
/// # Erreurs
/// Celles de `diagnostiquer`.
pub fn plugins_non_pontes(c: &Chemins) -> Resultat<Option<usize>> {
    Ok(diagnostiquer(c)?.into_iter().find_map(|p| match p {
        Probleme::PontageIncomplet(n) => Some(n),
        _ => None,
    }))
}

/// Ce qu'une installation (« Add an installer » ou « Install » d'un gestionnaire) rend à la
/// fenêtre.
#[derive(Debug, Clone)]
pub struct Installe {
    /// Rapport de l'installation (plugins, code de l'installeur, consigne d'activation…).
    pub rapport: RapportInstallation,
    /// Ce qu'il reste à faire du lien de connexion (`wimao_recettes::traiter_le_lien`).
    pub lien: SuiteLien,
}

/// Installe `fichier`, puis fait la suite commune à toutes les installations
/// ([`apres_installer`] : verdicts mémorisés, lien de connexion).
///
/// `interactif` ouvre la fenêtre de l'installeur au lieu de le lancer en silence (second
/// essai proposé quand l'installation silencieuse n'a rien ajouté).
///
/// # Erreurs
/// Celles de `wimao_recettes::installer`. Une recette invalide, une mémoire de résultats
/// impossible à lire ou écrire, ou un lien qui ne peut pas être déclaré n'est pas une erreur
/// d'installation : c'est un avertissement (journal + toast), pour qu'une recette cassée
/// n'empêche pas d'installer un autre plugin.
pub fn installer(
    c: &Chemins,
    fichier: &Path,
    interactif: bool,
    p: &dyn Progression,
) -> Resultat<(Installe, Avertissements)> {
    let mut avertissements = Vec::new();
    let catalogue = charger_catalogue(c, &mut avertissements);
    let options = OptionsInstallation {
        interactif,
        tester: true,
    };
    let rapport = installer_plugin(c, fichier, &catalogue, options, p)?;
    let installe = apres_installer(c, rapport, &mut avertissements);
    Ok((installe, avertissements))
}

/// Suite commune de toute installation réussie : verdicts mémorisés, puis lien de connexion
/// déclaré s'il est libre (sinon la fenêtre demandera l'accord) ; une erreur de lien devient
/// un avertissement. La consigne d'activation reste dans le rapport, pour la fenêtre.
fn apres_installer(
    c: &Chemins,
    rapport: RapportInstallation,
    avertissements: &mut Avertissements,
) -> Installe {
    memoriser(c, &rapport.plugins, avertissements);
    let (lien, probleme) = traiter_le_lien(c, &rapport);
    if let Some(texte) = probleme {
        avertir(avertissements, texte);
    }
    Installe { rapport, lien }
}

/// Recette désignée par `nom` (nom de la recette ou de son fichier, voir
/// [`Catalogue::trouver_par_nom`]).
///
/// # Erreurs
/// [`Erreur::Introuvable`] si aucune recette ne porte ce nom.
pub fn recette_nommee(
    c: &Chemins,
    nom: &str,
    avertissements: &mut Avertissements,
) -> Resultat<Recette> {
    charger_catalogue(c, avertissements)
        .trouver_par_nom(nom)
        .cloned()
        .ok_or_else(|| Erreur::Introuvable(trf("a recipe named {name}", &[("name", nom)])))
}

/// Résultat d'une installation de gestionnaire.
#[derive(Debug, Clone)]
pub struct GestionnaireInstalle {
    /// Nom de la recette, pour les messages.
    pub nom: String,
    /// Rapport de l'installation et suite du lien de connexion.
    pub installe: Installe,
}

/// Installe le gestionnaire `nom` (spec 4.1) : télécharge son installeur si `fichier` est
/// absent, installe, puis fait la suite commune ([`apres_installer`] : verdicts mémorisés,
/// lien de connexion déclaré tout de suite s'il est libre, laissé à la décision de
/// l'utilisateur si un autre programme le gère ; même décision que la CLI).
///
/// # Erreurs
/// Celles de [`recette_nommee`] et d'[`installer_recette`]. Un lien qui ne peut pas être
/// déclaré n'est **pas** une erreur : le gestionnaire est installé ; c'est un avertissement.
pub fn installer_gestionnaire(
    c: &Chemins,
    nom: &str,
    fichier: Option<&Path>,
    p: &dyn Progression,
) -> Resultat<(GestionnaireInstalle, Avertissements)> {
    let mut avertissements = Vec::new();
    let recette = recette_nommee(c, nom, &mut avertissements)?;
    // En silence : un gestionnaire n'a pas d'installeur à ouvrir à la main (comme la CLI).
    let options = OptionsInstallation {
        interactif: false,
        tester: true,
    };
    let rapport = installer_recette(c, &recette, fichier, options, p)?;
    let installe = GestionnaireInstalle {
        nom: recette.nom,
        installe: apres_installer(c, rapport, &mut avertissements),
    };
    Ok((installe, avertissements))
}

/// Écrit dans `etat.toml` le verdict de chaque plugin de `plugins`.
pub fn memoriser(c: &Chemins, plugins: &PluginsTraites, avertissements: &mut Avertissements) {
    if plugins.is_empty() {
        return; // rien à mémoriser : inutile de réécrire le fichier
    }
    let (mut etat, probleme) = Etat::charger(&c.donnees);
    if let Some(texte) = probleme {
        avertir(avertissements, texte);
    }
    for (plugin, etat_pontage, test) in plugins {
        let (statut, detail) = brut_depuis_installation(etat_pontage, test.as_ref());
        etat.noter(&plugin.chemin, statut, &detail);
    }
    if let Err(erreur) = etat.enregistrer(&c.donnees) {
        avertir(avertissements, format!("{} {erreur}", tr("Error:")));
    }
}

/// Réparation prête à lancer : les problèmes à réparer et les programmes ouverts qu'elle
/// fermerait (à nommer dans une confirmation, spec 4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReparationPrevue {
    /// Problèmes à réparer (vide : seuls les réglages des gestionnaires seront réappliqués).
    pub problemes: Vec<Probleme>,
    /// Programmes ouverts que la réparation fermerait (elle arrête Wine) ; vide : pas de
    /// question.
    pub programmes: Vec<String>,
}

/// Prépare une réparation : diagnostique si `problemes` est absent, puis cherche les
/// programmes ouverts à nommer avant de réparer (même décision que la CLI :
/// [`programmes_a_nommer_avant_reparation`]).
///
/// Les recettes invalides sont ignorées ici sans avertissement : [`reparer`], qui suit,
/// recharge le catalogue et les signale.
///
/// # Erreurs
/// Celles de `diagnostiquer` et de la lecture de `/proc`.
pub fn preparer_reparation(
    c: &Chemins,
    problemes: Option<Vec<Probleme>>,
) -> Resultat<ReparationPrevue> {
    let problemes = match problemes {
        Some(liste) => liste,
        None => diagnostiquer(c)?,
    };
    let (catalogue, _) = Catalogue::charger(&dossiers_recettes(c));
    let programmes = programmes_a_nommer_avant_reparation(c, &catalogue, &problemes)?;
    Ok(ReparationPrevue {
        problemes,
        programmes,
    })
}

/// Répare `problemes` (rien si la liste est vide), puis réapplique **toujours** les réglages
/// des gestionnaires installés et redémarre leurs services (même après une réparation
/// partielle, des réglages ont pu se perdre), comme `wimao-cli reparer`. Les recettes
/// invalides sont rendues en avertissements, comme partout ailleurs.
///
/// # Erreurs
/// La première de `reparer` (noyau) puis de [`reparer_gestionnaires`].
pub fn reparer(
    c: &Chemins,
    problemes: &[Probleme],
    p: &dyn Progression,
) -> Resultat<Avertissements> {
    let mut avertissements = Vec::new();
    let reparation = reparer_probleme(c, problemes, p);
    let catalogue = charger_catalogue(c, &mut avertissements);
    let reglages = reparer_gestionnaires(c, &catalogue, p);
    reparation?;
    reglages?;
    Ok(avertissements)
}

/// Sauvegarde l'environnement dans `dossier` et rend le fichier `.sha256` à garder.
///
/// # Erreurs
/// Celles de `wimao_core::sauvegarde::sauvegarder` (disque plein, plugin ouvert…).
pub fn sauvegarder(c: &Chemins, dossier: &Path, p: &dyn Progression) -> Resultat<PathBuf> {
    sauvegarder_dans(c, dossier, p)
}

/// Restaure la sauvegarde décrite par `sha256` (l'environnement actuel est gardé en copie).
///
/// # Erreurs
/// Celles de `wimao_core::sauvegarde::restaurer` (empreinte fausse, plugin ouvert…).
pub fn restaurer(c: &Chemins, sha256: &Path, p: &dyn Progression) -> Resultat<()> {
    restaurer_sauvegarde(c, sha256, p)
}

/// Annule l'import de `~/.wine-mao` : l'environnement y retourne.
///
/// # Erreurs
/// Celles de `wimao_core::import::annuler` (plus notre lien, plugin ouvert…).
pub fn annuler_import(c: &Chemins, p: &dyn Progression) -> Resultat<()> {
    annuler_l_import(c, p)
}

/// Le rapport anonymisé à coller dans un signalement.
///
/// # Erreurs
/// Celles de `wimao_core::rapport::rapport`.
pub fn rapport(c: &Chemins) -> Resultat<String> {
    construire_rapport(c)
}

/// Bilan d'une installation pour le toast de fin : « 1 plugin installé », « 2 plugins
/// installés, 1 plugin mis à jour, 1 avec un problème »… `None` si le rapport ne contient
/// aucun plugin (la fenêtre choisit alors son propre message).
pub fn bilan_installation(rapport: &RapportInstallation) -> Option<String> {
    let morceaux = morceaux_du_bilan(&rapport.plugins, &rapport.mis_a_jour);
    (!morceaux.is_empty()).then(|| morceaux.join(", "))
}

/// Bilan d'une session de gestionnaire pour le toast de fin : celui d'une installation
/// ([`bilan_installation`]), suivi du nombre d'installeurs que le pilote a relancés en
/// silencieux (« 1 plugin installé, 2 installeurs lancés en silencieux »). Un installeur
/// fermé sans relance ou de type inconnu ne compte pas. `None` si la session n'a rien fait.
pub fn bilan_session(bilan: &BilanSession) -> Option<String> {
    let mut morceaux = morceaux_du_bilan(&bilan.plugins, &bilan.mis_a_jour);
    let relances = bilan
        .relances
        .iter()
        .filter(|r| matches!(r, IssueRelance::Relance { .. }))
        .count() as u64;
    if relances > 0 {
        morceaux.push(trn(
            "{n} installer run silently",
            "{n} installers run silently",
            relances,
            &[],
        ));
    }
    (!morceaux.is_empty()).then(|| morceaux.join(", "))
}

/// Morceaux communs aux bilans : plugins nouveaux, mis à jour, à vérifier, avec un problème.
///
/// Les verdicts suivent le tableau ([`brut_depuis_installation`]) : un plugin ⚠️ est « à
/// vérifier », un plugin ❌ est « avec un problème » ; un ✅ n'ajoute rien. Chaque morceau est
/// traduit avec son pluriel
/// (`trn`) ; l'appelant les joint par une virgule.
fn morceaux_du_bilan(plugins: &PluginsTraites, mis_a_jour: &BTreeSet<PathBuf>) -> Vec<String> {
    let maj = plugins
        .iter()
        .filter(|(plugin, _, _)| mis_a_jour.contains(&plugin.chemin))
        .count() as u64;
    let nouveaux = plugins.len() as u64 - maj;
    // Un seul verdict par plugin : ⚠️ « à vérifier », ❌ « avec un problème ».
    let nombre_de = |voulu: Statut| {
        plugins
            .iter()
            .filter(|(_, etat, test)| brut_depuis_installation(etat, test.as_ref()).0 == voulu)
            .count() as u64
    };
    let a_verifier = nombre_de(Statut::Avertissement);
    let problemes = nombre_de(Statut::Echec);
    let mut morceaux = Vec::new();
    if nouveaux > 0 {
        morceaux.push(trn(
            "{n} plugin installed",
            "{n} plugins installed",
            nouveaux,
            &[],
        ));
    }
    if maj > 0 {
        morceaux.push(trn("{n} plugin updated", "{n} plugins updated", maj, &[]));
    }
    if a_verifier > 0 {
        morceaux.push(trn("{n} to check", "{n} to check", a_verifier, &[]));
    }
    if problemes > 0 {
        morceaux.push(trn(
            "{n} with a problem",
            "{n} with a problem",
            problemes,
            &[],
        ));
    }
    morceaux
}

/// Question posée quand l'installation silencieuse n'a ajouté ni mis à jour aucun plugin.
/// Cite le code de sortie de l'installeur quand il est non nul : il explique souvent l'échec
/// (MiniGrand, en WiX Burn, rend 67 : son contenu passe par l'inMusic Software Center).
pub fn message_aucun_plugin(code_installeur: Option<i32>) -> String {
    match code_installeur.filter(|code| *code != 0) {
        Some(code) => trf(
            "The silent installation did not add or update a plugin (installer exit code {code}). Open the installer normally?",
            &[("code", &code.to_string())],
        ),
        None => tr(
            "The silent installation did not add or update a plugin. Open the installer normally?",
        ),
    }
}

// --- Historique et retour en arrière ------------------------------------------------------

/// Ce que la page Historique affiche : les entrées (la plus récente d'abord) et les points de
/// restauration, pour savoir à quelles entrées proposer « Go back to before this ».
pub fn charger_historique(c: &Chemins) -> Resultat<(Vec<Entree>, ListePoints)> {
    Ok((historique::lire(c)?, restauration::lister(c)?))
}

/// Un retour en arrière prêt à être confirmé : le point et la liste exacte de ce qui serait
/// supprimé.
pub struct ApercuRetour {
    /// Point de restauration visé.
    pub point: PointRestauration,
    /// Fichiers et liens que le retour supprimerait (`restauration::apercu_retour`).
    pub fichiers: Vec<PathBuf>,
}

/// Prépare le retour au point `id` : retrouve le point **utilisable** (jamais un point périmé
/// ou incomplet) et calcule ce qui serait supprimé. Ne modifie rien.
///
/// Erreurs : [`Erreur::Introuvable`] si le point a disparu entre l'affichage de la page et le
/// clic (supprimé comme trop ancien, ou devenu périmé) ; erreurs de `lister` et `apercu_retour`.
pub fn apercu_du_retour(c: &Chemins, id: &str) -> Resultat<ApercuRetour> {
    let point = restauration::lister(c)?
        .points
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| Erreur::Introuvable(id.to_owned()))?;
    let fichiers = restauration::apercu_retour(c, &point)?;
    Ok(ApercuRetour { point, fichiers })
}

/// Fait le retour confirmé (`restauration::revenir`, qui écrit lui-même l'entrée d'historique).
pub fn retour_en_arriere(c: &Chemins, apercu: &ApercuRetour) -> Resultat<BilanRetour> {
    restauration::revenir(c, &apercu.point, &apercu.fichiers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wimao_core::Format;
    use wimao_recettes::TypeInstalleur;

    fn plugin(nom: &str) -> PluginWindows {
        PluginWindows {
            chemin: PathBuf::from(format!("/p/{nom}.vst3")),
            format: Format::Vst3,
            nom: nom.to_owned(),
        }
    }

    fn rapport(
        plugins: Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>,
        mis_a_jour: &[&str],
    ) -> RapportInstallation {
        RapportInstallation {
            point: None,
            recette: None,
            methode: TypeInstalleur::Inno,
            plugins,
            mis_a_jour: mis_a_jour
                .iter()
                .map(|n| plugin(n).chemin)
                .collect::<BTreeSet<_>>(),
            interactif_necessaire: false,
            code_installeur: Some(0),
            gestionnaire: false,
            lien_a_declarer: None,
            consigne_activation: None,
        }
    }

    // La langue n'est pas fixée ici : sans catalogue chargé, les textes sont en anglais.
    // `serial` : d'autres tests du processus pourraient changer la langue en même temps.
    #[test]
    #[serial_test::serial]
    fn bilan_avec_pluriels_mises_a_jour_et_problemes() {
        wimao_core::i18n::init_langue(Some("en"));
        assert_eq!(bilan_installation(&rapport(Vec::new(), &[])), None);
        let un = rapport(vec![(plugin("A"), EtatPontage::Ponte, None)], &[]);
        assert_eq!(bilan_installation(&un).unwrap(), "1 plugin installed");
        let melange = rapport(
            vec![
                (plugin("A"), EtatPontage::Ponte, None),
                (plugin("B"), EtatPontage::Ponte, None),
                (
                    plugin("C"),
                    EtatPontage::Ponte,
                    Some(ResultatTest::Probleme("x".into())),
                ),
            ],
            &["C"],
        );
        assert_eq!(
            bilan_installation(&melange).unwrap(),
            "2 plugins installed, 1 plugin updated, 1 with a problem"
        );
        wimao_core::i18n::init_langue(Some("fr"));
        assert_eq!(
            bilan_installation(&melange).unwrap(),
            "2 plugins installés, 1 plugin mis à jour, 1 avec un problème"
        );
        wimao_core::i18n::init_langue(Some("en"));
    }

    // Un ⚠️ est « à vérifier », seul un ❌ est « avec un problème ».
    #[test]
    #[serial_test::serial]
    fn bilan_distingue_a_verifier_et_probleme() {
        wimao_core::i18n::init_langue(Some("en"));
        let doublon = (plugin("A"), EtatPontage::Doublon, None);
        let echec = (
            plugin("B"),
            EtatPontage::Ponte,
            Some(ResultatTest::Probleme("x".into())),
        );
        let les_deux = rapport(vec![doublon.clone(), echec], &[]);
        assert_eq!(
            bilan_installation(&les_deux).unwrap(),
            "2 plugins installed, 1 to check, 1 with a problem"
        );
        wimao_core::i18n::init_langue(Some("fr"));
        assert_eq!(
            bilan_installation(&les_deux).unwrap(),
            "2 plugins installés, 1 à vérifier, 1 avec un problème"
        );
        wimao_core::i18n::init_langue(Some("es"));
        assert_eq!(
            bilan_installation(&les_deux).unwrap(),
            "2 plugins instalados, 1 por revisar, 1 con un problema"
        );
        wimao_core::i18n::init_langue(Some("fr"));
        // Un ⚠️ seul : aucun « avec un problème ».
        let seul = rapport(vec![doublon], &[]);
        assert_eq!(
            bilan_installation(&seul).unwrap(),
            "1 plugin installé, 1 à vérifier"
        );
        wimao_core::i18n::init_langue(Some("en"));
        assert_eq!(
            bilan_installation(&seul).unwrap(),
            "1 plugin installed, 1 to check"
        );
    }

    fn session(
        plugins: Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>,
        relances: Vec<IssueRelance>,
    ) -> BilanSession {
        BilanSession {
            point: wimao_core::restauration::PointRestauration {
                id: "2026-11-01-10h00m00".into(),
                motif: "session".into(),
                dossier: PathBuf::from("/d/restauration/1"),
            },
            plugins,
            mis_a_jour: BTreeSet::new(),
            relances,
            avertissements: Vec::new(),
        }
    }

    fn relance(nom: &str) -> IssueRelance {
        IssueRelance::Relance {
            nom: nom.into(),
            type_: TypeInstalleur::Inno,
            code: Some(0),
        }
    }

    #[test]
    #[serial_test::serial]
    fn bilan_de_session_avec_relances() {
        wimao_core::i18n::init_langue(Some("en"));
        assert_eq!(bilan_session(&session(Vec::new(), Vec::new())), None);
        // Un installeur « inconnu » laissé tel quel n'est pas une relance.
        let bilan = session(
            vec![(plugin("A"), EtatPontage::Ponte, None)],
            vec![
                relance("Jura.exe"),
                relance("Kontakt.exe"),
                IssueRelance::Inconnu {
                    nom: "Autre.exe".into(),
                },
            ],
        );
        assert_eq!(
            bilan_session(&bilan).unwrap(),
            "1 plugin installed, 2 installers run silently"
        );
        let seulement_relance = session(Vec::new(), vec![relance("Jura.exe")]);
        assert_eq!(
            bilan_session(&seulement_relance).unwrap(),
            "1 installer run silently"
        );
        wimao_core::i18n::init_langue(Some("fr"));
        assert_eq!(
            bilan_session(&bilan).unwrap(),
            "1 plugin installé, 2 installeurs lancés en silencieux"
        );
        wimao_core::i18n::init_langue(Some("en"));
    }

    /// Écrit une recette de gestionnaire `nom` dont le programme est `C:\<nom>\<nom>.exe`.
    fn recette_gestionnaire(dossier: &Path, nom: &str, editeur: &str) {
        std::fs::write(
            dossier.join(format!("{nom}.toml")),
            format!(
                "nom = \"{nom}\"\nediteur = \"{editeur}\"\ntype = \"gestionnaire\"\n\
                 reconnaitre = [\"{nom}*.exe\"]\n\
                 [gestionnaire]\nprogramme = 'C:\\{nom}\\{nom}.exe'\n"
            ),
        )
        .unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn lignes_gestionnaires_etat_installe() {
        wimao_core::i18n::init_langue(Some("en"));
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let recettes = racine.path().join("recettes");
        std::fs::create_dir_all(&recettes).unwrap();
        recette_gestionnaire(&recettes, "Zeta", "");
        recette_gestionnaire(&recettes, "Alpha", "Alpha Inc.");
        // Une recette d'installeur n'est pas un gestionnaire : pas de ligne.
        std::fs::write(
            recettes.join("plugin.toml"),
            "nom = \"Plugin\"\nreconnaitre = [\"Plugin*.exe\"]\n",
        )
        .unwrap();
        let programme = c.drive_c().join("Alpha/Alpha.exe");
        std::fs::create_dir_all(programme.parent().unwrap()).unwrap();
        std::fs::write(&programme, "MZ").unwrap();
        let (catalogue, erreurs) = Catalogue::charger(&[recettes]);
        assert!(erreurs.is_empty(), "{erreurs:?}");

        let lignes = lignes_gestionnaires(&c, &catalogue);
        let resume: Vec<(&str, bool)> = lignes
            .iter()
            .map(|l| (l.nom.as_str(), l.installe))
            .collect();
        assert_eq!(resume, [("Alpha", true), ("Zeta", false)]);
        assert_eq!(lignes[0].sous_titre(false), "Alpha Inc. · Installed");
        assert_eq!(lignes[0].sous_titre(true), "Alpha Inc. · In use");
        // Sans éditeur, l'état seul.
        assert_eq!(lignes[1].sous_titre(false), "Not installed");
        wimao_core::i18n::init_langue(Some("fr"));
        assert_eq!(lignes[0].sous_titre(false), "Alpha Inc. · Installé");
        wimao_core::i18n::init_langue(Some("en"));
    }

    #[test]
    #[serial_test::serial]
    fn message_aucun_plugin_cite_le_code() {
        wimao_core::i18n::init_langue(Some("en"));
        let avec_code = message_aucun_plugin(Some(67));
        assert!(avec_code.contains("67"), "{avec_code}");
        assert!(
            avec_code.contains("Open the installer normally?"),
            "{avec_code}"
        );
        // Code nul ou absent : rien à citer.
        for code in [Some(0), None] {
            let texte = message_aucun_plugin(code);
            assert!(!texte.contains("code"), "{texte}");
            assert!(texte.contains("did not add or update a plugin"), "{texte}");
        }
        wimao_core::i18n::init_langue(Some("fr"));
        assert!(message_aucun_plugin(Some(67)).contains("67"));
        wimao_core::i18n::init_langue(Some("en"));
    }

    #[test]
    fn historique_vide_dans_un_dossier_neuf() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let (entrees, points) = charger_historique(&c).unwrap();
        assert!(entrees.is_empty());
        assert!(points.points.is_empty() && points.perimes.is_empty());
    }

    #[test]
    fn retour_vers_un_point_inconnu_est_introuvable() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let erreur = apercu_du_retour(&c, "2026-01-01-00h00m00").err().unwrap();
        assert!(matches!(erreur, Erreur::Introuvable(id) if id == "2026-01-01-00h00m00"));
    }
}
