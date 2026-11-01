//! Correctif msvcp140 : la bibliothèque C++ de Microsoft à la place de celle de Wine.
//!
//! Utilise [`Wine`], [`telecharger_verifie`], `cabextract` et les outils de `dll`.
//! Utilisé par `correctifs::correctifs`.
//!
//! Pourquoi : Maschine 3 appelle `basic_istream::operator=(&&)`, absente du
//! `msvcp140.dll` de Wine (NOTES.md). On pose donc celui de Microsoft, tiré du
//! « VC++ Redistributable » comme dans `scripts/install-msvcp140.sh`.
//!
//! Deux sources, dans l'ordre :
//! 1. le cache de l'installeur Microsoft, déjà dans l'environnement
//!    (`ProgramData/Package Cache/**/vcRuntimeMinimum_{amd64,x86}/cab1.cab`),
//!    dont `cabextract` sort `msvcp140.dll_amd64` et `msvcp140.dll` ;
//! 2. à défaut, on installe `vc_redist.x64.exe` et `vc_redist.x86.exe` en silencieux,
//!    ce qui crée ce cache, puis on reprend la source 1.

use super::dll::{
    dossier_windows, fichiers_identiques, poser_surcharge, remplacer_fichier, restaurer_original,
    retirer_surcharge, sauvegarder_original, surcharge_est_native, SYSTEM32, SYSWOW64,
};
use super::Correctif;
use crate::chemins::Chemins;
use crate::erreur::{Erreur, Resultat};
use crate::i18n::{tr, trf};
use crate::outils::lancer;
use crate::progression::Progression;
use crate::telechargement::{empreinte, telecharger_verifie};
use crate::wine::Wine;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Nom de la DLL (sans extension), aussi nom de la déclaration `native,builtin`.
const DLL: &str = "msvcp140";
/// Adresse stable de Microsoft pour le « VC++ Redistributable » 64 bits (dernière version 14.x).
const URL_VC_REDIST_X64: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";
/// Idem pour la version 32 bits.
const URL_VC_REDIST_X86: &str = "https://aka.ms/vs/17/release/vc_redist.x86.exe";
/// Noms des installeurs dans le cache de WiMao.
const FICHIER_VC_REDIST_X64: &str = "vc_redist.x64.exe";
/// Voir [`FICHIER_VC_REDIST_X64`].
const FICHIER_VC_REDIST_X86: &str = "vc_redist.x86.exe";
/// Options silencieuses de l'installeur Microsoft (WiX Burn) : `/quiet` sans
/// fenêtre, `/norestart` pour qu'il ne tente pas de redémarrer la machine.
const OPTIONS_SILENCIEUSES: [&str; 3] = ["/install", "/quiet", "/norestart"];
/// Code de sortie de l'installeur quand tout est fait mais qu'il « demande un
/// redémarrage » (`ERROR_SUCCESS_REBOOT_REQUIRED`) : sans objet sous Wine, c'est un succès.
const CODE_REDEMARRAGE_DEMANDE: i32 = 3010;
/// Délai de chaque installeur (comme les autres installations silencieuses, NOTES.md :
/// jamais plus de 30 min ; celui-ci copie quelques fichiers, 10 min suffisent).
const DELAI_REDISTRIBUABLE: Duration = Duration::from_secs(10 * 60);
/// Délai de `cabextract` : quelques Mo à décompresser.
const DELAI_CABEXTRACT: Duration = Duration::from_secs(60);
/// Dossier du Package Cache de Microsoft sous `drive_c`.
const DOSSIER_PACKAGE_CACHE: &str = "ProgramData/Package Cache";
/// Sous-dossier d'un paquet du Package Cache qui contient les `vcRuntimeMinimum_*`.
const DOSSIER_PAQUETS: &str = "packages";
/// Paquet 64 bits du runtime minimal.
const PAQUET_X64: &str = "vcRuntimeMinimum_amd64";
/// Paquet 32 bits du runtime minimal.
const PAQUET_X86: &str = "vcRuntimeMinimum_x86";
/// Archive qui contient `msvcp140.dll`.
const NOM_CAB: &str = "cab1.cab";
/// Noms possibles de la DLL 64 bits dans le cab : Microsoft y ajoute l'architecture.
const DLLS_DANS_CAB_X64: [&str; 1] = ["msvcp140.dll_amd64"];
/// Noms possibles de la DLL 32 bits dans le cab. Le cab de la version 14.44 (constaté
/// avec Wine v2) la nomme `msvcp140.dll_x86`, alors que celui du script d'origine
/// (Native Access, version plus ancienne) la nommait `msvcp140.dll`.
const DLLS_DANS_CAB_X86: [&str; 2] = ["msvcp140.dll_x86", "msvcp140.dll"];
/// Dossier de travail (sous le cache de WiMao) où sont extraites les DLL.
const DOSSIER_EXTRACTION: &str = "msvcp140";
/// Empreinte de la DLL 64 bits testée avec Maschine 3 (Global Constraints).
const SHA256_CONNUE_X64: &str = "0f885b509a685d2bbfa652fed26b5fb31d88fbdab0a978c641d1c7b8aa460aa9";
/// Empreinte de la DLL 32 bits testée.
const SHA256_CONNUE_X86: &str = "83f935454ae8e450b6f042509ecf28cceff95edb2495c63a782b9d45c2eaf1c0";
/// `msvcp140.dll` 64 bits de Wine, dans son dossier d'installation.
const DLL_WINE_X64: &str = "lib/wine/x86_64-windows/msvcp140.dll";
/// `msvcp140.dll` 32 bits de Wine.
const DLL_WINE_X86: &str = "lib/wine/i386-windows/msvcp140.dll";

