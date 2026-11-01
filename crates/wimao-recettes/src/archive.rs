//! Installation depuis une archive (zip, 7z, rar) : extraction puis tri de ce qu'elle contient.
//!
//! Utilise l'outil externe `7z` (paquet `7zip`/`p7zip-full`) via [`wimao_core::lancer`],
//! [`crate::detection::detecter`] pour reconnaître un installeur, la recherche de
//! `VSTPluginMain` de `wimao-core` pour reconnaître une DLL VST2, et `walkdir`.
//! Utilisé par l'installation d'un plugin (`installation.rs`) quand le fichier téléchargé est une archive.
//!
//! # Sécurité : archive malveillante
//!
//! L'archive est extraite dans un dossier temporaire neuf (sous le cache de WiMao) et
//! rien n'est copié ailleurs que depuis l'intérieur de ce dossier :
//! - `7z x` refuse d'écrire hors du dossier de sortie (les `../` des noms sont neutralisés) ;
//! - on ne suit **aucun lien symbolique** (une archive peut en contenir un vers `/etc` ou
//!   `~/.ssh` : le copier reviendrait à publier ce fichier dans le préfixe Wine) ;
//! - seul le nom de fichier (`file_name`) sert à construire la destination, jamais le
//!   chemin relatif de l'archive.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use walkdir::WalkDir;
use wimao_core::i18n::{tr, trf};
use wimao_core::inventaire::{est_vst2, DOSSIER_CLAP, DOSSIER_VST2, DOSSIER_VST3};
use wimao_core::{lancer, Chemins, Erreur, Progression, Resultat};

use crate::detection::{detecter, TypeInstalleur};

/// Outil d'extraction (déjà exigé par le projet, voir `ARCHITECTURE.md`).
pub(crate) const OUTIL_7Z: &str = "7z";
/// Délai maximal d'une extraction : large, des archives de plusieurs Gio existent.
const DELAI_EXTRACTION: Duration = Duration::from_secs(30 * 60);
/// Code de sortie de `7z` pour « avertissement » (fichier ignoré, etc.) : l'extraction a
/// eu lieu, on continue. 2 et plus = erreur fatale.
const CODE_7Z_AVERTISSEMENT: i32 = 1;
/// Dossier, sous le cache, des extractions temporaires et des installeurs sortis des archives.
const DOSSIER_CACHE: &str = "archives";

/// Ce que contenait une archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContenuArchive {
    /// Un seul installeur reconnu, copié hors du dossier temporaire (qui est supprimé) :
    /// l'appelant l'installe lui-même.
    Installeur(PathBuf),
    /// Plugins copiés dans le préfixe : chemins de destination.
    PluginsCopies(Vec<PathBuf>),
    /// Rien d'utilisable.
    Vide,
}

/// Ce qui a été trouvé dans le dossier extrait (chemins à l'intérieur de celui-ci).
#[derive(Debug, Default)]
struct Trouvailles {
    /// `.exe`/`.msi` dont le type est reconnu (ni `Inconnu` ni `Archive`).
    installeurs: Vec<PathBuf>,
    /// Bundles ou fichiers `.vst3`.
    vst3: Vec<PathBuf>,
    /// Fichiers `.clap`.
    clap: Vec<PathBuf>,
    /// DLL qui exportent `VSTPluginMain`.
    vst2: Vec<PathBuf>,
}

/// Extrait `archive` et en tire soit l'installeur unique, soit les plugins.
///
/// - exactement un `.exe`/`.msi` reconnu → [`ContenuArchive::Installeur`] ;
/// - sinon les `*.vst3` vont dans `Common Files/VST3`, les `*.clap` dans `Common Files/CLAP`
///   et les `*.dll` contenant `VSTPluginMain` dans `Common Files/VST2` du `drive_c` de
///   `c.prefixe` → [`ContenuArchive::PluginsCopies`] ; rien de tout cela → `Vide`.
///
/// Le dossier temporaire est supprimé dans tous les cas, y compris en cas d'erreur.
///
/// Erreurs : [`Erreur::Introuvable`] si `7z` n'est pas installé ; [`Erreur::Commande`] si
/// l'extraction échoue (archive corrompue, mot de passe…) ; [`Erreur::Delai`] ;
/// [`Erreur::Io`] pour les copies.
pub fn installer_archive(
    c: &Chemins,
    archive: &Path,
    p: &dyn Progression,
) -> Resultat<ContenuArchive> {
    installer_archive_avec(c, archive, p, Path::new(OUTIL_7Z))
}

