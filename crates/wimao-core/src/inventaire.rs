//! Inventaire des plugins Windows installés dans un préfixe Wine.
//!
//! Utilise `walkdir` pour parcourir les dossiers de plugins de `drive_c`
//! (voir [`Chemins::drive_c`](crate::chemins::Chemins::drive_c)). Utilisé par les
//! recettes (comparer l'avant/après d'un installateur pour trouver ce qu'il a
//! ajouté ou mis à jour, via [`Inventaire::nouveaux`] et [`Inventaire::modifies`]) et
//! par l'interface (liste des plugins).

use crate::erreur::{Erreur, Resultat};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::WalkDir;

/// Dossiers où chercher des plugins, relatifs à `drive_c`. Ils reprennent la
/// configuration yabridgectl de l'installation actuelle (voir NOTES.md) : ce que
/// yabridge saura relier est exactement ce que WiMao doit voir.
pub const DOSSIERS_PLUGINS: &[&str] = &[
    DOSSIER_VST3,
    DOSSIER_CLAP,
    DOSSIER_VST2,
    "Program Files/Steinberg/VstPlugins",
    "Program Files/VstPlugins",
];

/// Dossier des plugins VST3, relatif à `drive_c`. Public : les installations par
/// archive y copient leurs plugins (un seul endroit définit où WiMao range et cherche).
pub const DOSSIER_VST3: &str = "Program Files/Common Files/VST3";
/// Dossier des plugins CLAP, relatif à `drive_c`.
pub const DOSSIER_CLAP: &str = "Program Files/Common Files/CLAP";
/// Dossier des plugins VST2, relatif à `drive_c`.
pub const DOSSIER_VST2: &str = "Program Files/Common Files/VST2";

/// Symbole exporté par toute DLL VST2 moderne ; sert à la distinguer d'une DLL
/// de support (bibliothèque d'un autre logiciel déposée dans le même dossier).
pub const SYMBOLE_VST2: &[u8] = b"VSTPluginMain";
/// Dossiers d'un bundle VST3 qui contiennent le binaire Windows (64 puis 32 bits),
/// d'après la disposition des bundles VST3 de Steinberg.
pub const DOSSIERS_BINAIRE_VST3: [&str; 2] = ["Contents/x86_64-win", "Contents/x86-win"];
/// Taille des blocs lus lors de la recherche du symbole : petite en mémoire
/// quelle que soit la taille de la DLL (certaines font des centaines de Mo).
const TAILLE_BLOC: usize = 64 * 1024;

/// Format d'un plugin audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Format {
    /// VST2 : une DLL qui exporte `VSTPluginMain`.
    Vst2,
    /// VST3 : un dossier (bundle) ou fichier `.vst3`.
    Vst3,
    /// CLAP : un fichier `.clap`.
    Clap,
}

/// Un plugin trouvé dans le préfixe. L'ordre est celui du chemin d'abord.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginWindows {
    /// Chemin du fichier ou du bundle (jamais l'intérieur d'un bundle).
    pub chemin: PathBuf,
    /// Format du plugin.
    pub format: Format,
    /// Nom de fichier sans extension (« Xpand!2_x64 »), tel qu'affiché.
    pub nom: String,
}

/// Empreinte légère du binaire d'un plugin : suffit à voir qu'un installeur l'a
/// remplacé (mise à jour) sans relire son contenu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Signature {
    /// Date de modification la plus récente des fichiers du binaire (`None` si le
    /// système ne la donne pas).
    pub modifie: Option<SystemTime>,
    /// Taille totale des fichiers du binaire, en octets.
    pub taille: u64,
    /// Date de changement (`ctime`) la plus récente des fichiers du binaire : le noyau la
    /// met à jour à chaque écriture et aucun programme ne peut la remettre en arrière,
    /// contrairement à `modifie` (un installeur qui rétablit la date d'origine d'une DLL
    /// de même taille passerait sinon inaperçu). `None` si aucun fichier.
    pub change: Option<SystemTime>,
}

/// Ensemble des plugins d'un préfixe à un instant donné, et la [`Signature`] de chacun
/// (clé : [`PluginWindows::chemin`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventaire(
    pub BTreeSet<PluginWindows>,
    pub BTreeMap<PathBuf, Signature>,
);

