//! Pontage yabridge isolé : relie les plugins Windows du préfixe WiMao à Linux.
//!
//! Utilise [`Chemins`] (où sont le préfixe, `HOME`, yabridge), [`Inventaire`]
//! (les plugins à relier), [`lancer`] (pour `yabridgectl sync`) et [`crate::flatpak`]
//! (liens des logiciels de musique en Flatpak). Utilisé par
//! les recettes après une installation, par la CLI et par l'interface.
//!
//! # Isolation
//! WiMao n'utilise jamais `~/.config/yabridgectl` : la configuration de
//! yabridgectl est écrite dans `<donnees>/yabridgectl/yabridgectl/config.toml`
//! (`XDG_CONFIG_HOME=<donnees>/yabridgectl` pendant `sync`). Les liens produits,
//! eux, sont forcément dans `~/.vst3/yabridge`, `~/.vst/yabridge` et
//! `~/.clap/yabridge` (là où les logiciels de musique les cherchent).
//!
//! # Disposition constatée (yabridgectl 5.1.1, `vst2_location = 'centralized'`)
//! Relevée empiriquement dans un `HOME` temporaire ; `<rel>` est le chemin du
//! plugin **relatif au dossier de plugins** où il se trouve (les sous-dossiers
//! sont conservés, le nom du dossier de plugins lui-même ne l'est pas) :
//! - VST3 : `~/.vst3/yabridge/<rel>/Contents/x86_64-linux/<nom>.so` (le
//!   plugin ponté) et `…/Contents/x86_64-win/<nom>.vst3` = lien vers le plugin
//!   Windows. Ex. `VST3/Éditeur/X.vst3` donne `~/.vst3/yabridge/Éditeur/X.vst3/…`.
//!   Quand le plugin Windows est un bundle (dossier `X.vst3`), le lien vise son **binaire**
//!   `X.vst3/Contents/x86_64-win/X.vst3`, pas le dossier (Serum 2, vrai disque) : voir
//!   `cibles_du_plugin`.
//!   Constaté aussi sur un vrai disque (Serum2) : `…/X.vst3/Contents/Resources` est un lien vers
//!   le dossier `Contents/Resources` du plugin Windows. Il fait partie du pont comme le lien
//!   `x86_64-win` ; cassé, il ne doit pas empêcher de retirer le bundle.
//! - VST2 : `~/.vst/yabridge/<rel>.so` (le plugin ponté) et `<rel>.dll` = lien
//!   vers la DLL. Ex. `VstPlugins/Plugin Alliance/X_x64.dll` donne
//!   `~/.vst/yabridge/Plugin Alliance/X_x64.so`.
//! - CLAP : `~/.clap/yabridge/<rel>` (même principe, d'après le README de
//!   yabridge) avec un lien `<rel>-win`. **Non vérifié** : aucun vrai plugin
//!   CLAP Windows n'était disponible pour l'essai ; à confirmer.
//!
//! # Nettoyage après un retour en arrière
//! [`retirer_liens_casses`] défait ce que `sync` a fait pour un plugin disparu du préfixe :
//! il retire le lien devenu cassé et son pendant ponté (voir la disposition ci-dessus). Appelé
//! par `restauration::revenir`. [`racines_du_prefixe`] dit quelles cibles sont « à WiMao » (le
//! préfixe, et après un import l'ancien chemin `~/.wine-mao`, voir [`crate::import`]).
//!
//! # Contournements
//! - `yabridgectl set --path` plante dans la version fournie (yabridgectl 5.1.1 :
//!   panique de clap, constatée en développement) : on écrit `yabridge_home`
//!   directement dans le fichier.
//! - Sans `yabridge_dir` dans le `PATH`, la vérification finale de `sync` ne
//!   trouve pas `yabridge-host.exe(.so)` et échoue ; et sans `WINEPREFIX` elle
//!   créerait un `~/.wine` par défaut (fenêtre « installer Wine Mono » à
//!   l'écran) : on fixe donc `WINEPREFIX` et `WINEDLLOVERRIDES` comme
//!   `environnement.rs`.

use crate::chemins::Chemins;
use crate::erreur::{Erreur, Resultat};
use crate::flatpak;
use crate::i18n::trf;
use crate::import;
use crate::inventaire::{
    Format, Inventaire, PluginWindows, DOSSIERS_BINAIRE_VST3, DOSSIERS_PLUGINS,
};
use crate::journal;
use crate::outils::lancer;
use crate::wine::{preparer_environnement, SURCHARGES_SANS_FENETRES};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use walkdir::WalkDir;

/// Délai de `yabridgectl sync` : il vérifie chaque plugin (lancement de Wine),
/// ce qui peut durer longtemps avec des centaines de plugins.
const DELAI_SYNC: Duration = Duration::from_secs(5 * 60);
/// Valeur de `vst2_location` : tous les VST2 pontés dans `~/.vst/yabridge`
/// (c'est le choix de l'installation actuelle, voir NOTES.md).
const VST2_CENTRALISE: &str = "centralized";
/// Suffixe 64 bits que les éditeurs ajoutent aux DLL VST2.
const SUFFIXE_X64: &str = "_x64";

/// Ce que `ponter` a décidé pour un plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EtatPontage {
    /// Sera (ou a été) ponté par WiMao.
    Ponte,
    /// VST2 écarté car le même plugin existe en VST3 (qui est préférable).
    Doublon,
    /// Déjà ponté par une autre installation ; le lien pointe vers ce chemin,
    /// hors du préfixe WiMao. WiMao n'y touche pas.
    DejaPonteAilleurs(PathBuf),
}

/// Contenu du `config.toml` de yabridgectl (les seules clés dont WiMao a besoin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigYabridge {
    /// Dossiers de plugins Windows à parcourir.
    pub plugin_dirs: Vec<PathBuf>,
    /// Emplacement des VST2 pontés (`centralized`).
    pub vst2_location: String,
    /// Saute la vérification finale (toujours `false` : on veut la vérification).
    pub no_verify: bool,
    /// Dossier des fichiers de yabridge (remplace `~/.local/share/yabridge`).
    pub yabridge_home: PathBuf,
    /// Fichiers ou dossiers que yabridgectl doit ignorer.
    pub blacklist: Vec<PathBuf>,
}

/// Calcule la configuration yabridgectl et l'état de chaque plugin, sans rien écrire.
///
/// Fonction pure (hors lecture des liens existants) : testable sans yabridgectl.
/// Erreurs : [`Erreur::Io`] si un lien existant est illisible.
pub fn configuration(
    c: &Chemins,
    plugins: &Inventaire,
) -> Resultat<(ConfigYabridge, BTreeMap<PathBuf, EtatPontage>)> {
    let noms_vst3: Vec<String> = plugins
        .0
        .iter()
        .filter(|p| p.format == Format::Vst3)
        .map(|p| p.nom.to_lowercase())
        .collect();
    let mut etats = BTreeMap::new();
    let mut blacklist = Vec::new();
    // Calculées une fois : elles demandent la lecture de `import.toml` et d'un lien.
    let racines = racines_du_prefixe(c);
    for plugin in &plugins.0 {
        let etat = if est_doublon(plugin, &noms_vst3) {
            EtatPontage::Doublon
        } else {
            etat_selon_liens(c, &racines, plugin)?
        };
        if etat != EtatPontage::Ponte {
            blacklist.push(plugin.chemin.clone());
        }
        etats.insert(plugin.chemin.clone(), etat);
    }
    let config = ConfigYabridge {
        plugin_dirs: dossiers_existants(c),
        vst2_location: VST2_CENTRALISE.to_string(),
        no_verify: false,
        yabridge_home: c.yabridge_dir.clone(),
        blacklist,
    };
    Ok((config, etats))
}

