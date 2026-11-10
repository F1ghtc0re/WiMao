//! Logiciels de musique installés en Flatpak : on leur pose le lien yabridge.
//!
//! Utilise [`Chemins`] (le dossier personnel, le yabridge de WiMao), [`crate::pontage`]
//! (`assurer_lien_yabridge_a`, qui pose un lien sans jamais rien remplacer) et [`lancer`]
//! (pour interroger `flatpak`) et [`pour_shell`] (citation des arguments).
//! Utilisé par `pontage::assurer_liens` (pontage, démarrage de l'interface, « Réparer ») et par
//! [`crate::diagnostic::diagnostiquer`] (permissions et lien de chaque logiciel).
//!
//! # Pourquoi
//! Un logiciel en Flatpak est isolé : son `XDG_DATA_HOME` vaut `~/.var/app/<id>/data`, et non
//! `~/.local/share`. Les « chainloaders » que `yabridgectl sync` copie dans `~/.vst3/yabridge`
//! cherchent yabridge dans `$XDG_DATA_HOME/yabridge` : sans lien à cet endroit, le plugin
//! ponté ne se charge pas. On y pose donc le même lien que `~/.local/share/yabridge`, vers
//! `/opt/wimao/yabridge`.
//!
//! WiMao ne lance jamais `flatpak override` : donner à un logiciel l'accès à `/opt/wimao`, aux
//! dossiers de plugins et aux données de WiMao reste le choix de la personne (le diagnostic
//! lui donne la commande). Ici, seul le lien est posé, dans le dossier de données du
//! logiciel lui-même.
//!
//! # Permissions
//! Un logiciel Flatpak ne voit que les dossiers que son manifeste ou un `flatpak override` lui
//! ouvrent. [`systemes_de_fichiers`] lit ses permissions, [`chemins_invisibles`] dit lesquels
//! des dossiers dont WiMao a besoin lui échappent, et [`commande_override`] écrit la commande que
//! la personne peut lancer pour les lui ouvrir (le diagnostic l'affiche, WiMao ne la lance pas).
//!
//! `flatpak` absent ou en échec n'est jamais une erreur : il n'y a simplement aucun logiciel.

use crate::chemins::Chemins;
use crate::erreur::Resultat;
use crate::outils::{lancer, pour_shell};
use crate::pontage::{assurer_lien_yabridge_a, LienYabridge};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

/// Identifiants Flatpak des logiciels de musique connus (ceux qui chargent des plugins).
pub const LOGICIELS_FLATPAK: [&str; 8] = [
    "com.bitwig.BitwigStudio",
    "fm.reaper.Reaper",
    "org.ardour.Ardour",
    "studio.kx.carla",
    "io.lmms.LMMS",
    "org.zrythm.Zrythm",
    "org.rncbc.qtractor",
    "com.renoise.Renoise",
];

/// Délai de `flatpak list` : instantané en temps normal.
const DELAI_FLATPAK: Duration = Duration::from_secs(20);

/// Dossier d'installation de WiMao (Wine, yabridge), lu en lecture seule par les logiciels.
pub const DOSSIER_WIMAO: &str = "/opt/wimao";

/// Dossiers (sous le dossier personnel) où `yabridgectl sync` range les plugins pontés.
const DOSSIERS_PONTES: [&str; 3] = [".vst3/yabridge", ".vst/yabridge", ".clap/yabridge"];