/// Comme [`installer_archive`] avec l'outil d'extraction à utiliser : permet aux tests
/// d'injecter un faux `7z` (script) pour provoquer un avertissement à volonté.
fn installer_archive_avec(
    c: &Chemins,
    archive: &Path,
    p: &dyn Progression,
    outil: &Path,
) -> Resultat<ContenuArchive> {
    let cache = c.cache.join(DOSSIER_CACHE);
    fs::create_dir_all(&cache)?;
    // `TempDir` supprime le dossier quand il sort de portée, même sur erreur.
    let temporaire = tempfile::Builder::new()
        .prefix("extraction-")
        .tempdir_in(&cache)?;
    p.etape(&trf(
        "Extracting {name}",
        &[("name", &nom_affiche(archive))],
    ));
    extraire(outil, archive, temporaire.path(), p)?;

    let trouvailles = explorer(temporaire.path())?;
    if let [installeur] = trouvailles.installeurs.as_slice() {
        return sortir_installeur(installeur, &cache, archive).map(ContenuArchive::Installeur);
    }
    copier_plugins(c, &trouvailles, p)
}

/// Nom de fichier d'un chemin, pour les messages.
fn nom_affiche(chemin: &Path) -> String {
    chemin
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Lance `7z x -y -o<destination> -- <archive>`.
///
/// `--` : une archive dont le nom commence par `-` ne doit pas être prise pour une option.
/// `-y` : répondre oui aux questions (écrasement), car rien ne peut répondre au clavier.
///
/// Un code 1 (avertissement) ne fait pas échouer l'extraction mais peut signaler des
/// fichiers non extraits : il est annoncé par `p` avec la ligne d'avertissement de `7z`.
pub(crate) fn extraire(
    outil: &Path,
    archive: &Path,
    destination: &Path,
    p: &dyn Progression,
) -> Resultat<()> {
    let mut sortie_option = std::ffi::OsString::from("-o");
    sortie_option.push(destination);
    let mut commande = Command::new(outil);
    commande
        .arg("x")
        .arg("-y")
        .arg(sortie_option)
        .arg("--")
        .arg(archive);
    let sortie = match lancer(&mut commande, DELAI_EXTRACTION) {
        // `lancer` rend `Introuvable` pour un programme absent : on précise le paquet.
        Err(Erreur::Introuvable(_)) => {
            return Err(Erreur::Introuvable(tr(
                "The tool 7z is not installed (package 7zip or p7zip-full)",
            )))
        }
        autre => autre?,
    };
    if sortie.code == 0 {
        return Ok(());
    }
    if sortie.code == CODE_7Z_AVERTISSEMENT {
        p.etape(&trf(
            "Warning from 7z, some files may not have been extracted: {detail}",
            &[("detail", &ligne_utile(&sortie.texte))],
        ));
        return Ok(());
    }
    Err(Erreur::Commande {
        programme: OUTIL_7Z.to_string(),
        code: Some(sortie.code),
        sortie: sortie.texte,
    })
}

/// Ligne la plus parlante de la sortie de `7z` : la première qui mentionne un
/// avertissement ou une erreur (la toute première est la bannière du programme),
/// sinon la première ligne non vide.
fn ligne_utile(texte: &str) -> String {
    let lignes = || texte.lines().map(str::trim).filter(|l| !l.is_empty());
    lignes()
        .find(|l| {
            let minuscules = l.to_lowercase();
            minuscules.contains("warning") || minuscules.contains("error")
        })
        .or_else(|| lignes().next())
        .unwrap_or_default()
        .to_string()
}

/// Parcourt le dossier extrait sans suivre de lien symbolique et classe ce qu'il contient.
fn explorer(racine: &Path) -> Resultat<Trouvailles> {
    let mut trouvailles = Trouvailles::default();
    let mut parcours = WalkDir::new(racine).follow_links(false).into_iter();
    while let Some(entree) = parcours.next() {
        let entree = entree.map_err(|e| std::io::Error::other(e.to_string()))?;
        let type_entree = entree.file_type();
        // Lien symbolique : jamais lu ni copié (voir « Sécurité » en tête de fichier).
        if type_entree.is_symlink() {
            continue;
        }
        let chemin = entree.path();
        match extension_minuscule(chemin).as_deref() {
            Some("vst3") => {
                trouvailles.vst3.push(chemin.to_path_buf());
                // Un bundle est un seul plugin : on ne regarde pas dedans.
                parcours.skip_current_dir();
            }
            Some("clap") if type_entree.is_file() => trouvailles.clap.push(chemin.to_path_buf()),
            Some("dll") if type_entree.is_file() && est_vst2(chemin)? => {
                trouvailles.vst2.push(chemin.to_path_buf());
            }
            Some("exe" | "msi") if type_entree.is_file() && est_installeur(chemin)? => {
                trouvailles.installeurs.push(chemin.to_path_buf());
            }
            _ => {}
        }
    }
    Ok(trouvailles)
}

/// Extension en minuscules.
fn extension_minuscule(chemin: &Path) -> Option<String> {
    chemin
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
}

/// Vrai si le fichier est un installeur d'une famille connue (une archive imbriquée
/// ou un `.exe` quelconque ne compte pas).
fn est_installeur(fichier: &Path) -> Resultat<bool> {
    Ok(!matches!(
        detecter(fichier)?,
        TypeInstalleur::Inconnu | TypeInstalleur::Archive
    ))
}

/// Copie l'installeur hors du dossier temporaire (qui va être supprimé) vers
/// `<cache>/archives/<nom de l'archive sans extension>/`, et rend le nouveau chemin.
fn sortir_installeur(installeur: &Path, cache: &Path, archive: &Path) -> Resultat<PathBuf> {
    let sous_dossier = archive
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());
    let dossier = cache.join(sous_dossier);
    fs::create_dir_all(&dossier)?;
    let destination = dossier.join(nom_fichier(installeur));
    fs::copy(installeur, &destination)?;
    Ok(destination)
}

