//! Sous-commande `verifier` : teste l'ouverture de chaque plugin déjà ponté d'un préfixe.
//!
//! Utilise `wimao_core::tester_plugin`, `Chemins` et `std::fs`.
//! Utilisé par `commandes::executer`. Ne crée ni ne modifie rien dans les dossiers de
//! WiMao ni dans le préfixe (hors ce que Wine écrit lui-même quand il charge un plugin) :
//! on retrouve les plugins en lisant les liens de `~/.vst3/yabridge` et `~/.vst/yabridge`,
//! pas en relevant l'inventaire du préfixe.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use wimao_core::i18n::tr;
use wimao_core::{tester_plugin, Chemins, Format, Resultat};

use super::Issue;
use crate::sortie::afficher;
use wimao_core::tableau::{depuis_test, Ligne, Statut};

/// Dossier des VST3 pontés, sous le dossier personnel.
const DOSSIER_VST3_PONTES: &str = ".vst3/yabridge";
/// Dossier des VST2 pontés, sous le dossier personnel.
const DOSSIER_VST2_PONTES: &str = ".vst/yabridge";
/// Dossier d'un bundle VST3 ponté qui contient le lien vers le plugin Windows
/// (voir `wimao_core::pontage`, « Disposition constatée »).
const DOSSIER_LIEN_VST3: &str = "Contents/x86_64-win";
/// Extension d'un bundle VST3.
const EXTENSION_VST3: &str = "vst3";
/// Extension du lien vers la DLL Windows d'un VST2 ponté (le plugin ponté est le `.so` voisin).
const EXTENSION_DLL: &str = "dll";
/// Extension du plugin VST2 ponté.
const EXTENSION_SO: &str = "so";

/// Un plugin ponté côté Linux, retrouvé par son lien vers le préfixe.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PluginPonte {
    /// Nom affiché (nom du fichier Windows, sans extension).
    pub nom: String,
    /// Format du plugin.
    pub format: Format,
    /// Bundle ou fichier Linux à tester.
    pub bundle: PathBuf,
    /// Vrai si le lien pointe dans le préfixe vers un fichier qui n'existe plus (plugin
    /// Windows supprimé) : il est signalé ❌ sans être testé, au lieu d'être ignoré.
    pub lien_casse: bool,
}

/// `verifier` : teste chaque plugin ponté du préfixe et imprime le tableau.
///
/// `prefixe` remplace celui de WiMao (par exemple `~/.wine-mao`) ; seul ce champ de
/// [`Chemins`] change. Rend [`Issue::PluginEnEchec`] si au moins un plugin est ❌.
pub fn verifier(c: &Chemins, prefixe: Option<PathBuf>) -> Resultat<Issue> {
    let mut c = c.clone();
    if let Some(p) = prefixe {
        c.prefixe = p;
    }
    let plugins = plugins_pontes(&c.home, &c.prefixe);
    if plugins.is_empty() {
        afficher!("{}", tr("No bridged plugin found for this environment"));
        return Ok(Issue::Succes);
    }
    let mut issue = Issue::Succes;
    for plugin in plugins {
        let (statut, detail) = verdict(&c, &plugin);
        if statut == Statut::Echec {
            issue = Issue::PluginEnEchec;
        }
        let ligne = Ligne {
            nom: plugin.nom,
            format: plugin.format,
            statut,
            detail,
        };
        afficher!("{}", ligne.texte());
    }
    Ok(issue)
}

/// Verdict d'un plugin : ❌ « lien cassé » sans lancer de test, sinon le résultat du test.
fn verdict(c: &Chemins, plugin: &PluginPonte) -> (Statut, String) {
    if plugin.lien_casse {
        return (
            Statut::Echec,
            tr("broken link: the Windows plugin no longer exists in the environment"),
        );
    }
    depuis_test(&tester_plugin(c, &plugin.bundle, plugin.format))
}