/// Les logiciels de [`LOGICIELS_FLATPAK`] qui sont installés, dans l'ordre de `flatpak list`.
///
/// Tout échec (`flatpak` absent du `PATH`, code de sortie non nul, délai dépassé) donne une
/// liste vide, sans erreur ni message : sur une machine sans Flatpak, il n'y a rien à faire.
pub fn logiciels_installes() -> Vec<String> {
    let mut cmd = Command::new("flatpak");
    cmd.args(["list", "--app", "--columns=application"]);
    match lancer(&mut cmd, DELAI_FLATPAK) {
        Ok(sortie) if sortie.code == 0 => sortie
            .texte
            .lines()
            .map(str::trim)
            .filter(|ligne| LOGICIELS_FLATPAK.contains(ligne))
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Emplacement du lien yabridge d'un logiciel : `~/.var/app/<id>/data/yabridge`.
pub fn emplacement_lien_flatpak(c: &Chemins, id: &str) -> PathBuf {
    c.home.join(".var/app").join(id).join("data/yabridge")
}

/// Ce que le logiciel `id` peut voir : les entrées de la ligne `filesystems=` de
/// `flatpak info --show-permissions <id>` (séparées par `;`, entrées vides retirées).
///
/// `None` si `flatpak` échoue (absent, logiciel inconnu, délai dépassé) : on ne peut rien
/// affirmer, donc le diagnostic ne signale rien. Sans ligne `filesystems=`, le logiciel n'a
/// aucun accès particulier : liste vide.
pub fn systemes_de_fichiers(id: &str) -> Option<Vec<String>> {
    let mut cmd = Command::new("flatpak");
    cmd.args(["info", "--show-permissions", id]);
    let sortie = lancer(&mut cmd, DELAI_FLATPAK)
        .ok()
        .filter(|s| s.code == 0)?;
    let entrees = sortie
        .texte
        .lines()
        .find_map(|ligne| ligne.trim().strip_prefix("filesystems="))
        .map(|valeur| {
            valeur
                .split(';')
                .map(str::trim)
                .filter(|e| !e.is_empty())
                .map(str::to_owned)
                .collect()
        });
    Some(entrees.unwrap_or_default())
}

/// Dossier qu'une entrée de `filesystems` ouvre, ou `None` si elle n'en désigne pas un qu'on
/// sait comparer : `host` et `home` sont traités à part par l'appelant ; les entrées
/// symboliques (`xdg-run/…`, `host-os`…) et les retraits (`!…`) ne comptent pas.
fn dossier_ouvert(entree: &str, home: &Path) -> Option<PathBuf> {
    // Le suffixe de mode (`:ro`, `:rw`, `:create`) ne change pas ce qui est visible.
    let nom = entree.rsplit_once(':').map_or(entree, |(nom, _)| nom);
    if let Some(reste) = nom.strip_prefix("~/") {
        Some(home.join(reste))
    } else if let Some(dossier) = dossier_xdg(nom, home) {
        Some(dossier)
    } else if nom.starts_with('/') {
        Some(PathBuf::from(nom))
    } else {
        None
    }
}

/// Dossier désigné par une entrée symbolique `xdg-data[/sous]`, `xdg-config[/sous]` ou
/// `xdg-cache[/sous]` (valeurs par défaut des variables XDG : `~/.local/share`, `~/.config`,
/// `~/.cache`). `None` pour tout autre nom (dont `xdg-run`, `xdg-documents`…).
fn dossier_xdg(nom: &str, home: &Path) -> Option<PathBuf> {
    let (base, sous) = nom.split_once('/').unwrap_or((nom, ""));
    let racine = match base {
        "xdg-data" => home.join(".local/share"),
        "xdg-config" => home.join(".config"),
        "xdg-cache" => home.join(".cache"),
        _ => return None,
    };
    Some(racine.join(sous))
}

/// Parmi `requis`, les chemins que le logiciel aux permissions `filesystems` ne voit pas.
///
/// - `host` couvre tout ; `xdg-data[/sous]`, `xdg-config[/sous]` et `xdg-cache[/sous]` valent
///   `~/.local/share`, `~/.config` et `~/.cache` (avec leur sous-dossier) ; `home` couvre ce qui est sous `home` ;
/// - une entrée explicite (`/opt/wimao`, `~/.vst3`) couvre son dossier et ses sous-dossiers
///   (comparaison par composants : `/opt/wim` ne couvre pas `/opt/wimao`), `~/` étant développé
///   avec `home` et le suffixe `:ro`/`:rw`/`:create` ignoré.
pub fn chemins_invisibles(filesystems: &[String], home: &Path, requis: &[PathBuf]) -> Vec<PathBuf> {
    let mode = |e: &str| e.rsplit_once(':').map_or(e, |(nom, _)| nom).to_owned();
    let tout = filesystems.iter().any(|e| mode(e) == "host");
    let maison = filesystems.iter().any(|e| mode(e) == "home");
    let dossiers: Vec<PathBuf> = filesystems
        .iter()
        .filter_map(|e| dossier_ouvert(e, home))
        .collect();
    requis
        .iter()
        .filter(|chemin| {
            let vu = tout
                || (maison && chemin.starts_with(home))
                || dossiers.iter().any(|d| chemin.starts_with(d));
            !vu
        })
        .cloned()
        .collect()
}

/// La commande `flatpak override --user --filesystem=… <id>` qui ouvre `invisibles` au logiciel
/// `id`, déjà citée pour le shell (voir [`pour_shell`]) : c'est ce que
/// l'interface met dans le presse-papiers. WiMao ne l'exécute jamais.
///
/// `:ro` (lecture seule) pour `/opt/wimao` et les dossiers de plugins ; les données de WiMao,
/// que le logiciel doit pouvoir lire et écrire, sont sans suffixe. Les chemins sous `home`
/// sont écrits `~/…` : le `~` reste littéral pour `sh` (il n'est développé qu'en début de mot)
/// et flatpak le comprend.
pub fn commande_override(id: &str, invisibles: &[PathBuf], home: &Path) -> String {
    let mut commande = String::from("flatpak override --user");
    for chemin in invisibles {
        let ecrit = match chemin.strip_prefix(home) {
            Ok(relatif) => format!("~/{}", relatif.display()),
            Err(_) => chemin.display().to_string(),
        };
        // Seuls les dossiers de plugins et `/opt/wimao` sont en lecture seule.
        let lecture_seule = chemin == Path::new(DOSSIER_WIMAO)
            || [".vst3", ".vst", ".clap"]
                .iter()
                .any(|d| chemin == &home.join(d));
        let suffixe = if lecture_seule { ":ro" } else { "" };
        let argument = format!("--filesystem={ecrit}{suffixe}");
        commande.push(' ');
        commande.push_str(&pour_shell(Path::new(&argument)));
    }
    commande.push(' ');
    commande.push_str(&pour_shell(Path::new(id)));
    commande
}

/// Pose le lien yabridge de chacun des logiciels `ids` et rend, pour chacun, ce qui a été fait.
///
/// Un échec pour un logiciel n'empêche pas les suivants. Comme pour le lien principal, rien
/// n'est jamais remplacé : un lien vers ailleurs ou un vrai dossier sont rendus tels quels
/// (`AutreLien`, `Dossier`).
pub fn assurer_liens_flatpak(c: &Chemins, ids: &[String]) -> Vec<(String, Resultat<LienYabridge>)> {
    ids.iter()
        .map(|id| {
            let resultat = assurer_lien_yabridge_a(c, &emplacement_lien_flatpak(c, id));
            (id.clone(), resultat)
        })
        .collect()
}

/// Met à « maintenant » la date de modification des `.so` pontés (`~/.vst3/yabridge`,
/// `~/.vst/yabridge`, `~/.clap/yabridge`, récursivement, liens non suivis). Rend le nombre de
/// fichiers touchés.
///
/// Pourquoi : les logiciels de musique retiennent le résultat de leur analyse des plugins et ne
/// réanalysent que les fichiers modifiés. Un plugin écarté avant que le lien n'existe
/// resterait donc écarté ; une date récente les force à le réessayer. Les dossiers absents
/// sont ignorés ; un fichier qu'on ne peut pas ouvrir est passé (il n'est pas compté).
pub fn rafraichir_dates_plugins(c: &Chemins) -> Resultat<usize> {
    let maintenant = SystemTime::now();
    let mut touches = 0;
    for dossier in DOSSIERS_PONTES {
        // `follow_links(false)` est le défaut de walkdir : on le dit pour que ce soit clair.
        for entree in WalkDir::new(c.home.join(dossier)).follow_links(false) {
            // Dossier absent ou illisible : rien à rafraîchir ici.
            let Ok(entree) = entree else { continue };
            if !entree.file_type().is_file() || !est_un_so(entree.path()) {
                continue;
            }
            let ouvert = std::fs::File::options().write(true).open(entree.path());
            if ouvert.and_then(|f| f.set_modified(maintenant)).is_ok() {
                touches += 1;
            }
        }
    }
    Ok(touches)
}

/// Vrai pour un fichier dont l'extension est `so`.
fn est_un_so(chemin: &Path) -> bool {
    chemin.extension().is_some_and(|e| e == "so")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{faux_flatpak, PathTemporaire};
    use serial_test::serial;
    use std::time::UNIX_EPOCH;

    #[test]
    #[serial]
    fn logiciels_installes_garde_les_connus() {
        let t = tempfile::tempdir().unwrap();
        faux_flatpak(
            t.path(),
            "printf 'com.bitwig.BitwigStudio\\norg.gimp.GIMP\\nfm.reaper.Reaper\\n'",
        );
        let _path = PathTemporaire::poser(t.path());
        assert_eq!(
            logiciels_installes(),
            ["com.bitwig.BitwigStudio", "fm.reaper.Reaper"]
        );
    }

    #[test]
    #[serial]
    fn logiciels_installes_sans_flatpak_est_vide() {
        let t = tempfile::tempdir().unwrap();
        // Un `flatpak` qui échoue, même s'il affiche un logiciel connu.
        faux_flatpak(t.path(), "echo fm.reaper.Reaper\nexit 1");
        let _path = PathTemporaire::poser(t.path());
        assert!(logiciels_installes().is_empty());
    }

    // --- Permissions ---

    /// Les trois chemins requis des tests, avec `/home/u` pour dossier personnel.
    fn requis_test() -> (PathBuf, Vec<PathBuf>) {
        (
            PathBuf::from("/home/u"),
            vec![
                PathBuf::from("/opt/wimao"),
                PathBuf::from("/home/u/.local/share/wimao"),
                PathBuf::from("/home/u/.vst3"),
            ],
        )
    }

    /// `chemins_invisibles` avec les entrées `e` et les chemins requis de [`requis_test`].
    fn invisibles(e: &[&str]) -> Vec<PathBuf> {
        let (home, requis) = requis_test();
        let e: Vec<String> = e.iter().map(|s| s.to_string()).collect();
        chemins_invisibles(&e, &home, &requis)
    }

    #[test]
    fn invisibles_host_home_et_entrees_explicites() {
        assert!(invisibles(&["host"]).is_empty());
        assert_eq!(invisibles(&["home"]), [PathBuf::from("/opt/wimao")]);
        assert_eq!(
            invisibles(&[
                "/opt/wimao:ro",
                "~/.local/share/wimao",
                "xdg-run/pipewire-0"
            ]),
            [PathBuf::from("/home/u/.vst3")]
        );
        // Un dossier parent couvre ses sous-dossiers, pas un nom qui lui ressemble.
        assert!(!invisibles(&["/opt"]).contains(&PathBuf::from("/opt/wimao")));
        assert!(invisibles(&["/opt/wim"]).contains(&PathBuf::from("/opt/wimao")));
        assert_eq!(invisibles(&[]).len(), 3);
    }

    #[test]
    fn invisibles_entrees_symboliques_xdg() {
        // `xdg-data/wimao` = ~/.local/share/wimao ; le suffixe de mode est ignoré.
        assert_eq!(
            invisibles(&["/opt/wimao", "xdg-data/wimao:ro"]),
            [PathBuf::from("/home/u/.vst3")]
        );
        // `xdg-data` seul couvre tout ~/.local/share, pas ~/.vst3.
        assert!(!invisibles(&["xdg-data"]).contains(&PathBuf::from("/home/u/.local/share/wimao")));
        assert!(invisibles(&["xdg-data"]).contains(&PathBuf::from("/home/u/.vst3")));
        // Un autre sous-dossier ne couvre pas wimao ; `xdg-config`/`xdg-cache` non plus.
        assert!(invisibles(&["xdg-data/autre", "xdg-config", "xdg-cache"])
            .contains(&PathBuf::from("/home/u/.local/share/wimao")));
    }

    #[test]
    fn commande_override_ne_cite_que_les_invisibles() {
        let invisibles = [
            PathBuf::from("/opt/wimao"),
            PathBuf::from("/home/u/.local/share/wimao"),
            PathBuf::from("/home/u/.vst3"),
        ];
        let commande =
            commande_override("com.bitwig.BitwigStudio", &invisibles, Path::new("/home/u"));
        assert_eq!(
            commande,
            "flatpak override --user --filesystem=/opt/wimao:ro \
             --filesystem=~/.local/share/wimao --filesystem=~/.vst3:ro com.bitwig.BitwigStudio"
        );
        // Le shell laisse le `~` tel quel : flatpak le reçoit littéralement.
        let sortie = Command::new("sh")
            .args([
                "-c",
                &commande.replace("flatpak override", "printf '%s\\n'"),
            ])
            .output()
            .unwrap();
        let texte = String::from_utf8_lossy(&sortie.stdout);
        assert!(
            texte.contains("--filesystem=~/.local/share/wimao\n"),
            "{texte}"
        );
    }

    #[test]
    #[serial]
    fn systemes_de_fichiers_lit_la_ligne() {
        let t = tempfile::tempdir().unwrap();
        faux_flatpak(
            t.path(),
            "printf '[Context]\\nshared=network;\\nfilesystems=home;/opt/wimao:ro;;\\n'",
        );
        let _path = PathTemporaire::poser(t.path());
        assert_eq!(
            systemes_de_fichiers("fm.reaper.Reaper").unwrap(),
            ["home", "/opt/wimao:ro"]
        );
    }

    #[test]
    #[serial]
    fn systemes_de_fichiers_en_echec_est_none() {
        let t = tempfile::tempdir().unwrap();
        faux_flatpak(t.path(), "echo 'filesystems=host;'\nexit 1");
        let _path = PathTemporaire::poser(t.path());
        assert_eq!(systemes_de_fichiers("fm.reaper.Reaper"), None);
    }

    #[test]
    fn lien_flatpak_cree_puis_deja_bon() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let ids = vec!["fm.reaper.Reaper".to_string()];
        let lien = emplacement_lien_flatpak(&c, "fm.reaper.Reaper");
        assert_eq!(lien, c.home.join(".var/app/fm.reaper.Reaper/data/yabridge"));

        let r = assurer_liens_flatpak(&c, &ids);
        assert!(matches!(r[0].1, Ok(LienYabridge::Cree)));
        assert_eq!(std::fs::read_link(&lien).unwrap(), c.yabridge_dir);

        let r = assurer_liens_flatpak(&c, &ids);
        assert!(matches!(r[0].1, Ok(LienYabridge::DejaBon)));
    }

    #[test]
    fn lien_flatpak_existant_laisse_intact() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let ailleurs = t.path().join("ailleurs");
        std::fs::create_dir(&ailleurs).unwrap();

        // Lien vers un autre dossier.
        let lien = emplacement_lien_flatpak(&c, "fm.reaper.Reaper");
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&ailleurs, &lien).unwrap();
        let r = assurer_liens_flatpak(&c, &["fm.reaper.Reaper".to_string()]);
        assert!(matches!(&r[0].1, Ok(LienYabridge::AutreLien(cible)) if *cible == ailleurs));
        assert_eq!(std::fs::read_link(&lien).unwrap(), ailleurs);

        // Vrai dossier.
        let lien = emplacement_lien_flatpak(&c, "org.ardour.Ardour");
        std::fs::create_dir_all(&lien).unwrap();
        std::fs::write(lien.join("fichier"), "a").unwrap();
        let r = assurer_liens_flatpak(&c, &["org.ardour.Ardour".to_string()]);
        assert!(matches!(r[0].1, Ok(LienYabridge::Dossier)));
        assert_eq!(std::fs::read_to_string(lien.join("fichier")).unwrap(), "a");
    }

    #[test]
    fn dates_rafraichies_seulement_les_so() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let dossier = c.home.join(".vst3/yabridge/X.vst3/Contents/x86_64-linux");
        std::fs::create_dir_all(&dossier).unwrap();
        let ancienne = UNIX_EPOCH + Duration::from_secs(978_307_200); // 2001-01-01
        let (so, txt) = (dossier.join("X.so"), dossier.join("notes.txt"));
        for f in [&so, &txt] {
            std::fs::write(f, "x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(f)
                .unwrap()
                .set_modified(ancienne)
                .unwrap();
        }
        assert_eq!(rafraichir_dates_plugins(&c).unwrap(), 1);
        let date = |f: &Path| std::fs::metadata(f).unwrap().modified().unwrap();
        assert!(date(&so) > ancienne + Duration::from_secs(86_400));
        assert_eq!(date(&txt), ancienne);
    }

    /// Premier `.so` ponté de la machine (`~/.vst3/yabridge/**/x86_64-linux/*.so`).
    fn premier_so_ponte() -> Option<PathBuf> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        WalkDir::new(home.join(".vst3/yabridge"))
            .into_iter()
            .flatten()
            .map(|e| e.into_path())
            .find(|p| {
                est_un_so(p)
                    && p.parent().and_then(Path::file_name) == Some("x86_64-linux".as_ref())
            })
    }

    /// Un logiciel en Flatpak simulé : `XDG_DATA_HOME` est un dossier où le seul lien est
    /// `yabridge` ; le plugin ponté doit quand même se charger.
    #[test]
    #[ignore = "Wine requis"]
    fn plugin_ponte_se_charge_depuis_un_flatpak_simule() {
        let Some(so) = premier_so_ponte() else {
            eprintln!("aucun plugin ponté dans ~/.vst3/yabridge : test sauté");
            return;
        };
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::depuis_env().unwrap();
        std::os::unix::fs::symlink(&c.yabridge_dir, t.path().join("yabridge")).unwrap();
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/donnees/charger_vst3.py");
        let var = |nom: &str| std::env::var_os(nom).unwrap_or_default();
        let sortie = Command::new("python3")
            .arg("-I")
            .arg(&script)
            .arg(&so)
            .env_clear()
            .env("HOME", var("HOME"))
            .env("PATH", "/usr/bin:/bin")
            .env("XDG_DATA_HOME", t.path())
            .env("XDG_RUNTIME_DIR", var("XDG_RUNTIME_DIR"))
            .env("DISPLAY", var("DISPLAY"))
            .output()
            .unwrap();
        let texte = String::from_utf8_lossy(&sortie.stdout);
        assert!(texte.contains("ModuleEntry -> True"), "sortie : {texte}");
    }
}
