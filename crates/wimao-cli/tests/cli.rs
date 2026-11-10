//! Tests de bout en bout de `wimao-cli` : on lance le vrai programme (`assert_cmd`).
//!
//! Chaque test reçoit un `HOME` et un `WIMAO_DONNEES` temporaires : le programme ne
//! peut ainsi jamais toucher aux vraies données de l'utilisateur (`~/.wine-mao`,
//! `~/.local/share/wimao`…). Aucun test ne lance Wine.

use assert_cmd::Command;
use std::path::PathBuf;
use tempfile::TempDir;

/// Échantillon d'installeur Inno Setup fourni avec les tests de `wimao-recettes`.
fn echantillon(nom: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../wimao-recettes/tests/echantillons")
        .join(nom)
}

/// Nom d'utilisateur fixe des tests : il apparaît dans `HOME` et `USER`, pour vérifier
/// que le rapport anonymisé ne le contient pas.
const UTILISATEUR_TEST: &str = "utilisateur-secret";

/// Une commande `wimao-cli` isolée : `HOME` et données dans `racine`.
fn wimao(racine: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("wimao-cli").expect("binaire wimao-cli compilé");
    cmd.env("HOME", racine.path().join("home").join(UTILISATEUR_TEST))
        .env("USER", UTILISATEUR_TEST)
        .env("WIMAO_DONNEES", racine.path().join("donnees"));
    // Rien de ce que la machine de développement définit ne doit influencer le programme.
    for variable in [
        "WIMAO_LOCALEDIR",
        "WIMAO_WINE_DIR",
        "WIMAO_YABRIDGE_DIR",
        "WIMAO_RECETTES",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_CACHE_HOME",
        "XDG_RUNTIME_DIR",
    ] {
        cmd.env_remove(variable);
    }
    cmd
}

fn sortie_texte(octets: &[u8]) -> String {
    String::from_utf8_lossy(octets).into_owned()
}

#[test]
fn detecter_reconnait_inno_setup() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "detecter"])
        .arg(echantillon("inno.bin"))
        .assert()
        .success();
    assert!(sortie_texte(&sortie.get_output().stdout).contains("Inno Setup"));
}

#[test]
fn le_rapport_espagnol_ne_contient_pas_le_nom_de_l_utilisateur() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "es", "rapport"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("Distribución"), "pas en espagnol : {texte}");
    assert!(
        !texte.contains(UTILISATEUR_TEST),
        "nom d'utilisateur présent"
    );
}

/// Lance `installer fichier-absent.exe` dans la langue donnée ; vérifie le code 1 et rend stderr.
fn installer_absent(langue: &str) -> String {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", langue, "installer", "fichier-absent.exe"])
        .assert()
        .code(1);
    sortie_texte(&sortie.get_output().stderr)
}

#[test]
fn installer_un_fichier_absent_echoue_en_francais() {
    let erreur = installer_absent("fr");
    assert!(
        erreur.contains("Erreur : introuvable : le fichier fichier-absent.exe n'existe pas"),
        "stderr : {erreur}"
    );
}

#[test]
fn installer_un_fichier_absent_echoue_en_espagnol_sans_mot_francais() {
    let erreur = installer_absent("es");
    assert!(
        erreur.contains("Error: no encontrado: el archivo fichier-absent.exe no existe"),
        "stderr : {erreur}"
    );
    for mot_francais in ["introuvable", "Erreur", "fichier "] {
        assert!(
            !erreur.contains(mot_francais),
            "{mot_francais} dans : {erreur}"
        );
    }
}

#[test]
fn installer_un_fichier_absent_echoue_en_anglais() {
    let erreur = installer_absent("en");
    assert!(
        erreur.contains("Error: not found: the file fichier-absent.exe does not exist"),
        "stderr : {erreur}"
    );
}

#[test]
fn l_aide_est_traduite() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "--help"])
        .assert()
        .success();
    assert!(sortie_texte(&sortie.get_output().stdout).contains("Installe un plugin"));
}