impl Inventaire {
    /// Relève les plugins présents sous `drive_c` (dossiers de [`DOSSIERS_PLUGINS`]).
    ///
    /// Un dossier de plugins absent est ignoré (préfixe neuf). Les liens
    /// symboliques ne sont pas suivis, ce qui évite les boucles. Un sous-dossier
    /// ou une DLL illisible fait échouer le relevé : mieux vaut une erreur claire
    /// qu'un inventaire silencieusement incomplet.
    ///
    /// Erreurs : [`Erreur::Io`], dont le message contient le chemin fautif.
    pub fn relever(drive_c: &Path) -> Resultat<Inventaire> {
        let mut plugins = BTreeSet::new();
        for relatif in DOSSIERS_PLUGINS {
            let dossier = drive_c.join(relatif);
            if dossier.is_dir() {
                parcourir(&dossier, &mut plugins)?;
            }
        }
        let signatures = plugins
            .iter()
            .map(|p| Ok((p.chemin.clone(), signature(p)?)))
            .collect::<Resultat<_>>()?;
        Ok(Inventaire(plugins, signatures))
    }

    /// Plugins présents ici et absents de `avant`, dans l'ordre de l'inventaire.
    pub fn nouveaux(&self, avant: &Inventaire) -> Vec<PluginWindows> {
        self.0.difference(&avant.0).cloned().collect()
    }

    /// Plugins présents ici **et** dans `avant`, dont la [`Signature`] a changé : un
    /// installeur les a remplacés (mise à jour, réinstallation d'une autre version).
    /// Un plugin sans signature d'un côté n'est pas compté (pas de quoi comparer).
    pub fn modifies(&self, avant: &Inventaire) -> Vec<PluginWindows> {
        self.0
            .intersection(&avant.0)
            .filter(|p| match (self.1.get(&p.chemin), avant.1.get(&p.chemin)) {
                (Some(maintenant), Some(alors)) => maintenant != alors,
                _ => false,
            })
            .cloned()
            .collect()
    }
}

/// Parcourt un dossier de plugins et ajoute ce qu'il y trouve à `plugins`.
fn parcourir(dossier: &Path, plugins: &mut BTreeSet<PluginWindows>) -> Resultat<()> {
    let accepte_dll = est_dossier_vst2(dossier);
    let mut parcours = WalkDir::new(dossier).follow_links(false).into_iter();
    while let Some(entree) = parcours.next() {
        let entree = entree.map_err(|e| {
            let chemin = e.path().unwrap_or(dossier).to_path_buf();
            erreur_io(&chemin, e.into())
        })?;
        let chemin = entree.path();
        let format = match extension_minuscule(chemin).as_deref() {
            Some("vst3") => Some(Format::Vst3),
            Some("clap") => Some(Format::Clap),
            Some("dll") if accepte_dll && entree.file_type().is_file() => {
                est_vst2(chemin)?.then_some(Format::Vst2)
            }
            _ => None,
        };
        if let Some(format) = format {
            // Un bundle `.vst3` est un seul plugin : on ne regarde pas dedans.
            if entree.file_type().is_dir() {
                parcours.skip_current_dir();
            }
            plugins.insert(PluginWindows {
                chemin: chemin.to_path_buf(),
                format,
                nom: nom_sans_extension(chemin),
            });
        }
    }
    Ok(())
}

/// Signature du binaire d'un plugin : le fichier lui-même, ou pour un bundle (dossier) les
/// fichiers de son dossier binaire Windows (`DOSSIERS_BINAIRE_VST3`) ; à défaut, tous
/// les fichiers du bundle. Les liens ne sont pas suivis. Erreur d'E/S avec le chemin.
///
/// Publique pour la session de gestionnaire : elle compare la signature actuelle d'un plugin
/// à celle du relevé précédent pour savoir s'il a fini d'être écrit.
pub fn signature(plugin: &PluginWindows) -> Resultat<Signature> {
    let racine = DOSSIERS_BINAIRE_VST3
        .iter()
        .map(|d| plugin.chemin.join(d))
        .find(|d| d.is_dir())
        .unwrap_or_else(|| plugin.chemin.clone());
    let mut resultat = Signature {
        modifie: None,
        taille: 0,
        change: None,
    };
    for entree in WalkDir::new(&racine).follow_links(false) {
        let entree = entree.map_err(|e| {
            let chemin = e.path().unwrap_or(&racine).to_path_buf();
            erreur_io(&chemin, e.into())
        })?;
        if !entree.file_type().is_file() {
            continue;
        }
        let meta = entree
            .metadata()
            .map_err(|e| erreur_io(entree.path(), e.into()))?;
        resultat.taille += meta.len();
        resultat.modifie = resultat.modifie.max(meta.modified().ok());
        resultat.change = resultat.change.max(Some(date_de_changement(&meta)));
    }
    Ok(resultat)
}

