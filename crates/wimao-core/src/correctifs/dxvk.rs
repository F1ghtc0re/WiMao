//! Correctif DXVK : Direct3D 9/10/11 traduit en Vulkan à la place de wined3d (OpenGL).
//!
//! Utilise [`Wine`] (registre), [`telecharger_verifie`], `tar` (extraction de
//! l'archive) et les outils de `dll`. Utilisé par `correctifs::correctifs`.
//!
//! Pourquoi : avec wined3d les interfaces Direct2D/Direct3D 11 (Serum 2) scintillent,
//! et celles des plugins iZotope (Ozone, Vocal Doubler), en Direct3D 9, restent
//! blanches (NOTES.md). DXVK corrige les deux. Seules `d3d11`, `dxgi`, `d3d10core`
//! et `d3d9` sont remplacées, comme dans `scripts/install-dxvk.sh`.
//!
//! Reconnaissance : `est_applique` compare les 8 fichiers de l'environnement aux
//! empreintes SHA-256 officielles de DXVK 3.1.1 (`EMPREINTES_DLL`), donc sans
//! avoir besoin d'aucune copie de référence ni de réseau (un environnement
//! importé, équipé par l'ancien `scripts/install-dxvk.sh`, est reconnu tel quel).
//!
//! Source des DLL (pour `appliquer` seulement) : `/opt/wimao/dxvk/{x64,x32}` si le paquet WiMao les fournit,
//! sinon l'archive officielle, téléchargée dans le cache et extraite dans
//! `<cache>/dxvk-3.1.1/`.

use super::dll::{
    dossier_windows, poser_surcharge, remplacer_fichier, restaurer_original, retirer_surcharge,
    sauvegarder_original, surcharge_est_native, SYSTEM32, SYSWOW64,
};
use super::Correctif;
use crate::chemins::Chemins;
use crate::erreur::{Erreur, Resultat};
use crate::i18n::{tr, trf};
use crate::outils::lancer;
use crate::progression::Progression;
use crate::telechargement::{empreinte, telecharger_verifie};
use crate::wine::Wine;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Adresse officielle de l'archive (publication GitHub de DXVK).
const URL_ARCHIVE: &str =
    "https://github.com/doitsujin/dxvk/releases/download/v3.1.1/dxvk-3.1.1.tar.gz";
/// Empreinte SHA-256 de l'archive, relevée dans `scripts/install-dxvk.sh`.
const SHA256_ARCHIVE: &str = "40565b4a724aadc4433fa4e010b4b23916d9b1f1baeee64e17186db94f54e608";
/// Nom de l'archive dans le cache.
const FICHIER_ARCHIVE: &str = "dxvk-3.1.1.tar.gz";
/// Dossier créé par l'extraction, sous le cache (`dxvk-<version>`).
const DOSSIER_EXTRAIT: &str = "dxvk-3.1.1";
/// Dossier de DXVK fourni par le paquet WiMao (contient `x64/` et `x32/`).
const DOSSIER_PAQUET: &str = "/opt/wimao/dxvk";
/// Sous-dossier des DLL 64 bits dans une source DXVK.
const SOUS_DOSSIER_X64: &str = "x64";
/// Sous-dossier des DLL 32 bits dans une source DXVK.
const SOUS_DOSSIER_X32: &str = "x32";
/// Les DLL remplacées (sans l'extension).
const DLLS: [&str; 4] = ["d3d11", "dxgi", "d3d10core", "d3d9"];
/// Empreintes SHA-256 des 8 DLL officielles de DXVK 3.1.1, sous la forme
/// (dossier de l'environnement, nom de la DLL sans extension, empreinte).
///
/// Relevées sur les fichiers extraits de l'archive `dxvk-3.1.1.tar.gz` dont
/// l'empreinte est `SHA256_ARCHIVE` (vérifiée avant relevé). Elles permettent de
/// reconnaître DXVK dans un environnement sans aucune copie de référence.
/// À mettre à jour avec `SHA256_ARCHIVE` et `URL_ARCHIVE` si la version change.
const EMPREINTES_DLL: [(&str, &str, &str); 8] = [
    (
        SYSTEM32,
        "d3d11",
        "0a203b6255c893430f0a8461c618962df04e62aa41538d5a875b21868317cf9f",
    ),
    (
        SYSTEM32,
        "dxgi",
        "e3c1178eb7f0dd59f91bcfcc257549e5b9cfdb5e8508719293de7f8704f53160",
    ),
    (
        SYSTEM32,
        "d3d10core",
        "beffdd081dce91c2bccdc87845b88cbbb7848d39d4642143462d2556d69bd5d9",
    ),
    (
        SYSTEM32,
        "d3d9",
        "c126316f7478eaff1ad8087702eda5930f2400187b9c8b3592219c34f6632780",
    ),
    (
        SYSWOW64,
        "d3d11",
        "f44543cbe5c40202915a02ed6e9ac8140c3524e846ae67b01f6faa79485d1fd3",
    ),
    (
        SYSWOW64,
        "dxgi",
        "abd1449dc28d536a4289c256ae7f4b6bbd15fa552792b7170c2ddece473c8bda",
    ),
    (
        SYSWOW64,
        "d3d10core",
        "110c41a5f67a0ab6d4eea586708883ce05aede204a0df6665e6362669079c2ba",
    ),
    (
        SYSWOW64,
        "d3d9",
        "265888c31ca78dffa290c39cb7e50bfb02762590e41927906e46fb32f01497fa",
    ),
];
/// Délai de `tar` : l'archive fait quelques Mo, une minute est très large.
const DELAI_EXTRACTION: Duration = Duration::from_secs(60);