#[test]
fn revenir_a_un_point_inconnu_est_une_erreur() {
    let t = TempDir::new().unwrap();
    // Aucun point de ce nom : erreur (code 1), rien n'est demandé ni modifié.
    wimao(&t)
        .args([
            "--langue",
            "fr",
            "restauration",
            "revenir",
            "2020-01-01-00h00m00",
        ])
        .assert()
        .code(1);
}

#[test]
fn restaurer_une_sauvegarde_absente_echoue_avant_toute_question() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "restaurer"])
        .arg(t.path().join("absent.sha256"))
        .assert()
        .code(1);
    assert!(sortie_texte(&sortie.get_output().stderr).contains("n'existe pas"));
}

#[test]
fn restaurer_annule_sans_confirmation_garde_le_code_0() {
    let t = TempDir::new().unwrap();
    let sha = t.path().join("sauvegarde.sha256");
    std::fs::write(&sha, b"").unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "restaurer"])
        .arg(&sha)
        .write_stdin("n\n")
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("mis de côté"), "{texte}");
    assert!(texte.contains("ancien-<date>"), "{texte}");
    assert!(texte.contains("Annulé"), "{texte}");
}

#[test]
fn verifier_un_prefixe_sans_plugin_ponte_reussit() {
    let t = TempDir::new().unwrap();
    std::fs::create_dir_all(t.path().join("home")).unwrap();
    let prefixe = t.path().join("prefixe-vide");
    std::fs::create_dir_all(&prefixe).unwrap();
    wimao(&t)
        .args(["--langue", "fr", "verifier", "--prefixe"])
        .arg(&prefixe)
        .assert()
        .success();
    // verifier n'écrit rien dans les dossiers de WiMao (pas même le journal).
    assert!(!t.path().join("donnees").exists());
}

/// Identifiant d'un point de restauration fabriqué à la main (forme `AAAA-MM-JJ-HHhMMmSS`).
const ID_POINT: &str = "2026-01-01-10h00m00";

/// Fabrique un préfixe (avec un fichier `drive_c/nouveau.txt` absent du point) et un point
/// de restauration valide, sans lancer Wine : les `.reg` et `fichiers.txt` sont écrits
/// directement, dans le format décrit par `wimao_core::restauration`.
fn preparer_point(t: &TempDir) -> PathBuf {
    let donnees = t.path().join("donnees");
    let prefixe = donnees.join("prefixe");
    std::fs::create_dir_all(prefixe.join("drive_c")).unwrap();
    let nouveau = prefixe.join("drive_c/nouveau.txt");
    std::fs::write(&nouveau, b"x").unwrap();
    ecrire_point(t);
    nouveau
}

/// Écrit seulement le point de restauration `ID_POINT` (voir [`preparer_point`]), sans toucher
/// au préfixe.
fn ecrire_point(t: &TempDir) {
    let point = t.path().join("donnees/restauration").join(ID_POINT);
    std::fs::create_dir_all(&point).unwrap();
    for reg in ["system.reg", "user.reg", "userdef.reg"] {
        std::fs::write(point.join(reg), b"").unwrap();
    }
    std::fs::write(point.join("motif.txt"), "test").unwrap();
    // Chemins séparés par NUL ; « / » final = dossier ; marque de fin avec le nombre d'entrées.
    let liste = b"system.reg\0user.reg\0userdef.reg\0drive_c/\0\x01WIMAO-FIN:4\0";
    std::fs::write(point.join("fichiers.txt"), liste).unwrap();
}

#[test]
fn restauration_lister_montre_les_points() {
    let t = TempDir::new().unwrap();
    preparer_point(&t);
    let sortie = wimao(&t)
        .args(["--langue", "fr", "restauration", "lister"])
        .assert()
        .success();
    assert!(sortie_texte(&sortie.get_output().stdout).contains(ID_POINT));
}

