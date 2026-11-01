//! Test d'ouverture d'un plugin ponté, sur un écran virtuel (Xvfb).
//!
//! Outils requis : `carla-single` (Carla), `Xvfb` et `xwininfo` (paquets `xvfb` et
//! `x11-utils`). S'il en manque un, le résultat est une `Remarque` qui dit lequel
//! installer. Il n'y a volontairement aucun repli sur l'écran réel : `xwininfo` y
//! verrait les fenêtres « (GUI) » d'autres logiciels (faux `Pret`) et le plugin
//! s'ouvrirait devant l'utilisateur.
//!
//! Charge le plugin dans `carla-single`, comme le fait `scripts/tester-plugins.sh`
//! (comportement éprouvé à la main), et cherche une fenêtre « (GUI) » : si elle
//! apparaît, l'interface du plugin s'ouvre. Tout se passe sur Xvfb, donc aucune
//! fenêtre n'apparaît sur l'écran de l'utilisateur.
//!
//! Utilise : [`Chemins`] (Wine, yabridge, préfixe), [`Format`], [`lancer`], [`tr`].
//! Utilisé par : `wimao-recettes::installation` (test des plugins installés) et la commande
//! `verifier` de `wimao-cli`.
//!
//! `carla-single` est un script Python : on ne le tue jamais par son nom mais par
//! groupe de processus (`process_group(0)` puis `kill(-pgid)`), ce qui emporte aussi
//! l'hôte yabridge et ses enfants. Xvfb est arrêté dans tous les cas (garde `Groupe`).

use crate::chemins::Chemins;
use crate::i18n::{tr, trf};
use crate::inventaire::Format;
use crate::outils::lancer;
use crate::wine::preparer_environnement;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Délai maximal avant de renoncer à voir la fenêtre du plugin.
const DELAI_TOTAL: Duration = Duration::from_secs(300);
/// Intervalle entre deux recherches de la fenêtre.
const INTERVALLE: Duration = Duration::from_secs(3);
/// Attente après l'apparition de la fenêtre : un plugin qui plante juste après
/// s'être affiché ne doit pas être déclaré prêt trop vite.
const ATTENTE_APRES_FENETRE: Duration = Duration::from_secs(10);
/// Premier numéro d'écran essayé : loin des écrans réels (:0, :1…).
const PREMIER_ECRAN: u32 = 90;
/// Dernier numéro d'écran essayé (au-delà, on renonce à Xvfb).
const DERNIER_ECRAN: u32 = 199;
/// Temps laissé à Xvfb pour créer son socket.
const DELAI_XVFB: Duration = Duration::from_secs(5);
/// Temps laissé à un processus pour finir après un signal poli, avant SIGKILL.
const DELAI_ARRET: Duration = Duration::from_secs(2);
/// Échecs consécutifs de `xwininfo` après lesquels l'écran virtuel est déclaré muet
/// (inutile d'attendre 300 s si `xwininfo` ne peut pas lire l'écran).
const ECHECS_XWININFO_MAX: u32 = 3;
/// Pause avant la seconde vérification de Xvfb : un serveur qui meurt juste après avoir
/// créé son socket (par exemple parce qu'un autre l'occupe) est ainsi détecté.
const PAUSE_VERIF_XVFB: Duration = Duration::from_millis(300);
/// Délai maximal d'un appel à `xwininfo`.
const DELAI_XWININFO: Duration = Duration::from_secs(10);
/// Motifs du journal de Carla qui signalent un plantage (repris de `tester-plugins.sh`).
const MOTIFS_PLANTAGE: &[&str] = &["aborting", "stack overflow", "Unhandled exception"];
/// Message de Carla quand il ne peut pas démarrer son moteur audio (aucun serveur JACK ou
/// PipeWire joignable) ; relevé à la vérification réelle du sous-projet 3, dans un
/// environnement isolé dont le `XDG_RUNTIME_DIR` ne contenait pas le socket de PipeWire.
const MOTIF_SANS_AUDIO: &str = "Failed to init engine";
/// Fin du titre de la fenêtre d'interface d'un plugin dans la sortie de `xwininfo`.
const MARQUE_FENETRE: &str = "(GUI)\"";
/// Géométrie de l'écran virtuel.
const GEOMETRIE_ECRAN: &str = "1920x1080x24";

