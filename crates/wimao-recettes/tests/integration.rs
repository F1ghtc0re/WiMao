//! Test d'intégration de bout en bout : les trois types d'installeurs courants (NSIS, MSI,
//! Inno Setup) passent par l'orchestrateur [`wimao_recettes::installer`] avec un vrai Wine.
//!
//! Utilise les installeurs factices fabriqués par `tests/fabriquer-fixtures.sh` (ils
//! installent un faux plugin `WiMaoTest.vst3`), le vrai Wine et le vrai yabridge
//! (`WIMAO_WINE_DIR` / `WIMAO_YABRIDGE_DIR`, sinon leurs emplacements habituels), et
//! `wimao_core::restauration` pour vérifier le retour en arrière.
//! Lancé à la main ou par le job `integration` de la CI : `cargo test -- --ignored`.
//! Voir `CONTRIBUTING.md`, section « Tests d'intégration avec Wine ».
//!
//! Tout ce que WiMao écrit va dans un dossier temporaire ([`Chemins::pour_test`]) : ni le
//! dossier personnel, ni `~/.wine-mao`, ni `~/.config/yabridgectl` ne sont touchés.

use std::path::{Path, PathBuf};

use wimao_core::historique::{self, Action, ResultatAction};
use wimao_core::pontage::EtatPontage;
use wimao_core::restauration;
use wimao_core::{Chemins, ProgressionMuette, Wine};
use wimao_recettes::{installer, Catalogue, OptionsInstallation};

/// Variable qui donne le dossier des installeurs factices (sortie de `fabriquer-fixtures.sh`).
const VARIABLE_FIXTURES: &str = "WIMAO_FIXTURES";
/// Les installeurs factices attendus dans ce dossier : un par type d'installeur.
const INSTALLEURS: [&str; 3] = ["test-nsis.exe", "test.msi", "test-inno.exe"];
/// Variable qui, à `1`, retire `test-inno.exe` de la liste (même valeur que pour
/// `fabriquer-fixtures.sh`) : utilisée par la CI, où l'installeur d'Inno Setup échoue.
const VARIABLE_SANS_INNO: &str = "WIMAO_FIXTURES_SANS_INNO";
/// Nom du faux plugin installé par chacun d'eux.
const NOM_PLUGIN: &str = "WiMaoTest";
/// Dossier du faux plugin, tel qu'il doit apparaître dans le préfixe (et dans `.vst3/yabridge`).
const BUNDLE_PLUGIN: &str = "WiMaoTest.vst3";

/// Arrête le `wineserver` du préfixe à la fin du test, même si une assertion échoue :
/// sinon un Wine orphelin resterait lancé sur le dossier temporaire supprimé.
struct ArretWine<'a>(&'a Chemins);

impl Drop for ArretWine<'_> {
    fn drop(&mut self) {
        let _ = Wine::nouveau(self.0).arreter();
    }
}

/// Installeurs à tester : tous, sauf `test-inno.exe` si `WIMAO_FIXTURES_SANS_INNO=1`
/// (CI). Le retrait est annoncé, pour qu'un résultat vert ne trompe personne.
fn installeurs_attendus() -> Vec<&'static str> {
    let sans_inno = std::env::var(VARIABLE_SANS_INNO).is_ok_and(|v| v == "1");
    if sans_inno {
        eprintln!("test-inno.exe ignoré : {VARIABLE_SANS_INNO}=1");
    }
    INSTALLEURS
        .into_iter()
        .filter(|nom| !(sans_inno && *nom == "test-inno.exe"))
        .collect()
}

/// Cherche récursivement un dossier ou fichier nommé `nom` sous `racine` (sans suivre les
/// liens symboliques). Rend la liste de ce qui a été trouvé.
fn trouver(racine: &Path, nom: &str) -> Vec<PathBuf> {
    walkdir::WalkDir::new(racine)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == nom)
        .map(|e| e.into_path())
        .collect()
}