#[test]
fn revenir_montre_l_apercu_et_sans_confirmation_ne_retire_rien() {
    let t = TempDir::new().unwrap();
    let nouveau = preparer_point(&t);
    let sortie = wimao(&t)
        .args(["--langue", "fr", "restauration", "revenir", ID_POINT])
        .write_stdin("\n")
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("nouveau.txt"), "aperçu absent : {texte}");
    assert!(texte.contains("Annulé"), "{texte}");
    assert!(nouveau.exists(), "le fichier ne devait pas être retiré");
}

#[test]
fn revenir_a_un_point_perime_est_refuse_sans_apercu_ni_question() {
    let t = TempDir::new().unwrap();
    let nouveau = preparer_point(&t);
    let point = t.path().join("donnees/restauration").join(ID_POINT);
    std::fs::write(point.join("perime"), b"").unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "restauration", "revenir", ID_POINT])
        .write_stdin("o\n")
        .assert()
        .code(1);
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(!texte.contains("nouveau.txt"), "aperçu affiché : {texte}");
    assert!(!texte.contains("Remettre"), "question posée : {texte}");
    assert!(nouveau.exists());
}

#[test]
fn une_erreur_d_usage_sort_en_code_1_et_l_aide_en_code_0() {
    let t = TempDir::new().unwrap();
    // Sous-commande inconnue et langue refusée : code 1 (le code 2 est réservé aux plugins ❌).
    wimao(&t).arg("n-existe-pas").assert().code(1);
    wimao(&t)
        .args(["--langue", "xx", "rapport"])
        .assert()
        .code(1);
    wimao(&t).arg("--version").assert().code(0);
    wimao(&t).arg("--help").assert().code(0);
}

#[test]
fn une_sortie_fermee_ne_fait_pas_paniquer() {
    use std::process::{Command as Processus, Stdio};
    let t = TempDir::new().unwrap();
    let exe = assert_cmd::cargo::cargo_bin("wimao-cli");
    let mut enfant = Processus::new(exe)
        .args(["--langue", "fr", "rapport"])
        .env("HOME", t.path().join("home"))
        .env("WIMAO_DONNEES", t.path().join("donnees"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Le lecteur ferme le tube sans rien lire : l'écriture du rapport rencontre BrokenPipe.
    drop(enfant.stdout.take());
    let fin = enfant.wait_with_output().unwrap();
    let erreur = sortie_texte(&fin.stderr);
    assert!(!erreur.contains("panicked"), "{erreur}");
    assert_eq!(fin.status.code(), Some(0), "{erreur}");
}

/// Dossier de recettes de test de `wimao-recettes` (une valide, trois invalides).
fn dossier_recettes() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../wimao-recettes/tests/recettes")
}

#[test]
fn recettes_liste_les_recettes_du_dossier() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .env("WIMAO_RECETTES", dossier_recettes())
        .args(["--langue", "fr", "recettes"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("Native Access 2"), "{texte}");
    assert!(texte.contains("gestionnaire"), "{texte}");
    // Gestionnaire absent de l'environnement de test : son état est dit.
    assert!(texte.contains("· Non installé"), "{texte}");
    // Les recettes invalides sont signalées à part, sans empêcher la liste.
    let erreur = sortie_texte(&sortie.get_output().stderr);
    assert!(erreur.contains("recette ignorée"), "{erreur}");
}

#[test]
fn gestionnaire_inconnu_code_1() {
    let t = TempDir::new().unwrap();
    for sous_commande in ["installer", "ouvrir"] {
        let sortie = wimao(&t)
            .env("WIMAO_RECETTES", dossier_recettes())
            .args(["--langue", "fr", "gestionnaire", sous_commande, "inconnu"])
            .assert()
            .code(1);
        let erreur = sortie_texte(&sortie.get_output().stderr);
        assert!(erreur.contains("une recette nommée inconnu"), "{erreur}");
    }
}

#[test]
fn gestionnaire_se_trouve_par_le_nom_de_son_fichier() {
    let t = TempDir::new().unwrap();
    // « valide » est le fichier valide.toml : la recette est trouvée, donc l'erreur vient du
    // `--fichier` inexistant (message précis), pas de « recette introuvable ».
    let sortie = wimao(&t)
        .env("WIMAO_RECETTES", dossier_recettes())
        .args(["--langue", "fr", "gestionnaire", "installer", "VALIDE"])
        .arg("--fichier")
        .arg(t.path().join("absent.exe"))
        .assert()
        .code(1);
    let erreur = sortie_texte(&sortie.get_output().stderr);
    assert!(!erreur.contains("une recette nommée"), "{erreur}");
    assert!(erreur.contains("absent.exe"), "{erreur}");
}

#[test]
fn relayer_lien_schema_inconnu_code_1() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .env("WIMAO_RECETTES", dossier_recettes())
        .args(["--langue", "fr", "relayer-lien", "inconnu://x"])
        .assert()
        .code(1);
    assert!(sortie.get_output().stdout.is_empty());
}

