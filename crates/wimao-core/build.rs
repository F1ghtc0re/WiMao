//! Script de compilation de `wimao-core` : compile les traductions `po/*.po`.
//!
//! Utilise `msgfmt` (paquet `gettext`) pour produire
//! `<OUT_DIR>/locale/<langue>/LC_MESSAGES/wimao.mo`. Ce dossier est le dernier
//! recours de [`i18n`](../src/i18n.rs) quand `WIMAO_LOCALEDIR` n'est pas défini et
//! que les traductions ne sont pas installées dans `/usr/share/locale` : il permet
//! de lancer `cargo run` / `cargo test` sans rien installer.
//!
//! Pose aussi le cfg `wimao_paquet` quand `WIMAO_PAQUET` est non vide : dans ce cas ce dernier
//! recours n'est pas compilé, pour qu'aucun chemin de `OUT_DIR` ne reste dans le binaire
//! d'un paquet (voir `crates/wimao/build.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;

/// Dossier des fichiers `.po`, relatif à la racine du dépôt (deux niveaux au-dessus de la crate).
const DOSSIER_PO: &str = "../../po";

fn main() {
    // Cfg des builds de paquet : même logique que `crates/wimao/build.rs`.
    println!("cargo:rerun-if-env-changed=WIMAO_PAQUET");
    println!("cargo:rustc-check-cfg=cfg(wimao_paquet)");
    if std::env::var_os("WIMAO_PAQUET").is_some_and(|v| !v.is_empty()) {
        println!("cargo:rustc-cfg=wimao_paquet");
    }
    if let Err(message) = compiler_traductions() {
        // Échec explicite : sans les `.mo`, les tests de langue ne peuvent pas passer.
        panic!("compilation des traductions impossible : {message}");
    }
}

/// Compile chaque `po/<langue>.po` en `.mo` ; `Err` décrit le premier problème.
fn compiler_traductions() -> Result<(), String> {
    let manifeste = std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?;
    let sortie = std::env::var("OUT_DIR").map_err(|e| e.to_string())?;
    let dossier_po = Path::new(&manifeste).join(DOSSIER_PO);
    // Recompiler si une traduction est ajoutée, retirée ou modifiée.
    println!("cargo:rerun-if-changed={}", dossier_po.display());

    let entrees = std::fs::read_dir(&dossier_po)
        .map_err(|e| format!("lecture de {} : {e}", dossier_po.display()))?;
    for entree in entrees.flatten() {
        let chemin = entree.path();
        if chemin.extension().is_some_and(|e| e == "po") {
            compiler_une_langue(&chemin, Path::new(&sortie))?;
        }
    }
    Ok(())
}

/// Compile un `.po` vers `<sortie>/locale/<langue>/LC_MESSAGES/wimao.mo`.
fn compiler_une_langue(po: &Path, sortie: &Path) -> Result<(), String> {
    let langue = po
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("nom de fichier invalide : {}", po.display()))?;
    let dossier: PathBuf = sortie.join("locale").join(langue).join("LC_MESSAGES");
    std::fs::create_dir_all(&dossier).map_err(|e| e.to_string())?;
    let etat = Command::new("msgfmt")
        .arg(po)
        .arg("-o")
        .arg(dossier.join("wimao.mo"))
        .status()
        .map_err(|e| format!("msgfmt introuvable (installez le paquet gettext) : {e}"))?;
    if etat.success() {
        Ok(())
    } else {
        Err(format!("msgfmt a échoué sur {}", po.display()))
    }
}