/// Dossier des fixtures, ou `None` (avec un message clair) quand la variable est absente :
/// le test se saute alors au lieu de réussir en silence. Variable définie mais fixture
/// manquante : le test échoue (panique), jamais de faux succès.
fn dossier_fixtures() -> Option<PathBuf> {
    let Some(dossier) = std::env::var_os(VARIABLE_FIXTURES).map(PathBuf::from) else {
        eprintln!(
            "SAUTÉ : {VARIABLE_FIXTURES} n'est pas définie. Fabriquez les installeurs avec \
             tests/fabriquer-fixtures.sh <dossier> puis relancez avec {VARIABLE_FIXTURES}=<dossier>."
        );
        return None;
    };
    for nom in installeurs_attendus() {
        assert!(
            dossier.join(nom).is_file(),
            "{VARIABLE_FIXTURES}={} : le fichier {nom} manque (relancez tests/fabriquer-fixtures.sh)",
            dossier.display()
        );
    }
    Some(dossier)
}

/// Installe `fichier` dans un environnement neuf, vérifie le résultat, puis revient en arrière.
fn verifier_un_installeur(fichier: &Path, cache_commun: &Path) {
    let nom = fichier.file_name().unwrap_or_default().to_string_lossy();
    let racine = tempfile::tempdir().expect("dossier temporaire");
    let mut c = Chemins::pour_test(racine.path());
    // Cache partagé : Mono, DXVK et msvcp140 ne sont téléchargés qu'une fois pour les trois.
    c.cache = cache_commun.to_path_buf();
    let _arret = ArretWine(&c);

    let options = OptionsInstallation {
        interactif: false,
        tester: false,
    };
    let rapport = installer(
        &c,
        fichier,
        &Catalogue::default(),
        options,
        &ProgressionMuette,
    )
    .unwrap_or_else(|e| panic!("{nom} : installation échouée : {e}"));

    // Un seul plugin nouveau, ponté.
    assert!(!rapport.interactif_necessaire, "{nom} : rien installé");
    assert_eq!(rapport.plugins.len(), 1, "{nom} : {:?}", rapport.plugins);
    let (plugin, etat, _) = &rapport.plugins[0];
    assert_eq!(plugin.nom, NOM_PLUGIN, "{nom}");
    assert_eq!(*etat, EtatPontage::Ponte, "{nom}");
    assert!(
        c.home.join(".vst3/yabridge").join(BUNDLE_PLUGIN).exists(),
        "{nom} : le plugin n'est pas ponté dans ~/.vst3/yabridge"
    );

    // Retour en arrière : plus aucun WiMaoTest.vst3 dans le préfixe.
    let point = rapport.point.expect("point de restauration créé");
    let listee = restauration::lister(&c).expect("liste des points");
    assert!(listee.points.iter().any(|p| p.id == point.id), "{nom}");
    let apercu = restauration::apercu_retour(&c, &point).expect("aperçu du retour");
    restauration::revenir(&c, &point, &apercu)
        .unwrap_or_else(|e| panic!("{nom} : retour en arrière échoué : {e}"));
    let restes = trouver(&c.drive_c(), BUNDLE_PLUGIN);
    assert!(
        restes.is_empty(),
        "{nom} : restes après retour : {restes:?}"
    );
}

#[test]
#[ignore = "Wine requis"]
fn installer_les_trois_types() {
    let Some(fixtures) = dossier_fixtures() else {
        return;
    };
    let cache = tempfile::tempdir().expect("dossier de cache");
    for nom in installeurs_attendus() {
        verifier_un_installeur(&fixtures.join(nom), cache.path());
    }
}