/// `msvcp140.dll` de Microsoft.
#[derive(Debug, Clone, Copy)]
pub struct Msvcp140;

/// Les deux archives `cab1.cab` (64 et 32 bits) de la version la plus récente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cabs {
    /// Archive du paquet 64 bits.
    pub x64: PathBuf,
    /// Archive du paquet 32 bits.
    pub x86: PathBuf,
}

/// Version d'un dossier du Package Cache nommé `{GUID}v<version>` (par exemple
/// `{abc}v14.40.33810`) sous forme de nombres, pour une comparaison numérique :
/// 14.10 est plus récent que 14.9, ce que ne verrait pas l'ordre alphabétique.
/// Un nom sans version donne une liste vide, donc le plus ancien.
fn version_du_dossier(nom: &str) -> Vec<u64> {
    let Some((_, version)) = nom.rsplit_once('v') else {
        return Vec::new();
    };
    version.split('.').map(|n| n.parse().unwrap_or(0)).collect()
}

/// Le `cab1.cab` du paquet `paquet` de la version la plus récente du Package Cache.
fn cab_le_plus_recent(cache: &Path, paquet: &str) -> Resultat<Option<PathBuf>> {
    let mut candidats = Vec::new();
    for entree in std::fs::read_dir(cache)? {
        let entree = entree?;
        let cab = entree
            .path()
            .join(DOSSIER_PAQUETS)
            .join(paquet)
            .join(NOM_CAB);
        if cab.is_file() {
            let version = version_du_dossier(&entree.file_name().to_string_lossy());
            candidats.push((version, cab));
        }
    }
    // Versions égales (GUID différents) : le chemin départage, pour un résultat stable.
    Ok(candidats.into_iter().max().map(|(_, cab)| cab))
}

impl Msvcp140 {
    /// Cherche dans le Package Cache de `drive_c` les `cab1.cab` 64 et 32 bits de la
    /// version la plus récente (comme `find … | sort -V | tail -1` du script).
    ///
    /// `None` si le cache n'existe pas ou s'il manque l'un des deux paquets : on
    /// ne pose jamais une seule des deux architectures.
    ///
    /// Erreurs : [`Erreur::Io`] si le cache existe mais ne peut pas être lu.
    pub fn trouver_cabs(drive_c: &Path) -> Resultat<Option<Cabs>> {
        let cache = drive_c.join(DOSSIER_PACKAGE_CACHE);
        if !cache.is_dir() {
            return Ok(None);
        }
        let x64 = cab_le_plus_recent(&cache, PAQUET_X64)?;
        let x86 = cab_le_plus_recent(&cache, PAQUET_X86)?;
        Ok(x64.zip(x86).map(|(x64, x86)| Cabs { x64, x86 }))
    }
}