/// DXVK 3.1.1 (la plus récente testée avec Serum 2 et iZotope, NOTES.md).
#[derive(Debug, Clone, Copy)]
pub struct Dxvk;

/// Où trouver les DLL de DXVK : un dossier 64 bits et un dossier 32 bits.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SourceDxvk {
    /// DLL 64 bits (vont dans `system32`).
    x64: PathBuf,
    /// DLL 32 bits (vont dans `syswow64`).
    x32: PathBuf,
}

impl SourceDxvk {
    /// Les deux associations (dossier source, dossier de l'environnement).
    fn paires(&self) -> [(&Path, &'static str); 2] {
        [(&self.x64, SYSTEM32), (&self.x32, SYSWOW64)]
    }
}

/// La source DXVK contenue dans `racine`, si elle a ses deux sous-dossiers.
fn source_dans(racine: &Path) -> Option<SourceDxvk> {
    let x64 = racine.join(SOUS_DOSSIER_X64);
    let x32 = racine.join(SOUS_DOSSIER_X32);
    (x64.is_dir() && x32.is_dir()).then_some(SourceDxvk { x64, x32 })
}

/// La source déjà disponible sans rien télécharger : le paquet `paquet`, sinon
/// l'archive déjà extraite dans le cache.
fn source_existante(c: &Chemins, paquet: &Path) -> Option<SourceDxvk> {
    source_dans(paquet).or_else(|| source_dans(&c.cache.join(DOSSIER_EXTRAIT)))
}

/// Vrai si chaque fichier de `attendues` existe dans l'environnement avec
/// l'empreinte SHA-256 indiquée. La table est un paramètre pour que les tests
/// puissent l'adapter à de faux fichiers (les vraies DLL pèsent des Mo).
fn fichiers_poses(c: &Chemins, attendues: &[(&str, &str, &str)]) -> Resultat<bool> {
    for (sous, dll, sha256) in attendues {
        let pose = dossier_windows(c, sous).join(format!("{dll}.dll"));
        // Absent : pas DXVK (et `empreinte` échouerait sur un fichier manquant).
        if !pose.is_file() || empreinte(&pose)? != *sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Télécharge (vérifié) et extrait l'archive si aucune source n'est disponible.
fn preparer_source(c: &Chemins) -> Resultat<SourceDxvk> {
    if let Some(s) = source_existante(c, Path::new(DOSSIER_PAQUET)) {
        return Ok(s);
    }
    let archive = c.cache.join(FICHIER_ARCHIVE);
    telecharger_verifie(&[URL_ARCHIVE], Some(SHA256_ARCHIVE), &archive)?;
    let dossier = c.cache.join(DOSSIER_EXTRAIT);
    extraire(&archive, &c.cache, &dossier)?;
    source_dans(&dossier).ok_or_else(|| {
        Erreur::Introuvable(trf(
            "folders {x64}/ and {x32}/ in the extracted DXVK archive ({folder})",
            &[
                ("x64", SOUS_DOSSIER_X64),
                ("x32", SOUS_DOSSIER_X32),
                ("folder", &dossier.display().to_string()),
            ],
        ))
    })
}

/// Extrait `archive` dans `cache` (elle crée `dossier`). Un dossier d'une
/// extraction précédente est supprimé d'abord, et celui d'une extraction ratée
/// aussi : un dossier incomplet serait pris plus tard pour une source valide.
fn extraire(archive: &Path, cache: &Path, dossier: &Path) -> Resultat<()> {
    let _ = std::fs::remove_dir_all(dossier);
    let mut cmd = Command::new("tar");
    cmd.arg("-xzf").arg(archive).arg("-C").arg(cache);
    let sortie = lancer(&mut cmd, DELAI_EXTRACTION)?;
    if sortie.code != 0 {
        let _ = std::fs::remove_dir_all(dossier);
        return Err(Erreur::Commande {
            programme: format!("tar -xzf {}", archive.display()),
            code: Some(sortie.code),
            sortie: sortie.texte,
        });
    }
    Ok(())
}

impl Correctif for Dxvk {
    fn nom(&self) -> &'static str {
        "DXVK"
    }

    /// Appliqué si les 8 fichiers ont l'empreinte officielle de DXVK 3.1.1
    /// (`EMPREINTES_DLL`) et que les 4 déclarations valent `native,builtin`.
    /// Aucune copie de référence ni téléchargement n'est nécessaire : la
    /// vérification reste sans effet de bord.
    fn est_applique(&self, c: &Chemins) -> Resultat<bool> {
        // Les fichiers d'abord ; le registre est lu dans `user.reg`, sans lancer Wine.
        if !fichiers_poses(c, &EMPREINTES_DLL)? {
            return Ok(false);
        }
        for dll in DLLS {
            if !surcharge_est_native(c, dll)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Sauvegarde les DLL de Wine, pose celles de DXVK et les déclare `native,builtin`.
    ///
    /// Erreurs : téléchargement ([`Erreur::Telechargement`], [`Erreur::Empreinte`]),
    /// [`Erreur::Commande`] (`tar`, `reg`), [`Erreur::Io`].
    fn appliquer(&self, c: &Chemins, p: &dyn Progression) -> Resultat<()> {
        p.etape(&tr("Installing DXVK…"));
        let source = preparer_source(c)?;
        let wine = Wine::nouveau(c);
        for dll in DLLS {
            for (dossier_source, sous) in source.paires() {
                sauvegarder_original(c, sous, dll)?;
                let nom = format!("{dll}.dll");
                remplacer_fichier(
                    &dossier_source.join(&nom),
                    &dossier_windows(c, sous).join(nom),
                )?;
            }
            poser_surcharge(&wine, dll)?;
        }
        // Registre écrit sur disque à l'arrêt du serveur : `est_applique` le relit.
        wine.arreter()
    }

    /// Remet les DLL de Wine sauvegardées et supprime les déclarations.
    /// Répétable : sans sauvegarde ni déclaration, ne fait rien.
    fn retirer(&self, c: &Chemins) -> Resultat<()> {
        let wine = Wine::nouveau(c);
        for dll in DLLS {
            restaurer_original(c, SYSTEM32, dll)?;
            restaurer_original(c, SYSWOW64, dll)?;
            retirer_surcharge(&wine, dll)?;
        }
        wine.arreter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chemins::Chemins;
    use crate::correctifs::dll::prefixe_nu_pour_test;
    use crate::correctifs::Correctif;
    use crate::progression::ProgressionMuette;
    use crate::wine::Wine;

    fn fausse_source(racine: &std::path::Path, contenu: &[u8]) {
        for sous in ["x64", "x32"] {
            std::fs::create_dir_all(racine.join(sous)).unwrap();
            for dll in DLLS {
                std::fs::write(racine.join(sous).join(format!("{dll}.dll")), contenu).unwrap();
            }
        }
    }

    #[test]
    fn la_source_du_paquet_est_prioritaire_sur_le_cache() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let paquet = d.path().join("paquet");
        fausse_source(&paquet, b"p");
        fausse_source(&c.cache.join(DOSSIER_EXTRAIT), b"c");
        let s = source_existante(&c, &paquet).unwrap();
        assert!(s.x64.starts_with(&paquet));
        let s = source_existante(&c, &d.path().join("absent")).unwrap();
        assert!(s.x64.starts_with(&c.cache));
        std::fs::remove_dir_all(c.cache.join(DOSSIER_EXTRAIT)).unwrap();
        assert!(source_existante(&c, &d.path().join("absent")).is_none());
    }

    /// Pose `contenu` pour les 8 DLL dans l'environnement de test.
    fn poser_huit_dll(c: &Chemins, contenu: &[u8]) {
        for sous in [SYSTEM32, SYSWOW64] {
            std::fs::create_dir_all(dossier_windows(c, sous)).unwrap();
            for dll in DLLS {
                std::fs::write(dossier_windows(c, sous).join(format!("{dll}.dll")), contenu)
                    .unwrap();
            }
        }
    }

    /// Table de fausses empreintes : celle de `contenu` pour les 8 DLL.
    fn fausse_table(contenu: &[u8]) -> Vec<(&'static str, &'static str, String)> {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x");
        std::fs::write(&f, contenu).unwrap();
        let sha = empreinte(&f).unwrap();
        [SYSTEM32, SYSWOW64]
            .into_iter()
            .flat_map(|s| DLLS.map(|dll| (s, dll)))
            .map(|(s, dll)| (s, dll, sha.clone()))
            .collect()
    }

    /// Cas réel du bug : environnement importé avec les bons fichiers, mais
    /// aucune copie de référence (ni paquet ni cache) sur la machine.
    #[test]
    fn fichiers_poses_reconnait_sans_copie_de_reference() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        assert!(source_existante(&c, &d.path().join("absent")).is_none());
        let table = fausse_table(b"dxvk");
        let attendues: Vec<_> = table.iter().map(|(s, d, h)| (*s, *d, h.as_str())).collect();
        poser_huit_dll(&c, b"dxvk");
        assert!(fichiers_poses(&c, &attendues).unwrap());
    }

    #[test]
    fn fichiers_poses_refuse_un_fichier_different_ou_absent() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let table = fausse_table(b"dxvk");
        let attendues: Vec<_> = table.iter().map(|(s, d, h)| (*s, *d, h.as_str())).collect();
        // Rien de posé : faux.
        assert!(!fichiers_poses(&c, &attendues).unwrap());
        poser_huit_dll(&c, b"dxvk");
        std::fs::write(c.drive_c().join("windows/syswow64/d3d9.dll"), b"autre").unwrap();
        assert!(!fichiers_poses(&c, &attendues).unwrap());
        // Un fichier retiré : faux aussi.
        std::fs::remove_file(c.drive_c().join("windows/syswow64/d3d9.dll")).unwrap();
        assert!(!fichiers_poses(&c, &attendues).unwrap());
    }

    /// Avec le dossier extrait `src/dxvk/dxvk-3.1.1` (absent en CI, le test se
    /// saute alors), les constantes doivent être celles des vrais fichiers.
    #[test]
    fn les_empreintes_embarquees_sont_celles_des_dll_officielles() {
        let racine = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/dxvk/dxvk-3.1.1");
        let Some(source) = source_dans(&racine) else {
            eprintln!("dossier {racine:?} absent : test sauté");
            return;
        };
        for (sous, dll, sha256) in EMPREINTES_DLL {
            let dossier = if sous == SYSTEM32 {
                &source.x64
            } else {
                &source.x32
            };
            let f = dossier.join(format!("{dll}.dll"));
            assert_eq!(empreinte(&f).unwrap(), sha256, "{f:?}");
        }
    }

    /// Arrête le `wineserver` à la fin du test, même si une assertion échoue.
    struct ArretWine<'a>(&'a Chemins);

    impl Drop for ArretWine<'_> {
        fn drop(&mut self) {
            let _ = Wine::nouveau(self.0).arreter();
        }
    }