/// Ce qu'on trouve à `<XDG_DATA_HOME>/yabridge`, **sans rien modifier** ([`etat_lien_yabridge`]).
/// Sert à [`assurer_lien_yabridge`] (qui crée le lien s'il est absent) et au diagnostic (qui
/// signale un lien vers un autre yabridge, sans jamais le changer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EtatLienYabridge {
    /// Rien à cet endroit : WiMao le crée au prochain pontage, ce n'est pas un problème.
    Absent,
    /// Un lien qui mène à [`Chemins::yabridge_dir`] (directement ou par d'autres liens).
    Bon,
    /// Un lien qui mène à un dossier existant, mais différent de `yabridge_dir` (par exemple le
    /// yabridge de développement `/opt/wine-mao-v2/yabridge`) : sa cible, absolue, est donnée.
    Ailleurs(PathBuf),
    /// Un lien dont la cible n'existe pas (ou plus) : sa cible, absolue, est donnée.
    Casse(PathBuf),
    /// Un vrai dossier (ou fichier) occupe la place.
    Dossier,
}

/// Emplacement du lien : `<XDG_DATA_HOME>/yabridge` (par défaut `~/.local/share/yabridge`).
pub fn emplacement_lien_yabridge(c: &Chemins) -> PathBuf {
    c.xdg_data_home().join("yabridge")
}

/// Regarde `<XDG_DATA_HOME>/yabridge` sans rien créer ni modifier (voir [`EtatLienYabridge`]).
///
/// Un lien est [`EtatLienYabridge::Bon`] si sa cible est littéralement `yabridge_dir` (même si ce
/// dossier n'existe pas encore : paquet pas installé, le lien sera bon dès son installation), ou si
/// les deux se résolvent au même dossier (liens intermédiaires, cible relative). Erreurs :
/// [`Erreur::Io`] si l'emplacement est illisible pour une autre raison qu'être absent.
pub fn etat_lien_yabridge(c: &Chemins) -> Resultat<EtatLienYabridge> {
    etat_lien_yabridge_a(c, &emplacement_lien_yabridge(c))
}

/// Comme [`etat_lien_yabridge`], mais pour un lien à un emplacement quelconque (par exemple
/// `~/.var/app/<id>/data/yabridge` pour un logiciel de musique en Flatpak, voir
/// [`crate::flatpak`]). Ne modifie rien.
pub fn etat_lien_yabridge_a(c: &Chemins, lien: &Path) -> Resultat<EtatLienYabridge> {
    match std::fs::symlink_metadata(lien) {
        Ok(meta) if meta.file_type().is_symlink() => {
            let cible = std::fs::read_link(lien)?;
            // Une cible relative se lit depuis le dossier du lien.
            let cible_absolue = lien.parent().unwrap_or(Path::new("/")).join(&cible);
            if cible_absolue == c.yabridge_dir || memes_fichiers(lien, &c.yabridge_dir) {
                Ok(EtatLienYabridge::Bon)
            } else if lien.exists() {
                // `exists` suit le lien : vrai seulement si la cible existe.
                Ok(EtatLienYabridge::Ailleurs(cible_absolue))
            } else {
                Ok(EtatLienYabridge::Casse(cible_absolue))
            }
        }
        Ok(_) => Ok(EtatLienYabridge::Dossier),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(EtatLienYabridge::Absent),
        Err(e) => Err(e.into()),
    }
}

/// Résultat de [`assurer_lien_yabridge`] : ce qu'on a trouvé à `<XDG_DATA_HOME>/yabridge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LienYabridge {
    /// Le lien manquait : WiMao l'a créé vers le yabridge de [`Chemins::yabridge_dir`].
    Cree,
    /// Le lien existait déjà et vise `yabridge_dir` : rien à faire.
    DejaBon,
    /// Le lien existe mais vise ailleurs (par exemple le yabridge de développement
    /// `/opt/wine-mao-v2/yabridge`) : laissé tel quel, la cible est donnée.
    AutreLien(PathBuf),
    /// Un vrai dossier (ou fichier) occupe la place : laissé tel quel.
    Dossier,
}

/// S'assure que `<XDG_DATA_HOME>/yabridge` (par défaut `~/.local/share/yabridge`) existe
/// et vise le yabridge de WiMao ([`Chemins::yabridge_dir`]).
///
/// Pourquoi : les « chainloaders » que `yabridgectl sync` copie dans `~/.vst3/yabridge`,
/// `~/.vst/yabridge` et `~/.clap/yabridge` cherchent la bibliothèque `libyabridge-*.so` et
/// l'hôte `yabridge-host.exe` dans cet ordre : à côté d'eux, puis dans
/// `~/.local/share/yabridge`, puis dans le `PATH`. Le paquet `wimao-bridge` installe
/// yabridge dans `/opt/wimao/yabridge` et ne peut pas écrire dans le dossier personnel
/// de chaque utilisateur : c'est donc WiMao qui pose ce lien.
///
/// Le lien peut être créé avant que `yabridge_dir` n'existe (paquet pas encore installé) : il
/// est alors pendant, puis reconnu [`LienYabridge::DejaBon`] une fois le paquet installé,
/// sans conséquence.
///
/// Ne remplace jamais rien : un lien vers une autre installation, un vrai dossier ou un
/// lien cassé (qui est un lien vers ailleurs) sont laissés intacts et signalés par la
/// valeur renvoyée. Erreurs : [`Erreur::Io`] si le lien ou ses dossiers ne peuvent pas
/// être créés.
pub fn assurer_lien_yabridge(c: &Chemins) -> Resultat<LienYabridge> {
    assurer_lien_yabridge_a(c, &emplacement_lien_yabridge(c))
}

/// Comme [`assurer_lien_yabridge`], mais pour le lien posé à `lien` (même règle : ne remplace
/// jamais rien). Sert aux logiciels de musique en Flatpak, dont le `XDG_DATA_HOME` est
/// `~/.var/app/<id>/data` ([`crate::flatpak`]).
pub fn assurer_lien_yabridge_a(c: &Chemins, lien: &Path) -> Resultat<LienYabridge> {
    match etat_lien_yabridge_a(c, lien)? {
        EtatLienYabridge::Bon => Ok(LienYabridge::DejaBon),
        // Un lien cassé est un lien vers ailleurs : on n'y touche pas non plus.
        EtatLienYabridge::Ailleurs(cible) | EtatLienYabridge::Casse(cible) => {
            Ok(LienYabridge::AutreLien(cible))
        }
        EtatLienYabridge::Dossier => Ok(LienYabridge::Dossier),
        EtatLienYabridge::Absent => {
            if let Some(parent) = lien.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(&c.yabridge_dir, lien)?;
            Ok(LienYabridge::Cree)
        }
    }
}

/// Appelle [`assurer_lien_yabridge`] et note dans le journal ce qui mérite d'être su
/// (lien ailleurs, vrai dossier, échec). Ne fait jamais échouer le pontage : sans ce
/// lien, `sync` fonctionne quand même, seuls les chainloaders risquent de ne pas
/// trouver yabridge. La note au journal est discrète ; c'est le diagnostic
/// ([`crate::diagnostic::diagnostiquer`], problème `LienYabridge`) qui le signale à la
/// personne, en lui donnant la commande pour y remédier (WiMao ne change jamais ce lien).
fn assurer_lien_et_noter(c: &Chemins) {
    match assurer_lien_yabridge(c) {
        Ok(LienYabridge::Cree | LienYabridge::DejaBon) => {}
        Ok(LienYabridge::AutreLien(cible)) => journal::noter_info(
            "lien yabridge",
            &format!(
                "~/.local/share/yabridge vise {} et non {} : laissé tel quel",
                cible.display(),
                c.yabridge_dir.display()
            ),
        ),
        Ok(LienYabridge::Dossier) => journal::noter_info(
            "lien yabridge",
            "~/.local/share/yabridge est un vrai dossier : laissé tel quel",
        ),
        Err(e) => journal::noter("lien yabridge", &format!("lien non créé : {e}")),
    }
    assurer_liens_flatpak_et_noter(c);
}