/// Retrouve les plugins pontés dont le lien Windows pointe dans `prefixe`.
///
/// Parcourt `~/.vst3/yabridge` et `~/.vst/yabridge` sans suivre les liens. Un dossier
/// absent ou illisible est ignoré (aucun plugin ponté), triés par nom pour un affichage stable.
pub fn plugins_pontes(home: &Path, prefixe: &Path) -> Vec<PluginPonte> {
    let prefixe_reel = prefixe
        .canonicalize()
        .unwrap_or_else(|_| prefixe.to_path_buf());
    let dans_prefixe = |cible: &Path| {
        cible.starts_with(prefixe)
            || cible.starts_with(&prefixe_reel)
            || cible
                .canonicalize()
                .is_ok_and(|r| r.starts_with(&prefixe_reel))
    };
    let mut trouves = BTreeSet::new();
    for (dossier, format) in [
        (DOSSIER_VST3_PONTES, Format::Vst3),
        (DOSSIER_VST2_PONTES, Format::Vst2),
    ] {
        for (lien, cible) in liens_du_dossier(&home.join(dossier)) {
            if !dans_prefixe(&cible) {
                continue;
            }
            if let Some(mut plugin) = plugin_depuis_lien(&lien, format) {
                plugin.lien_casse = !cible.exists();
                trouves.insert(plugin);
            }
        }
    }
    let mut liste: Vec<PluginPonte> = trouves.into_iter().collect();
    liste.sort_by_key(|p| p.nom.to_lowercase());
    liste
}

/// Tous les liens symboliques sous `racine`, avec leur cible rendue absolue.
fn liens_du_dossier(racine: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut liens = Vec::new();
    let mut a_voir = vec![racine.to_path_buf()];
    while let Some(dossier) = a_voir.pop() {
        let Ok(entrees) = std::fs::read_dir(&dossier) else {
            continue; // dossier absent ou illisible : rien à vérifier ici
        };
        for entree in entrees.flatten() {
            let chemin = entree.path();
            match entree.file_type() {
                Ok(t) if t.is_symlink() => {
                    if let Ok(cible) = std::fs::read_link(&chemin) {
                        liens.push((chemin.clone(), normaliser(&dossier.join(cible))));
                    }
                }
                Ok(t) if t.is_dir() => a_voir.push(chemin),
                _ => {}
            }
        }
    }
    liens
}

/// Normalise un chemin **sans toucher au disque** : retire `.` et résout `..` lexicalement.
///
/// Nécessaire pour un lien relatif (`../../pfx/drive_c/X.vst3`) : sans cela, `starts_with`
/// ne reconnaît pas le préfixe, et `canonicalize` échoue pour un lien cassé.
fn normaliser(chemin: &Path) -> PathBuf {
    let mut sortie = PathBuf::new();
    for composant in chemin.components() {
        match composant {
            Component::CurDir => {}
            Component::ParentDir if sortie.file_name().is_some() => {
                sortie.pop();
            }
            autre => sortie.push(autre),
        }
    }
    sortie
}