    #[test]
    #[ignore = "Wine requis"]
    fn dxvk_appliquer_puis_retirer() {
        let d = tempfile::tempdir().unwrap();
        let c = prefixe_nu_pour_test(d.path());
        let _arret = ArretWine(&c);
        // Contenu d'origine, pour comparer après `retirer`.
        let avant: Vec<_> = ["system32", "syswow64"]
            .iter()
            .flat_map(|s| DLLS.iter().map(move |d| (*s, *d)))
            .map(|(s, d)| {
                let f = c.drive_c().join("windows").join(s).join(format!("{d}.dll"));
                (f.clone(), std::fs::read(f).unwrap())
            })
            .collect();

        assert!(!Dxvk.est_applique(&c).unwrap());
        Dxvk.appliquer(&c, &ProgressionMuette).unwrap();
        assert!(Dxvk.est_applique(&c).unwrap());
        assert!(c.donnees.join("originaux/system32/d3d11.dll").exists());
        assert!(c.donnees.join("originaux/syswow64/d3d9.dll").exists());

        Dxvk.retirer(&c).unwrap();
        assert!(!Dxvk.est_applique(&c).unwrap());
        for (f, contenu) in avant {
            assert_eq!(std::fs::read(&f).unwrap(), contenu, "{f:?}");
        }
        Dxvk.retirer(&c).unwrap(); // répétable
    }
}