/// Issue du test d'ouverture d'un plugin. Les textes sont déjà traduits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultatTest {
    /// L'interface du plugin s'est ouverte et le plugin est resté vivant.
    Pret,
    /// Le test n'a pas pu conclure (outil absent, format non testé, interface non vérifiée).
    Remarque(String),
    /// Le plugin ou son hôte a planté ou s'est arrêté ; le texte donne la cause.
    Probleme(String),
}

/// Dossier des sockets X11, où Xvfb crée `X<n>` quand il est prêt.
const DOSSIER_SOCKETS_X11: &str = "/tmp/.X11-unix";

/// Réglages de durée du test ; la version de production est [`Delais::standard`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct Delais {
    /// Durée maximale de l'attente de la fenêtre.
    pub total: Duration,
    /// Intervalle entre deux recherches.
    pub intervalle: Duration,
    /// Attente entre l'apparition de la fenêtre et le verdict.
    pub apres_fenetre: Duration,
}

impl Delais {
    /// Les délais réels : 300 s, toutes les 3 s, puis 10 s.
    pub(crate) fn standard() -> Delais {
        Delais {
            total: DELAI_TOTAL,
            intervalle: INTERVALLE,
            apres_fenetre: ATTENTE_APRES_FENETRE,
        }
    }
}

/// Tout ce que les tests doivent pouvoir remplacer, pour ne dépendre ni de l'environnement
/// de la machine ni de `/tmp/.X11-unix`.
pub(crate) struct Reglages {
    /// Durées d'attente.
    pub delais: Delais,
    /// Dossier d'outils cherché avant tous les autres (faux outils des tests).
    pub outils_prioritaires: Option<PathBuf>,
    /// `PATH` de base ; `None` = celui du processus.
    pub path_base: Option<OsString>,
    /// Dossier où Xvfb crée son socket.
    pub dossier_sockets: PathBuf,
}

impl Reglages {
    /// Réglages de production.
    pub(crate) fn standard() -> Reglages {
        Reglages {
            delais: Delais::standard(),
            outils_prioritaires: None,
            path_base: None,
            dossier_sockets: PathBuf::from(DOSSIER_SOCKETS_X11),
        }
    }
}

/// Teste l'ouverture de `bundle_linux` (le plugin ponté, par exemple sous
/// `~/.vst3/yabridge/`) dans Carla, sur un écran virtuel. N'écrit rien d'autre que
/// des fichiers temporaires (journal de Carla).
pub fn tester_plugin(c: &Chemins, bundle_linux: &Path, format: Format) -> ResultatTest {
    tester_plugin_avec_delai(c, bundle_linux, format, &Reglages::standard())
}

/// Cœur de [`tester_plugin`], avec les délais et un dossier d'outils prioritaire
/// (pour y placer de faux `carla-single` et `xwininfo` dans les tests, sans toucher
/// au `PATH` global du processus).
pub(crate) fn tester_plugin_avec_delai(
    c: &Chemins,
    bundle_linux: &Path,
    format: Format,
    r: &Reglages,
) -> ResultatTest {
    let sous_commande = match format {
        Format::Vst3 => "vst3",
        Format::Vst2 => "vst",
        Format::Clap => return ResultatTest::Remarque(tr("CLAP plugins are not tested yet")),
    };
    let path = construire_path(c, r);
    let Some(carla) = trouver_programme("carla-single", &path) else {
        return ResultatTest::Remarque(tr("Install Carla to test plugins"));
    };
    if trouver_programme("xwininfo", &path).is_none() {
        return ResultatTest::Remarque(tr("Install x11-utils to test plugins"));
    }
    // Gardée jusqu'à la fin de la fonction : Xvfb est arrêté quoi qu'il arrive.
    let (_xvfb, ecran) = match demarrer_ecran(&path, &r.dossier_sockets) {
        Ok(r) => r,
        Err(texte) => return ResultatTest::Remarque(texte),
    };
    let journal = chemin_journal();
    let resultat = lancer_et_observer(
        c,
        &carla,
        sous_commande,
        bundle_linux,
        &path,
        &ecran,
        &journal,
        r.delais,
    );
    let _ = std::fs::remove_file(&journal);
    resultat
}