/// Extrait `msvcp140.dll*` de `cab` dans un dossier `dest` neuf.
fn extraire_cab(cab: &Path, dest: &Path) -> Resultat<()> {
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest)?;
    let mut cmd = Command::new("cabextract");
    cmd.arg("-q")
        .arg("-d")
        .arg(dest)
        .arg("-F")
        .arg(format!("{DLL}.dll*"))
        .arg(cab);
    let sortie = lancer(&mut cmd, DELAI_CABEXTRACT).map_err(|e| match e {
        // `lancer` rend `Introuvable` pour un programme absent : on précise le paquet.
        Erreur::Introuvable(_) => Erreur::Introuvable(tr(
            "cabextract (install the \"cabextract\" package of your distribution)",
        )),
        autre => autre,
    })?;
    if sortie.code != 0 {
        return Err(Erreur::Commande {
            programme: format!("cabextract {}", cab.display()),
            code: Some(sortie.code),
            sortie: sortie.texte,
        });
    }
    Ok(())
}

/// Prévient si l'empreinte de `fichier` n'est pas celle déjà testée. La DLL est
/// utilisée quand même : Microsoft publie de nouvelles versions compatibles.
fn prevenir_si_inconnue(p: &dyn Progression, fichier: &Path, connue: &str) -> Resultat<()> {
    let hash = empreinte(fichier)?;
    if hash != connue {
        p.etape(&trf(
            "Unknown Microsoft library version, used anyway: {hash}",
            &[("hash", &hash)],
        ));
    }
    Ok(())
}

/// Source 2 : télécharge puis installe en silencieux les deux installeurs Microsoft.
fn installer_redistribuables(c: &Chemins) -> Resultat<()> {
    // `sha256: None` : Microsoft remplace ces fichiers sans prévenir ; ce qui compte
    // (la DLL) est comparé aux empreintes connues après extraction.
    let installeurs = [
        (URL_VC_REDIST_X64, FICHIER_VC_REDIST_X64),
        (URL_VC_REDIST_X86, FICHIER_VC_REDIST_X86),
    ];
    for (url, fichier) in installeurs {
        let exe = telecharger_verifie(&[url], None, &c.cache.join(fichier))?;
        lancer_installeur(c, &exe)?;
    }
    Ok(())
}

/// Lance un installeur Microsoft dans Wine ; 0 et 3010 sont des succès.
fn lancer_installeur(c: &Chemins, exe: &Path) -> Resultat<()> {
    let mut args: Vec<&OsStr> = vec![exe.as_os_str()];
    args.extend(OPTIONS_SILENCIEUSES.iter().map(OsStr::new));
    let sortie = Wine::nouveau(c).executer(&args, DELAI_REDISTRIBUABLE)?;
    if sortie.code != 0 && sortie.code != CODE_REDEMARRAGE_DEMANDE {
        return Err(Erreur::Commande {
            programme: format!("wine {} {}", exe.display(), OPTIONS_SILENCIEUSES.join(" ")),
            code: Some(sortie.code),
            sortie: sortie.texte,
        });
    }
    Ok(())
}

/// Le premier des `noms` qui existe dans `dossier` (voir [`DLLS_DANS_CAB_X86`]).
fn dll_extraite(dossier: &Path, noms: &[&str]) -> Resultat<PathBuf> {
    noms.iter()
        .map(|nom| dossier.join(nom))
        .find(|f| f.is_file())
        .ok_or_else(|| {
            Erreur::Introuvable(trf(
                "{names} in {folder} after cabextract",
                &[
                    ("names", &noms.join(" | ")),
                    ("folder", &dossier.display().to_string()),
                ],
            ))
        })
}

/// Les deux DLL extraites des cabs : (64 bits, 32 bits).
fn extraire_les_dll(c: &Chemins, cabs: &Cabs) -> Resultat<(PathBuf, PathBuf)> {
    let racine = c.cache.join(DOSSIER_EXTRACTION);
    let (dossier64, dossier86) = (racine.join("x64"), racine.join("x86"));
    extraire_cab(&cabs.x64, &dossier64)?;
    extraire_cab(&cabs.x86, &dossier86)?;
    Ok((
        dll_extraite(&dossier64, &DLLS_DANS_CAB_X64)?,
        dll_extraite(&dossier86, &DLLS_DANS_CAB_X86)?,
    ))
}

