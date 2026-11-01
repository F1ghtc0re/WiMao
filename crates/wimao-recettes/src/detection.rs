//! Reconnaissance du type d'un fichier d'installation et options d'installation silencieuse.
//!
//! Utilise [`wimao_core::inventaire::chercher_premier_motif`] (recherche en flux de plusieurs
//! chaînes en une passe, partagée avec l'inventaire des plugins : la logique n'est pas dupliquée).
//! Utilisé par [`crate::archive`] (reconnaître un installeur dans une archive) et par
//! l'installation d'un plugin (choisir les arguments silencieux).
//!
//! # Où sont les marqueurs ? (observé sur de vrais installeurs, lecture seule)
//!
//! - **Inno Setup** : `Inno Setup Setup Data (6.1.0)` à ~734 Kio du début dans un installeur
//!   de 698 Mio (`Install-Hype-v1.2.1.14-release.exe`), c'est-à-dire dans le programme de
//!   démarrage placé avant les données compressées.
//! - **WiX Burn** : `.wixburn` à l'octet 624 (nom d'une section de l'en-tête PE, donc dans
//!   le premier Kio) de `DB-33_1.4.0_Setup.exe` et `VacuumPro_1.2.0_Setup.exe`.
//! - NSIS et InstallAware n'ont pas pu être observés sur cette machine : ce sont des
//!   chaînes du programme de démarrage, en général au début, sans garantie.
//!
//! Aucun marqueur observé n'est au-delà du premier Mo, mais NSIS/InstallAware sont
//! inconnus et un fichier `Inconnu` doit de toute façon être lu jusqu'au bout pour être
//! déclaré inconnu. La recherche lit donc **tout le fichier en flux** (mémoire constante),
//! en s'arrêtant au premier marqueur trouvé : les cas courants coûtent quelques Kio de
//! lecture, même sur un installeur de plusieurs centaines de Mo.
//!
//! Quand plusieurs marqueurs existent, le plus proche du début du fichier l'emporte
//! (et non l'ordre de la liste) : le programme de démarrage vient toujours avant les
//! données, qui peuvent contenir n'importe quelle chaîne.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use wimao_core::inventaire::chercher_premier_motif;
use wimao_core::{Erreur, Resultat};

/// En-tête des fichiers OLE (conteneur des `.msi`).
const ENTETE_OLE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// En-tête d'un fichier ZIP.
const ENTETE_ZIP: [u8; 4] = *b"PK\x03\x04";
/// Extensions (en minuscules) reconnues comme archives même sans en-tête ZIP.
const EXTENSIONS_ARCHIVE: [&str; 3] = ["zip", "7z", "rar"];

/// Chaînes cherchées dans le fichier, avec le type qu'elles désignent.
const MARQUEURS: [(&[u8], TypeInstalleur); 4] = [
    (b"Inno Setup", TypeInstalleur::Inno),
    (b".wixburn", TypeInstalleur::WixBurn),
    (b"Nullsoft", TypeInstalleur::Nsis),
    (b"InstallAware", TypeInstalleur::InstallAware),
];

/// Options silencieuses de chaque famille (contraintes globales du projet).
const ARGS_INNO: [&str; 4] = ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"];
/// WiX Burn : `-quiet -norestart`.
const ARGS_WIXBURN: [&str; 2] = ["-quiet", "-norestart"];
/// NSIS : `/S` (la casse compte : `/s` est ignoré par NSIS).
const ARGS_NSIS: [&str; 1] = ["/S"];
/// InstallAware : `/s`.
const ARGS_INSTALLAWARE: [&str; 1] = ["/s"];
/// MSI, autour du chemin du fichier : `msiexec /i <f> /qn /norestart`.
const ARGS_MSI_AVANT: [&str; 2] = ["msiexec", "/i"];
/// Fin des arguments MSI.
const ARGS_MSI_APRES: [&str; 2] = ["/qn", "/norestart"];

/// Famille d'un fichier d'installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeInstalleur {
    /// Inno Setup.
    Inno,
    /// Paquet Windows Installer (`.msi`).
    Msi,
    /// Installeur WiX Burn (bundle `.exe`).
    WixBurn,
    /// Nullsoft Scriptable Install System.
    Nsis,
    /// InstallAware.
    InstallAware,
    /// Archive (zip, 7z, rar) : à extraire, pas à exécuter.
    Archive,
    /// Rien de connu : installation normale (interface visible) à proposer.
    Inconnu,
}