/// Lance Carla et surveille son journal et l'écran jusqu'au verdict.
#[allow(clippy::too_many_arguments)]
fn lancer_et_observer(
    c: &Chemins,
    carla: &Path,
    sous_commande: &str,
    bundle: &Path,
    path: &OsString,
    ecran: &str,
    journal: &Path,
    delais: Delais,
) -> ResultatTest {
    let Ok(fichier) = std::fs::File::create(journal) else {
        return ResultatTest::Remarque(tr("The plugin host could not be started"));
    };
    let Ok(fichier_erreur) = fichier.try_clone() else {
        return ResultatTest::Remarque(tr("The plugin host could not be started"));
    };
    let mut cmd = Command::new(carla);
    cmd.args([sous_commande]).arg(bundle);
    appliquer_environnement(&mut cmd, c, path, ecran);
    cmd.stdin(Stdio::null())
        .stdout(fichier)
        .stderr(fichier_erreur)
        .process_group(0);
    let Ok(enfant) = cmd.spawn() else {
        return ResultatTest::Remarque(tr("The plugin host could not be started"));
    };
    // SIGKILL d'emblée : Carla n'a rien à sauvegarder, et ses enfants Wine non plus.
    let mut groupe = Groupe::nouveau(enfant, libc::SIGKILL);
    surveiller(&mut groupe, path, ecran, journal, delais)
}

/// Boucle de surveillance : fenêtre, plantage dans le journal, ou arrêt de l'hôte.
fn surveiller(
    groupe: &mut Groupe,
    path: &OsString,
    ecran: &str,
    journal: &Path,
    delais: Delais,
) -> ResultatTest {
    let echeance = Instant::now() + delais.total;
    let mut echecs = 0;
    loop {
        let mut derniere_erreur = String::new();
        match observer_ecran(path, ecran) {
            Observation::Fenetre => {
                std::thread::sleep(delais.apres_fenetre);
                return verdict_apres_fenetre(groupe, journal);
            }
            Observation::Aucune => echecs = 0,
            Observation::Echec(e) => {
                echecs += 1;
                derniere_erreur = e;
            }
        }
        if let Some(ligne) = ligne_de_plantage(journal) {
            return ResultatTest::Probleme(ligne);
        }
        if groupe.est_termine() {
            return verdict_hote_arrete(journal);
        }
        if echecs >= ECHECS_XWININFO_MAX {
            return ResultatTest::Remarque(trf(
                "The virtual screen does not respond: {error}",
                &[("error", &derniere_erreur)],
            ));
        }
        if Instant::now() >= echeance {
            return ResultatTest::Remarque(tr("Loaded, but the interface could not be checked"));
        }
        std::thread::sleep(delais.intervalle);
    }
}

/// Après l'attente suivant l'apparition de la fenêtre : un plantage ou un arrêt
/// pendant ce temps est un problème, sinon le plugin est prêt.
fn verdict_apres_fenetre(groupe: &mut Groupe, journal: &Path) -> ResultatTest {
    if let Some(ligne) = ligne_de_plantage(journal) {
        return ResultatTest::Probleme(ligne);
    }
    if groupe.est_termine() {
        return verdict_hote_arrete(journal);
    }
    ResultatTest::Pret
}

/// Carla s'est arrêté sans motif de plantage connu. S'il n'a pas pu démarrer son moteur
/// audio ([`MOTIF_SANS_AUDIO`] dans son journal), le plugin n'y est pour rien : c'est une
/// `Remarque` (test impossible), pas un `Probleme` qui accuserait le plugin à tort.
fn verdict_hote_arrete(journal: &Path) -> ResultatTest {
    let sans_audio = std::fs::read(journal)
        .map(|t| String::from_utf8_lossy(&t).contains(MOTIF_SANS_AUDIO))
        .unwrap_or(false);
    if sans_audio {
        ResultatTest::Remarque(tr(
            "The audio server (PipeWire or JACK) could not be reached: the plugin was not tested",
        ))
    } else {
        ResultatTest::Probleme(tr("The plugin host stopped"))
    }
}

/// Première ligne du journal qui contient un motif de plantage.
fn ligne_de_plantage(journal: &Path) -> Option<String> {
    let texte = std::fs::read(journal).ok()?;
    let texte = String::from_utf8_lossy(&texte);
    texte
        .lines()
        .find(|l| MOTIFS_PLANTAGE.iter().any(|m| l.contains(m)))
        .map(|l| l.trim().to_string())
}

/// Ce que `xwininfo -root -tree` a permis de constater.
enum Observation {
    /// Une fenêtre dont le titre finit par « (GUI) » existe.
    Fenetre,
    /// L'écran répond mais n'a pas (encore) cette fenêtre.
    Aucune,
    /// `xwininfo` a échoué (code non nul, délai, lancement impossible) ; première ligne de l'erreur.
    Echec(String),
}

