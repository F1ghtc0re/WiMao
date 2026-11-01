//! Test d'intégration de bout en bout d'une session de gestionnaire d'éditeur, avec un vrai
//! Wine : installation par recette, ouverture, pilote (relance en silencieux d'un installeur
//! ouvert avec sa fenêtre), pontage du plugin installé, fermeture, et relais d'un lien.
//!
//! Le gestionnaire est un faux : `tests/fixtures/gestionnaire/FauxGestionnaire.exe` (voir son
//! `.c`), livré dans `faux-gestionnaire-setup.exe` (fabriqué par `tests/fabriquer-fixtures.sh`)
//! et décrit par la recette `tests/recettes-gestionnaire/faux-gestionnaire.toml`. À
//! l'ouverture, il lance `test-nsis.exe` sans option silencieuse ; le pilote doit le fermer
//! et le relancer en silencieux, ce qui installe le faux plugin `WiMaoTest.vst3`.
//!
//! Utilise : `wimao_recettes` (`installer_recette`, `ouvrir`, `Session`, `relayer_lien`),
//! `wimao_core` (chemins de test, processus, Wine). Lancé à la main ou par le job
//! `integration` de la CI : `cargo test -- --ignored`, avec `WIMAO_FIXTURES`. Voir
//! `CONTRIBUTING.md`, section « Tests d'intégration avec Wine ».
//!
//! Tout ce que WiMao écrit va dans un dossier temporaire ([`Chemins::pour_test`], HOME et XDG
//! de test pour Wine) : ni le dossier personnel, ni `~/.wine-mao`, ni `~/.config/yabridgectl`,
//! ni les associations de liens du bureau ne sont touchés. `relayer_lien` ne déclare aucun
//! lien au bureau : il lance seulement le programme du gestionnaire dans Wine.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use wimao_core::pontage::EtatPontage;
use wimao_core::processus::processus_du_prefixe;
use wimao_core::{lancer, Chemins, ProgressionMuette, Wine};
use wimao_recettes::{
    charger, installer_recette, ouvrir, relayer_lien, utilisateur_windows, Catalogue, IssueRelance,
    OptionsInstallation, Recette, TypeInstalleur,
};

/// Variable qui donne le dossier des installeurs factices (sortie de `fabriquer-fixtures.sh`).
const VARIABLE_FIXTURES: &str = "WIMAO_FIXTURES";
/// Installeur du faux gestionnaire, dans le dossier des fixtures.
const INSTALLEUR_GESTIONNAIRE: &str = "faux-gestionnaire-setup.exe";
/// Installeur NSIS que le faux gestionnaire lance (il installe `WiMaoTest.vst3`).
const INSTALLEUR_NSIS: &str = "test-nsis.exe";
/// Recette du faux gestionnaire, relative au dossier de la crate.
const RECETTE: &str = "tests/recettes-gestionnaire/faux-gestionnaire.toml";
/// Dossier, sous `Downloads`, où le test dépose l'installeur : `{telechargements}\FauxEditeur`
/// de la recette (surveillé par le pilote).
const DOSSIER_TELECHARGEMENTS: &str = "FauxEditeur";
/// Nom du faux plugin installé par `test-nsis.exe`.
const NOM_PLUGIN: &str = "WiMaoTest";
/// Dossier du faux plugin ponté, sous `~/.vst3/yabridge` du HOME de test.
const BUNDLE_PLUGIN: &str = "WiMaoTest.vst3";
/// Intervalle entre deux `tick` : celui du pilote ([`wimao_recettes::INTERVALLE_PILOTE`]).
const INTERVALLE_TICK: Duration = Duration::from_secs(2);
/// Durée maximale de la session : le pilote relance en quelques secondes, le faux
/// gestionnaire quitte 10 s après ; 5 minutes laissent de la marge à une machine lente.
const DUREE_MAX_SESSION: Duration = Duration::from_secs(5 * 60);
/// Lien relayé par le test de relais, et fichier où le faux gestionnaire le consigne.
const LIEN: &str = "wimao-test://ok?x=1";
/// Fichier des liens reçus par le faux gestionnaire, relatif à `drive_c`.
const FICHIER_LIENS: &str = "faux-gestionnaire-liens.txt";
/// Fichier écrit par le faux gestionnaire quand il quitte normalement, relatif à `drive_c`.
const FICHIER_FIN: &str = "faux-gestionnaire-fin.txt";
/// Attente maximale du lien relayé (démarrage de Wine compris).
const DUREE_MAX_LIEN: Duration = Duration::from_secs(60);
/// Intervalle entre deux lectures du fichier des liens.
const INTERVALLE_LECTURE_LIEN: Duration = Duration::from_millis(500);
/// Délai de `wineserver -w` à la fin : tous les processus du préfixe doivent s'être terminés.
const DELAI_FIN_WINE: Duration = Duration::from_secs(120);