/// Une installation réussie laisse une entrée `Installation` `Reussi` dans l'historique, dont
/// le point est celui que `restauration::lister` rend en premier (le plus récent).
#[test]
#[ignore = "Wine requis"]
fn installation_ajoute_une_entree_avec_point() {
    let Some(fixtures) = dossier_fixtures() else {
        return;
    };
    let nom = installeurs_attendus()[0];
    let racine = tempfile::tempdir().expect("dossier temporaire");
    let c = Chemins::pour_test(racine.path());
    let _arret = ArretWine(&c);
    let options = OptionsInstallation {
        interactif: false,
        tester: false,
    };
    installer(
        &c,
        &fixtures.join(nom),
        &Catalogue::default(),
        options,
        &ProgressionMuette,
    )
    .unwrap_or_else(|e| panic!("{nom} : installation échouée : {e}"));

    let entrees = historique::lire(&c).expect("historique lisible");
    let entree = entrees
        .iter()
        .find(|e| e.action == Action::Installation)
        .unwrap_or_else(|| panic!("{nom} : aucune entrée Installation : {entrees:?}"));
    assert_eq!(entree.resultat, ResultatAction::Reussi, "{entree:?}");
    // Sans recette, le sujet est le nom du fichier installé.
    assert_eq!(entree.sujet, nom, "{entree:?}");
    let dernier = restauration::lister(&c)
        .expect("liste des points")
        .points
        .into_iter()
        .next()
        .expect("un point de restauration");
    assert_eq!(entree.point.as_deref(), Some(dernier.id.as_str()));
}

/// Le parcours « historique » de bout en bout : on installe l'installeur factice, on lit
/// l'historique, on prend le `point` de l'entrée `Installation` (c'est ce que fait la page
/// « Historique » de l'interface quand on clique sur « Revenir »), puis on revient à ce point.
/// Le faux plugin disparaît du préfixe et l'historique gagne une entrée `Retour` réussie.
#[test]
#[ignore = "Wine requis"]
fn retour_depuis_l_historique() {
    let Some(fixtures) = dossier_fixtures() else {
        return;
    };
    let nom = installeurs_attendus()[0];
    let racine = tempfile::tempdir().expect("dossier temporaire");
    let c = Chemins::pour_test(racine.path());
    let _arret = ArretWine(&c);
    let options = OptionsInstallation {
        interactif: false,
        tester: false,
    };
    installer(
        &c,
        &fixtures.join(nom),
        &Catalogue::default(),
        options,
        &ProgressionMuette,
    )
    .unwrap_or_else(|e| panic!("{nom} : installation échouée : {e}"));
    assert!(
        !trouver(&c.drive_c(), BUNDLE_PLUGIN).is_empty(),
        "{nom} : le faux plugin doit exister avant le retour"
    );

    // L'identifiant de point vient de l'historique, comme dans l'interface.
    let entrees = historique::lire(&c).expect("historique lisible");
    let id_point = entrees
        .iter()
        .find(|e| e.action == Action::Installation)
        .and_then(|e| e.point.clone())
        .unwrap_or_else(|| panic!("{nom} : aucune entrée Installation avec point : {entrees:?}"));
    let point = restauration::lister(&c)
        .expect("liste des points")
        .points
        .into_iter()
        .find(|p| p.id == id_point)
        .unwrap_or_else(|| panic!("{nom} : le point {id_point} de l'historique n'existe pas"));

    let apercu = restauration::apercu_retour(&c, &point).expect("aperçu du retour");
    restauration::revenir(&c, &point, &apercu)
        .unwrap_or_else(|e| panic!("{nom} : retour échoué : {e}"));

    let restes = trouver(&c.drive_c(), BUNDLE_PLUGIN);
    assert!(
        restes.is_empty(),
        "{nom} : restes après retour : {restes:?}"
    );
    let apres = historique::lire(&c).expect("historique lisible");
    let retour = apres
        .iter()
        .find(|e| e.action == Action::Retour)
        .unwrap_or_else(|| panic!("{nom} : aucune entrée Retour : {apres:?}"));
    assert_eq!(retour.resultat, ResultatAction::Reussi, "{retour:?}");
}