/// Pose le lien yabridge des logiciels de musique installés en Flatpak ([`crate::flatpak`]) et
/// note au journal ce qui mérite d'être su, comme pour le lien principal.
///
/// Si au moins un lien vient d'être créé, les dates des `.so` pontés sont rafraîchies : les
/// logiciels gardent un cache de l'analyse des plugins et ne réanalysent que ce qui a changé ;
/// sans cela un plugin déjà écarté (parce qu'il ne se chargeait pas sans le lien) le resterait.
fn assurer_liens_flatpak_et_noter(c: &Chemins) {
    let mut cree = false;
    for (id, resultat) in flatpak::assurer_liens_flatpak(c, &flatpak::logiciels_installes()) {
        let lien = flatpak::emplacement_lien_flatpak(c, &id);
        match resultat {
            Ok(LienYabridge::Cree) => cree = true,
            Ok(LienYabridge::DejaBon) => {}
            Ok(LienYabridge::AutreLien(cible)) => journal::noter_info(
                "lien yabridge",
                &format!(
                    "{} vise {} et non {} : laissé tel quel",
                    lien.display(),
                    cible.display(),
                    c.yabridge_dir.display()
                ),
            ),
            Ok(LienYabridge::Dossier) => journal::noter_info(
                "lien yabridge",
                &format!("{} est un vrai dossier : laissé tel quel", lien.display()),
            ),
            Err(e) => journal::noter(
                "lien yabridge",
                &format!("lien {} non créé : {e}", lien.display()),
            ),
        }
    }
    if cree {
        if let Err(e) = flatpak::rafraichir_dates_plugins(c) {
            journal::noter("lien yabridge", &format!("dates des plugins : {e}"));
        }
    }
}

/// Relie les plugins de l'inventaire à Linux avec `yabridgectl sync`.
///
/// Écrit la configuration isolée puis lance `sync` (jamais `--prune` : on ne
/// supprime rien de ce que WiMao n'a pas créé). Renvoie l'état de chaque plugin.
///
/// Erreurs : [`Erreur::Introuvable`] si `yabridgectl` manque, [`Erreur::Io`],
/// [`Erreur::Delai`], ou [`Erreur::Commande`] si `sync` échoue.
pub fn ponter(c: &Chemins, plugins: &Inventaire) -> Resultat<BTreeMap<PathBuf, EtatPontage>> {
    assurer_lien_et_noter(c);
    let (config, etats) = configuration(c, plugins)?;
    ecrire_configuration(c, &config)?;
    lancer_sync(&mut commande_sync(c)?)?;
    Ok(etats)
}

/// Chemin du plugin ponté côté Linux (bundle `.vst3`, `.so` VST2, `.clap`), s'il
/// existe et que son lien pointe bien vers ce plugin Windows ; sinon `None`.
pub fn bundle_linux(c: &Chemins, plugin: &PluginWindows) -> Option<PathBuf> {
    let (ponte, lien) = emplacements(c, plugin)?;
    let cible = lire_lien(&lien).ok()??;
    let meme = cibles_du_plugin(plugin)
        .iter()
        .any(|attendu| cible == *attendu || memes_fichiers(&cible, attendu));
    (meme && ponte.symlink_metadata().is_ok()).then_some(ponte)
}

/// Ce vers quoi le lien d'un pont peut pointer pour désigner `plugin` : le plugin lui-même
/// (DLL VST2, `.clap`, VST3 en un seul fichier) et, pour un VST3, le binaire Windows rangé
/// dans son bundle (`<bundle>/Contents/x86_64-win/<nom>.vst3`, ou `x86-win`).
///
/// Sur `x86-win` : le lien lu par [`bundle_linux`] est toujours celui du dossier
/// `Contents/x86_64-win` du bundle ponté (voir `emplacements`). La cible `x86-win` ne sert donc
/// que si yabridgectl y relie le binaire 32 bits d'un bundle qui n'a pas de binaire 64 bits ;
/// ce cas n'a pas été constaté (aucun tel plugin à l'essai). On la garde parce que la liste
/// vient de `DOSSIERS_BINAIRE_VST3`, partagée avec l'inventaire, et qu'une cible de trop ne
/// fait rien prendre pour ponté à tort : le lien doit encore désigner un binaire de `plugin`.
///
/// Pourquoi le binaire : pour un bundle (un **dossier** `.vst3`, cas de Serum 2 et de la
/// plupart des VST3 récents), yabridgectl relie le binaire et non le dossier (constaté sur le
/// vrai disque, voir « Disposition constatée »). Sans lui, un tel plugin passait pour non ponté :
/// toast « pas encore ponté » à chaque lancement et aucun test d'ouverture après l'installation.
fn cibles_du_plugin(plugin: &PluginWindows) -> Vec<PathBuf> {
    let mut cibles = vec![plugin.chemin.clone()];
    if plugin.format == Format::Vst3 {
        if let Some(nom) = plugin.chemin.file_name() {
            cibles.extend(
                DOSSIERS_BINAIRE_VST3
                    .iter()
                    .map(|dossier| plugin.chemin.join(dossier).join(nom)),
            );
        }
    }
    cibles
}