/// Dernier composant d'un chemin (jamais un `..` : il vient du parcours du dossier extrait).
fn nom_fichier(chemin: &Path) -> std::ffi::OsString {
    chemin.file_name().unwrap_or_default().to_owned()
}

/// Copie les plugins trouvés dans le `drive_c` du préfixe.
fn copier_plugins(
    c: &Chemins,
    trouvailles: &Trouvailles,
    p: &dyn Progression,
) -> Resultat<ContenuArchive> {
    let groupes = [
        (&trouvailles.vst3, DOSSIER_VST3),
        (&trouvailles.clap, DOSSIER_CLAP),
        (&trouvailles.vst2, DOSSIER_VST2),
    ];
    let mut copies = Vec::new();
    for (sources, relatif) in groupes {
        if sources.is_empty() {
            continue;
        }
        let dossier = c.drive_c().join(relatif);
        fs::create_dir_all(&dossier)?;
        for source in sources {
            p.etape(&trf("Copying {name}", &[("name", &nom_affiche(source))]));
            let destination = dossier.join(nom_fichier(source));
            copier(source, &destination)?;
            copies.push(destination);
        }
    }
    Ok(if copies.is_empty() {
        ContenuArchive::Vide
    } else {
        ContenuArchive::PluginsCopies(copies)
    })
}