#[test]
fn aide_de_gestionnaire_liste_installer_et_ouvrir() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "gestionnaire", "--help"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("installer"), "{texte}");
    assert!(texte.contains("ouvrir"), "{texte}");
}

#[test]
fn historique_vide_puis_rempli() {
    let t = TempDir::new().unwrap();
    // Sans fichier : code 0 et le message traduit.
    let sortie = wimao(&t)
        .args(["--langue", "fr", "historique"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("Aucune action pour le moment"), "{texte}");

    // Deux lignes écrites à la main, dans l'ordre chronologique du fichier.
    let donnees = t.path().join("donnees");
    std::fs::create_dir_all(&donnees).unwrap();
    let lignes = concat!(
        r#"{"date":"2026-11-01T10:00:00+01:00","action":"installation","sujet":"PremierPlugin","resultat":"reussi","point":"2026-11-01-10h00m00"}"#,
        "\n",
        r#"{"date":"2026-11-01T11:00:00+01:00","action":"sauvegarde","sujet":"SecondeAction","resultat":"echec"}"#,
        "\n"
    );
    std::fs::write(donnees.join("historique.jsonl"), lignes).unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "historique"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    let (premier, second) = (
        texte.find("PremierPlugin").expect("premier sujet"),
        texte.find("SecondeAction").expect("second sujet"),
    );
    assert!(second < premier, "le plus récent d'abord : {texte}");
    assert!(
        texte.contains("2026-11-01-10h00m00"),
        "point absent : {texte}"
    );
    assert!(
        texte.contains("Installation") && texte.contains("Sauvegarde"),
        "{texte}"
    );
    assert!(
        texte.contains("Réussi") && texte.contains("Échec"),
        "{texte}"
    );
}

#[test]
fn historique_limite_a_vingt_entrees_sauf_avec_tout() {
    let t = TempDir::new().unwrap();
    let donnees = t.path().join("donnees");
    std::fs::create_dir_all(&donnees).unwrap();
    let mut lignes = String::new();
    for i in 0..25 {
        lignes.push_str(&format!(
            "{{\"date\":\"2026-11-01T10:00:{i:02}+01:00\",\"action\":\"retour\",\"sujet\":\"sujet-{i:02}-fin\",\"resultat\":\"reussi\"}}\n"
        ));
    }
    std::fs::write(donnees.join("historique.jsonl"), lignes).unwrap();
    let compter = |args: &[&str]| {
        let sortie = wimao(&t).args(args).assert().success();
        sortie_texte(&sortie.get_output().stdout)
            .matches("-fin")
            .count()
    };
    assert_eq!(compter(&["historique"]), 20);
    assert_eq!(compter(&["historique", "--tout"]), 25);
}

// ---------------------------------------------------------------------------------------
// importer
// ---------------------------------------------------------------------------------------

/// Écrit un script shell exécutable (faux outil) dans `chemin`, dossiers parents créés.
fn ecrire_faux_outil(chemin: &std::path::Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
    std::fs::write(chemin, script).unwrap();
    std::fs::set_permissions(chemin, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Dossier personnel de test (celui que `wimao()` donne en `HOME`).
fn home_de_test(racine: &TempDir) -> PathBuf {
    racine.path().join("home").join(UTILISATEUR_TEST)
}

/// Faux `<home>/.wine-mao` (registre et `drive_c/`) plus de faux `wineserver` et `yabridgectl`
/// qui réussissent sans rien faire, sous `racine`. Rend les deux variables d'environnement
/// (`WIMAO_WINE_DIR`, `WIMAO_YABRIDGE_DIR`) à poser sur la commande : ainsi le vrai Wine n'est
/// jamais lancé contre un faux environnement et rien ne touche au vrai dossier personnel.
fn faux_environnement_existant(racine: &TempDir) -> [(&'static str, PathBuf); 2] {
    let ancien = home_de_test(racine).join(".wine-mao");
    std::fs::create_dir_all(ancien.join("drive_c")).unwrap();
    for reg in ["system.reg", "user.reg", "userdef.reg"] {
        std::fs::write(ancien.join(reg), "contenu").unwrap();
    }
    let wine = racine.path().join("faux-wine");
    ecrire_faux_outil(&wine.join("bin/wineserver"), "#!/bin/sh\nexit 0\n");
    let yabridge = racine.path().join("faux-yabridge");
    ecrire_faux_outil(&yabridge.join("yabridgectl"), "#!/bin/sh\nexit 0\n");
    [("WIMAO_WINE_DIR", wine), ("WIMAO_YABRIDGE_DIR", yabridge)]
}

/// Vrai si `chemin` est un lien symbolique.
fn est_un_lien(chemin: &std::path::Path) -> bool {
    chemin
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

#[test]
fn importer_sans_environnement_existant() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "importer", "--oui"])
        .assert()
        .code(1);
    let erreur = sortie_texte(&sortie.get_output().stderr);
    assert!(
        erreur.contains("introuvable : environnement Wine dans"),
        "stderr : {erreur}"
    );
}

#[test]
fn importer_oui_puis_annuler_oui() {
    let t = TempDir::new().unwrap();
    let outils = faux_environnement_existant(&t);
    let ancien = home_de_test(&t).join(".wine-mao");

    let mut import = wimao(&t);
    import.args(["--langue", "fr", "importer", "--oui"]);
    for (nom, valeur) in &outils {
        import.env(nom, valeur);
    }
    let sortie = import.assert().success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("Import terminé"), "stdout : {texte}");
    assert!(est_un_lien(&ancien), "le lien doit exister");
    assert!(t.path().join("donnees/prefixe/system.reg").is_file());

    let mut annulation = wimao(&t);
    annulation.args(["--langue", "fr", "importer", "--annuler", "--oui"]);
    for (nom, valeur) in &outils {
        annulation.env(nom, valeur);
    }
    let sortie = annulation.assert().success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("Import annulé"), "stdout : {texte}");
    assert!(
        !est_un_lien(&ancien) && ancien.is_dir(),
        "dossier de retour"
    );
    assert!(ancien.join("system.reg").is_file());
}