/// Deux chemins désignent-ils le même fichier (liens résolus) ?
fn memes_fichiers(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Vrai pour un VST2 dont le nom (sans `_x64`, sans casse) est celui d'un VST3.
fn est_doublon(plugin: &PluginWindows, noms_vst3: &[String]) -> bool {
    if plugin.format != Format::Vst2 {
        return false;
    }
    let nom = plugin.nom.to_lowercase();
    let nom = nom.strip_suffix(SUFFIXE_X64).unwrap_or(&nom);
    noms_vst3.iter().any(|v| v == nom)
}

/// Racines sous lesquelles une cible de lien est considérée comme « à WiMao » : le préfixe
/// courant, plus l'ancien chemin d'un environnement importé ([`import::ancien_emplacement`],
/// en pratique `~/.wine-mao`).
///
/// Pourquoi l'ancien chemin : les ponts créés avant l'import par l'ancienne installation
/// pointent sous `~/.wine-mao/drive_c/…`. Sans cette racine, ils seraient classés
/// [`EtatPontage::DejaPonteAilleurs`] et laissés de côté ; avec elle, le prochain [`ponter`] les
/// réécrit vers le préfixe (spec, section 5.3). On compare des chemins, sans `canonicalize`.
///
/// Garde-fou : l'ancien chemin n'est ajouté que tant qu'il est **le lien de l'import** vers
/// notre préfixe ([`import::annulation_possible`]). C'est ce qui rend sûr de traiter comme
/// nôtre un lien cassé qui passe par lui : il désigne un fichier de notre préfixe. Après une
/// annulation (ou si l'utilisateur a remplacé le lien), `~/.wine-mao` redevient l'environnement
/// de l'utilisateur et ses liens ne sont plus à nous.
pub fn racines_du_prefixe(c: &Chemins) -> Vec<PathBuf> {
    // Mêmes noms que pour les processus : la règle « deux noms après l'import » n'est écrite
    // qu'à un seul endroit.
    import::noms_du_prefixe(c)
}

/// Vrai si `cible` est sous une des `racines` (calculées par [`racines_du_prefixe`]) :
/// comparaison par composants de chemin, sans toucher au disque.
fn dans_le_prefixe(racines: &[PathBuf], cible: &Path) -> bool {
    racines.iter().any(|r| cible.starts_with(r))
}

/// [`EtatPontage::DejaPonteAilleurs`] si le lien existant sort des `racines` du préfixe WiMao.
fn etat_selon_liens(
    c: &Chemins,
    racines: &[PathBuf],
    plugin: &PluginWindows,
) -> Resultat<EtatPontage> {
    let Some((_, lien)) = emplacements(c, plugin) else {
        return Ok(EtatPontage::Ponte);
    };
    Ok(match lire_lien(&lien)? {
        Some(cible) if !dans_le_prefixe(racines, &cible) => EtatPontage::DejaPonteAilleurs(cible),
        _ => EtatPontage::Ponte,
    })
}

/// Où yabridgectl place le plugin ponté et le lien vers le plugin Windows
/// (voir « Disposition constatée » en tête de fichier). `None` si le plugin n'est
/// dans aucun dossier de plugins du préfixe.
fn emplacements(c: &Chemins, plugin: &PluginWindows) -> Option<(PathBuf, PathBuf)> {
    let rel = chemin_relatif(c, &plugin.chemin)?;
    let nom_fichier = rel.file_name()?.to_owned();
    Some(match plugin.format {
        Format::Vst3 => {
            let ponte = c.home.join(".vst3/yabridge").join(&rel);
            let lien = ponte.join("Contents/x86_64-win").join(nom_fichier);
            (ponte, lien)
        }
        Format::Vst2 => {
            let base = c.home.join(".vst/yabridge");
            (base.join(rel.with_extension("so")), base.join(&rel))
        }
        Format::Clap => {
            let ponte = c.home.join(".clap/yabridge").join(&rel);
            let mut lien = ponte.clone().into_os_string();
            lien.push("-win");
            (ponte, PathBuf::from(lien))
        }
    })
}

/// Les trois dossiers de plugins pontés, avec leur format : c'est là, et seulement là, que
/// [`retirer_liens_casses`] a le droit de supprimer.
fn racines_yabridge(c: &Chemins) -> [(Format, PathBuf); 3] {
    [
        (Format::Vst3, c.home.join(".vst3/yabridge")),
        (Format::Vst2, c.home.join(".vst/yabridge")),
        (Format::Clap, c.home.join(".clap/yabridge")),
    ]
}

/// Retire des dossiers yabridge les liens cassés qui pointaient vers le préfixe, avec leur
/// pendant ponté ; rend le nombre de **plugins** retirés (un par bundle VST3, par lien `.dll`
/// VST2, par lien `-win` CLAP), pas le nombre de liens.
///
/// Sert après un retour à un point de restauration : le plugin Windows a disparu du préfixe
/// mais yabridge a laissé ses liens (désormais cassés) et son fichier ponté, que les logiciels de
/// musique afficheraient comme plugin mort.
///
/// Garde-fous :
/// - on ne parcourt que `<home>/.vst3/yabridge`, `<home>/.vst/yabridge` et
///   `<home>/.clap/yabridge`, sans jamais suivre un lien (racines comprises) ;
/// - un lien n'est retiré que s'il est cassé **et** que sa cible est sous une des
///   [`racines_du_prefixe`] : un lien cassé qui vient d'une autre installation n'est pas à nous ;
/// - le pendant ponté se déduit de la « Disposition constatée » en tête de fichier ; un bundle
///   VST3 n'est supprimé que s'il ne contient que des dossiers, `Contents/x86_64-linux/*.so` et
///   des liens cassés vers le préfixe (`x86_64-win/<X>.vst3`, `Resources`) ; sinon seuls ces
///   liens partent ;
/// - les liens sont d'abord regroupés par bundle, puis on agit : le résultat ne dépend pas de
///   l'ordre du parcours ;
/// - seuls les dossiers que la suppression a vidés sont retirés (en remontant), jamais les
///   trois racines ni un dossier vide qui existait déjà.
///
/// Erreur : [`Erreur::Io`] si un dossier est illisible ou une suppression échoue.
pub fn retirer_liens_casses(c: &Chemins) -> Resultat<usize> {
    let mut retires = 0;
    let racines_prefixe = racines_du_prefixe(c);
    for (format, racine) in racines_yabridge(c) {
        let liens = liens_casses_du_prefixe(&racines_prefixe, &racine)?;
        retires += match format {
            Format::Vst3 => retirer_bundles_vst3(&racine, &liens)?,
            Format::Vst2 | Format::Clap => {
                let mut n = 0;
                for lien in &liens {
                    n += usize::from(retirer_lien_plat(&racine, lien, format)?);
                }
                n
            }
        };
    }
    Ok(retires)
}

/// Liens symboliques de `racine` dont la cible n'existe plus et se trouve sous une des
/// `racines_prefixe` ([`racines_du_prefixe`]). Une racine absente donne une liste vide. Ne suit
/// aucun lien.
fn liens_casses_du_prefixe(racines_prefixe: &[PathBuf], racine: &Path) -> Resultat<Vec<PathBuf>> {
    let mut liens = Vec::new();
    // `follow_root_links(false)` : si la racine elle-même est un lien, on n'y entre pas.
    for entree in WalkDir::new(racine)
        .follow_links(false)
        .follow_root_links(false)
        .min_depth(1)
    {
        let entree = match entree {
            Ok(e) => e,
            // Racine absente : rien à nettoyer.
            Err(e)
                if e.depth() == 0
                    && e.io_error()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
            {
                return Ok(liens)
            }
            Err(e) => return Err(Erreur::Io(e.into())),
        };
        if !entree.path_is_symlink() {
            continue;
        }
        let Some(cible) = lire_lien(entree.path())? else {
            continue;
        };
        // Cassé = la cible n'existe pas (une autre erreur, comme un droit refusé, n'est pas
        // une preuve : on garde le lien).
        let casse = matches!(
            std::fs::metadata(entree.path()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound
        );
        if casse && dans_le_prefixe(racines_prefixe, &cible) {
            liens.push(entree.into_path());
        }
    }
    Ok(liens)
}

/// Bundle VST3 d'un lien : l'ancêtre `*.vst3` le plus proche (lien lui-même exclu) sous
/// `racine`. `None` pour un lien qui n'est pas dans un bundle.
fn bundle_du_lien<'a>(racine: &Path, lien: &'a Path) -> Option<&'a Path> {
    lien.ancestors()
        .skip(1)
        .take_while(|a| a.starts_with(racine) && *a != racine)
        .find(|a| a.extension().is_some_and(|x| x == "vst3"))
}

/// Retire les plugins VST3 dont `liens` (cassés, vers le préfixe) sont les liens ; rend leur
/// nombre. Regroupe par bundle avant d'agir : un bundle avec ses deux liens (`x86_64-win` et
/// `Resources`) compte pour un.
fn retirer_bundles_vst3(racine: &Path, liens: &[PathBuf]) -> Resultat<usize> {
    let mut groupes: BTreeMap<&Path, Vec<&Path>> = BTreeMap::new();
    let mut seuls = Vec::new();
    for lien in liens {
        match bundle_du_lien(racine, lien) {
            Some(b) => groupes.entry(b).or_default().push(lien),
            None => seuls.push(lien.as_path()),
        }
    }
    let mut retires = 0;
    for (bundle, ses_liens) in groupes {
        if bundle_ne_contient_que_le_pont(bundle, liens) {
            std::fs::remove_dir_all(bundle)?;
            nettoyer_parents(racine, bundle);
        } else {
            for lien in ses_liens {
                std::fs::remove_file(lien)?;
                nettoyer_parents(racine, lien);
            }
        }
        retires += 1;
    }
    for lien in seuls {
        std::fs::remove_file(lien)?;
        nettoyer_parents(racine, lien);
        retires += 1;
    }
    Ok(retires)
}

/// Retire un lien cassé VST2 ou CLAP et son pendant ponté. `Ok(false)` si le lien avait déjà
/// disparu. VST2 : le `.so` n'est cherché que si le lien se nomme `*.dll` (sans casse) ; CLAP :
/// le pendant est le chemin sans le suffixe `-win`. Sinon, seul le lien part.
fn retirer_lien_plat(racine: &Path, lien: &Path, format: Format) -> Resultat<bool> {
    if lien.symlink_metadata().is_err() {
        return Ok(false);
    }
    std::fs::remove_file(lien)?;
    let pendant = match format {
        Format::Vst2 => lien
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("dll"))
            .then(|| lien.with_extension("so")),
        _ => lien
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix("-win"))
            .map(|sans_win| lien.with_file_name(sans_win)),
    };
    if let Some(p) = pendant {
        supprimer_fichier_regulier(&p)?;
    }
    nettoyer_parents(racine, lien);
    Ok(true)
}

/// Un bundle VST3 ne contient-il que ce que yabridge y met : des dossiers, des fichiers `.so`
/// sous `Contents/x86_64-linux`, et des liens de `casses` (liens cassés vers le préfixe,
/// comme `x86_64-win/<X>.vst3` et `Resources`) ? Sans suivre de lien. Au moindre autre
/// élément (ou bundle illisible) : non, et le bundle est alors conservé.
fn bundle_ne_contient_que_le_pont(bundle: &Path, casses: &[PathBuf]) -> bool {
    let linux = bundle.join("Contents/x86_64-linux");
    WalkDir::new(bundle)
        .follow_links(false)
        .follow_root_links(false)
        .into_iter()
        .all(|e| match e {
            Ok(e) if e.file_type().is_dir() => true,
            Ok(e) if e.path_is_symlink() => casses.iter().any(|l| l == e.path()),
            Ok(e) => {
                e.file_type().is_file()
                    && e.path().parent() == Some(linux.as_path())
                    && e.path().extension().is_some_and(|x| x == "so")
            }
            Err(_) => false,
        })
}

