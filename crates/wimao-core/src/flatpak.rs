//! Logiciels de musique installés en Flatpak : on leur pose le lien yabridge.
//!
//! Utilise [`Chemins`] (le dossier personnel, le yabridge de WiMao), [`crate::pontage`]
//! (`assurer_lien_yabridge_a`, qui pose un lien sans jamais rien remplacer) et [`lancer`]
//! (pour interroger `flatpak`). Utilisé par `pontage::assurer_lien_et_noter`, donc à chaque
//! pontage.
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
//! `flatpak` absent ou en échec n'est jamais une erreur : il n'y a simplement aucun logiciel.

use crate::chemins::Chemins;
use crate::erreur::Resultat;
use crate::outils::lancer;
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
    use serial_test::serial;
    use std::os::unix::fs::PermissionsExt;
    use std::time::UNIX_EPOCH;

    /// Met `dossier` en tête du `PATH` le temps du test et le rétablit à la fin.
    struct PathTemporaire(Option<std::ffi::OsString>);

    impl PathTemporaire {
        /// `seul` : le `PATH` devient exactement `dossier` (pour simuler un `flatpak` absent).
        fn poser(dossier: &Path, seul: bool) -> Self {
            let ancien = std::env::var_os("PATH");
            let nouveau = match (&ancien, seul) {
                (Some(a), false) => format!("{}:{}", dossier.display(), a.to_string_lossy()),
                _ => dossier.display().to_string(),
            };
            std::env::set_var("PATH", nouveau);
            PathTemporaire(ancien)
        }
    }

    impl Drop for PathTemporaire {
        fn drop(&mut self) {
            match &self.0 {
                Some(a) => std::env::set_var("PATH", a),
                None => std::env::remove_var("PATH"),
            }
        }
    }

    /// Écrit un faux `flatpak` (script shell) dans `dossier`.
    fn faux_flatpak(dossier: &Path, corps: &str) {
        let script = dossier.join("flatpak");
        std::fs::write(&script, format!("#!/bin/sh\n{corps}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    #[serial]
    fn logiciels_installes_garde_les_connus() {
        let t = tempfile::tempdir().unwrap();
        faux_flatpak(
            t.path(),
            "printf 'com.bitwig.BitwigStudio\\norg.gimp.GIMP\\nfm.reaper.Reaper\\n'",
        );
        let _path = PathTemporaire::poser(t.path(), false);
        assert_eq!(
            logiciels_installes(),
            ["com.bitwig.BitwigStudio", "fm.reaper.Reaper"]
        );
    }

    #[test]
    #[serial]
    fn logiciels_installes_sans_flatpak_est_vide() {
        let t = tempfile::tempdir().unwrap();
        // `PATH` réduit à un dossier vide : aucun `flatpak`.
        {
            let _path = PathTemporaire::poser(t.path(), true);
            assert!(logiciels_installes().is_empty());
        }
        // Un `flatpak` qui échoue, même s'il affiche un logiciel connu.
        faux_flatpak(t.path(), "echo fm.reaper.Reaper\nexit 1");
        let _path = PathTemporaire::poser(t.path(), false);
        assert!(logiciels_installes().is_empty());
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
