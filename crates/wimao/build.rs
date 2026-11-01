//! Script de compilation minimal : active le cfg `wimao_paquet` pour les builds de paquet.
//!
//! Quand la variable d'environnement `WIMAO_PAQUET` est non vide à la compilation (c'est ce que
//! fait `paquets/fabriquer-wimao.sh`), le cfg `wimao_paquet` est posé. Le code de développement
//! qui connaît des chemins de la machine de fabrication (`env!("CARGO_MANIFEST_DIR")`, `OUT_DIR`)
//! est alors **exclu de la compilation** (`#[cfg(not(wimao_paquet))]`) : aucun chemin du poste
//! de fabrication ne reste dans le binaire, qui est donc identique d'un dossier de travail à
//! l'autre (reproductibilité). Sans la variable, rien ne change : `cargo run` et `cargo test`
//! gardent leurs replis de développement.

fn main() {
    // Recompiler si la variable apparaît, disparaît ou change.
    println!("cargo:rerun-if-env-changed=WIMAO_PAQUET");
    // Déclare le cfg à rustc, sinon le lint `unexpected_cfgs` s'en plaint.
    println!("cargo:rustc-check-cfg=cfg(wimao_paquet)");
    if std::env::var_os("WIMAO_PAQUET").is_some_and(|v| !v.is_empty()) {
        println!("cargo:rustc-cfg=wimao_paquet");
    }
}