/// Un seul test Wine à la fois : deux environnements Wine en parallèle doublent la durée de
/// chaque étape, et le cache partagé ([`cache_commun`]) n'est pas fait pour deux écrivains.
static UN_A_LA_FOIS: Mutex<()> = Mutex::new(());

/// Arrête le `wineserver` du préfixe à la fin du test, même si une assertion échoue :
/// sinon un Wine orphelin resterait lancé sur le dossier temporaire supprimé.
struct ArretWine<'a>(&'a Chemins);

impl Drop for ArretWine<'_> {
    fn drop(&mut self) {
        let _ = Wine::nouveau(self.0).arreter();
    }
}

/// Dossier des fixtures, ou `None` (avec un message clair) quand la variable est absente :
/// le test se saute alors au lieu de réussir en silence. Variable définie mais fixture
/// manquante : le test échoue, jamais de faux succès.
fn dossier_fixtures() -> Option<PathBuf> {
    let Some(dossier) = std::env::var_os(VARIABLE_FIXTURES).map(PathBuf::from) else {
        eprintln!(
            "SAUTÉ : {VARIABLE_FIXTURES} n'est pas définie. Fabriquez les installeurs avec \
             tests/fabriquer-fixtures.sh <dossier> puis relancez avec {VARIABLE_FIXTURES}=<dossier>."
        );
        return None;
    };
    for nom in [INSTALLEUR_GESTIONNAIRE, INSTALLEUR_NSIS] {
        assert!(
            dossier.join(nom).is_file(),
            "{VARIABLE_FIXTURES}={} : le fichier {nom} manque (relancez tests/fabriquer-fixtures.sh)",
            dossier.display()
        );
    }
    Some(dossier)
}

/// Cache commun aux tests (Mono, DXVK et msvcp140 téléchargés une seule fois) : le dossier
/// temporaire que cargo réserve aux tests d'intégration (`target/tmp`).
fn cache_commun() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cache-gestionnaire")
}

/// Recette du faux gestionnaire.
fn recette() -> Recette {
    let fichier = Path::new(env!("CARGO_MANIFEST_DIR")).join(RECETTE);
    charger(&fichier).unwrap_or_else(|e| panic!("recette {RECETTE} illisible : {e}"))
}

/// Chemins de test sous `racine`, avec le cache commun.
fn chemins_de_test(racine: &Path) -> Chemins {
    let mut c = Chemins::pour_test(racine);
    c.cache = cache_commun();
    c
}

/// Installe le faux gestionnaire par sa recette (méthode `extraire-7z`, sans Wine pour
/// l'extraction ; l'environnement Wine est créé au passage).
fn installer_le_gestionnaire(c: &Chemins, r: &Recette, fixtures: &Path) {
    let options = OptionsInstallation {
        interactif: false,
        tester: false,
    };
    let fichier = fixtures.join(INSTALLEUR_GESTIONNAIRE);
    let rapport = installer_recette(c, r, Some(&fichier), options, &ProgressionMuette)
        .unwrap_or_else(|e| panic!("installation du faux gestionnaire échouée : {e}"));
    assert!(
        rapport.gestionnaire,
        "la recette doit être vue comme un gestionnaire"
    );
    assert_eq!(rapport.lien_a_declarer.as_deref(), Some("wimao-test"));
}

/// Dépose `test-nsis.exe` dans `{telechargements}\FauxEditeur`, là où le faux gestionnaire
/// le cherche (argument de la recette) et où le pilote surveille.
fn deposer_l_installeur(c: &Chemins, fixtures: &Path) {
    let dossier = c
        .drive_c()
        .join("users")
        .join(utilisateur_windows(c))
        .join("Downloads")
        .join(DOSSIER_TELECHARGEMENTS);
    std::fs::create_dir_all(&dossier).expect("dossier de téléchargement");
    std::fs::copy(
        fixtures.join(INSTALLEUR_NSIS),
        dossier.join(INSTALLEUR_NSIS),
    )
    .expect("copie de test-nsis.exe");
}

/// Attend que tous les processus du préfixe soient terminés (`wineserver -w`), puis vérifie
/// qu'aucun ne reste.
fn verifier_aucun_processus(c: &Chemins) {
    let mut attente = Wine::nouveau(c)
        .commande("wineserver")
        .expect("commande wineserver");
    attente.arg("-w");
    lancer(&mut attente, DELAI_FIN_WINE).expect("wineserver -w");
    let restes = processus_du_prefixe(c).expect("lecture de /proc");
    assert!(restes.is_empty(), "processus restants : {restes:?}");
}