/// Date de changement (`ctime`) d'un fichier, avec sa fraction de seconde.
fn date_de_changement(meta: &std::fs::Metadata) -> SystemTime {
    use std::os::unix::fs::MetadataExt;
    let secondes = meta.ctime();
    let nanos = std::time::Duration::from_nanos(meta.ctime_nsec().max(0) as u64);
    if secondes >= 0 {
        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secondes as u64) + nanos
    } else {
        // Avant 1970 (horloge absurde) : sans importance, seule la comparaison compte.
        SystemTime::UNIX_EPOCH - std::time::Duration::from_secs(secondes.unsigned_abs()) + nanos
    }
}

/// Vrai si le dossier est l'un de ceux où l'on cherche des DLL VST2
/// (`VST2` ou `VstPlugins`) ; dans les dossiers VST3/CLAP, une DLL isolée n'est pas un plugin.
fn est_dossier_vst2(dossier: &Path) -> bool {
    let nom = dossier
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    nom == "vst2" || nom == "vstplugins"
}

/// Extension en minuscules (Windows ne distingue pas `.DLL` de `.dll`).
fn extension_minuscule(chemin: &Path) -> Option<String> {
    chemin
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
}

/// Nom de fichier sans sa dernière extension.
fn nom_sans_extension(chemin: &Path) -> String {
    chemin
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Vrai si la DLL contient `VSTPluginMain` (lecture en flux). Erreur d'E/S avec le chemin sinon.
pub fn est_vst2(dll: &Path) -> Resultat<bool> {
    let fichier = File::open(dll).map_err(|e| erreur_io(dll, e))?;
    contient_motif(fichier, SYMBOLE_VST2).map_err(|e| erreur_io(dll, e))
}

/// Cherche `motif` dans un flux, par blocs, sans le charger en entier.
fn contient_motif(flux: impl Read, motif: &[u8]) -> io::Result<bool> {
    Ok(chercher_premier_motif(flux, &[motif])?.is_some())
}

/// Cherche plusieurs motifs en **une seule passe** sur un flux, par blocs, sans le
/// charger en entier, et rend l'indice (dans `motifs`) de celui qui apparaît le
/// plus tôt dans le flux (`None` si aucun). Utilisé aussi par la détection des
/// installeurs (`wimao-recettes`) : un installeur fait parfois des centaines de Mo,
/// on ne veut pas le relire une fois par motif.
///
/// Les `plus_long - 1` derniers octets de chaque bloc sont recopiés devant le
/// suivant : sans ce chevauchement, une chaîne à cheval sur deux blocs serait ratée.
/// La recherche s'arrête au premier bloc qui contient un motif ; si deux motifs
/// sont dans le même bloc, c'est celui qui commence le plus tôt qui gagne.
///
/// Les motifs ne doivent pas être vides (un motif vide est ignoré).
///
/// Erreurs : celles de lecture du flux.
pub fn chercher_premier_motif(mut flux: impl Read, motifs: &[&[u8]]) -> io::Result<Option<usize>> {
    let plus_long = motifs.iter().map(|m| m.len()).max().unwrap_or(0);
    if plus_long == 0 {
        return Ok(None);
    }
    let reste = plus_long - 1;
    let mut tampon = vec![0u8; TAILLE_BLOC + reste];
    let mut conserves = 0;
    loop {
        let lus = match flux.read(&mut tampon[conserves..]) {
            Ok(0) => return Ok(None),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let total = conserves + lus;
        if let Some(indice) = premier_dans(&tampon[..total], motifs) {
            return Ok(Some(indice));
        }
        conserves = reste.min(total);
        tampon.copy_within(total - conserves..total, 0);
    }
}

/// Indice du motif qui commence le plus tôt dans `bloc`, s'il y en a un.
fn premier_dans(bloc: &[u8], motifs: &[&[u8]]) -> Option<usize> {
    (0..bloc.len()).find_map(|debut| {
        motifs
            .iter()
            .position(|m| !m.is_empty() && bloc[debut..].starts_with(m))
    })
}

/// Erreur d'E/S dont le message nomme le chemin concerné.
fn erreur_io(chemin: &Path, source: io::Error) -> Erreur {
    Erreur::Io(io::Error::new(
        source.kind(),
        format!("{} : {source}", chemin.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Crée un fichier (et ses dossiers parents) avec ce contenu.
    fn creer(racine: &Path, relatif: &str, contenu: &[u8]) {
        let chemin = racine.join(relatif);
        fs::create_dir_all(chemin.parent().unwrap()).unwrap();
        fs::write(chemin, contenu).unwrap();
    }

    #[test]
    fn un_bundle_vst3_compte_pour_un_seul_plugin() {
        let t = tempfile::tempdir().unwrap();
        creer(
            t.path(),
            "Program Files/Common Files/VST3/Serum2.vst3/Contents/x86_64-win/Serum2.vst3",
            b"x",
        );
        let inv = Inventaire::relever(t.path()).unwrap();
        assert_eq!(inv.0.len(), 1);
        let p = inv.0.iter().next().unwrap();
        assert_eq!((p.format, p.nom.as_str()), (Format::Vst3, "Serum2"));
        assert!(p.chemin.ends_with("VST3/Serum2.vst3"));
    }

    #[test]
    fn vst2_avec_symbole_trouve_nom_avec_espace_et_point_d_exclamation() {
        let t = tempfile::tempdir().unwrap();
        creer(
            t.path(),
            "Program Files/VstPlugins/Xpand!2_x64.dll",
            b"MZ....VSTPluginMain....",
        );
        creer(t.path(), "Program Files/VstPlugins/libfoo.dll", b"MZ rien");
        let inv = Inventaire::relever(t.path()).unwrap();
        let noms: Vec<_> = inv.0.iter().map(|p| (p.format, p.nom.as_str())).collect();
        assert_eq!(noms, vec![(Format::Vst2, "Xpand!2_x64")]);
    }

    #[test]
    fn nom_accentue_avec_espace_est_trouve() {
        let t = tempfile::tempdir().unwrap();
        creer(
            t.path(),
            "Program Files/Common Files/VST3/Électrique Bass.vst3",
            b"x",
        );
        let inv = Inventaire::relever(t.path()).unwrap();
        assert_eq!(inv.0.iter().next().unwrap().nom, "Électrique Bass");
    }

    #[test]
    fn clap_et_dossier_absent() {
        let t = tempfile::tempdir().unwrap();
        assert!(Inventaire::relever(t.path()).unwrap().0.is_empty());
        creer(t.path(), "Program Files/Common Files/CLAP/Synth.clap", b"x");
        let inv = Inventaire::relever(t.path()).unwrap();
        assert_eq!(inv.0.iter().next().unwrap().format, Format::Clap);
    }

    #[test]
    fn dll_dans_dossier_vst3_est_ignoree() {
        let t = tempfile::tempdir().unwrap();
        creer(
            t.path(),
            "Program Files/Common Files/VST3/aide.dll",
            b"VSTPluginMain",
        );
        assert!(Inventaire::relever(t.path()).unwrap().0.is_empty());
    }

    #[test]
    fn nouveaux_rend_exactement_le_plugin_ajoute() {
        let t = tempfile::tempdir().unwrap();
        creer(t.path(), "Program Files/Common Files/VST3/A.vst3", b"x");
        let avant = Inventaire::relever(t.path()).unwrap();
        creer(t.path(), "Program Files/Common Files/VST3/B.vst3", b"x");
        let apres = Inventaire::relever(t.path()).unwrap();
        let nouveaux = apres.nouveaux(&avant);
        assert_eq!(nouveaux.len(), 1);
        assert_eq!(nouveaux[0].nom, "B");
        assert!(avant.nouveaux(&apres).is_empty());
        assert!(apres.modifies(&avant).is_empty(), "A n'a pas changé");
    }

    #[test]
    fn lien_symbolique_vers_parent_ne_boucle_pas() {
        let t = tempfile::tempdir().unwrap();
        creer(t.path(), "Program Files/Common Files/VST3/A.vst3", b"x");
        let dossier = t.path().join("Program Files/Common Files/VST3");
        std::os::unix::fs::symlink(&dossier, dossier.join("boucle")).unwrap();
        assert_eq!(Inventaire::relever(t.path()).unwrap().0.len(), 1);
    }

    /// Lecteur de test qui rend au plus `max` octets par `read`, pour forcer
    /// un motif à tomber à cheval sur deux lectures successives.
    struct LecteurLimite<'a> {
        donnees: &'a [u8],
        max: usize,
    }

    impl Read for LecteurLimite<'_> {
        fn read(&mut self, tampon: &mut [u8]) -> io::Result<usize> {
            let n = self.max.min(tampon.len()).min(self.donnees.len());
            tampon[..n].copy_from_slice(&self.donnees[..n]);
            self.donnees = &self.donnees[n..];
            Ok(n)
        }
    }

    #[test]
    fn motif_a_cheval_sur_deux_lectures_est_trouve() {
        // Le motif (13 octets) commence 5 octets avant la frontière des lectures.
        let max = 100;
        let mut donnees = vec![0u8; max - 5];
        donnees.extend_from_slice(SYMBOLE_VST2);
        donnees.extend_from_slice(&[0u8; 300]);
        for taille in [max, 1] {
            let lecteur = LecteurLimite {
                donnees: &donnees,
                max: taille,
            };
            assert!(
                contient_motif(lecteur, SYMBOLE_VST2).unwrap(),
                "{taille} octet(s) par lecture"
            );
        }
        let vide = LecteurLimite {
            donnees: &[0u8; 500],
            max: 1,
        };
        assert!(!contient_motif(vide, SYMBOLE_VST2).unwrap());
    }

    #[test]
    fn plusieurs_motifs_en_une_passe_rendent_le_plus_proche_du_debut() {
        let donnees = b"..bbb....aaa..";
        let motifs: [&[u8]; 3] = [b"aaa", b"bbb", b"zzz"];
        assert_eq!(
            chercher_premier_motif(&donnees[..], &motifs).unwrap(),
            Some(1)
        );
        assert_eq!(
            chercher_premier_motif(&donnees[..], &[b"zzz"]).unwrap(),
            None
        );
        assert_eq!(chercher_premier_motif(&donnees[..], &[]).unwrap(), None);
        // À cheval sur deux lectures d'un octet.
        let lecteur = LecteurLimite { donnees, max: 1 };
        assert_eq!(chercher_premier_motif(lecteur, &motifs).unwrap(), Some(1));
    }

    #[test]
    fn vst2_en_sous_dossier_et_extension_majuscule() {
        let t = tempfile::tempdir().unwrap();
        creer(
            t.path(),
            "Program Files/VstPlugins/Plugin Alliance/Foo.dll",
            b"VSTPluginMain",
        );
        creer(
            t.path(),
            "Program Files/VstPlugins/BAR.DLL",
            b"VSTPluginMain",
        );
        let inv = Inventaire::relever(t.path()).unwrap();
        let noms: Vec<_> = inv.0.iter().map(|p| p.nom.as_str()).collect();
        assert_eq!(noms, vec!["BAR", "Foo"]);
    }

    #[test]
    fn modifies_rend_le_plugin_dont_le_binaire_a_change() {
        let t = tempfile::tempdir().unwrap();
        let binaire = "Program Files/Common Files/VST3/A.vst3/Contents/x86_64-win/A.vst3";
        creer(t.path(), binaire, b"version 1");
        creer(t.path(), "Program Files/Common Files/VST3/B.vst3", b"x");
        let avant = Inventaire::relever(t.path()).unwrap();
        // Mise à jour : même chemin, binaire plus gros et plus récent.
        creer(t.path(), binaire, b"version 2, plus longue");
        let apres = Inventaire::relever(t.path()).unwrap();
        assert!(apres.nouveaux(&avant).is_empty());
        let modifies = apres.modifies(&avant);
        assert_eq!(modifies.len(), 1, "{modifies:?}");
        assert_eq!(modifies[0].nom, "A");
        // Seule la date change (même taille) : c'est aussi une mise à jour.
        let f = fs::File::options()
            .write(true)
            .open(t.path().join(binaire))
            .unwrap();
        let plus_tard = SystemTime::now() + std::time::Duration::from_secs(60);
        f.set_modified(plus_tard).unwrap();
        let encore = Inventaire::relever(t.path()).unwrap();
        assert_eq!(encore.modifies(&apres).len(), 1);
    }

    #[test]
    fn reecriture_meme_taille_meme_date_est_une_modification() {
        let t = tempfile::tempdir().unwrap();
        let dll = "Program Files/VstPlugins/Synth.dll";
        creer(t.path(), dll, b"aaaa VSTPluginMain");
        let avant = Inventaire::relever(t.path()).unwrap();
        let chemin = t.path().join(dll);
        let ancienne = fs::metadata(&chemin).unwrap().modified().unwrap();
        // De quoi laisser passer le grain de l'horloge du système de fichiers.
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Même nombre d'octets, puis l'ancienne date de modification remise : seule
        // la date de changement (ctime) trahit encore la réécriture.
        fs::write(&chemin, b"bbbb VSTPluginMain").unwrap();
        fs::File::options()
            .write(true)
            .open(&chemin)
            .unwrap()
            .set_modified(ancienne)
            .unwrap();
        let apres = Inventaire::relever(t.path()).unwrap();
        let modifies = apres.modifies(&avant);
        assert_eq!(modifies.len(), 1, "{modifies:?}");
        assert_eq!(modifies[0].nom, "Synth");
    }
}