/// Le plugin ponté que désigne un lien vers le plugin Windows, s'il a la bonne forme.
///
/// - VST3 : `<bundle>.vst3/Contents/x86_64-win/<nom>.vst3` donne le bundle ;
/// - VST2 : `<nom>.dll` donne le `<nom>.so` voisin (qui doit exister).
fn plugin_depuis_lien(lien: &Path, format: Format) -> Option<PluginPonte> {
    let nom = lien.file_stem()?.to_string_lossy().into_owned();
    let extension = lien.extension()?.to_str()?;
    let bundle = match format {
        Format::Vst3 if extension == EXTENSION_VST3 => {
            let dossier_lien = lien.parent()?;
            if !dossier_lien.ends_with(DOSSIER_LIEN_VST3) {
                return None;
            }
            dossier_lien.parent()?.parent()?.to_path_buf()
        }
        Format::Vst2 if extension == EXTENSION_DLL => {
            let so = lien.with_extension(EXTENSION_SO);
            so.symlink_metadata().ok()?;
            so
        }
        _ => return None,
    };
    Some(PluginPonte {
        nom,
        format,
        bundle,
        lien_casse: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    /// Crée un VST3 ponté dont le lien Windows pointe vers `cible`.
    fn vst3_ponte(home: &Path, rel: &str, cible: &Path) {
        let win = home
            .join(DOSSIER_VST3_PONTES)
            .join(rel)
            .join(DOSSIER_LIEN_VST3);
        std::fs::create_dir_all(&win).unwrap();
        let nom = Path::new(rel).file_name().unwrap();
        symlink(cible, win.join(nom)).unwrap();
    }

    #[test]
    fn seuls_les_plugins_lies_au_prefixe_sont_retrouves() {
        let t = TempDir::new().unwrap();
        let (home, prefixe) = (t.path().join("home"), t.path().join("pfx"));
        std::fs::create_dir_all(&prefixe).unwrap();
        vst3_ponte(
            &home,
            "Éditeur/Xpand!2.vst3",
            &prefixe.join("drive_c/Xpand!2.vst3"),
        );
        vst3_ponte(&home, "Autre.vst3", Path::new("/ailleurs/Autre.vst3"));

        let liste = plugins_pontes(&home, &prefixe);
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].nom, "Xpand!2");
        assert_eq!(liste[0].format, Format::Vst3);
        assert_eq!(
            liste[0].bundle,
            home.join(".vst3/yabridge/Éditeur/Xpand!2.vst3")
        );
    }

    #[test]
    fn un_vst2_est_retrouve_par_son_so() {
        let t = TempDir::new().unwrap();
        let (home, prefixe) = (t.path().join("home"), t.path().join("pfx"));
        let dossier = home.join(DOSSIER_VST2_PONTES);
        std::fs::create_dir_all(&dossier).unwrap();
        std::fs::write(dossier.join("Synth_x64.so"), b"").unwrap();
        symlink(
            prefixe.join("drive_c/Synth_x64.dll"),
            dossier.join("Synth_x64.dll"),
        )
        .unwrap();

        let liste = plugins_pontes(&home, &prefixe);
        assert_eq!(liste.len(), 1);
        assert_eq!(liste[0].format, Format::Vst2);
        assert_eq!(liste[0].bundle, dossier.join("Synth_x64.so"));
    }

    #[test]
    fn sans_dossier_ponte_la_liste_est_vide() {
        let t = TempDir::new().unwrap();
        assert!(plugins_pontes(t.path(), &t.path().join("pfx")).is_empty());
    }

    #[test]
    fn un_lien_relatif_avec_deux_points_est_normalise() {
        assert_eq!(
            normaliser(Path::new("/h/.vst3/yabridge/X.vst3/../../../pfx/./a")),
            PathBuf::from("/h/pfx/a")
        );
        let t = TempDir::new().unwrap();
        let (home, prefixe) = (t.path().join("home"), t.path().join("pfx"));
        std::fs::create_dir_all(prefixe.join("drive_c")).unwrap();
        std::fs::write(prefixe.join("drive_c/R.vst3"), b"").unwrap();
        // Lien relatif : depuis <home>/.vst3/yabridge/R.vst3/Contents/x86_64-win/ vers pfx.
        vst3_ponte(
            &home,
            "R.vst3",
            Path::new("../../../../../../pfx/drive_c/R.vst3"),
        );
        let liste = plugins_pontes(&home, &prefixe);
        assert_eq!(liste.len(), 1, "{liste:?}");
        assert!(!liste[0].lien_casse);
    }

    #[test]
    fn un_lien_casse_vers_le_prefixe_est_liste() {
        let t = TempDir::new().unwrap();
        let (home, prefixe) = (t.path().join("home"), t.path().join("pfx"));
        std::fs::create_dir_all(&prefixe).unwrap();
        vst3_ponte(&home, "Perdu.vst3", &prefixe.join("drive_c/Perdu.vst3"));
        let liste = plugins_pontes(&home, &prefixe);
        assert_eq!(liste.len(), 1);
        assert!(liste[0].lien_casse);
        let (statut, _) = verdict(&Chemins::pour_test(t.path()), &liste[0]);
        assert_eq!(statut, Statut::Echec);
    }

    #[test]
    fn un_prefixe_donne_par_un_lien_symbolique_est_reconnu() {
        let t = TempDir::new().unwrap();
        let (home, reel) = (t.path().join("home"), t.path().join("reel"));
        std::fs::create_dir_all(reel.join("drive_c")).unwrap();
        std::fs::write(reel.join("drive_c/S.vst3"), b"").unwrap();
        let lien = t.path().join("pfx-lien");
        symlink(&reel, &lien).unwrap();
        // Le plugin pointe vers le chemin réel, le préfixe est donné par le lien (et l'inverse).
        vst3_ponte(&home, "S.vst3", &reel.join("drive_c/S.vst3"));
        assert_eq!(plugins_pontes(&home, &lien).len(), 1);
        let autre = TempDir::new().unwrap();
        vst3_ponte(autre.path(), "S.vst3", &lien.join("drive_c/S.vst3"));
        assert_eq!(plugins_pontes(autre.path(), &reel).len(), 1);
    }
}