/// Interroge l'écran virtuel avec `xwininfo`.
fn observer_ecran(path: &OsString, ecran: &str) -> Observation {
    let Some(xwininfo) = trouver_programme("xwininfo", path) else {
        return Observation::Echec("xwininfo".to_string());
    };
    let mut cmd = Command::new(xwininfo);
    cmd.args(["-root", "-tree"])
        .env("DISPLAY", ecran)
        .env("PATH", path);
    match lancer(&mut cmd, DELAI_XWININFO) {
        Err(e) => Observation::Echec(e.to_string()),
        Ok(s) if s.code != 0 => Observation::Echec(premiere_ligne(&s.texte)),
        Ok(s) if s.texte.lines().any(|l| l.contains(MARQUE_FENETRE)) => Observation::Fenetre,
        Ok(_) => Observation::Aucune,
    }
}

/// Première ligne non vide d'un texte (message d'erreur lisible en une ligne).
fn premiere_ligne(texte: &str) -> String {
    texte
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Variables d'environnement de Carla et de son hôte Wine.
fn appliquer_environnement(cmd: &mut Command, c: &Chemins, path: &OsString, ecran: &str) {
    // Carla lance `yabridge-host`, donc Wine : mêmes variables que nos commandes Wine.
    // `HOME` = `c.home` aussi en usage réel, où c'est le vrai dossier personnel (aucun
    // changement) ; en test il garde Carla, yabridge et Wine dans le dossier temporaire
    // (avec XDG_* et `winemenubuilder.exe=d`). Pas de surcharge Mono/Gecko ici : le
    // plugin testé peut avoir besoin de Mono, installé dans l'environnement.
    preparer_environnement(cmd, c, &[]);
    cmd.env("DISPLAY", ecran)
        .env("PATH", path)
        .env("WINEPREFIX", &c.prefixe)
        .env("WINEDEBUG", "-all")
        // Sans cela Qt (Carla) pourrait choisir Wayland et ouvrir sur l'écran réel.
        .env("QT_QPA_PLATFORM", "xcb")
        .env_remove("WAYLAND_DISPLAY");
}

/// `PATH` du test : outils prioritaires, Wine, yabridge, puis le `PATH` de base.
fn construire_path(c: &Chemins, r: &Reglages) -> OsString {
    let mut dossiers: Vec<PathBuf> = r.outils_prioritaires.clone().into_iter().collect();
    dossiers.push(c.wine_bin());
    dossiers.push(c.yabridge_dir.clone());
    if let Some(actuel) = r.path_base.clone().or_else(|| std::env::var_os("PATH")) {
        dossiers.extend(std::env::split_paths(&actuel));
    }
    std::env::join_paths(dossiers).unwrap_or_default()
}

/// Cherche un exécutable dans les dossiers de `path`.
fn trouver_programme(nom: &str, path: &OsString) -> Option<PathBuf> {
    std::env::split_paths(path)
        .map(|d| d.join(nom))
        .find(|p| p.is_file())
}

/// Chemin d'un journal temporaire unique pour ce test.
fn chemin_journal() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "wimao-test-plugin-{}-{nanos}.log",
        std::process::id()
    ))
}