/// Supprime `chemin` seulement si c'est un fichier ordinaire (ni dossier ni lien) ; absent : rien.
fn supprimer_fichier_regulier(chemin: &Path) -> Resultat<()> {
    match chemin.symlink_metadata() {
        Ok(m) if m.file_type().is_file() => Ok(std::fs::remove_file(chemin)?),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Supprime les parents de `chemin` devenus vides, du plus profond vers `racine` (exclue, jamais
/// supprimée). S'arrête au premier dossier non vide : `remove_dir` refuse de supprimer un dossier
/// qui contient quelque chose, c'est la protection. Les dossiers vides qui n'ont pas été vidés par
/// nous ne sont pas touchés. Comme `restauration::retirer_dossiers_vides`.
fn nettoyer_parents(racine: &Path, chemin: &Path) {
    for parent in chemin.ancestors().skip(1) {
        if parent == racine || !parent.starts_with(racine) || std::fs::remove_dir(parent).is_err() {
            break;
        }
    }
}

/// Chemin de `plugin` relatif à son dossier de plugins (`DOSSIERS_PLUGINS`).
fn chemin_relatif(c: &Chemins, plugin: &Path) -> Option<PathBuf> {
    let drive_c = c.drive_c();
    DOSSIERS_PLUGINS
        .iter()
        .find_map(|d| plugin.strip_prefix(drive_c.join(d)).ok())
        .map(Path::to_path_buf)
}

/// Cible du lien symbolique `lien` ; `None` s'il n'existe pas ou n'est pas un lien.
fn lire_lien(lien: &Path) -> Resultat<Option<PathBuf>> {
    match std::fs::read_link(lien) {
        // Une cible relative se lit depuis le dossier du lien, pas depuis le
        // dossier courant : sinon un lien interne serait pris pour un lien externe.
        Ok(cible) => Ok(Some(lien.parent().unwrap_or(Path::new("/")).join(cible))),
        Err(e)
            if matches!(
                e.raw_os_error(),
                Some(libc::ENOENT | libc::ENOTDIR | libc::EINVAL)
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(Erreur::Io(std::io::Error::new(
            e.kind(),
            format!("{} : {e}", lien.display()),
        ))),
    }
}

/// Dossiers de plugins (`DOSSIERS_PLUGINS`) qui existent dans le préfixe.
fn dossiers_existants(c: &Chemins) -> Vec<PathBuf> {
    let mut dossiers: Vec<PathBuf> = DOSSIERS_PLUGINS
        .iter()
        .map(|d| c.drive_c().join(d))
        .filter(|d| d.is_dir())
        .collect();
    dossiers.sort();
    dossiers
}

/// Dossier lu par yabridgectl comme `XDG_CONFIG_HOME` (isolé de `~/.config`).
fn dossier_config_isole(c: &Chemins) -> PathBuf {
    c.donnees.join("yabridgectl")
}

/// Fichier de configuration : yabridgectl y ajoute lui-même `yabridgectl/config.toml`.
fn fichier_configuration(c: &Chemins) -> PathBuf {
    dossier_config_isole(c).join("yabridgectl/config.toml")
}

/// Écrit la configuration au format TOML.
fn ecrire_configuration(c: &Chemins, config: &ConfigYabridge) -> Resultat<()> {
    let fichier = fichier_configuration(c);
    if let Some(parent) = fichier.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let texte = toml::to_string(config).map_err(|e| {
        Erreur::NonPrisEnCharge(trf(
            "yabridgectl configuration: {error}",
            &[("error", &e.to_string())],
        ))
    })?;
    std::fs::write(&fichier, texte)?;
    Ok(())
}

/// Prépare `yabridgectl sync` : stdin fermé (fait par [`lancer`]), configuration
/// isolée, `PATH` avec Wine et yabridge en tête, préfixe WiMao explicite.
fn commande_sync(c: &Chemins) -> Resultat<Command> {
    let programme = c.yabridge_dir.join("yabridgectl");
    if !programme.exists() {
        return Err(Erreur::Introuvable(trf(
            "yabridgectl ({path} does not exist): install the WiMao yabridge package or set WIMAO_YABRIDGE_DIR",
            &[("path", &programme.display().to_string())],
        )));
    }
    let mut cmd = Command::new(programme);
    // `sync` lance Wine pour vérifier chaque plugin : mêmes variables que nos
    // commandes Wine (HOME, isolation éventuelle, sans fenêtres Mono/Gecko).
    preparer_environnement(&mut cmd, c, &[SURCHARGES_SANS_FENETRES]);
    // Après `preparer_environnement` : remplace son éventuel `XDG_CONFIG_HOME` de
    // test, car yabridgectl doit toujours lire sa configuration isolée.
    cmd.arg("sync")
        .env("XDG_CONFIG_HOME", dossier_config_isole(c))
        .env("PATH", chemin_de_sync(c)?)
        .env("WINEPREFIX", &c.prefixe)
        .env("WINEDEBUG", "-all");
    Ok(cmd)
}

/// `yabridgectl sync` pour la configuration **de l'utilisateur**, après l'annulation d'un import
/// (`import::annuler`) : ses liens vers `~/.wine-mao` sont recréés à l'identique.
///
/// - `xdg_config_home` : dossier qui contient `yabridgectl/config.toml`. L'appelant y met une
///   **copie** de `~/.config/yabridgectl/config.toml` : yabridgectl réécrit son fichier pendant
///   `sync` (champ `last_known_config`, constaté dans le binaire fourni) et WiMao ne doit jamais
///   modifier celui de l'utilisateur. Les liens produits ne dépendent que du contenu de la
///   configuration et de `HOME`, donc la copie donne exactement le même résultat.
/// - `wineprefix` : l'environnement de l'utilisateur (`~/.wine-mao`) ; sans `WINEPREFIX`, la
///   vérification finale de `sync` créerait un `~/.wine` par défaut.
/// - Mêmes `HOME`, `PATH` et surcharges de DLL que [`commande_sync`].
///
/// Erreurs : comme [`commande_sync`].
pub(crate) fn commande_sync_utilisateur(
    c: &Chemins,
    xdg_config_home: &Path,
    wineprefix: &Path,
) -> Resultat<Command> {
    let mut cmd = commande_sync(c)?;
    cmd.env("XDG_CONFIG_HOME", xdg_config_home)
        .env("WINEPREFIX", wineprefix);
    Ok(cmd)
}

/// Lance `cmd` (une commande `yabridgectl sync`) avec le délai de `sync` ; un code de sortie non
/// nul est une [`Erreur::Commande`].
pub(crate) fn lancer_sync(cmd: &mut Command) -> Resultat<()> {
    let sortie = lancer(cmd, DELAI_SYNC)?;
    if sortie.code != 0 {
        return Err(Erreur::Commande {
            programme: "yabridgectl sync".to_string(),
            code: Some(sortie.code),
            sortie: sortie.texte,
        });
    }
    Ok(())
}

/// `PATH` de `sync` : Wine, yabridge, puis le `PATH` actuel.
///
/// Erreur : [`Erreur::NonPrisEnCharge`] si un dossier contient `:` (impossible à
/// mettre dans un `PATH`) ; un `PATH` vide en silence ferait échouer `sync` de
/// façon incompréhensible.
fn chemin_de_sync(c: &Chemins) -> Resultat<OsString> {
    let mut elements = vec![c.wine_bin(), c.yabridge_dir.clone()];
    if let Some(actuel) = std::env::var_os("PATH") {
        elements.extend(std::env::split_paths(&actuel));
    }
    std::env::join_paths(elements).map_err(|e| {
        Erreur::NonPrisEnCharge(trf(
            "PATH for yabridgectl: a folder contains \":\" ({error}); move Wine or yabridge",
            &[("error", &e.to_string())],
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// Plugin du préfixe de test, dans le dossier `dossier` (relatif à `drive_c`).
    fn plugin(c: &Chemins, dossier: &str, fichier: &str, format: Format) -> PluginWindows {
        let chemin = c.drive_c().join(dossier).join(fichier);
        let nom = Path::new(fichier).file_stem().unwrap().to_string_lossy();
        PluginWindows {
            chemin,
            format,
            nom: nom.into_owned(),
        }
    }

    fn inventaire(p: &[&PluginWindows]) -> Inventaire {
        Inventaire(p.iter().map(|x| (*x).clone()).collect(), Default::default())
    }

    // --- assurer_lien_yabridge : un test par cas ---

    #[test]
    fn lien_yabridge_absent_est_cree() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        assert_eq!(assurer_lien_yabridge(&c).unwrap(), LienYabridge::Cree);
        let lien = c.home.join(".local/share/yabridge");
        assert_eq!(std::fs::read_link(lien).unwrap(), c.yabridge_dir);
    }

    #[test]
    fn lien_yabridge_deja_bon_est_reconnu() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        assurer_lien_yabridge(&c).unwrap();
        assert_eq!(assurer_lien_yabridge(&c).unwrap(), LienYabridge::DejaBon);
    }

    #[test]
    fn lien_yabridge_vers_ailleurs_est_laisse() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let lien = c.home.join(".local/share/yabridge");
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        let ailleurs = t.path().join("ailleurs");
        symlink(&ailleurs, &lien).unwrap();
        assert_eq!(
            assurer_lien_yabridge(&c).unwrap(),
            LienYabridge::AutreLien(ailleurs.clone())
        );
        assert_eq!(std::fs::read_link(lien).unwrap(), ailleurs);
    }

    #[test]
    fn lien_yabridge_vrai_dossier_est_laisse() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let lien = c.home.join(".local/share/yabridge");
        std::fs::create_dir_all(&lien).unwrap();
        assert_eq!(assurer_lien_yabridge(&c).unwrap(), LienYabridge::Dossier);
        assert!(lien.is_dir() && !lien.is_symlink());
    }

    // --- etat_lien_yabridge : lecture seule, un test par cas ---

    #[test]
    fn etat_du_lien_yabridge_absent_ne_cree_rien() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        assert_eq!(etat_lien_yabridge(&c).unwrap(), EtatLienYabridge::Absent);
        assert!(std::fs::symlink_metadata(emplacement_lien_yabridge(&c)).is_err());
    }

    #[test]
    fn etat_du_lien_yabridge_bon_meme_si_la_cible_n_existe_pas_encore() {
        let t = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(t.path());
        // yabridge_dir n'existe pas (paquet pas installé) : le lien est pendant mais attendu.
        c.yabridge_dir = t.path().join("yabridge-pas-installe");
        assurer_lien_yabridge(&c).unwrap();
        assert!(!c.yabridge_dir.exists());
        assert_eq!(etat_lien_yabridge(&c).unwrap(), EtatLienYabridge::Bon);
    }

    #[test]
    fn etat_du_lien_yabridge_bon_par_un_lien_intermediaire() {
        let t = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(t.path());
        c.yabridge_dir = t.path().join("yabridge");
        std::fs::create_dir_all(&c.yabridge_dir).unwrap();
        let intermediaire = t.path().join("intermediaire");
        symlink(&c.yabridge_dir, &intermediaire).unwrap();
        let lien = emplacement_lien_yabridge(&c);
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        symlink(&intermediaire, &lien).unwrap();
        assert_eq!(etat_lien_yabridge(&c).unwrap(), EtatLienYabridge::Bon);
    }

    #[test]
    fn etat_du_lien_yabridge_vers_un_autre_dossier_existant() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let ailleurs = t.path().join("ailleurs");
        std::fs::create_dir_all(&ailleurs).unwrap();
        let lien = emplacement_lien_yabridge(&c);
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        symlink(&ailleurs, &lien).unwrap();
        assert_eq!(
            etat_lien_yabridge(&c).unwrap(),
            EtatLienYabridge::Ailleurs(ailleurs)
        );
    }

    #[test]
    fn etat_du_lien_yabridge_casse() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let disparu = t.path().join("disparu");
        let lien = emplacement_lien_yabridge(&c);
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        symlink(&disparu, &lien).unwrap();
        assert_eq!(
            etat_lien_yabridge(&c).unwrap(),
            EtatLienYabridge::Casse(disparu)
        );
    }

    #[test]
    fn etat_du_lien_yabridge_vrai_dossier() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        std::fs::create_dir_all(emplacement_lien_yabridge(&c)).unwrap();
        assert_eq!(etat_lien_yabridge(&c).unwrap(), EtatLienYabridge::Dossier);
    }

    #[test]
    fn vst2_avec_son_vst3_est_un_doublon_dans_la_blacklist() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let dll = plugin(&c, "Program Files/VstPlugins", "Hype_x64.dll", Format::Vst2);
        let v3 = plugin(
            &c,
            "Program Files/Common Files/VST3",
            "Hype.vst3",
            Format::Vst3,
        );
        let (config, etats) = configuration(&c, &inventaire(&[&dll, &v3])).unwrap();
        assert_eq!(etats[&dll.chemin], EtatPontage::Doublon);
        assert_eq!(etats[&v3.chemin], EtatPontage::Ponte);
        assert_eq!(config.blacklist, vec![dll.chemin]);
    }

    #[test]
    fn vst2_sans_vst3_est_ponte() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let dll = plugin(
            &c,
            "Program Files/VstPlugins",
            "Xpand!2_x64.dll",
            Format::Vst2,
        );
        let (config, etats) = configuration(&c, &inventaire(&[&dll])).unwrap();
        assert_eq!(etats[&dll.chemin], EtatPontage::Ponte);
        assert!(config.blacklist.is_empty());
    }

    #[test]
    fn lien_vers_une_autre_installation_est_dejaponte_ailleurs() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let serum = plugin(
            &c,
            "Program Files/Common Files/VST3",
            "Serum2.vst3",
            Format::Vst3,
        );
        let dossier = c
            .home
            .join(".vst3/yabridge/Serum2.vst3/Contents/x86_64-win");
        std::fs::create_dir_all(&dossier).unwrap();
        let ailleurs = PathBuf::from("/autre/drive_c/Program Files/Common Files/VST3/Serum2.vst3");
        symlink(&ailleurs, dossier.join("Serum2.vst3")).unwrap();
        let (config, etats) = configuration(&c, &inventaire(&[&serum])).unwrap();
        assert_eq!(
            etats[&serum.chemin],
            EtatPontage::DejaPonteAilleurs(ailleurs)
        );
        assert_eq!(config.blacklist, vec![serum.chemin]);
    }

    #[test]
    fn lien_vers_notre_prefixe_reste_ponte() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let p = plugin(
            &c,
            "Program Files/VstPlugins/Sous Dossier",
            "A_x64.dll",
            Format::Vst2,
        );
        let lien = c.home.join(".vst/yabridge/Sous Dossier/A_x64.dll");
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        symlink(&p.chemin, &lien).unwrap();
        let (_, etats) = configuration(&c, &inventaire(&[&p])).unwrap();
        assert_eq!(etats[&p.chemin], EtatPontage::Ponte);
    }

    #[test]
    fn lien_vers_l_ancien_emplacement_est_le_notre() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let p = plugin(
            &c,
            "Program Files/Common Files/VST3",
            "A.vst3",
            Format::Vst3,
        );
        std::fs::create_dir_all(&p.chemin).unwrap();
        // Lien posé par l'ancienne installation : il passe par `~/.wine-mao`.
        let ancien = c.home.join(".wine-mao");
        let dossier = c.home.join(".vst3/yabridge/A.vst3/Contents/x86_64-win");
        std::fs::create_dir_all(&dossier).unwrap();
        let cible = ancien.join("drive_c/Program Files/Common Files/VST3/A.vst3");
        symlink(&cible, dossier.join("A.vst3")).unwrap();
        // Avant l'import, ce lien est celui d'une autre installation.
        let (_, etats) = configuration(&c, &inventaire(&[&p])).unwrap();
        assert_eq!(
            etats[&p.chemin],
            EtatPontage::DejaPonteAilleurs(cible.clone())
        );
        // État après un import : `~/.wine-mao` est un lien vers notre préfixe, et la trace
        // `import.toml` donne l'ancien chemin.
        symlink(&c.prefixe, &ancien).unwrap();
        let dossier_import = c.donnees.join(crate::import::DOSSIER_IMPORT);
        std::fs::create_dir_all(&dossier_import).unwrap();
        std::fs::write(
            dossier_import.join(crate::import::FICHIER_TRACE),
            format!(
                "ancien = \"{}\"\ndate = \"2026-11-01T10:00:00+01:00\"\nsauvegarde = \"/s\"\n",
                ancien.display()
            ),
        )
        .unwrap();
        let (config, etats) = configuration(&c, &inventaire(&[&p])).unwrap();
        assert_eq!(etats[&p.chemin], EtatPontage::Ponte);
        assert!(config.blacklist.is_empty());
    }

    #[test]
    fn configuration_liste_les_dossiers_existants_et_s_ecrit_en_toml() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let vst3 = c.drive_c().join("Program Files/Common Files/VST3");
        std::fs::create_dir_all(&vst3).unwrap();
        let (config, _) = configuration(&c, &Inventaire::default()).unwrap();
        assert_eq!(config.plugin_dirs, vec![vst3]);
        assert_eq!(config.vst2_location, "centralized");
        ecrire_configuration(&c, &config).unwrap();
        let lu = std::fs::read_to_string(fichier_configuration(&c)).unwrap();
        assert_eq!(toml::from_str::<ConfigYabridge>(&lu).unwrap(), config);
        assert!(fichier_configuration(&c).starts_with(c.donnees.join("yabridgectl")));
    }

    #[test]
    fn bundle_linux_suit_la_disposition_constatee() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let v3 = plugin(
            &c,
            "Program Files/Common Files/VST3/Éditeur",
            "X.vst3",
            Format::Vst3,
        );
        let v2 = plugin(&c, "Program Files/VstPlugins/PA", "Y_x64.dll", Format::Vst2);
        assert_eq!(bundle_linux(&c, &v3), None);
        let win = c
            .home
            .join(".vst3/yabridge/Éditeur/X.vst3/Contents/x86_64-win");
        std::fs::create_dir_all(&win).unwrap();
        symlink(&v3.chemin, win.join("X.vst3")).unwrap();
        let central = c.home.join(".vst/yabridge/PA");
        std::fs::create_dir_all(&central).unwrap();
        symlink(&v2.chemin, central.join("Y_x64.dll")).unwrap();
        std::fs::write(central.join("Y_x64.so"), b"").unwrap();
        assert_eq!(
            bundle_linux(&c, &v3),
            Some(c.home.join(".vst3/yabridge/Éditeur/X.vst3"))
        );
        assert_eq!(bundle_linux(&c, &v2), Some(central.join("Y_x64.so")));
    }

    /// Bundle VST3 (dossier, le cas de Serum 2 et de la plupart des VST3 récents) : yabridgectl
    /// relie le **binaire** `Contents/x86_64-win/<nom>.vst3` du bundle, pas le dossier. Défaut
    /// trouvé à la vérification réelle du sous-projet 3 : le plugin passait pour non ponté (toast
    /// « 1 plugin n'est pas encore ponté » à chaque lancement) et n'était jamais testé.
    #[test]
    fn bundle_linux_reconnait_le_lien_vers_le_binaire_d_un_bundle() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let v3 = plugin(
            &c,
            "Program Files/Common Files/VST3",
            "Serum2.vst3",
            Format::Vst3,
        );
        // Le bundle Windows et son binaire, comme les pose un installeur.
        let binaire = v3.chemin.join("Contents/x86_64-win/Serum2.vst3");
        std::fs::create_dir_all(binaire.parent().unwrap()).unwrap();
        std::fs::write(&binaire, b"MZ").unwrap();
        // Le pont, comme le pose yabridgectl : lien vers le binaire, pas vers le dossier.
        let ponte = c.home.join(".vst3/yabridge/Serum2.vst3");
        let win = ponte.join("Contents/x86_64-win");
        std::fs::create_dir_all(&win).unwrap();
        symlink(&binaire, win.join("Serum2.vst3")).unwrap();
        assert_eq!(bundle_linux(&c, &v3), Some(ponte.clone()));

        // Même chose quand le lien passe par un autre nom du préfixe (`~/.wine-mao` après
        // un import) : on compare alors les fichiers, liens résolus.
        let autre_nom = t.path().join("ancien-nom");
        symlink(c.drive_c().parent().unwrap(), &autre_nom).unwrap();
        let par_l_autre_nom = autre_nom.join(
            "drive_c/Program Files/Common Files/VST3/Serum2.vst3/Contents/x86_64-win/Serum2.vst3",
        );
        std::fs::remove_file(win.join("Serum2.vst3")).unwrap();
        symlink(&par_l_autre_nom, win.join("Serum2.vst3")).unwrap();
        assert_eq!(bundle_linux(&c, &v3), Some(ponte));

        // Un lien vers le binaire d'un **autre** bundle ne compte pas.
        let autre = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Autre.vst3/Contents/x86_64-win/Autre.vst3");
        std::fs::create_dir_all(autre.parent().unwrap()).unwrap();
        std::fs::write(&autre, b"MZ").unwrap();
        std::fs::remove_file(win.join("Serum2.vst3")).unwrap();
        symlink(&autre, win.join("Serum2.vst3")).unwrap();
        assert_eq!(bundle_linux(&c, &v3), None);
    }

    #[test]
    fn bundle_linux_ignore_un_lien_vers_un_autre_plugin() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let v3 = plugin(
            &c,
            "Program Files/Common Files/VST3",
            "X.vst3",
            Format::Vst3,
        );
        let win = c.home.join(".vst3/yabridge/X.vst3/Contents/x86_64-win");
        std::fs::create_dir_all(&win).unwrap();
        symlink("/ailleurs/X.vst3", win.join("X.vst3")).unwrap();
        assert_eq!(bundle_linux(&c, &v3), None);
    }

    #[test]
    fn commande_sync_est_isolee_et_sans_prune() {
        let t = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(t.path());
        let faux = t.path().join("yabridge");
        std::fs::create_dir_all(&faux).unwrap();
        std::fs::write(faux.join("yabridgectl"), b"").unwrap();
        c.yabridge_dir = faux;
        let cmd = commande_sync(&c).unwrap();
        assert_eq!(cmd.get_args().collect::<Vec<_>>(), vec!["sync"]);
        let env = |nom: &str| {
            cmd.get_envs()
                .find(|(k, _)| *k == nom)
                .and_then(|(_, v)| v.map(|v| v.to_os_string()))
        };
        assert_eq!(
            env("XDG_CONFIG_HOME"),
            Some(c.donnees.join("yabridgectl").into())
        );
        assert_eq!(env("HOME"), Some(c.home.clone().into()));
        assert_eq!(env("WINEPREFIX"), Some(c.prefixe.clone().into()));
        // Chemins de test : isolation complète (Ruling 15).
        assert_eq!(
            env("WINEDLLOVERRIDES"),
            Some("mscoree,mshtml=;winemenubuilder.exe=d".into())
        );
        assert_eq!(
            env("XDG_DATA_HOME"),
            Some(c.home.join(".local/share").into())
        );
        let path = env("PATH").unwrap();
        assert_eq!(std::env::split_paths(&path).next(), Some(c.wine_bin()));
    }

    #[test]
    fn dossier_avec_deux_points_dans_le_path_est_une_erreur() {
        let t = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(t.path());
        c.wine_dir = PathBuf::from("/opt/a:b");
        assert!(chemin_de_sync(&c).is_err());
    }

    #[test]
    fn lien_relatif_se_lit_depuis_le_dossier_du_lien() {
        let t = tempfile::tempdir().unwrap();
        let lien = t.path().join("dossier/lien");
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        symlink("cible", &lien).unwrap();
        assert_eq!(
            lire_lien(&lien).unwrap(),
            Some(t.path().join("dossier/cible"))
        );
    }

    /// Test réel : exige Wine, yabridge et un petit plugin VST3 Windows
    /// (variable `WIMAO_TEST_PLUGIN_VST3`, fichier ou dossier `.vst3` lu seulement).
    #[test]
    #[ignore = "Wine requis"]
    fn ponter_reellement_un_vst3() {
        let Some(source) = std::env::var_os("WIMAO_TEST_PLUGIN_VST3").map(PathBuf::from) else {
            eprintln!("WIMAO_TEST_PLUGIN_VST3 absente : test ignoré");
            return;
        };
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        crate::environnement::creer_environnement(&c, &crate::ProgressionMuette).unwrap();
        let dest = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Essai.vst3");
        copier(&source, &dest);
        // Un second plugin, déjà « ponté ailleurs » AVANT le sync : WiMao ne doit pas y toucher.
        let autre = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Ailleurs.vst3");
        copier(&source, &autre);
        let win = c
            .home
            .join(".vst3/yabridge/Ailleurs.vst3/Contents/x86_64-win");
        std::fs::create_dir_all(&win).unwrap();
        let etranger = PathBuf::from("/autre/prefixe/drive_c/VST3/Ailleurs.vst3");
        std::os::unix::fs::symlink(&etranger, win.join("Ailleurs.vst3")).unwrap();

        let inv = Inventaire::relever(&c.drive_c()).unwrap();
        let etats = ponter(&c, &inv).unwrap();
        assert_eq!(etats[&dest], EtatPontage::Ponte);
        assert_eq!(
            etats[&autre],
            EtatPontage::DejaPonteAilleurs(etranger.clone())
        );
        assert!(c.home.join(".vst3/yabridge/Essai.vst3").exists());
        assert!(!c.home.join(".config/yabridgectl").exists());
        assert_eq!(
            std::fs::read_link(win.join("Ailleurs.vst3")).unwrap(),
            etranger
        );
        let essai = inv.0.iter().find(|p| p.chemin == dest).unwrap();
        assert!(bundle_linux(&c, essai).is_some());
        // Idempotence : un second pontage rend les mêmes états et ne casse rien.
        assert_eq!(ponter(&c, &inv).unwrap(), etats);
        assert_eq!(
            std::fs::read_link(win.join("Ailleurs.vst3")).unwrap(),
            etranger
        );
        assert!(bundle_linux(&c, essai).is_some());
        let _ = crate::wine::Wine::nouveau(&c).arreter();
    }

    /// Copie un fichier ou un dossier (le plugin source n'est jamais modifié).
    fn copier(source: &Path, dest: &Path) {
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let etat = Command::new("cp").arg("-r").arg(source).arg(dest).status();
        assert!(etat.unwrap().success());
    }

    /// Crée le lien `lien` vers `cible` (qui n'existe pas) après avoir créé ses parents.
    fn lien_casse(lien: &Path, cible: &Path) {
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        symlink(cible, lien).unwrap();
    }

    /// Bundle VST3 factice `<home>/.vst3/yabridge/Ed/X.vst3` : le `.so` ponté et le lien
    /// `x86_64-win/X.vst3` vers `cible`. Rend (bundle, lien).
    fn bundle_vst3(c: &Chemins, cible: &Path) -> (PathBuf, PathBuf) {
        let bundle = c.home.join(".vst3/yabridge/Ed/X.vst3");
        let linux = bundle.join("Contents/x86_64-linux");
        std::fs::create_dir_all(&linux).unwrap();
        std::fs::write(linux.join("X.so"), "pont").unwrap();
        let lien = bundle.join("Contents/x86_64-win/X.vst3");
        lien_casse(&lien, cible);
        (bundle, lien)
    }

    #[test]
    fn lien_casse_dans_le_prefixe_est_retire_avec_son_bundle() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let cible = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Ed/X.vst3");
        let (bundle, _) = bundle_vst3(&c, &cible);
        assert_eq!(retirer_liens_casses(&c).unwrap(), 1);
        assert!(!bundle.exists());
        // Le dossier éditeur, devenu vide, part aussi ; la racine reste.
        assert!(!c.home.join(".vst3/yabridge/Ed").exists());
        assert!(c.home.join(".vst3/yabridge").is_dir());
    }

    #[test]
    fn lien_casse_hors_du_prefixe_est_garde() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let (bundle, lien) = bundle_vst3(&c, Path::new("/opt/ailleurs/X.vst3"));
        assert_eq!(retirer_liens_casses(&c).unwrap(), 0);
        assert!(lien.symlink_metadata().is_ok());
        assert!(bundle.is_dir());
    }

    #[test]
    fn lien_valide_dans_le_prefixe_est_garde() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let cible = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Ed/X.vst3");
        std::fs::create_dir_all(&cible).unwrap();
        let (bundle, _) = bundle_vst3(&c, &cible);
        assert_eq!(retirer_liens_casses(&c).unwrap(), 0);
        assert!(bundle.is_dir());
    }

    #[test]
    fn lien_vst2_casse_retire_le_so() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let dossier = c.home.join(".vst/yabridge/P");
        let dll = dossier.join("X_x64.dll");
        lien_casse(
            &dll,
            &c.drive_c().join("Program Files/VstPlugins/P/X_x64.dll"),
        );
        let so = dossier.join("X_x64.so");
        std::fs::write(&so, "pont").unwrap();
        assert_eq!(retirer_liens_casses(&c).unwrap(), 1);
        assert!(dll.symlink_metadata().is_err());
        assert!(!so.exists());
    }

    #[test]
    fn lien_clap_casse_retire_le_pendant_sans_win() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let dossier = c.home.join(".clap/yabridge/Ed");
        let lien = dossier.join("X.clap-win");
        lien_casse(
            &lien,
            &c.drive_c()
                .join("Program Files/Common Files/CLAP/Ed/X.clap"),
        );
        let ponte = dossier.join("X.clap");
        std::fs::write(&ponte, "pont").unwrap();
        assert_eq!(retirer_liens_casses(&c).unwrap(), 1);
        assert!(lien.symlink_metadata().is_err());
        assert!(!ponte.exists());
    }

    #[test]
    fn bundle_avec_autre_contenu_est_garde() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let cible = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Ed/X.vst3");
        let (bundle, lien) = bundle_vst3(&c, &cible);
        std::fs::write(bundle.join("note.txt"), "à moi").unwrap();
        assert_eq!(retirer_liens_casses(&c).unwrap(), 1);
        assert!(lien.symlink_metadata().is_err(), "le lien est retiré");
        assert!(bundle.join("note.txt").is_file(), "le bundle reste");
    }

    #[test]
    fn sans_dossiers_yabridge_rien_a_faire() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        assert_eq!(retirer_liens_casses(&c).unwrap(), 0);
    }

    #[test]
    fn bundle_avec_lien_resources_casse_est_retire_une_fois() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let base = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Ed/X.vst3");
        let (bundle, _) = bundle_vst3(&c, &base);
        lien_casse(
            &bundle.join("Contents/Resources"),
            &base.join("Contents/Resources"),
        );
        assert_eq!(retirer_liens_casses(&c).unwrap(), 1);
        assert!(!bundle.exists());
    }

    #[test]
    fn dossier_vide_preexistant_survit() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let vide = c.home.join(".vst3/yabridge/Autre/Vide");
        std::fs::create_dir_all(&vide).unwrap();
        let cible = c
            .drive_c()
            .join("Program Files/Common Files/VST3/Ed/X.vst3");
        let (bundle, _) = bundle_vst3(&c, &cible);
        assert_eq!(retirer_liens_casses(&c).unwrap(), 1);
        assert!(!bundle.exists());
        assert!(vide.is_dir(), "le dossier vide d'avant reste");
    }

    #[test]
    fn lien_vst2_sans_extension_dll_n_efface_que_le_lien() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let dossier = c.home.join(".vst/yabridge/P");
        let lien = dossier.join("X");
        lien_casse(&lien, &c.drive_c().join("Program Files/VstPlugins/P/X"));
        let so = dossier.join("X.so");
        std::fs::write(&so, "autre").unwrap();
        assert_eq!(retirer_liens_casses(&c).unwrap(), 1);
        assert!(lien.symlink_metadata().is_err());
        assert!(so.exists());
    }
}