/// Copie un fichier ou un dossier (récursivement), sans jamais suivre de lien symbolique.
pub(crate) fn copier(source: &Path, destination: &Path) -> Resultat<()> {
    for entree in WalkDir::new(source).follow_links(false) {
        let entree = entree.map_err(|e| std::io::Error::other(e.to_string()))?;
        let relatif = entree.path().strip_prefix(source).unwrap_or(Path::new(""));
        // `join("")` ajouterait un séparateur final : pour un fichier seul (relatif vide),
        // la cible est la destination elle-même.
        let cible = if relatif.as_os_str().is_empty() {
            destination.to_path_buf()
        } else {
            destination.join(relatif)
        };
        if entree.file_type().is_dir() {
            fs::create_dir_all(&cible)?;
        } else if entree.file_type().is_file() {
            fs::copy(entree.path(), &cible)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::Mutex;
    use wimao_core::ProgressionMuette;

    /// Progression qui garde les messages reçus.
    #[derive(Default)]
    struct Enregistreur(Mutex<Vec<String>>);

    impl Progression for Enregistreur {
        fn etape(&self, message: &str) {
            if let Ok(mut v) = self.0.lock() {
                v.push(message.to_string());
            }
        }
    }

    /// Dossier d'essai : `a_archiver/` (contenu de l'archive), chemins WiMao isolés.
    fn preparer() -> (tempfile::TempDir, Chemins, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let source = t.path().join("a_archiver");
        fs::create_dir_all(&source).unwrap();
        (t, c, source)
    }

    fn ecrire(chemin: &Path, contenu: &[u8]) {
        fs::create_dir_all(chemin.parent().unwrap()).unwrap();
        fs::write(chemin, contenu).unwrap();
    }

    /// Crée `<racine>/test.zip` avec le contenu de `source` (via 7z, requis par le module).
    fn zipper(racine: &Path, source: &Path) -> PathBuf {
        let zip = racine.join("test.zip");
        let sortie = Command::new("7z")
            .current_dir(source)
            .args(["a", "-tzip", "-snl"])
            .arg(&zip)
            .arg(".")
            .output()
            .unwrap();
        assert!(sortie.status.success(), "7z a a échoué");
        zip
    }

    fn echantillon(nom: &str) -> Vec<u8> {
        fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/echantillons")
                .join(nom),
        )
        .unwrap()
    }

    #[test]
    fn plugins_copies_aux_bons_endroits() {
        let (t, c, source) = preparer();
        ecrire(&source.join("Foo.vst3/Contents/x86_64-win/Foo.vst3"), b"x");
        ecrire(&source.join("sous dossier/Bar.dll"), b"MZ VSTPluginMain");
        ecrire(&source.join("libsupport.dll"), b"MZ rien");
        ecrire(&source.join("readme.txt"), b"lisez-moi");
        let zip = zipper(t.path(), &source);

        let r = installer_archive(&c, &zip, &ProgressionMuette).unwrap();

        let vst3 = c.drive_c().join("Program Files/Common Files/VST3/Foo.vst3");
        let vst2 = c.drive_c().join("Program Files/Common Files/VST2/Bar.dll");
        let ContenuArchive::PluginsCopies(mut chemins) = r else {
            panic!("PluginsCopies attendu, obtenu {r:?}");
        };
        chemins.sort();
        assert_eq!(chemins, vec![vst2.clone(), vst3.clone()]);
        assert!(vst3.join("Contents/x86_64-win/Foo.vst3").is_file());
        assert!(vst2.is_file());
        assert!(!c
            .drive_c()
            .join("Program Files/Common Files/VST2/libsupport.dll")
            .exists());
        // Le dossier temporaire d'extraction a disparu.
        let restes: Vec<_> = fs::read_dir(c.cache.join("archives")).unwrap().collect();
        assert!(restes.is_empty(), "restes : {restes:?}");
    }

    #[test]
    fn clap_copie_dans_common_files_clap() {
        let (t, c, source) = preparer();
        ecrire(&source.join("Synth.clap"), b"x");
        let zip = zipper(t.path(), &source);
        let r = installer_archive(&c, &zip, &ProgressionMuette).unwrap();
        let attendu = c
            .drive_c()
            .join("Program Files/Common Files/CLAP/Synth.clap");
        assert_eq!(r, ContenuArchive::PluginsCopies(vec![attendu.clone()]));
        assert!(attendu.is_file());
    }

    #[test]
    fn un_seul_installeur_reconnu_est_rendu_et_survit_au_nettoyage() {
        let (t, c, source) = preparer();
        ecrire(&source.join("Setup.exe"), &echantillon("inno.bin"));
        ecrire(&source.join("autre/outil.exe"), &echantillon("inconnu.bin"));
        let zip = zipper(t.path(), &source);

        let r = installer_archive(&c, &zip, &ProgressionMuette).unwrap();

        let ContenuArchive::Installeur(chemin) = r else {
            panic!("Installeur attendu, obtenu {r:?}");
        };
        assert_eq!(chemin, c.cache.join("archives/test/Setup.exe"));
        assert_eq!(fs::read(&chemin).unwrap(), echantillon("inno.bin"));
        // Aucun dossier d'extraction ne subsiste à côté.
        assert_eq!(fs::read_dir(c.cache.join("archives")).unwrap().count(), 1);
    }

    #[test]
    fn archive_sans_rien_d_utile_est_vide() {
        let (t, c, source) = preparer();
        ecrire(&source.join("readme.txt"), b"rien");
        let zip = zipper(t.path(), &source);
        assert_eq!(
            installer_archive(&c, &zip, &ProgressionMuette).unwrap(),
            ContenuArchive::Vide
        );
    }

    #[test]
    fn archive_corrompue_donne_une_erreur_de_commande() {
        let (t, c, _) = preparer();
        let faux = t.path().join("faux.zip");
        fs::write(&faux, b"ceci n'est pas une archive").unwrap();
        let e = installer_archive(&c, &faux, &ProgressionMuette).unwrap_err();
        assert!(matches!(e, Erreur::Commande { .. }), "{e:?}");
    }

    #[test]
    fn lien_symbolique_vers_un_secret_n_est_pas_copie() {
        let (t, c, source) = preparer();
        let secret = t.path().join("secret.vst3");
        ecrire(&secret, b"secret");
        std::os::unix::fs::symlink(&secret, source.join("Piege.vst3")).unwrap();
        let zip = zipper(t.path(), &source);
        // Selon la version de 7-Zip, le lien dangereux est soit ignoré sans erreur
        // (7-Zip 26 : archive vide), soit refusé avec le code 2 « Dangerous link path was
        // ignored » (7-Zip 23, Ubuntu 24.04) : les deux sont sûrs. L'essentiel est
        // vérifié ensuite : le secret n'est jamais copié dans l'environnement.
        match installer_archive(&c, &zip, &ProgressionMuette) {
            Ok(r) => assert_eq!(r, ContenuArchive::Vide),
            Err(e) => assert!(matches!(e, Erreur::Commande { .. }), "{e:?}"),
        }
        assert!(!c
            .drive_c()
            .join("Program Files/Common Files/VST3/Piege.vst3")
            .exists());
    }

    #[test]
    fn avertissement_de_7z_est_annonce_et_l_extraction_continue() {
        let (t, c, _) = preparer();
        // Faux 7z : extrait un plugin CLAP dans le dossier de sortie (-o...), se plaint, sort en 1.
        let faux = t.path().join("faux-7z");
        crate::test_utils::ecrire_executable(
            &faux,
            "#!/bin/sh\nfor a in \"$@\"; do case \"$a\" in -o*) d=\"${a#-o}\";; esac; done\n\
             echo '7-Zip (faux) 1.0'\necho 'WARNING: Cannot open file X.dat'\n\
             echo x > \"$d/Synth.clap\"\nexit 1\n",
        );
        let archive = t.path().join("a.zip");
        fs::write(&archive, b"x").unwrap();

        let suivi = Enregistreur::default();
        let r = installer_archive_avec(&c, &archive, &suivi, &faux).unwrap();

        assert!(
            matches!(r, ContenuArchive::PluginsCopies(ref v) if v.len() == 1),
            "{r:?}"
        );
        let messages = suivi.0.lock().unwrap().join("\n");
        assert!(
            messages.contains("WARNING: Cannot open file X.dat"),
            "avertissement non annoncé : {messages}"
        );
    }

    #[test]
    fn chemin_d_archive_en_remontee_n_ecrit_rien_hors_du_dossier_temporaire() {
        let (t, c, _) = preparer();
        let zip = t.path().join("piege.zip");
        crate::test_utils::zip_malveillant(&zip);
        // Résultat indifférent (7z peut refuser ou neutraliser le `..`) ; l'important
        // est qu'aucun fichier n'apparaisse à côté du dossier d'extraction.
        let _ = installer_archive(&c, &zip, &ProgressionMuette);
        assert!(!c.cache.join("archives/evil.vst3").exists());
        assert!(!c.cache.join("evil.vst3").exists());
        // Positivement : le cache ne contient que le dossier « archives », et celui-ci
        // est vide (dossier d'extraction supprimé) ; rien n'a été écrit à côté.
        let noms: Vec<_> = fs::read_dir(&c.cache)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(noms, vec![std::ffi::OsString::from("archives")]);
        assert_eq!(fs::read_dir(c.cache.join("archives")).unwrap().count(), 0);
    }
}