/// Relecture finale I2, de bout en bout : un point créé sur l'environnement importé est
/// « périmé » après l'annulation (il décrit l'environnement rendu à `~/.wine-mao`).
#[test]
fn annuler_l_import_perime_les_points() {
    let t = TempDir::new().unwrap();
    let outils = faux_environnement_existant(&t);
    let lancer = |arguments: &[&str]| {
        let mut cmd = wimao(&t);
        cmd.args(arguments);
        for (nom, valeur) in &outils {
            cmd.env(nom, valeur);
        }
        let sortie = cmd.assert().success();
        sortie_texte(&sortie.get_output().stdout)
    };
    lancer(&["--langue", "fr", "importer", "--oui"]);
    // Un point « d'installation » sur l'environnement importé, écrit à la main (pas de Wine).
    ecrire_point(&t);
    let avant = lancer(&["--langue", "en", "restauration", "lister"]);
    assert!(
        avant.contains(ID_POINT) && !avant.contains("outdated"),
        "{avant}"
    );
    lancer(&["--langue", "fr", "importer", "--annuler", "--oui"]);
    let apres = lancer(&["--langue", "en", "restauration", "lister"]);
    assert!(
        apres
            .lines()
            .any(|l| l.contains(ID_POINT) && l.contains("outdated")),
        "{apres}"
    );
}

