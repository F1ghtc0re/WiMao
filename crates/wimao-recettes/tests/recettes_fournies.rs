//! Test des recettes fournies dans `recettes/` à la racine du dépôt : elles doivent toutes se
//! charger sans erreur (clés connues, consignes en `fr`/`en`/`es`, adresses en HTTPS…), et
//! chaque installeur d'éditeur doit être reconnu par son nom de fichier.
//!
//! Utilise `wimao_recettes::{Catalogue, TypeRecette}`. Lancé par `cargo test`, sans Wine.

use std::path::{Path, PathBuf};

use wimao_recettes::{Catalogue, TypeRecette};

/// Nombre de gestionnaires fournis (Native Access, Arturia, inMusic, Plugin Alliance).
const GESTIONNAIRES_ATTENDUS: usize = 4;
/// Nombre d'installeurs fournis (Serum 2, Valhalla Supermassive).
const INSTALLEURS_ATTENDUS: usize = 2;

/// Racine du dépôt : deux niveaux au-dessus de la crate.
fn dossier_recettes() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../recettes")
}

/// Catalogue des recettes fournies, en échouant à la première erreur de chargement.
fn catalogue_fourni() -> Catalogue {
    let (catalogue, erreurs) = Catalogue::charger(&[dossier_recettes()]);
    assert!(erreurs.is_empty(), "recettes invalides : {erreurs:?}");
    catalogue
}

#[test]
fn recettes_fournies_valides() {
    let catalogue = catalogue_fourni();
    let nombre = |type_| catalogue.0.iter().filter(|r| r.type_ == type_).count();
    assert_eq!(nombre(TypeRecette::Gestionnaire), GESTIONNAIRES_ATTENDUS);
    assert_eq!(nombre(TypeRecette::Installeur), INSTALLEURS_ATTENDUS);
    assert_eq!(
        catalogue.0.len(),
        GESTIONNAIRES_ATTENDUS + INSTALLEURS_ATTENDUS
    );
}

#[test]
fn serum_reconnu() {
    let catalogue = catalogue_fourni();
    let recette = catalogue.trouver(Path::new("Install_Xfer_Serum2_2.1.5.exe"));
    assert_eq!(recette.map(|r| r.nom.as_str()), Some("Serum 2"));
}

#[test]
fn les_noms_d_installeurs_connus_sont_reconnus() {
    let catalogue = catalogue_fourni();
    for (fichier, nom) in [
        ("Native-Access_2.exe", "Native Access 2"),
        (
            "Arturia_Software_Center__2_13_0_3440.exe",
            "Arturia Software Center",
        ),
        // Noms réels relevés à la vérification du 2026-11-01 (inMusic : cible de la redirection
        // de inmusic.to/ISCPC ; Plugin Alliance : le zip public et le MSI qu'il contient).
        (
            "inMusic Software Center-1.40.0 Setup.exe",
            "inMusic Software Center",
        ),
        ("pa_installation_manager_win_1_4_0.zip", "Plugin Alliance"),
        ("PA-InstallationManager-v1.4.0.msi", "Plugin Alliance"),
        // Nom HYPOTHÉTIQUE pour Valhalla : son site refuse le téléchargement automatique, le
        // motif reste à confirmer avec le vrai fichier.
        ("ValhallaSupermassive_5_0_0.zip", "Valhalla Supermassive"),
    ] {
        let trouvee = catalogue.trouver(Path::new(fichier));
        assert_eq!(trouvee.map(|r| r.nom.as_str()), Some(nom), "{fichier}");
    }
}

#[test]
fn aucune_recette_ne_cite_un_profil_windows_en_dur() {
    // Un chemin `C:\users\<nom>` ne doit passer que par la variable `{utilisateur}`.
    for fichier in std::fs::read_dir(dossier_recettes()).unwrap() {
        let chemin = fichier.unwrap().path();
        if chemin.extension().is_some_and(|e| e == "toml") {
            let texte = std::fs::read_to_string(&chemin).unwrap().to_lowercase();
            for (i, _) in texte.match_indices("c:\\users\\") {
                let suite = &texte[i + "c:\\users\\".len()..];
                assert!(
                    suite.starts_with('{'),
                    "{} : profil en dur",
                    chemin.display()
                );
            }
        }
    }
}