/// Reconnaît le type de `fichier`. Règles, dans l'ordre : en-tête OLE → `Msi` ;
/// en-tête `PK\x03\x04` ou extension `.zip`/`.7z`/`.rar` → `Archive` ; puis recherche en
/// flux des marqueurs (voir la documentation du module) ; sinon `Inconnu`.
///
/// Erreurs : [`Erreur::Io`] (le message contient le chemin) si le fichier est illisible.
///
/// ```no_run
/// use std::path::Path;
/// use wimao_recettes::detection::{detecter, TypeInstalleur};
/// assert_eq!(detecter(Path::new("Setup.msi")).unwrap(), TypeInstalleur::Msi);
/// ```
pub fn detecter(fichier: &Path) -> Resultat<TypeInstalleur> {
    detecter_io(fichier).map_err(|e| {
        Erreur::Io(io::Error::new(
            e.kind(),
            format!("{} : {e}", fichier.display()),
        ))
    })
}

/// Cœur de [`detecter`], en erreurs d'E/S brutes (le chemin est ajouté par l'appelant).
fn detecter_io(fichier: &Path) -> io::Result<TypeInstalleur> {
    let mut flux = File::open(fichier)?;
    let mut entete = [0u8; ENTETE_OLE.len()];
    let lus = lire_au_plus(&mut flux, &mut entete)?;
    if entete[..lus].starts_with(&ENTETE_OLE) {
        return Ok(TypeInstalleur::Msi);
    }
    if entete[..lus].starts_with(&ENTETE_ZIP) || a_extension_archive(fichier) {
        return Ok(TypeInstalleur::Archive);
    }
    // On repart du début : l'en-tête déjà lu fait partie de la zone cherchée.
    let mut flux = File::open(fichier)?;
    let motifs: Vec<&[u8]> = MARQUEURS.iter().map(|(motif, _)| *motif).collect();
    Ok(match chercher_premier_motif(&mut flux, &motifs)? {
        Some(indice) => MARQUEURS[indice].1,
        None => TypeInstalleur::Inconnu,
    })
}