#[test]
#[ignore = "Wine requis"]
fn session_pilote_et_ponte() {
    let Some(fixtures) = dossier_fixtures() else {
        return;
    };
    let _seul = UN_A_LA_FOIS.lock().unwrap_or_else(|e| e.into_inner());
    let racine = tempfile::tempdir().expect("dossier temporaire");
    let c = chemins_de_test(racine.path());
    let _arret = ArretWine(&c);
    let r = recette();
    installer_le_gestionnaire(&c, &r, &fixtures);
    deposer_l_installeur(&c, &fixtures);

    let mut session = ouvrir(&c, &r, &ProgressionMuette).expect("ouverture de la session");
    let debut = Instant::now();
    loop {
        for evenement in session
            .tick(Instant::now(), &ProgressionMuette)
            .expect("tick")
        {
            eprintln!("{:>4} s : {evenement:?}", debut.elapsed().as_secs());
        }
        // Fin de session : gestionnaire fermé ET aucun installeur en cours. Ici
        // `installeurs_en_cours()` reste vide : `test-nsis.exe` ne contient ni « setup », ni
        // « install », ni « msiexec ». C'est sans conséquence : la relance du pilote est
        // synchrone dans `tick` (elle est finie quand `tick` rend la main), et le faux
        // gestionnaire ne quitte que 10 s après la fin de l'installeur qu'il a lancé.
        let ferme = session.gestionnaire_ferme().expect("état du gestionnaire");
        let en_cours = session
            .installeurs_en_cours()
            .expect("installeurs en cours");
        if ferme && en_cours.is_empty() {
            break;
        }
        assert!(
            debut.elapsed() < DUREE_MAX_SESSION,
            "session encore ouverte après {DUREE_MAX_SESSION:?} (installeurs : {en_cours:?})"
        );
        std::thread::sleep(INTERVALLE_TICK);
    }
    eprintln!("gestionnaire fermé après {} s", debut.elapsed().as_secs());
    let bilan = session
        .fermer(true, &ProgressionMuette)
        .expect("fermeture de la session");

    let relance_nsis = bilan.relances.iter().any(|issue| {
        matches!(
            issue,
            IssueRelance::Relance {
                type_: TypeInstalleur::Nsis,
                code: Some(0),
                ..
            }
        )
    });
    assert!(relance_nsis, "relances : {:?}", bilan.relances);
    // Le faux gestionnaire écrit ce fichier juste avant de quitter normalement : sans lui,
    // il a été fermé de force (par exemple par le pilote, avec son installeur).
    assert!(
        c.drive_c().join(FICHIER_FIN).is_file(),
        "le faux gestionnaire n'a pas fini normalement ({FICHIER_FIN} absent)"
    );
    let ponte = bilan
        .plugins
        .iter()
        .any(|(plugin, etat, _)| plugin.nom == NOM_PLUGIN && *etat == EtatPontage::Ponte);
    assert!(ponte, "plugins : {:?}", bilan.plugins);
    assert!(
        c.home.join(".vst3/yabridge").join(BUNDLE_PLUGIN).exists(),
        "le plugin n'est pas ponté dans ~/.vst3/yabridge du HOME de test"
    );
    verifier_aucun_processus(&c);
}

#[test]
#[ignore = "Wine requis"]
fn relais_transmet_l_adresse() {
    let Some(fixtures) = dossier_fixtures() else {
        return;
    };
    let _seul = UN_A_LA_FOIS.lock().unwrap_or_else(|e| e.into_inner());
    let racine = tempfile::tempdir().expect("dossier temporaire");
    let c = chemins_de_test(racine.path());
    let _arret = ArretWine(&c);
    let r = recette();
    installer_le_gestionnaire(&c, &r, &fixtures);
    let catalogue = Catalogue(vec![r]);

    relayer_lien(&c, &catalogue, LIEN).expect("relais du lien");
    let fichier = c.drive_c().join(FICHIER_LIENS);
    let debut = Instant::now();
    loop {
        let contenu = std::fs::read_to_string(&fichier).unwrap_or_default();
        if contenu.lines().any(|ligne| ligne == LIEN) {
            break;
        }
        assert!(
            debut.elapsed() < DUREE_MAX_LIEN,
            "lien absent de {} après {DUREE_MAX_LIEN:?} : {contenu:?}",
            fichier.display()
        );
        std::thread::sleep(INTERVALLE_LECTURE_LIEN);
    }
}