impl Correctif for Msvcp140 {
    fn nom(&self) -> &'static str {
        "msvcp140"
    }

    /// Appliqué si les fichiers de l'environnement diffèrent de ceux de Wine
    /// (`<wine>/lib/wine/{x86_64,i386}-windows/msvcp140.dll`) et que la
    /// déclaration vaut `native,builtin`. Les deux conditions sont nécessaires : un
    /// environnement neuf contient déjà des fichiers qui diffèrent de ceux de Wine
    /// (simples substituts), mais pas la déclaration.
    ///
    /// Si la DLL de Wine à laquelle comparer manque, la réponse est « non » (rien à
    /// comparer). La déclaration est lue dans `user.reg`, sans lancer Wine ; toute autre
    /// erreur d'entrée/sortie remonte.
    fn est_applique(&self, c: &Chemins) -> Resultat<bool> {
        let nom = format!("{DLL}.dll");
        for (sous, relatif) in [(SYSTEM32, DLL_WINE_X64), (SYSWOW64, DLL_WINE_X86)] {
            let wine = c.wine_dir.join(relatif);
            if !wine.exists() {
                // Sans DLL de référence on ne peut rien comparer : « non appliqué ».
                return Ok(false);
            }
            let pose = dossier_windows(c, sous).join(&nom);
            if !pose.exists() || fichiers_identiques(&pose, &wine)? {
                return Ok(false);
            }
        }
        surcharge_est_native(c, DLL)
    }

    /// Pose `msvcp140.dll` de Microsoft (64 bits dans `system32`, 32 bits dans
    /// `syswow64`) après avoir sauvegardé celles de Wine, et la déclare `native,builtin`.
    ///
    /// Erreurs : téléchargement, [`Erreur::Commande`] (installeur, `cabextract`),
    /// [`Erreur::Delai`], [`Erreur::Introuvable`] (cabextract absent, ou cabs
    /// toujours absents après l'installation Microsoft), [`Erreur::Io`].
    fn appliquer(&self, c: &Chemins, p: &dyn Progression) -> Resultat<()> {
        p.etape(&tr("Installing the Microsoft C++ library (msvcp140)…"));
        // Avant tout : les installeurs Microsoft écrasent eux-mêmes `msvcp140.dll`
        // dans le préfixe (constaté), l'original de Wine serait perdu ensuite.
        for sous in [SYSTEM32, SYSWOW64] {
            sauvegarder_original(c, sous, DLL)?;
        }
        let cabs = match Msvcp140::trouver_cabs(&c.drive_c())? {
            Some(cabs) => cabs,
            None => {
                installer_redistribuables(c)?;
                Msvcp140::trouver_cabs(&c.drive_c())?.ok_or_else(|| {
                    Erreur::Introuvable(tr(
                        "cab1.cab of vcRuntimeMinimum in ProgramData/Package Cache after installing the VC++ Redistributable",
                    ))
                })?
            }
        };
        let (dll64, dll32) = extraire_les_dll(c, &cabs)?;
        prevenir_si_inconnue(p, &dll64, SHA256_CONNUE_X64)?;
        prevenir_si_inconnue(p, &dll32, SHA256_CONNUE_X86)?;
        let nom = format!("{DLL}.dll");
        for (source, sous) in [(&dll64, SYSTEM32), (&dll32, SYSWOW64)] {
            remplacer_fichier(source, &dossier_windows(c, sous).join(&nom))?;
        }
        let wine = Wine::nouveau(c);
        poser_surcharge(&wine, DLL)?;
        // Registre écrit sur disque à l'arrêt du serveur : `est_applique` le relit.
        wine.arreter()
    }

    /// Remet les `msvcp140.dll` de Wine et supprime la déclaration. Répétable.
    fn retirer(&self, c: &Chemins) -> Resultat<()> {
        restaurer_original(c, SYSTEM32, DLL)?;
        restaurer_original(c, SYSWOW64, DLL)?;
        let wine = Wine::nouveau(c);
        retirer_surcharge(&wine, DLL)?;
        wine.arreter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correctifs::dll::prefixe_nu_pour_test;
    use crate::progression::ProgressionMuette;
    use std::path::Path;

    fn creer_cab(drive_c: &Path, version_dossier: &str, paquet: &str) {
        let d = drive_c
            .join("ProgramData/Package Cache")
            .join(version_dossier)
            .join("packages")
            .join(paquet);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cab1.cab"), b"cab").unwrap();
    }

    #[test]
    fn trouver_cabs_retient_la_version_la_plus_recente() {
        let d = tempfile::tempdir().unwrap();
        for v in ["{A}v14.38.33135", "{A}v14.40.33810"] {
            creer_cab(d.path(), v, "vcRuntimeMinimum_amd64");
            creer_cab(d.path(), v, "vcRuntimeMinimum_x86");
        }
        let cabs = Msvcp140::trouver_cabs(d.path()).unwrap().unwrap();
        assert!(cabs.x64.to_string_lossy().contains("v14.40"), "{cabs:?}");
        assert!(cabs.x86.to_string_lossy().contains("v14.40"), "{cabs:?}");
    }

    #[test]
    fn la_comparaison_est_numerique_pas_alphabetique() {
        let d = tempfile::tempdir().unwrap();
        for v in ["{B}v14.9.1", "{A}v14.10.1"] {
            creer_cab(d.path(), v, "vcRuntimeMinimum_amd64");
            creer_cab(d.path(), v, "vcRuntimeMinimum_x86");
        }
        let cabs = Msvcp140::trouver_cabs(d.path()).unwrap().unwrap();
        assert!(cabs.x64.to_string_lossy().contains("v14.10"), "{cabs:?}");
    }

    #[test]
    fn trouver_cabs_sans_cache_ou_incomplet() {
        let d = tempfile::tempdir().unwrap();
        assert!(Msvcp140::trouver_cabs(d.path()).unwrap().is_none());
        creer_cab(d.path(), "{A}v14.40.1", "vcRuntimeMinimum_amd64");
        assert!(Msvcp140::trouver_cabs(d.path()).unwrap().is_none());
    }

    #[test]
    fn comparaison_des_versions() {
        assert!(version_du_dossier("{A}v14.10.1") > version_du_dossier("{B}v14.9.9"));
        assert!(version_du_dossier("{A}v14.40") > version_du_dossier("{A}sans-version"));
    }

    /// Arrête le `wineserver` à la fin du test, même si une assertion échoue.
    struct ArretWine<'a>(&'a Chemins);

    impl Drop for ArretWine<'_> {
        fn drop(&mut self) {
            let _ = Wine::nouveau(self.0).arreter();
        }
    }

    /// Source 2 (téléchargement chez Microsoft, donc réseau requis) puis `retirer`.
    #[test]
    #[ignore = "Wine requis"]
    fn msvcp_installe_depuis_microsoft_puis_retire() {
        let d = tempfile::tempdir().unwrap();
        let c = prefixe_nu_pour_test(d.path());
        let _arret = ArretWine(&c);
        let avant: Vec<_> = [SYSTEM32, SYSWOW64]
            .iter()
            .map(|s| {
                let f = dossier_windows(&c, s).join("msvcp140.dll");
                (f.clone(), std::fs::read(f).unwrap())
            })
            .collect();
        // Ni cache ni déclaration dans un environnement neuf (hors Mono/DXVK).
        assert!(Msvcp140::trouver_cabs(&c.drive_c()).unwrap().is_none());
        assert!(!Msvcp140.est_applique(&c).unwrap());

        Msvcp140.appliquer(&c, &ProgressionMuette).unwrap();
        assert!(Msvcp140.est_applique(&c).unwrap());

        Msvcp140.retirer(&c).unwrap();
        assert!(!Msvcp140.est_applique(&c).unwrap());
        for (f, contenu) in avant {
            assert_eq!(std::fs::read(&f).unwrap(), contenu, "{f:?}");
        }
    }
}