/// Démarre Xvfb sur le premier écran libre. Renvoie la garde (qui arrête Xvfb) et la
/// valeur de `DISPLAY`. Le texte d'erreur est déjà traduit.
fn demarrer_ecran(path: &OsString, sockets: &Path) -> Result<(Groupe, String), String> {
    let Some(xvfb) = trouver_programme("Xvfb", path) else {
        return Err(tr("Install Xvfb to test plugins"));
    };
    let erreur = || tr("The virtual screen could not be started");
    let numero = premier_ecran_libre(sockets).ok_or_else(erreur)?;
    let ecran = format!(":{numero}");
    let enfant = Command::new(xvfb)
        .args([
            ecran.as_str(),
            "-screen",
            "0",
            GEOMETRIE_ECRAN,
            "-nolisten",
            "tcp",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|_| erreur())?;
    // SIGTERM : Xvfb nettoie son socket et son verrou en s'arrêtant.
    let mut groupe = Groupe::nouveau(enfant, libc::SIGTERM);
    let socket = sockets.join(format!("X{numero}"));
    let limite = Instant::now() + DELAI_XVFB;
    while !socket.exists() {
        if groupe.est_termine() || Instant::now() >= limite {
            return Err(erreur());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Si notre Xvfb est mort entre-temps, le socket est celui d'un autre serveur.
    std::thread::sleep(PAUSE_VERIF_XVFB);
    if groupe.est_termine() {
        return Err(erreur());
    }
    Ok((groupe, ecran))
}

/// Premier numéro d'écran dont le socket et le verrou X n'existent pas.
fn premier_ecran_libre(sockets: &Path) -> Option<u32> {
    (PREMIER_ECRAN..=DERNIER_ECRAN).find(|n| {
        !sockets.join(format!("X{n}")).exists() && !Path::new(&format!("/tmp/.X{n}-lock")).exists()
    })
}

/// Processus chef de groupe, arrêté (avec tout son groupe) quand la garde disparaît.
struct Groupe {
    enfant: Child,
    /// Signal envoyé en premier ; SIGKILL suit si le groupe ne finit pas.
    signal: libc::c_int,
}

impl Groupe {
    fn nouveau(enfant: Child, signal: libc::c_int) -> Groupe {
        Groupe { enfant, signal }
    }

    /// Vrai si le chef du groupe a fini.
    fn est_termine(&mut self) -> bool {
        !matches!(self.enfant.try_wait(), Ok(None))
    }

    fn envoyer(&self, signal: libc::c_int) {
        // `process_group(0)` a donné au groupe l'identifiant du pid de l'enfant.
        let pgid = self.enfant.id() as libc::pid_t;
        // SAFETY: `kill` ne touche pas à la mémoire ; `-pgid` vise le groupe créé
        // par nous, et un échec (groupe déjà fini) est sans conséquence.
        unsafe {
            libc::kill(-pgid, signal);
        }
    }
}

impl Drop for Groupe {
    fn drop(&mut self) {
        self.envoyer(self.signal);
        let limite = Instant::now() + DELAI_ARRET;
        while !self.est_termine() && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(20));
        }
        // Signal poli ignoré ou chef déjà parti avec des enfants : on achève le groupe.
        self.envoyer(libc::SIGKILL);
        let _ = self.enfant.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dossier d'outils factices ; le nom contient `faux-carla` pour que `pgrep -f`
    /// retrouve tout processus lancé depuis ce dossier. Le faux Xvfb crée le socket de
    /// l'écran demandé dans ce même dossier, puis reste vivant jusqu'à son arrêt.
    fn dossier_outils(carla: &str, xwininfo: &str) -> tempfile::TempDir {
        dossier_outils_complet(carla, Some(xwininfo), Some(XVFB_FACTICE))
    }

    /// Faux Xvfb : `$1` est `:<n>` ; le socket est créé à côté du script.
    const XVFB_FACTICE: &str = "touch \"$(dirname \"$0\")/X${1#:}\"; exec /bin/sleep 31339";

    fn dossier_outils_complet(
        carla: &str,
        xwininfo: Option<&str>,
        xvfb: Option<&str>,
    ) -> tempfile::TempDir {
        let d = tempfile::Builder::new()
            .prefix("faux-carla-")
            .tempdir()
            .unwrap();
        for (nom, corps) in [
            ("carla-single", Some(carla)),
            ("xwininfo", xwininfo),
            ("Xvfb", xvfb),
        ] {
            let Some(corps) = corps else { continue };
            let p = d.path().join(nom);
            crate::test_utils::ecrire_executable(
                &p,
                &format!(
                    "#!/bin/sh
{corps}
"
                ),
            );
        }
        d
    }

    fn reglages(outils: &tempfile::TempDir) -> Reglages {
        Reglages {
            delais: Delais {
                total: Duration::from_secs(2),
                intervalle: Duration::from_millis(200),
                apres_fenetre: Duration::from_millis(300),
            },
            outils_prioritaires: Some(outils.path().to_path_buf()),
            // Seuls les faux outils et /bin (sh, sleep) : jamais le vrai Xvfb ni xwininfo.
            path_base: Some(OsString::from("/nonexistent")),
            dossier_sockets: outils.path().to_path_buf(),
        }
    }

    fn tester(outils: &tempfile::TempDir) -> ResultatTest {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let r = tester_plugin_avec_delai(
            &c,
            Path::new("/faux/Plugin.vst3"),
            Format::Vst3,
            &reglages(outils),
        );
        // Plus aucun processus issu des faux outils, Xvfb factice compris.
        let restants = Command::new("pgrep")
            .args(["-f", "faux-carla"])
            .output()
            .unwrap();
        assert!(
            restants.stdout.is_empty(),
            "un faux processus a survécu : {}",
            String::from_utf8_lossy(&restants.stdout)
        );
        r
    }

    const SANS_FENETRE: &str = "echo 'xwininfo: pas de fenetre'";

    #[test]
    fn carla_et_yabridge_host_tournent_dans_le_home_isole() {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let mut cmd = Command::new("carla-single");
        appliquer_environnement(&mut cmd, &c, &OsString::from("/bin"), ":95");
        let env = |nom: &str| {
            cmd.get_envs()
                .find(|(k, _)| *k == nom)
                .and_then(|(_, v)| v.map(|v| v.to_os_string()))
        };
        assert_eq!(env("HOME"), Some(c.home.clone().into()));
        assert_eq!(env("XDG_CONFIG_HOME"), Some(c.home.join(".config").into()));
        assert_eq!(
            env("WINEDLLOVERRIDES"),
            Some("winemenubuilder.exe=d".into())
        );
        assert_eq!(env("DISPLAY"), Some(":95".into()));
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn plantage_dans_le_journal() {
        let o = dossier_outils(
            "echo 'Unhandled exception: boum'; exec /bin/sleep 31338",
            SANS_FENETRE,
        );
        match tester(&o) {
            ResultatTest::Probleme(t) => assert!(t.contains("Unhandled exception"), "{t}"),
            autre => panic!("attendu Probleme, obtenu {autre:?}"),
        }
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn hote_arrete() {
        let o = dossier_outils("exit 0", SANS_FENETRE);
        assert_eq!(
            tester(&o),
            ResultatTest::Probleme(tr("The plugin host stopped"))
        );
    }

    /// Carla s'arrête faute de serveur audio (message réel de Carla, relevé à la vérification
    /// du sous-projet 3) : le plugin n'y est pour rien, c'est une remarque et non un problème.
    #[test]
    #[serial_test::serial(faux_carla)]
    fn serveur_audio_absent_donne_une_remarque() {
        let o = dossier_outils(
            "echo '[carla] Failed to init engine, error was:'; \
             echo 'Failed to init temporary jack client'; exit 1",
            SANS_FENETRE,
        );
        assert_eq!(
            tester(&o),
            ResultatTest::Remarque(tr(
                "The audio server (PipeWire or JACK) could not be reached: the plugin was not tested"
            ))
        );
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn vivant_sans_fenetre_donne_une_remarque() {
        let o = dossier_outils("/bin/sleep 31338 & /bin/sleep 31338", SANS_FENETRE);
        assert_eq!(
            tester(&o),
            ResultatTest::Remarque(tr("Loaded, but the interface could not be checked"))
        );
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn fenetre_gui_donne_pret() {
        let o = dossier_outils(
            "exec /bin/sleep 31338",
            "echo '     0x1 \"Serum 2 (GUI)\": ()  10x10+0+0'",
        );
        assert_eq!(tester(&o), ResultatTest::Pret);
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn xwininfo_en_echec_donne_une_remarque_rapide() {
        let o = dossier_outils(
            "exec /bin/sleep 31338",
            "echo 'xwininfo: unable to open display'; exit 1",
        );
        let debut = Instant::now();
        match tester(&o) {
            ResultatTest::Remarque(t) => assert!(t.contains("unable to open display"), "{t}"),
            autre => panic!("attendu Remarque, obtenu {autre:?}"),
        }
        assert!(debut.elapsed() < Duration::from_millis(1800));
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn xvfb_absent() {
        let o = dossier_outils_complet("exit 0", Some(SANS_FENETRE), None);
        assert_eq!(
            tester(&o),
            ResultatTest::Remarque(tr("Install Xvfb to test plugins"))
        );
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn xwininfo_absent() {
        let o = dossier_outils_complet("exit 0", None, Some(XVFB_FACTICE));
        assert_eq!(
            tester(&o),
            ResultatTest::Remarque(tr("Install x11-utils to test plugins"))
        );
    }

    #[test]
    #[serial_test::serial(faux_carla)]
    fn xvfb_mort_apres_le_socket() {
        let o = dossier_outils_complet(
            "exit 0",
            Some(SANS_FENETRE),
            Some("touch \"$(dirname \"$0\")/X${1#:}\"; exit 0"),
        );
        assert_eq!(
            tester(&o),
            ResultatTest::Remarque(tr("The virtual screen could not be started"))
        );
    }

    #[test]
    fn clap_non_teste_sans_lancer_carla() {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let r = tester_plugin(&c, Path::new("/faux/P.clap"), Format::Clap);
        assert_eq!(
            r,
            ResultatTest::Remarque(tr("CLAP plugins are not tested yet"))
        );
    }
}