/// Lit jusqu'à remplir `tampon` ou jusqu'à la fin du fichier ; rend le nombre d'octets lus
/// (un fichier plus court que l'en-tête n'est pas une erreur).
fn lire_au_plus(flux: &mut impl Read, tampon: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < tampon.len() {
        match flux.read(&mut tampon[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

/// Vrai si l'extension (sans casse) est celle d'une archive connue.
fn a_extension_archive(fichier: &Path) -> bool {
    fichier
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .is_some_and(|e| EXTENSIONS_ARCHIVE.contains(&e.as_str()))
}

/// Arguments à passer pour installer en silence, ou `None` quand il n'y en a pas
/// (`Inconnu` : on ne sait pas ; `Archive` : on extrait, on n'exécute pas).
///
/// Pour `Msi`, la commande entière est rendue (`msiexec /i <fichier> /qn /norestart`) ;
/// pour les autres, uniquement les options à placer après le chemin de l'installeur.
/// Valeurs : contraintes globales du projet (Inno, WiX Burn, NSIS, InstallAware).
pub fn arguments_silencieux(t: TypeInstalleur, fichier: &Path) -> Option<Vec<OsString>> {
    let texte = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
    match t {
        TypeInstalleur::Inno => Some(texte(&ARGS_INNO)),
        TypeInstalleur::WixBurn => Some(texte(&ARGS_WIXBURN)),
        TypeInstalleur::Nsis => Some(texte(&ARGS_NSIS)),
        TypeInstalleur::InstallAware => Some(texte(&ARGS_INSTALLAWARE)),
        TypeInstalleur::Msi => {
            let mut args = texte(&ARGS_MSI_AVANT);
            args.push(fichier.as_os_str().to_owned());
            args.extend(texte(&ARGS_MSI_APRES));
            Some(args)
        }
        TypeInstalleur::Archive | TypeInstalleur::Inconnu => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Échantillon de `tests/echantillons/` : synthétique, ou installeur factice fabriqué par
    /// nous (NSIS, MSI) ; aucun extrait d'installeur propriétaire n'est dans le dépôt.
    fn echantillon(nom: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/echantillons")
            .join(nom)
    }

    #[test]
    fn chaque_echantillon_est_reconnu() {
        let attendus = [
            ("inno.bin", TypeInstalleur::Inno),
            ("wixburn.bin", TypeInstalleur::WixBurn),
            ("nsis.bin", TypeInstalleur::Nsis),
            ("installaware.bin", TypeInstalleur::InstallAware),
            ("msi.bin", TypeInstalleur::Msi),
            ("inconnu.bin", TypeInstalleur::Inconnu),
        ];
        for (nom, type_attendu) in attendus {
            assert_eq!(detecter(&echantillon(nom)).unwrap(), type_attendu, "{nom}");
        }
    }

    #[test]
    fn archives_par_entete_et_par_extension() {
        let t = tempfile::tempdir().unwrap();
        let zip = t.path().join("sans-extension");
        std::fs::write(&zip, b"PK\x03\x04reste").unwrap();
        assert_eq!(detecter(&zip).unwrap(), TypeInstalleur::Archive);
        for ext in ["7z", "RAR", "zip"] {
            let f = t.path().join(format!("a.{ext}"));
            std::fs::write(&f, b"n'importe quoi").unwrap();
            assert_eq!(detecter(&f).unwrap(), TypeInstalleur::Archive, "{ext}");
        }
    }

    #[test]
    fn fichier_vide_ou_plus_court_que_l_entete_est_inconnu() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("vide.exe");
        std::fs::write(&f, b"").unwrap();
        assert_eq!(detecter(&f).unwrap(), TypeInstalleur::Inconnu);
        std::fs::write(&f, b"MZ").unwrap();
        assert_eq!(detecter(&f).unwrap(), TypeInstalleur::Inconnu);
    }

    #[test]
    fn marqueur_loin_dans_le_fichier_est_trouve() {
        // Bien au-delà d'un bloc de lecture : la recherche lit tout le fichier.
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("gros.exe");
        let mut contenu = vec![0u8; 300_000];
        contenu.extend_from_slice(b"Nullsoft");
        std::fs::write(&f, contenu).unwrap();
        assert_eq!(detecter(&f).unwrap(), TypeInstalleur::Nsis);
    }

    #[test]
    fn le_marqueur_le_plus_proche_du_debut_l_emporte() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("double.exe");
        std::fs::write(&f, b"MZ..Nullsoft....Inno Setup").unwrap();
        assert_eq!(detecter(&f).unwrap(), TypeInstalleur::Nsis);
    }

    #[test]
    fn fichier_absent_donne_une_erreur_avec_le_chemin() {
        let e = detecter(Path::new("/nexiste/pas.exe")).unwrap_err();
        assert!(matches!(e, Erreur::Io(_)));
        assert!(e.to_string().contains("/nexiste/pas.exe"));
    }

    #[test]
    fn arguments_silencieux_valeurs_exactes() {
        let f = Path::new("/tmp/mon plugin.msi");
        let args = |t| {
            arguments_silencieux(t, f).map(|v| {
                v.iter()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(
            args(TypeInstalleur::Inno).unwrap(),
            ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/SP-"]
        );
        assert_eq!(
            args(TypeInstalleur::WixBurn).unwrap(),
            ["-quiet", "-norestart"]
        );
        assert_eq!(args(TypeInstalleur::Nsis).unwrap(), ["/S"]);
        assert_eq!(args(TypeInstalleur::InstallAware).unwrap(), ["/s"]);
        assert_eq!(
            args(TypeInstalleur::Msi).unwrap(),
            ["msiexec", "/i", "/tmp/mon plugin.msi", "/qn", "/norestart"]
        );
        assert!(args(TypeInstalleur::Inconnu).is_none());
        assert!(args(TypeInstalleur::Archive).is_none());
    }

    /// Lit `WIMAO_TEST_INSTALLEURS` (format `type=chemin;type=chemin`) et vérifie que
    /// chaque vrai installeur est reconnu ; sans la variable, se saute. Les installeurs sont propriétaires : ils ne
    /// sont jamais dans le dépôt, chacun fournit les siens.
    ///
    /// Exemple : `WIMAO_TEST_INSTALLEURS="inno=/x/Setup.exe;wixburn=/x/Autre.exe"
    /// cargo test -p wimao-recettes -- --ignored`.
    #[test]
    #[ignore = "installeurs réels requis"]
    fn installeurs_reels() {
        // Sans la variable, le test se saute en le disant (comme `WIMAO_FIXTURES`) : la CI
        // lance `--ignored` sur tout l'espace de travail et n'a pas d'installeurs réels.
        let Ok(liste) = std::env::var("WIMAO_TEST_INSTALLEURS") else {
            eprintln!(
                "installeurs_reels sauté : définir WIMAO_TEST_INSTALLEURS=type=chemin;type=chemin"
            );
            return;
        };
        for paire in liste.split(';').filter(|p| !p.is_empty()) {
            let (type_texte, chemin) = paire.split_once('=').expect("format type=chemin");
            let attendu = match type_texte {
                "inno" => TypeInstalleur::Inno,
                "wixburn" => TypeInstalleur::WixBurn,
                "nsis" => TypeInstalleur::Nsis,
                "installaware" => TypeInstalleur::InstallAware,
                "msi" => TypeInstalleur::Msi,
                "archive" => TypeInstalleur::Archive,
                "inconnu" => TypeInstalleur::Inconnu,
                autre => panic!("type inconnu : {autre}"),
            };
            let debut = std::time::Instant::now();
            let trouve = detecter(Path::new(chemin)).unwrap();
            println!("{chemin} : {trouve:?} en {:?}", debut.elapsed());
            assert_eq!(trouve, attendu, "{chemin}");
        }
    }
}