#[test]
fn importer_refus_ne_change_rien() {
    let t = TempDir::new().unwrap();
    let outils = faux_environnement_existant(&t);
    let ancien = home_de_test(&t).join(".wine-mao");
    let mut cmd = wimao(&t);
    cmd.args(["--langue", "fr", "importer"]).write_stdin("n\n");
    for (nom, valeur) in &outils {
        cmd.env(nom, valeur);
    }
    let sortie = cmd.assert().success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    // L'aperçu dit ce qui serait fait, puis le refus est annoncé.
    assert!(texte.contains("Un lien sera laissé"), "stdout : {texte}");
    assert!(texte.contains("rien n'a été modifié"), "stdout : {texte}");
    assert!(!est_un_lien(&ancien) && ancien.is_dir());
    assert!(!t.path().join("donnees/prefixe").exists());
}

#[test]
fn annuler_sans_import_est_une_erreur() {
    let t = TempDir::new().unwrap();
    wimao(&t)
        .args(["--langue", "fr", "importer", "--annuler", "--oui"])
        .assert()
        .code(1);
}

/// Écrit `applications.toml` avec une application dont le programme existe dans le préfixe
/// isolé du test (`<donnees>/prefixe/drive_c`).
fn declarer_application(racine: &TempDir, nom: &str) {
    let donnees = racine.path().join("donnees");
    let programme = donnees.join("prefixe/drive_c/Program Files/App/app.exe");
    std::fs::create_dir_all(programme.parent().unwrap()).unwrap();
    std::fs::write(&programme, b"MZ").unwrap();
    std::fs::write(
        donnees.join("applications.toml"),
        format!(
            "[[application]]\nnom = \"{nom}\"\nprogramme = 'C:\\Program Files\\App\\app.exe'\n\
             raccourci = \"x.lnk\"\najoutee = \"2026-01-01\"\n"
        ),
    )
    .unwrap();
}

#[test]
fn application_lister_vide_puis_avec_une_application() {
    let t = TempDir::new().unwrap();
    let sortie = wimao(&t)
        .args(["--langue", "fr", "application", "lister"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("Aucune application ajoutée"), "{texte}");

    declarer_application(&t, "Mon Appli");
    let sortie = wimao(&t)
        .args(["--langue", "fr", "application", "lister"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(
        texte.contains("Mon Appli — C:\\Program Files\\App\\app.exe"),
        "{texte}"
    );
}

#[test]
fn application_inconnue_code_1() {
    let t = TempDir::new().unwrap();
    for sous_commande in ["ouvrir", "retirer"] {
        let sortie = wimao(&t)
            .args(["--langue", "fr", "application", sous_commande, "inconnue"])
            .assert()
            .code(1);
        let erreur = sortie_texte(&sortie.get_output().stderr);
        assert!(
            erreur.contains("une application nommée inconnue"),
            "{erreur}"
        );
    }
}

#[test]
fn application_retirer_vide_la_liste_sans_rien_desinstaller() {
    let t = TempDir::new().unwrap();
    declarer_application(&t, "Mon Appli");
    let sortie = wimao(&t)
        .args(["--langue", "fr", "application", "retirer", "Mon Appli"])
        .assert()
        .success();
    let texte = sortie_texte(&sortie.get_output().stdout);
    assert!(texte.contains("rien n'a été désinstallé"), "{texte}");
    // Le programme est toujours là, mais la liste est vide.
    assert!(t
        .path()
        .join("donnees/prefixe/drive_c/Program Files/App/app.exe")
        .exists());
    let sortie = wimao(&t)
        .args(["--langue", "fr", "application", "lister"])
        .assert()
        .success();
    assert!(sortie_texte(&sortie.get_output().stdout).contains("Aucune application"));
}
