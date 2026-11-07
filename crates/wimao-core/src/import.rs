//! Import de l'environnement Wine existant `~/.wine-mao` dans WiMao, et annulation de l'import.
//!
//! L'utilisateur a déjà un environnement Wine de musique (`~/.wine-mao`, plus de 100 Go de
//! plugins et de licences). Plutôt que de tout réinstaller, WiMao le **déplace** à son
//! emplacement (`Chemins::prefixe`) et laisse à l'ancienne place un **lien symbolique** vers lui :
//! les ponts yabridge existants (`~/.vst3/yabridge`…), qui pointent sous `~/.wine-mao`, continuent
//! de fonctionner sans modification, et le logiciel de musique ne voit aucune différence.
//!
//! - [`detecter`] : y a-t-il un `~/.wine-mao` à importer (taille, nombre de plugins) ?
//! - [`programmes_ouverts`] : programmes Windows ouverts dans `~/.wine-mao`, que l'interface
//!   nomme avant de demander l'accord d'arrêter son `wineserver`.
//! - [`importer`] : vérifications, arrêt de Wine, petite sauvegarde, déplacement, lien, trace.
//! - [`ancien_emplacement`], [`annulation_possible`], [`annuler`] : lecture de la trace et retour
//!   à la situation d'avant l'import.
//! - [`reprendre_interruption`] : au démarrage, termine ou abandonne un import (ou une
//!   annulation) coupé net par une coupure de courant.
//!
//! Utilise [`Chemins`], [`Wine::arreter`], `restauration` (plugins en cours, écriture atomique,
//! identifiant horodaté), `diagnostic` (verrou d'installation), `processus`, [`Inventaire`],
//! `pontage` (`yabridgectl sync` de l'utilisateur), [`historique`] et [`journal`]. Utilisé par
//! l'assistant de premier lancement et le menu de l'interface, par la CLI (`importer`,
//! `importer --annuler`), par `pontage::racines_du_prefixe`, `processus` et `restauration`
//! ([`noms_du_prefixe`]).
//!
//! # Disposition sur disque
//!
//! ```text
//! <donnees>/import/
//!     import.toml                      trace : ancien, date, sauvegarde, etat
//!     import.annule-<identifiant>.toml trace d'un import annulé (rien n'est supprimé)
//!     import.abandonne-<identifiant>.toml  trace d'un import coupé avant le déplacement
//!     sauvegarde-<identifiant>/        system.reg, user.reg, userdef.reg, config.toml
//! ```
//!
//! # Choix non évidents
//!
//! - **Déplacement, jamais copie** : `rename` est instantané et atomique sur un même système de
//!   fichiers. Sur deux disques différents (`st_dev`), l'import est refusé avant toute
//!   modification ([`Erreur::AutreDisque`]) : copier plus de 100 Go doublerait la place occupée.
//! - **Trace écrite avant chaque déplacement** : `etat = "en-cours"` avant le `rename` de
//!   l'import, `"fait"` une fois le lien créé, `"annulation-en-cours"` avant que l'annulation ne
//!   retire le lien. Chaque écriture est atomique et synchronisée, et les dossiers parents sont
//!   synchronisés après chaque `rename` ou lien : après une coupure de courant, le disque ne
//!   peut pas montrer un déplacement sans la trace qui l'annonce. [`reprendre_interruption`] lit
//!   cet état au lancement suivant. Seul `"fait"` permet l'annulation et compte pour `pontage`.
//! - **Retour en arrière immédiat** : si le lien ne peut pas être créé après le déplacement, le
//!   dossier est remis à sa place avant de remonter l'erreur, et la trace comme la petite
//!   sauvegarde sont effacées. Après le lien, l'import est réussi : une erreur d'écriture de la
//!   trace `"fait"` ou de l'historique est seulement notée au journal (la trace `"en-cours"`
//!   restée en place sera terminée par [`reprendre_interruption`]).
//! - **Fonctions injectées** : le cœur `importer_dans` reçoit le test « même disque » et la
//!   création du lien, pour tester le refus « autre disque » et l'échec du lien sans vrai second
//!   disque ni système de fichiers en panne.
//! - **`~/.config/yabridgectl` n'est jamais modifié** : il est seulement lu (copie dans la petite
//!   sauvegarde). À l'annulation, `yabridgectl sync` doit retrouver les liens de l'utilisateur ;
//!   comme yabridgectl réécrit son fichier de configuration pendant `sync` (`last_known_config`),
//!   il travaille sur une **copie** de ce fichier, jamais sur l'original.
//! - **Deux noms pour un même environnement** : après l'import, un plugin peut tourner avec
//!   `WINEPREFIX=~/.wine-mao` (pont créé avant l'import) ou `WINEPREFIX=<donnees>/prefixe`.
//!   [`noms_du_prefixe`] rend ces deux noms : processus, programmes ouverts, plugins en cours et
//!   `wineserver` actif sont cherchés sous les deux avant **tout** arrêt de Wine (installation,
//!   session, réparation, retour en arrière, sauvegarde, annulation…). Le `wineserver`, lui, est
//!   le même (Wine l'identifie par le dossier réel, lien résolu).

use crate::chemins::Chemins;
use crate::diagnostic::poser_verrou_installation;
use crate::erreur::{Erreur, Resultat};
use crate::historique::{self, Action, ResultatAction, TexteNeutre};
use crate::i18n::{tr, trf};
use crate::inventaire::Inventaire;
use crate::journal;
use crate::pontage;
use crate::processus;
use crate::progression::Progression;
use crate::restauration::{
    self, ecrire_atomique, identifiant_local, plugins_en_cours, plugins_en_cours_dans,
};
use crate::wine::Wine;
use serde::{Deserialize, Serialize};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Dossier des fichiers de l'import, dans le dossier de données : `<donnees>/import`.
pub const DOSSIER_IMPORT: &str = "import";
/// Trace d'un import en vigueur, dans [`DOSSIER_IMPORT`] : ancien chemin, date et dossier de la
/// petite sauvegarde (format TOML, voir `Trace`).
pub const FICHIER_TRACE: &str = "import.toml";

/// Nom de l'environnement à importer, dans le dossier personnel.
const NOM_ANCIEN: &str = ".wine-mao";
/// Sujet des entrées d'historique (le chemin tel que l'utilisateur le connaît).
const SUJET: &str = "~/.wine-mao";
/// Les trois fichiers de registre copiés dans la petite sauvegarde.
const FICHIERS_REG: [&str; 3] = ["system.reg", "user.reg", "userdef.reg"];
/// Fichiers de registre sans lesquels un dossier n'est pas un environnement Wine. `userdef.reg`
/// n'en fait pas partie : il est sauvegardé s'il existe.
const REG_OBLIGATOIRES: [&str; 2] = ["system.reg", "user.reg"];
/// Disque `C:` de l'environnement.
const DOSSIER_DISQUE_C: &str = "drive_c";
/// Configuration yabridgectl de l'utilisateur, relative au dossier personnel (lue, jamais écrite).
const CONFIG_YABRIDGECTL: &str = ".config/yabridgectl/config.toml";
/// Nom du fichier de configuration de yabridgectl (copie dans la petite sauvegarde, copie de
/// travail de `sync`).
const NOM_CONFIG: &str = "config.toml";
/// Sous-dossier de `XDG_CONFIG_HOME` où yabridgectl cherche sa configuration.
const SOUS_DOSSIER_YABRIDGECTL: &str = "yabridgectl";
/// Début du nom du dossier de la petite sauvegarde.
const DEBUT_SAUVEGARDE: &str = "sauvegarde-";
/// Début du nom d'une trace d'import annulé (suivi de l'identifiant et de `.toml`).
const DEBUT_ANNULE: &str = "import.annule-";
/// Début du nom d'une trace d'import abandonné avant tout déplacement (voir
/// [`reprendre_interruption`]).
const DEBUT_ABANDONNE: &str = "import.abandonne-";
/// Dossier virtuel des processus Linux (plugins en cours).
const RACINE_PROC: &str = "/proc";
/// Début du nom du dossier temporaire qui reçoit la copie de la configuration de l'utilisateur
/// pendant `yabridgectl sync` (supprimé ensuite).
const DEBUT_COPIE_SYNC: &str = "sync-utilisateur-";
/// Commande que l'utilisateur doit lancer lui-même si WiMao ne peut pas le faire à sa place.
const COMMANDE_SYNC: &str = "yabridgectl sync";
/// Nombre maximal de suffixes `-2`, `-3`… essayés pour trouver un nom libre.
const ESSAIS_MAXIMAUX: u32 = 1000;

/// Un environnement Wine existant, prêt à être importé (voir [`detecter`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironnementExistant {
    /// Son emplacement : `<home>/.wine-mao`.
    pub chemin: PathBuf,
    /// Taille apparente en octets (somme des tailles des fichiers, liens non suivis).
    pub taille: u64,
    /// Nombre de plugins Windows trouvés ([`Inventaire::relever`]).
    pub plugins: usize,
}

/// Résultat d'un import réussi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Importe {
    /// Ancien emplacement, devenu un lien vers `Chemins::prefixe`.
    pub ancien: PathBuf,
    /// Dossier de la petite sauvegarde (registre et configuration yabridgectl).
    pub sauvegarde: PathBuf,
}

/// Contenu de [`FICHIER_TRACE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Trace {
    /// Ancien emplacement de l'environnement (`<home>/.wine-mao`), devenu un lien.
    ancien: PathBuf,
    /// Date de l'import, à l'heure locale avec décalage (`historique::date_locale_iso`).
    date: String,
    /// Dossier de la petite sauvegarde.
    sauvegarde: PathBuf,
    /// Où en est l'opération ; absent dans une trace d'une version précédente, qui vaut alors
    /// [`EtatImport::Fait`].
    #[serde(default)]
    etat: EtatImport,
}

/// Avancement d'un import ou d'une annulation, écrit dans la trace **avant** chaque étape qui
/// déplace quelque chose : après une coupure de courant, [`reprendre_interruption`] sait ainsi
/// ce qui était en train de se faire.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum EtatImport {
    /// `en-cours` : écrit avant le déplacement ; l'import n'est pas terminé.
    EnCours,
    /// `fait` : le lien existe, l'import est terminé (seul état qui permet l'annulation).
    #[default]
    Fait,
    /// `annulation-en-cours` : écrit avant que l'annulation ne retire le lien.
    AnnulationEnCours,
}

/// `<home>/.wine-mao`.
fn chemin_ancien(c: &Chemins) -> PathBuf {
    c.home.join(NOM_ANCIEN)
}

/// `<donnees>/import`.
fn dossier_import(c: &Chemins) -> PathBuf {
    c.donnees.join(DOSSIER_IMPORT)
}

/// `<donnees>/import/import.toml`.
fn chemin_trace(c: &Chemins) -> PathBuf {
    dossier_import(c).join(FICHIER_TRACE)
}

/// Copie de `c` dont l'environnement Wine est `prefixe` : pour interroger ou arrêter Wine sur
/// l'ancien emplacement avec les mêmes Wine, `HOME` et isolation.
fn chemins_sur(c: &Chemins, prefixe: &Path) -> Chemins {
    let mut copie = c.clone();
    copie.prefixe = prefixe.to_path_buf();
    copie
}

/// Vrai si `chemin` est un environnement Wine importable : un **vrai** dossier (pas un lien,
/// sinon on déplacerait un lien, voire notre propre lien après un import) qui contient
/// `system.reg`, `user.reg` et `drive_c/`.
fn est_environnement(chemin: &Path) -> bool {
    let vrai_dossier = chemin.symlink_metadata().is_ok_and(|m| m.is_dir());
    vrai_dossier
        && REG_OBLIGATOIRES.iter().all(|r| chemin.join(r).is_file())
        && chemin.join(DOSSIER_DISQUE_C).is_dir()
}

/// Vrai si l'environnement de WiMao existe déjà, sous quelque forme que ce soit (dossier,
/// fichier, lien même cassé) : l'import ne doit rien écraser.
fn prefixe_present(c: &Chemins) -> bool {
    c.prefixe.symlink_metadata().is_ok()
}

/// Cherche un environnement `~/.wine-mao` à importer.
///
/// `Some` seulement si `<home>/.wine-mao` est un vrai dossier (pas un lien) avec `system.reg`,
/// `user.reg` et `drive_c/`, **et** que l'environnement de WiMao (`c.prefixe`) n'existe pas
/// encore. Parcourt tout le dossier pour la taille : peut prendre quelques secondes sur un gros
/// environnement, à appeler hors du fil de l'interface. Un inventaire illisible donne 0 plugin
/// (noté au journal) plutôt que de cacher l'environnement.
pub fn detecter(c: &Chemins) -> Option<EnvironnementExistant> {
    let chemin = chemin_ancien(c);
    if !est_environnement(&chemin) || prefixe_present(c) {
        return None;
    }
    let plugins = match Inventaire::relever(&chemin.join(DOSSIER_DISQUE_C)) {
        Ok(inventaire) => inventaire.0.len(),
        Err(e) => {
            journal::noter(
                "import",
                &format!("inventaire de {} : {e}", chemin.display()),
            );
            0
        }
    };
    Some(EnvironnementExistant {
        taille: taille_apparente(&chemin),
        plugins,
        chemin,
    })
}

/// Somme des tailles des fichiers sous `dossier`, sans suivre les liens (un lien vers le dossier
/// personnel ne doit pas être compté). Une entrée illisible est ignorée : c'est une estimation
/// affichée, pas une vérification.
fn taille_apparente(dossier: &Path) -> u64 {
    WalkDir::new(dossier)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Programmes Windows ouverts dans `<home>/.wine-mao` (même filtre que
/// [`processus::programmes_ouverts`]). L'interface les nomme et demande l'accord avant
/// [`importer`], qui arrête le `wineserver` de cet environnement.
///
/// Erreur : [`Erreur::Io`] si `/proc` est illisible.
pub fn programmes_ouverts(c: &Chemins) -> Resultat<Vec<String>> {
    processus::programmes_ouverts_sur(&chemin_ancien(c))
}

/// Importe `<home>/.wine-mao` : le déplace vers `c.prefixe` et le remplace par un lien vers lui.
///
/// Étapes (spec, section 5.2), verrou d'installation tenu du début à la fin :
/// 1. vérifications, dans cet ordre : environnement présent, `c.prefixe` absent, verrou, aucun
///    plugin en cours sur `~/.wine-mao`, même disque ;
/// 2. arrêt du `wineserver` de `~/.wine-mao` ;
/// 3. petite sauvegarde dans `<donnees>/import/sauvegarde-<identifiant>/` ;
/// 4. trace `import.toml` avec `etat = "en-cours"`, écrite **avant** tout déplacement : si elle ne
///    peut pas l'être, l'import est refusé sans rien avoir touché ;
/// 5. déplacement (`rename`), puis synchronisation des deux dossiers parents ;
/// 6. lien `~/.wine-mao → c.prefixe` (dossier parent synchronisé) ; s'il échoue, le dossier est
///    remis à sa place, la trace et la petite sauvegarde sont retirées ;
/// 7. trace `etat = "fait"`, points de restauration existants périmés
///    ([`restauration::perimer_tous_les_points`] : ils ne décrivent pas l'environnement importé),
///    puis entrée d'historique.
///
/// Une coupure de courant entre 4 et 7 laisse une trace `en-cours` que
/// [`reprendre_interruption`] sait terminer ou abandonner au lancement suivant. Pour la même
/// raison, un échec d'écriture de la trace `fait` (étape 7) est seulement noté au journal :
/// l'environnement est déjà en place et [`reprendre_interruption`] finira de l'inscrire.
///
/// Après un échec antérieur au lien, la petite sauvegarde et la trace sont retirées : il ne
/// reste rien de l'essai (Wine a seulement été arrêté).
///
/// Erreurs : [`Erreur::Introuvable`] (pas d'environnement), [`Erreur::DejaPresent`],
/// [`Erreur::InstallationEnCours`], [`Erreur::PluginsEnCours`], [`Erreur::AutreDisque`],
/// erreurs de [`Wine::arreter`], [`Erreur::Io`].
pub fn importer(c: &Chemins, p: &dyn Progression) -> Resultat<Importe> {
    importer_dans(c, p, &reelles())
}

/// Opérations système qu'[`importer_dans`] et [`annuler_dans`] reçoivent de l'extérieur, pour
/// que les tests simulent les échecs (autre disque, lien ou renommage impossible, `fsync` en
/// échec, plugin en cours) sans vrai second disque ni système de fichiers en panne.
struct Injections<'a> {
    /// Racine de `/proc`, où l'on cherche les plugins en cours.
    proc: &'a Path,
    /// `meme_disque(ancien, destination)` : vrai si les deux sont sur le même système de fichiers.
    meme_disque: &'a dyn Fn(&Path, &Path) -> Resultat<bool>,
    /// `creer_lien(cible, lien)` : crée le lien symbolique `lien` vers `cible`.
    creer_lien: &'a dyn Fn(&Path, &Path) -> std::io::Result<()>,
    /// `renommer(de, vers)` : déplace l'environnement (`std::fs::rename`).
    renommer: &'a dyn Fn(&Path, &Path) -> std::io::Result<()>,
    /// `synchroniser(chemin)` : force sur disque le dossier qui contient `chemin`
    /// ([`synchroniser_parent`]).
    synchroniser: &'a dyn Fn(&Path) -> std::io::Result<()>,
}

/// Les vraies opérations, utilisées par [`importer`] et [`annuler`] (une fonction et non une
/// constante : `Path::new` n'est pas encore utilisable dans une constante).
fn reelles() -> Injections<'static> {
    Injections {
        proc: Path::new(RACINE_PROC),
        meme_disque: &meme_disque,
        creer_lien: &creer_lien_symbolique,
        renommer: &renommer_reellement,
        synchroniser: &synchroniser_parent,
    }
}

/// `std::os::unix::fs::symlink(cible, lien)`, sous forme de fonction (pour [`reelles`]).
fn creer_lien_symbolique(cible: &Path, lien: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(cible, lien)
}

/// `std::fs::rename(de, vers)`, sous forme de fonction (pour [`reelles`]).
fn renommer_reellement(de: &Path, vers: &Path) -> std::io::Result<()> {
    std::fs::rename(de, vers)
}

/// Synchronise le dossier parent de chaque chemin **sans jamais échouer** : une erreur est notée
/// au journal. Règle : une fois qu'un renommage ou un lien a réussi, un `fsync` raté ne doit
/// jamais être pris pour un échec de l'opération (on défairait alors à tort un déplacement
/// réel). Au pire, après une coupure de courant, la trace permet à [`reprendre_interruption`] de
/// terminer le travail.
fn synchroniser_ou_noter(synchroniser: &dyn Fn(&Path) -> std::io::Result<()>, chemins: &[&Path]) {
    for chemin in chemins {
        if let Err(e) = synchroniser(chemin) {
            journal::noter(
                "import",
                &format!("synchronisation du dossier de {} : {e}", chemin.display()),
            );
        }
    }
}

/// Cœur de [`importer`], avec les opérations système injectées ([`Injections`]).
fn importer_dans(c: &Chemins, p: &dyn Progression, op: &Injections) -> Resultat<Importe> {
    let ancien = chemin_ancien(c);
    // 1. Vérifications : rien n'est modifié tant qu'elles ne sont pas toutes passées.
    p.etape(&tr("Checking the existing Wine environment…"));
    if !est_environnement(&ancien) {
        return Err(Erreur::Introuvable(trf(
            "Wine environment in {path} (a folder with system.reg, user.reg and drive_c)",
            &[("path", &ancien.display().to_string())],
        )));
    }
    if prefixe_present(c) {
        return Err(Erreur::DejaPresent(c.prefixe.clone()));
    }
    let _verrou = poser_verrou_installation(c)?;
    if plugins_en_cours_dans(op.proc, std::slice::from_ref(&ancien))? {
        return Err(Erreur::PluginsEnCours);
    }
    // Le dossier qui recevra l'environnement : `rename` exige le même système de fichiers.
    let destination = c.prefixe.parent().unwrap_or(&c.donnees).to_path_buf();
    if !(op.meme_disque)(&ancien, &destination)? {
        return Err(Erreur::AutreDisque {
            source: ancien,
            destination,
        });
    }
    // 2. Plus aucun programme Windows ne doit écrire pendant le déplacement.
    p.etape(&tr("Stopping Wine…"));
    Wine::nouveau(&chemins_sur(c, &ancien)).arreter()?;
    // 3. Petite sauvegarde.
    p.etape(&tr("Saving the registry and the yabridgectl settings…"));
    let sauvegarde = petite_sauvegarde(c, &ancien)?;
    // 4 et 5. Trace « en cours », puis déplacement. `deplacer` n'échoue que si le renommage
    // n'a PAS eu lieu : on peut alors effacer ce que l'essai a écrit (trace, petite sauvegarde).
    p.etape(&tr("Moving the Wine environment…"));
    let mut trace = Trace {
        ancien: ancien.clone(),
        date: historique::date_locale_iso(),
        sauvegarde: sauvegarde.clone(),
        etat: EtatImport::EnCours,
    };
    if let Err(e) = deplacer(c, &trace, &destination, op) {
        effacer_essai(c, &sauvegarde);
        return Err(e);
    }
    // 6. Lien à l'ancienne place ; en cas d'échec, tout est remis comme avant.
    p.etape(&tr("Creating the link to the new location…"));
    if let Err(e) = (op.creer_lien)(&c.prefixe, &ancien) {
        return Err(remettre_en_place(c, &ancien, &sauvegarde, e, op));
    }
    // Le lien existe : un `fsync` raté ne justifie pas de tout défaire.
    synchroniser_ou_noter(op.synchroniser, &[&ancien]);
    // 7. L'import est réussi : la suite ne doit plus le faire échouer.
    trace.etat = EtatImport::Fait;
    if let Err(e) = ecrire_trace(c, &trace) {
        journal::noter(
            "import",
            &format!(
                "trace {} non passée à « fait » (reprise au prochain lancement) : {e}",
                chemin_trace(c).display()
            ),
        );
    }
    // `c.prefixe` désigne maintenant l'environnement importé : un point resté d'un
    // environnement précédent de WiMao (supprimé depuis) ne le décrit pas.
    restauration::perimer_tous_les_points(c, "import");
    historique::ajouter(
        c,
        Action::Import,
        SUJET,
        ResultatAction::Reussi,
        None,
        &c.prefixe.display().to_string(),
    );
    Ok(Importe { ancien, sauvegarde })
}

/// Étapes 4 et 5 de [`importer`] : trace `en-cours` (atomique et synchronisée), création du
/// dossier `destination`, `rename(ancien, c.prefixe)`, puis synchronisation des dossiers parents
/// pour que le renommage survive à une coupure de courant **après** la trace.
///
/// Rend une erreur **seulement si le renommage n'a pas eu lieu** (trace, dossier ou renommage
/// impossibles) : l'appelant efface alors l'essai sans risque. Une fois le renommage fait, un
/// `fsync` raté est seulement noté au journal ([`synchroniser_ou_noter`]).
fn deplacer(c: &Chemins, trace: &Trace, destination: &Path, op: &Injections) -> Resultat<()> {
    ecrire_trace(c, trace)?;
    std::fs::create_dir_all(destination)?;
    if let Err(e) = (op.renommer)(&trace.ancien, &c.prefixe) {
        // Filet : le test « même disque » a pu se tromper (point de montage entre les deux).
        if e.raw_os_error() == Some(libc::EXDEV) {
            return Err(Erreur::AutreDisque {
                source: trace.ancien.clone(),
                destination: destination.to_path_buf(),
            });
        }
        return Err(e.into());
    }
    synchroniser_ou_noter(op.synchroniser, &[&trace.ancien, &c.prefixe]);
    Ok(())
}

/// Efface ce qu'un import abandonné **avant le déplacement** a écrit : la trace et la petite
/// sauvegarde. Rien d'autre n'existait. Un échec est seulement noté au journal (la trace restée
/// `en-cours` serait abandonnée par [`reprendre_interruption`]).
fn effacer_essai(c: &Chemins, sauvegarde: &Path) {
    for resultat in [
        supprimer_si_present(&chemin_trace(c), false),
        supprimer_si_present(sauvegarde, true),
    ] {
        if let Err(e) = resultat {
            journal::noter("import", &format!("nettoyage après échec : {e}"));
        }
    }
}

/// Supprime `chemin` (dossier entier si `dossier`) ; absent n'est pas une erreur.
fn supprimer_si_present(chemin: &Path, dossier: bool) -> std::io::Result<()> {
    let resultat = if dossier {
        std::fs::remove_dir_all(chemin)
    } else {
        std::fs::remove_file(chemin)
    };
    match resultat {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Force l'écriture sur disque du dossier qui contient `chemin` (après un `rename`, un lien ou une
/// suppression) : sans cela, après une coupure de courant, le disque pourrait montrer les
/// opérations dans un autre ordre que celui de la trace.
fn synchroniser_parent(chemin: &Path) -> std::io::Result<()> {
    match chemin.parent() {
        Some(parent) => std::fs::File::open(parent)?.sync_all(),
        None => Ok(()),
    }
}

/// Après l'échec de la création du lien (`erreur`) : remet l'environnement à son ancienne place
/// et rend l'erreur à remonter ; une entrée d'historique `Echec` est écrite dans tous les cas.
///
/// - Retour réussi : la trace et la petite sauvegarde sont effacées (il ne reste rien de l'essai).
///   Un `fsync` raté après ce retour est seulement noté au journal : le retour a bien eu lieu.
/// - Retour impossible (très improbable : on vient de faire le renommage inverse sur le même
///   disque) : la trace reste `en-cours`, ce qui permet à [`reprendre_interruption`] de recréer
///   le lien plus tard ; le message dit où se trouve maintenant l'environnement, pour que
///   l'utilisateur ne le croie pas perdu.
fn remettre_en_place(
    c: &Chemins,
    ancien: &Path,
    sauvegarde: &Path,
    erreur: std::io::Error,
    op: &Injections,
) -> Erreur {
    let resultat = match (op.renommer)(&c.prefixe, ancien) {
        Ok(()) => {
            synchroniser_ou_noter(op.synchroniser, &[ancien, &c.prefixe]);
            effacer_essai(c, sauvegarde);
            Erreur::Io(erreur)
        }
        Err(retour) => {
            journal::noter(
                "import",
                &format!(
                    "remise en place de {} impossible : {retour}",
                    ancien.display()
                ),
            );
            Erreur::Io(std::io::Error::new(
                erreur.kind(),
                trf(
                    "the link {link} could not be created ({error}) and the Wine environment could not be moved back: it is now in {path}",
                    &[
                        ("link", &ancien.display().to_string()),
                        ("error", &erreur.to_string()),
                        ("path", &c.prefixe.display().to_string()),
                    ],
                ),
            ))
        }
    };
    historique::ajouter(
        c,
        Action::Import,
        SUJET,
        ResultatAction::Echec,
        None,
        &resultat.to_string(),
    );
    resultat
}

/// Vrai si `ancien` et `destination` sont sur le même système de fichiers (`st_dev`).
/// `destination` peut ne pas exister encore (premier lancement) : on regarde alors son plus
/// proche ancêtre existant, qui est l'endroit où ses dossiers seront créés.
fn meme_disque(ancien: &Path, destination: &Path) -> Resultat<bool> {
    let existant = destination
        .ancestors()
        .find(|a| a.exists())
        .unwrap_or(Path::new("/"));
    Ok(ancien.symlink_metadata()?.dev() == std::fs::metadata(existant)?.dev())
}

/// Copie les `.reg` de `ancien` et, s'il existe, `~/.config/yabridgectl/config.toml` dans un
/// nouveau dossier `<donnees>/import/sauvegarde-<identifiant>/` ; rend ce dossier. Chaque copie
/// est atomique ([`ecrire_atomique`]). La configuration de l'utilisateur est seulement lue. En
/// cas d'échec, le dossier à moitié rempli est supprimé.
fn petite_sauvegarde(c: &Chemins, ancien: &Path) -> Resultat<PathBuf> {
    let racine = dossier_import(c);
    std::fs::create_dir_all(&racine)?;
    let dossier = reserver_dossier(
        &racine,
        &format!("{DEBUT_SAUVEGARDE}{}", identifiant_local()?),
    )?;
    let remplir = || -> Resultat<()> {
        for nom in FICHIERS_REG {
            let source = ancien.join(nom);
            // `userdef.reg` peut manquer ; les deux autres existent (vu par `est_environnement`).
            if source.is_file() {
                ecrire_atomique(&dossier.join(nom), &std::fs::read(&source)?)?;
            }
        }
        let config = c.home.join(CONFIG_YABRIDGECTL);
        if config.is_file() {
            ecrire_atomique(&dossier.join(NOM_CONFIG), &std::fs::read(&config)?)?;
        }
        Ok(())
    };
    if let Err(e) = remplir() {
        let _ = std::fs::remove_dir_all(&dossier);
        return Err(e);
    }
    Ok(dossier)
}

/// Crée `<racine>/<base>` (ou `<base>-2`, `<base>-3`… si le nom est pris) et le rend.
/// `create_dir` échoue si le dossier existe : une sauvegarde n'en écrase jamais une autre.
fn reserver_dossier(racine: &Path, base: &str) -> Resultat<PathBuf> {
    for numero in 1..=ESSAIS_MAXIMAUX {
        let dossier = racine.join(nom_numerote(base, numero, ""));
        match std::fs::create_dir(&dossier) {
            Ok(()) => return Ok(dossier),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(aucun_nom_libre(racine, base))
}

/// Premier chemin libre parmi `<racine>/<base><fin>`, `<racine>/<base>-2<fin>`…
fn nom_libre(racine: &Path, base: &str, fin: &str) -> Resultat<PathBuf> {
    (1..=ESSAIS_MAXIMAUX)
        .map(|numero| racine.join(nom_numerote(base, numero, fin)))
        .find(|chemin| chemin.symlink_metadata().is_err())
        .ok_or_else(|| aucun_nom_libre(racine, base))
}

/// `<base><fin>` pour le numéro 1, `<base>-<numéro><fin>` ensuite.
fn nom_numerote(base: &str, numero: u32, fin: &str) -> String {
    if numero == 1 {
        format!("{base}{fin}")
    } else {
        format!("{base}-{numero}{fin}")
    }
}

/// Erreur quand tous les suffixes sont pris (n'arrive pas en pratique).
fn aucun_nom_libre(racine: &Path, base: &str) -> Erreur {
    Erreur::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        trf(
            "no free name found for {name} in {path}",
            &[("name", base), ("path", &racine.display().to_string())],
        ),
    ))
}

/// Écrit la trace de l'import, de façon atomique : fichier temporaire, `sync_all`, renommage et
/// synchronisation du dossier ([`ecrire_atomique`]). Le dossier `import/` est créé au besoin.
fn ecrire_trace(c: &Chemins, trace: &Trace) -> Resultat<()> {
    std::fs::create_dir_all(dossier_import(c))?;
    let texte = toml::to_string(trace).map_err(|e| Erreur::Io(std::io::Error::other(e)))?;
    ecrire_atomique(&chemin_trace(c), texte.as_bytes())
}

/// Lit la trace de l'import. `None` si elle n'existe pas ; illisible ou invalide, elle est aussi
/// ignorée (`None`), et c'est noté au journal.
fn lire_trace(c: &Chemins) -> Option<Trace> {
    let chemin = chemin_trace(c);
    let texte = match std::fs::read_to_string(&chemin) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            journal::noter("import", &format!("{} illisible : {e}", chemin.display()));
            return None;
        }
    };
    match toml::from_str(&texte) {
        Ok(trace) => Some(trace),
        Err(e) => {
            journal::noter("import", &format!("{} invalide : {e}", chemin.display()));
            None
        }
    }
}

/// Trace d'un import **terminé** (`etat = "fait"`) ; `None` sinon (aucune trace, ou opération
/// interrompue, qui relève de [`reprendre_interruption`]).
fn trace_faite(c: &Chemins) -> Option<Trace> {
    lire_trace(c).filter(|t| t.etat == EtatImport::Fait)
}

/// Ancien emplacement de l'environnement importé (en pratique `<home>/.wine-mao`), lu dans
/// `import.toml` ; `None` s'il n'y a pas d'import terminé en vigueur (jamais fait, annulé, ou
/// interrompu).
pub fn ancien_emplacement(c: &Chemins) -> Option<PathBuf> {
    trace_faite(c).map(|t| t.ancien)
}

/// Vrai si `ancien` est un lien symbolique dont la cible est exactement `prefixe`, c'est-à-dire
/// le lien créé par l'import (créé avec ce chemin absolu, d'où la comparaison exacte).
fn est_le_lien_de_l_import(ancien: &Path, prefixe: &Path) -> bool {
    std::fs::read_link(ancien).is_ok_and(|cible| cible == prefixe)
}

/// Vrai si `chemin` est un vrai dossier (pas un lien).
fn est_un_vrai_dossier(chemin: &Path) -> bool {
    chemin.symlink_metadata().is_ok_and(|m| m.is_dir())
}

/// Ancien emplacement, seulement si l'import est terminé (`etat = "fait"`) et que l'ancien
/// emplacement est **encore** le lien de l'import vers `c.prefixe`. Sert à
/// [`annulation_possible`] et à [`noms_du_prefixe`] (une seule lecture de la trace).
pub(crate) fn ancien_lien_actif(c: &Chemins) -> Option<PathBuf> {
    ancien_emplacement(c).filter(|ancien| est_le_lien_de_l_import(ancien, &c.prefixe))
}

/// Vrai si [`annuler`] peut être proposé : import terminé (`etat = "fait"`) et ancien
/// emplacement toujours lien de l'import. Ne vérifie pas les plugins en cours (cela change d'un
/// instant à l'autre : [`annuler`] le vérifie au moment d'agir).
pub fn annulation_possible(c: &Chemins) -> bool {
    ancien_lien_actif(c).is_some()
}

/// Les noms sous lesquels l'environnement de WiMao est connu : `c.prefixe`, plus, après un
/// import terminé dont le lien est toujours en place (`ancien_lien_actif`), l'ancien chemin
/// `~/.wine-mao`.
///
/// Pourquoi deux noms : un plugin ponté avant l'import, ou un programme lancé par un ancien
/// raccourci du bureau, tourne avec `WINEPREFIX=~/.wine-mao`. C'est pourtant **le même**
/// environnement (et le même `wineserver`) : un arrêt de Wine sur `c.prefixe` le tuerait. Seul
/// endroit du moteur qui applique cette règle ; utilisé par [`crate::processus`] (processus et
/// programmes ouverts), [`crate::restauration`] (plugins en cours, `wineserver` actif) et
/// `pontage::racines_du_prefixe`.
///
/// Après une annulation (ou si l'utilisateur a remplacé le lien), `~/.wine-mao` redevient
/// l'environnement de l'utilisateur : il n'est plus rendu, ses programmes ne sont plus à nous.
pub fn noms_du_prefixe(c: &Chemins) -> Vec<PathBuf> {
    let mut noms = vec![c.prefixe.clone()];
    noms.extend(ancien_lien_actif(c));
    noms
}

/// Annule l'import : l'environnement retourne à `~/.wine-mao` (spec, section 5.5).
///
/// 1. Vérifications, dans cet ordre : import terminé (`etat = "fait"`), son ancien chemin est
///    bien `<home>/.wine-mao` et `c.prefixe` un vrai dossier, `~/.wine-mao` est le lien de
///    l'import ([`Erreur::PasNotreLien`] sinon), aucun plugin en cours (sous les deux noms),
///    verrou, arrêt du `wineserver` ;
/// 2. trace `etat = "annulation-en-cours"`, retrait du lien, `rename(c.prefixe, ~/.wine-mao)`
///    (dossiers parents synchronisés) ; si ce renommage échoue, le lien est recréé et la trace
///    revient à `fait`. Enfin `import.toml` devient `import.annule-<identifiant>.toml` ;
/// 3. points de restauration périmés ([`restauration::perimer_tous_les_points`] : ils décrivent
///    l'environnement rendu à `~/.wine-mao`, pas celui qui sera créé ensuite) ;
/// 4. `yabridgectl sync` sur une copie de la configuration de l'utilisateur, pour que ses liens
///    retrouvent `~/.wine-mao` ; sans cette configuration, ou si `sync` échoue, un avertissement
///    passé à `p.etape` donne la commande à lancer (l'annulation reste réussie) ;
/// 5. entrée d'historique `AnnulationImport`.
///
/// Une coupure de courant pendant l'étape 2 est reprise par [`reprendre_interruption`].
///
/// Erreurs : [`Erreur::Introuvable`] (aucun import terminé, ou environnement absent),
/// [`Erreur::PasNotreLien`], [`Erreur::PluginsEnCours`], [`Erreur::InstallationEnCours`],
/// erreurs de [`Wine::arreter`], [`Erreur::Io`].
pub fn annuler(c: &Chemins, p: &dyn Progression) -> Resultat<()> {
    annuler_dans(c, p, &reelles())
}

/// Cœur de [`annuler`], avec les opérations système injectées ([`Injections`] : renommage et
/// `fsync` ; `proc`, `meme_disque` et `creer_lien` n'y servent pas).
fn annuler_dans(c: &Chemins, p: &dyn Progression, op: &Injections) -> Resultat<()> {
    // 1. Vérifications.
    p.etape(&tr("Checking the import…"));
    let Some(mut trace) = trace_faite(c) else {
        return Err(Erreur::Introuvable(trf(
            "import record {path} (there is no import to cancel)",
            &[("path", &chemin_trace(c).display().to_string())],
        )));
    };
    let ancien = trace.ancien.clone();
    // Une trace modifiée à la main ne doit pas faire déplacer l'environnement n'importe où.
    if ancien != chemin_ancien(c) {
        return Err(Erreur::PasNotreLien(ancien));
    }
    if !est_un_vrai_dossier(&c.prefixe) {
        return Err(Erreur::Introuvable(trf(
            "Wine environment in {path} (a folder with system.reg, user.reg and drive_c)",
            &[("path", &c.prefixe.display().to_string())],
        )));
    }
    if !est_le_lien_de_l_import(&ancien, &c.prefixe) {
        return Err(Erreur::PasNotreLien(ancien));
    }
    // Un plugin ponté avant l'import tourne avec `WINEPREFIX=~/.wine-mao`, un plugin ponté
    // par WiMao avec `WINEPREFIX=<donnees>/prefixe` : `plugins_en_cours` cherche sous les deux
    // noms ([`noms_du_prefixe`] ; le lien de l'import vient d'être vérifié ci-dessus).
    if plugins_en_cours(c)? {
        return Err(Erreur::PluginsEnCours);
    }
    let _verrou = poser_verrou_installation(c)?;
    // Calculé avant toute modification : une erreur ici ne laisse rien à moitié fait.
    let identifiant = identifiant_local()?;
    p.etape(&tr("Stopping Wine…"));
    // Un seul `wineserver` pour les deux noms : Wine l'identifie par le dossier réel.
    Wine::nouveau(c).arreter()?;
    // 2. Retour à l'ancienne place, annoncé d'abord dans la trace.
    p.etape(&tr("Moving the Wine environment back…"));
    trace.etat = EtatImport::AnnulationEnCours;
    ecrire_trace(c, &trace)?;
    // `remove_file` sur un lien retire le lien seul (jamais suivi). Seuls le retrait et le
    // renommage peuvent faire échouer l'annulation ; un `fsync` raté est seulement noté.
    let deplacement = std::fs::remove_file(&ancien).and_then(|()| {
        synchroniser_ou_noter(op.synchroniser, &[&ancien]);
        (op.renommer)(&c.prefixe, &ancien)
    });
    if let Err(e) = deplacement {
        annulation_ratee(c, &mut trace);
        return Err(e.into());
    }
    synchroniser_ou_noter(op.synchroniser, &[&ancien, &c.prefixe]);
    // Rien n'est supprimé : la trace est seulement renommée. Un échec est noté sans faire
    // échouer l'annulation, déjà faite ; la trace restée `annulation-en-cours` sera archivée par
    // `reprendre_interruption`.
    if let Err(e) = archiver_trace(c, DEBUT_ANNULE, &identifiant) {
        journal::noter("import", &format!("trace non renommée : {e}"));
    }
    // 3. Les points de restauration décrivent l'environnement qui vient de retourner à
    // `~/.wine-mao` : un environnement créé ensuite dans `c.prefixe` n'a rien à voir avec eux.
    // Périmés dès maintenant, même si l'utilisateur réimporte plus tard (entre-temps,
    // `~/.wine-mao` a pu changer hors de WiMao). Un échec est seulement noté au journal.
    restauration::perimer_tous_les_points(c, "import annulé");
    // 4. Liens yabridge de l'utilisateur.
    let avertissement = synchroniser_liens_utilisateur(c, &ancien, &identifiant, p);
    // 5. Historique.
    match &avertissement {
        Some(neutre) => historique::ajouter_details_neutres(
            c,
            Action::AnnulationImport,
            SUJET,
            ResultatAction::Reussi,
            None,
            neutre,
        ),
        None => historique::ajouter(
            c,
            Action::AnnulationImport,
            SUJET,
            ResultatAction::Reussi,
            None,
            "",
        ),
    }
    Ok(())
}

/// Après l'échec du retrait du lien ou du renommage de l'annulation : si l'environnement est
/// toujours dans `c.prefixe`, recrée le lien s'il a été retiré et remet la trace à `fait`
/// (situation d'avant l'appel). Un échec ici est noté au journal ; la trace
/// `annulation-en-cours` laissée en place sera reprise par [`reprendre_interruption`].
fn annulation_ratee(c: &Chemins, trace: &mut Trace) {
    let ancien = trace.ancien.clone();
    let forme = forme_ancien(&ancien, &c.prefixe);
    let attendu = matches!(forme, FormeAncien::Absent | FormeAncien::NotreLien);
    if !est_un_vrai_dossier(&c.prefixe) || !attendu {
        journal::noter(
            "import",
            &format!(
                "annulation interrompue : {} ou {} dans un état inattendu",
                c.prefixe.display(),
                ancien.display()
            ),
        );
        return;
    }
    let lien = if forme == FormeAncien::Absent {
        let lien = std::os::unix::fs::symlink(&c.prefixe, &ancien);
        if lien.is_ok() {
            synchroniser_ou_noter(&synchroniser_parent, &[&ancien]);
        }
        lien
    } else {
        Ok(()) // le retrait du lien a échoué : il est toujours là
    };
    let remise = lien.map_err(Erreur::from).and_then(|()| {
        trace.etat = EtatImport::Fait;
        ecrire_trace(c, trace)
    });
    if let Err(e) = remise {
        journal::noter(
            "import",
            &format!("remise du lien {} : {e}", ancien.display()),
        );
    }
}

/// Renomme `import.toml` en `<debut><identifiant>.toml` (suffixe `-2`… si pris) et synchronise
/// le dossier. `debut` vaut `import.annule-` ou `import.abandonne-`.
fn archiver_trace(c: &Chemins, debut: &str, identifiant: &str) -> Resultat<()> {
    let dossier = dossier_import(c);
    let archive = nom_libre(&dossier, &format!("{debut}{identifiant}"), ".toml")?;
    std::fs::rename(chemin_trace(c), &archive)?;
    synchroniser_ou_noter(&synchroniser_parent, &[&archive]);
    Ok(())
}

/// Forme de l'ancien emplacement, pour [`reprendre_interruption`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormeAncien {
    /// Rien à ce chemin.
    Absent,
    /// Le lien de l'import vers `c.prefixe`.
    NotreLien,
    /// Un vrai dossier.
    Dossier,
    /// Autre chose (fichier, autre lien…).
    Autre,
}

/// Forme de `ancien` (sans suivre de lien).
fn forme_ancien(ancien: &Path, prefixe: &Path) -> FormeAncien {
    match ancien.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FormeAncien::Absent,
        Err(_) => FormeAncien::Autre,
        Ok(m) if m.is_dir() => FormeAncien::Dossier,
        Ok(_) if est_le_lien_de_l_import(ancien, prefixe) => FormeAncien::NotreLien,
        Ok(_) => FormeAncien::Autre,
    }
}

/// Termine ou abandonne un import, ou une annulation, interrompu (coupure de courant, arrêt
/// brutal) ; rend un message traduit qui dit ce qui a été réparé, ou `None` s'il n'y avait rien
/// à faire.
///
/// **À appeler au démarrage** : la CLI le fait au début de chaque sous-commande (sauf
/// `relayer-lien` et `verifier --prefixe`, qui ne doivent rien écrire) et l'interface le fera à
/// son lancement. Ne fait rien (`None`) si une opération tient le verrou d'installation : un
/// import en cours dans un autre processus a une trace `en-cours` tout à fait normale.
///
/// Cas traités, selon `etat` dans `import.toml`, l'ancien chemin et `c.prefixe` :
///
/// | `etat` | ancien chemin | `c.prefixe` | réparation |
/// |---|---|---|---|
/// | `en-cours` | absent ou lien de l'import | vrai dossier | lien (re)créé, `etat = "fait"`, historique `Import` |
/// | `en-cours` | vrai dossier | absent | rien n'a bougé : trace renommée `import.abandonne-<id>.toml` |
/// | `annulation-en-cours` | absent | vrai dossier | renommage inverse terminé, trace `import.annule-<id>.toml`, historique `AnnulationImport` |
/// | `annulation-en-cours` | vrai dossier | absent | trace renommée `import.annule-<id>.toml`, historique `AnnulationImport` |
/// | `annulation-en-cours` | lien de l'import | vrai dossier | rien n'a bougé : `etat = "fait"` |
///
/// Chaque import ou annulation terminé ici périme aussi les points de restauration
/// ([`restauration::perimer_tous_les_points`]), comme sans interruption.
///
/// Tout autre cas est ambigu : rien n'est modifié et l'erreur [`Erreur::ImportInterrompu`]
/// nomme les deux chemins, à vérifier à la main. Une trace sans `etat` (version précédente) vaut
/// `fait` : rien à faire.
///
/// Les deux reprises d'annulation rappellent de lancer `yabridgectl sync` : il n'est pas relancé
/// ici (la reprise doit rester rapide et sans question au démarrage).
///
/// Erreurs : [`Erreur::ImportInterrompu`], [`Erreur::Io`].
pub fn reprendre_interruption(c: &Chemins) -> Resultat<Option<String>> {
    let Some(mut trace) = lire_trace(c) else {
        return Ok(None);
    };
    if trace.etat == EtatImport::Fait {
        return Ok(None);
    }
    let _verrou = match poser_verrou_installation(c) {
        Ok(v) => v,
        Err(Erreur::InstallationEnCours) => return Ok(None),
        Err(e) => return Err(e),
    };
    let ancien = trace.ancien.clone();
    let ambigu = || Erreur::ImportInterrompu {
        ancien: ancien.clone(),
        prefixe: c.prefixe.clone(),
    };
    if ancien != chemin_ancien(c) {
        return Err(ambigu());
    }
    let forme = forme_ancien(&ancien, &c.prefixe);
    let prefixe_dossier = est_un_vrai_dossier(&c.prefixe);
    let prefixe_absent = c.prefixe.symlink_metadata().is_err();
    let identifiant = identifiant_local()?;
    let chemin = ancien.display().to_string();
    match (trace.etat, forme) {
        (EtatImport::EnCours, FormeAncien::Absent | FormeAncien::NotreLien) if prefixe_dossier => {
            if forme == FormeAncien::Absent {
                std::os::unix::fs::symlink(&c.prefixe, &ancien)?;
                synchroniser_ou_noter(&synchroniser_parent, &[&ancien]);
            }
            trace.etat = EtatImport::Fait;
            ecrire_trace(c, &trace)?;
            // Comme à la fin d'un import non interrompu (étape 7 d'`importer`).
            restauration::perimer_tous_les_points(c, "import repris");
            // Même texte neutre pour l'historique (traduit à l'affichage) et pour le message
            // rendu à l'appelant (traduit tout de suite).
            let neutre = TexteNeutre::nouveau(historique::CLE_IMPORT_REPRIS, &[("path", &chemin)]);
            historique::ajouter_details_neutres(
                c,
                Action::Import,
                SUJET,
                ResultatAction::Reussi,
                None,
                &neutre,
            );
            Ok(Some(neutre.en_clair()))
        }
        (EtatImport::EnCours, FormeAncien::Dossier) if prefixe_absent => {
            archiver_trace(c, DEBUT_ABANDONNE, &identifiant)?;
            Ok(Some(trf(
                "The import of {path} had been interrupted before anything was moved: it has been abandoned",
                &[("path", &chemin)],
            )))
        }
        (EtatImport::AnnulationEnCours, FormeAncien::Absent) if prefixe_dossier => {
            std::fs::rename(&c.prefixe, &ancien)?;
            synchroniser_ou_noter(&synchroniser_parent, &[&ancien, &c.prefixe]);
            terminer_annulation_reprise(c, &chemin, &identifiant)
        }
        (EtatImport::AnnulationEnCours, FormeAncien::Dossier) if prefixe_absent => {
            // Le renommage avait eu lieu : il ne reste qu'à archiver la trace et à le noter.
            terminer_annulation_reprise(c, &chemin, &identifiant)
        }
        (EtatImport::AnnulationEnCours, FormeAncien::NotreLien) if prefixe_dossier => {
            trace.etat = EtatImport::Fait;
            ecrire_trace(c, &trace)?;
            Ok(Some(trf(
                "Cancelling the import of {path} had been interrupted before anything was moved: the import is still in place",
                &[("path", &chemin)],
            )))
        }
        _ => Err(ambigu()),
    }
}

/// Relance `yabridgectl sync` pour la configuration de l'utilisateur (étape 4 de [`annuler`]).
/// Rend l'avertissement (texte neutre, déjà affiché traduit) affiché si WiMao n'a pas pu le faire (déjà passé à `p.etape`), `None`
/// si tout s'est bien passé.
fn synchroniser_liens_utilisateur(
    c: &Chemins,
    ancien: &Path,
    identifiant: &str,
    p: &dyn Progression,
) -> Option<TexteNeutre> {
    let config = c.home.join(CONFIG_YABRIDGECTL);
    if !config.is_file() {
        let neutre = TexteNeutre::nouveau(
            historique::CLE_SANS_REGLAGES_YABRIDGECTL,
            &[
                ("path", &config.display().to_string()),
                ("command", COMMANDE_SYNC),
            ],
        );
        p.etape(&neutre.en_clair());
        return Some(neutre);
    }
    p.etape(&tr("Updating your plugin links (yabridgectl sync)…"));
    match sync_sur_une_copie(c, &config, ancien, identifiant) {
        Ok(()) => None,
        Err(e) => {
            journal::noter(
                "import",
                &format!("yabridgectl sync de l'utilisateur : {e}"),
            );
            let neutre = TexteNeutre::nouveau(
                historique::CLE_LIENS_NON_MIS_A_JOUR,
                &[("error", &e.to_string()), ("command", COMMANDE_SYNC)],
            );
            p.etape(&neutre.en_clair());
            Some(neutre)
        }
    }
}

/// Copie `config` dans `<donnees>/import/sync-utilisateur-<identifiant>/yabridgectl/` et y lance
/// `yabridgectl sync` (voir `pontage::commande_sync_utilisateur`), puis supprime la copie.
/// L'original n'est jamais ouvert en écriture.
fn sync_sur_une_copie(
    c: &Chemins,
    config: &Path,
    ancien: &Path,
    identifiant: &str,
) -> Resultat<()> {
    let racine = dossier_import(c);
    std::fs::create_dir_all(&racine)?;
    let copie = reserver_dossier(&racine, &format!("{DEBUT_COPIE_SYNC}{identifiant}"))?;
    let resultat = (|| {
        let sous_dossier = copie.join(SOUS_DOSSIER_YABRIDGECTL);
        std::fs::create_dir_all(&sous_dossier)?;
        std::fs::copy(config, sous_dossier.join(NOM_CONFIG))?;
        pontage::lancer_sync(&mut pontage::commande_sync_utilisateur(c, &copie, ancien)?)
    })();
    // La copie n'a plus d'usage, quelle que soit l'issue.
    let _ = std::fs::remove_dir_all(&copie);
    resultat
}

/// Fin commune des deux reprises d'une annulation dont l'environnement est revenu à
/// `chemin` (`~/.wine-mao`) : trace archivée en `import.annule-<id>.toml`, entrée d'historique
/// `AnnulationImport` et message qui rappelle de lancer `yabridgectl sync` (non relancé ici).
fn terminer_annulation_reprise(
    c: &Chemins,
    chemin: &str,
    identifiant: &str,
) -> Resultat<Option<String>> {
    archiver_trace(c, DEBUT_ANNULE, identifiant)?;
    // Comme à la fin d'une annulation non interrompue (voir `annuler`).
    restauration::perimer_tous_les_points(c, "annulation d'import reprise");
    let neutre = TexteNeutre::nouveau(
        historique::CLE_ANNULATION_REPRISE,
        &[("path", chemin), ("command", COMMANDE_SYNC)],
    );
    historique::ajouter_details_neutres(
        c,
        Action::AnnulationImport,
        SUJET,
        ResultatAction::Reussi,
        None,
        &neutre,
    );
    Ok(Some(neutre.en_clair()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erreur::Erreur;
    use crate::historique::{self, Action, ResultatAction};
    use crate::progression::ProgressionMuette;
    use std::sync::Mutex;

    /// Les trois fichiers de registre d'un environnement Wine.
    const REG: [&str; 3] = ["system.reg", "user.reg", "userdef.reg"];
    /// Contenu du faux `config.toml` de yabridgectl de l'utilisateur.
    const CONFIG_UTILISATEUR: &str = "plugin_dirs = ['/x']\n";

    /// Banc d'essai, entièrement sous `racine` (jamais le vrai `~/.wine-mao`) :
    /// - `Chemins::pour_test` (HOME de test `<racine>/home`) ;
    /// - faux `wineserver` (réussit sans rien faire) : `Wine::arreter` ne lance pas Wine ;
    /// - faux `yabridgectl` qui note `HOME`, `XDG_CONFIG_HOME`, `WINEPREFIX` et ses arguments
    ///   dans `<racine>/yabridgectl.trace`, puis **essaie d'écrire** dans le `config.toml` qu'il
    ///   lit (comme le vrai, qui y garde `last_known_config`) ;
    /// - faux `<home>/.wine-mao` : trois `.reg`, un plugin VST3 `A.vst3` ;
    /// - faux `<home>/.config/yabridgectl/config.toml`.
    fn banc(racine: &Path) -> Chemins {
        let mut c = Chemins::pour_test(racine);
        c.wine_dir = racine.join("wine");
        crate::test_utils::ecrire_executable(
            &c.wine_bin().join("wineserver"),
            "#!/bin/sh\nexit 0\n",
        );
        c.yabridge_dir = racine.join("yabridge");
        let trace = racine.join("yabridgectl.trace");
        crate::test_utils::ecrire_executable(
            &c.yabridge_dir.join("yabridgectl"),
            &format!(
                "#!/bin/sh\necho \"$HOME|$XDG_CONFIG_HOME|$WINEPREFIX|$*\" >> '{}'\n\
                 echo modifie >> \"$XDG_CONFIG_HOME/yabridgectl/config.toml\"\nexit 0\n",
                trace.display()
            ),
        );
        let ancien = c.home.join(".wine-mao");
        std::fs::create_dir_all(ancien.join("drive_c/Program Files/Common Files/VST3/A.vst3"))
            .unwrap();
        for nom in REG {
            std::fs::write(ancien.join(nom), format!("contenu {nom}")).unwrap();
        }
        let config = c.home.join(".config/yabridgectl");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("config.toml"), CONFIG_UTILISATEUR).unwrap();
        c
    }

    fn ancien(c: &Chemins) -> PathBuf {
        c.home.join(".wine-mao")
    }

    fn config_utilisateur(c: &Chemins) -> PathBuf {
        c.home.join(".config/yabridgectl/config.toml")
    }

    /// Vrai dossier (pas un lien).
    fn est_un_vrai_dossier(p: &Path) -> bool {
        p.symlink_metadata().is_ok_and(|m| m.is_dir())
    }

    /// Faux `/proc` vide (aucun processus) sous `racine`.
    fn proc_vide(racine: &Path) -> PathBuf {
        let proc = racine.join("proc");
        std::fs::create_dir_all(&proc).unwrap();
        proc
    }

    /// Noms des entrées de `dossier`, triés (liste vide s'il n'existe pas).
    fn noms_dans(dossier: &Path) -> Vec<String> {
        let mut noms: Vec<String> = match std::fs::read_dir(dossier) {
            Ok(entrees) => entrees
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => Vec::new(),
        };
        noms.sort();
        noms
    }

    /// Simule une interruption : l'environnement est à `c.prefixe` si `deplace`, sinon à
    /// `~/.wine-mao` ; `lien` crée le lien de l'import ; la trace porte `etat`.
    fn interruption(c: &Chemins, deplace: bool, lien: bool, etat: EtatImport) {
        if deplace {
            std::fs::create_dir_all(c.prefixe.parent().unwrap()).unwrap();
            std::fs::rename(ancien(c), &c.prefixe).unwrap();
        }
        if lien {
            std::os::unix::fs::symlink(&c.prefixe, ancien(c)).unwrap();
        }
        let trace = Trace {
            ancien: ancien(c),
            date: "2026-11-01T10:00:00+01:00".into(),
            sauvegarde: c.donnees.join("import/sauvegarde-x"),
            etat,
        };
        ecrire_trace(c, &trace).unwrap();
    }

    /// Traces archivées dont le nom commence par `debut`.
    fn archives(c: &Chemins, debut: &str) -> usize {
        noms_dans(&c.donnees.join(DOSSIER_IMPORT))
            .iter()
            .filter(|n| n.starts_with(debut) && n.ends_with(".toml"))
            .count()
    }

    /// Progression qui garde les messages, pour vérifier les avertissements.
    #[derive(Default)]
    struct Notes(Mutex<Vec<String>>);
    impl Progression for Notes {
        fn etape(&self, message: &str) {
            self.0.lock().unwrap().push(message.to_string());
        }
    }

    #[test]
    fn detecte_un_vrai_dossier() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let trouve = detecter(&c).unwrap();
        assert_eq!(trouve.chemin, ancien(&c));
        assert_eq!(trouve.plugins, 1);
        assert!(trouve.taille > 0);
    }

    #[test]
    fn ignore_un_lien_ou_un_dossier_incomplet() {
        // Cas 1 : `~/.wine-mao` est un lien vers un environnement complet.
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let ailleurs = d.path().join("ailleurs");
        std::fs::rename(ancien(&c), &ailleurs).unwrap();
        std::os::unix::fs::symlink(&ailleurs, ancien(&c)).unwrap();
        assert_eq!(detecter(&c), None);
        // Cas 2 : vrai dossier sans `user.reg`.
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        std::fs::remove_file(ancien(&c).join("user.reg")).unwrap();
        assert_eq!(detecter(&c), None);
    }

    #[test]
    fn ignore_si_le_prefixe_wimao_existe() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        std::fs::create_dir_all(&c.prefixe).unwrap();
        assert_eq!(detecter(&c), None);
    }

    #[test]
    fn import_complet() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let avant = std::fs::metadata(config_utilisateur(&c))
            .unwrap()
            .modified()
            .unwrap();
        let importe = importer(&c, &ProgressionMuette).unwrap();
        assert_eq!(importe.ancien, ancien(&c));
        assert!(c.prefixe.join("system.reg").is_file());
        assert!(est_un_vrai_dossier(&c.prefixe));
        assert_eq!(std::fs::read_link(ancien(&c)).unwrap(), c.prefixe);
        for nom in REG {
            assert_eq!(
                std::fs::read_to_string(importe.sauvegarde.join(nom)).unwrap(),
                format!("contenu {nom}")
            );
        }
        assert_eq!(
            std::fs::read_to_string(importe.sauvegarde.join("config.toml")).unwrap(),
            CONFIG_UTILISATEUR
        );
        // La configuration de l'utilisateur n'est pas touchée (contenu et date).
        assert_eq!(
            std::fs::read_to_string(config_utilisateur(&c)).unwrap(),
            CONFIG_UTILISATEUR
        );
        let apres = std::fs::metadata(config_utilisateur(&c))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(avant, apres);
        assert_eq!(ancien_emplacement(&c), Some(ancien(&c)));
        assert!(annulation_possible(&c));
        let entrees = historique::lire(&c).unwrap();
        assert!(entrees.iter().any(|e| e.action == Action::Import));
    }

    #[test]
    fn autre_disque_est_refuse_sans_rien_toucher() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let proc = proc_vide(d.path());
        let op = Injections {
            proc: &proc,
            meme_disque: &|_, _| Ok(false),
            ..reelles()
        };
        let r = importer_dans(&c, &ProgressionMuette, &op);
        assert!(matches!(r, Err(Erreur::AutreDisque { .. })), "{r:?}");
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(c.prefixe.symlink_metadata().is_err());
        assert!(!c.donnees.join(DOSSIER_IMPORT).exists());
    }

    /// Faux processus `pid` dans le faux `proc` : ligne de commande `cmdline` (arguments séparés
    /// par NUL) et `WINEPREFIX=prefixe`.
    fn faux_processus(proc: &Path, pid: &str, cmdline: &str, prefixe: &Path) {
        let dossier = proc.join(pid);
        std::fs::create_dir_all(&dossier).unwrap();
        std::fs::write(dossier.join("cmdline"), cmdline).unwrap();
        std::fs::write(
            dossier.join("environ"),
            format!("HOME=/h\0WINEPREFIX={}\0", prefixe.display()),
        )
        .unwrap();
    }

    /// Relecture finale I1 : après l'import, un plugin ponté avant l'import tourne avec
    /// `WINEPREFIX=~/.wine-mao`, et un ancien raccourci lance un gestionnaire sous ce nom. Ils
    /// utilisent le même `wineserver` que `c.prefixe` : tout arrêt de Wine doit les voir.
    /// Après l'annulation, `~/.wine-mao` est de nouveau l'environnement de l'utilisateur.
    #[test]
    fn apres_l_import_l_ancien_nom_compte_avant_d_arreter_wine() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        let proc = proc_vide(d.path());
        faux_processus(&proc, "4242", "yabridge-host.exe\0", &ancien(&c));
        faux_processus(&proc, "4243", "C:\\NA\\Native Access.exe\0", &ancien(&c));
        faux_processus(&proc, "4244", "/opt/wine/bin/wineserver\0", &ancien(&c));
        assert_eq!(noms_du_prefixe(&c), vec![c.prefixe.clone(), ancien(&c)]);
        assert!(crate::restauration::plugins_en_cours_selon(&proc, &c).unwrap());
        assert!(crate::restauration::wineserver_actif_selon(&proc, &c).unwrap());
        assert_eq!(
            processus::programmes_ouverts_selon(&proc, &c).unwrap(),
            ["Native Access.exe", "yabridge-host.exe"]
        );

        annuler(&c, &ProgressionMuette).unwrap();
        assert_eq!(noms_du_prefixe(&c), vec![c.prefixe.clone()]);
        assert!(!crate::restauration::plugins_en_cours_selon(&proc, &c).unwrap());
        assert!(!crate::restauration::wineserver_actif_selon(&proc, &c).unwrap());
        assert!(processus::programmes_ouverts_selon(&proc, &c)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn plugin_en_cours_refuse_l_import_sans_rien_toucher() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        // Faux `/proc` : un `yabridge-host` qui tourne sur `~/.wine-mao`.
        let proc = proc_vide(d.path());
        let processus = proc.join("4242");
        std::fs::create_dir_all(&processus).unwrap();
        std::fs::write(processus.join("cmdline"), "yabridge-host.exe\0").unwrap();
        std::fs::write(
            processus.join("environ"),
            format!("WINEPREFIX={}\0", ancien(&c).display()),
        )
        .unwrap();
        let op = Injections {
            proc: &proc,
            ..reelles()
        };
        let r = importer_dans(&c, &ProgressionMuette, &op);
        assert!(matches!(r, Err(Erreur::PluginsEnCours)), "{r:?}");
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(c.prefixe.symlink_metadata().is_err());
        assert!(!c.donnees.join(DOSSIER_IMPORT).exists());
    }

    #[test]
    fn echec_du_lien_remet_le_dossier_en_place() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let proc = proc_vide(d.path());
        let op = Injections {
            proc: &proc,
            creer_lien: &|_, _| Err(std::io::Error::other("lien impossible")),
            ..reelles()
        };
        let r = importer_dans(&c, &ProgressionMuette, &op);
        match r {
            Err(Erreur::Io(e)) => assert!(e.to_string().contains("lien impossible")),
            autre => panic!("erreur du lien attendue, obtenu {autre:?}"),
        }
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(ancien(&c).join("system.reg").is_file());
        assert!(c.prefixe.symlink_metadata().is_err());
        assert_eq!(ancien_emplacement(&c), None);
        // Il ne reste rien de l'essai : ni trace, ni petite sauvegarde.
        assert_eq!(
            noms_dans(&c.donnees.join(DOSSIER_IMPORT)),
            Vec::<String>::new()
        );
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].action, Action::Import);
        assert_eq!(entrees[0].resultat, ResultatAction::Echec);
    }

    #[test]
    fn remise_en_place_impossible_nomme_le_nouvel_emplacement() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let proc = proc_vide(d.path());
        // Le lien échoue après qu'un fichier a pris la place : le renommage inverse échoue aussi.
        let op = Injections {
            proc: &proc,
            creer_lien: &|_, lien| {
                std::fs::write(lien, "intrus")?;
                Err(std::io::Error::other("lien impossible"))
            },
            ..reelles()
        };
        let r = importer_dans(&c, &ProgressionMuette, &op);
        let message = match r {
            Err(e) => e.to_string(),
            Ok(_) => panic!("échec attendu"),
        };
        assert!(message.contains(c.prefixe.to_str().unwrap()), "{message}");
        assert!(c.prefixe.join("system.reg").is_file());
        // La trace reste « en cours » (reprise possible) et l'échec est dans l'historique.
        assert_eq!(lire_trace(&c).unwrap().etat, EtatImport::EnCours);
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].resultat, ResultatAction::Echec);
    }

    #[test]
    fn deja_present_est_refuse() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        std::fs::create_dir_all(&c.prefixe).unwrap();
        let r = importer(&c, &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::DejaPresent(_))), "{r:?}");
        assert!(est_un_vrai_dossier(&ancien(&c)));
    }

    #[test]
    fn annuler_remet_tout_en_place() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        annuler(&c, &ProgressionMuette).unwrap();
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(ancien(&c).join("system.reg").is_file());
        assert!(c.prefixe.symlink_metadata().is_err());
        let dossier = c.donnees.join(DOSSIER_IMPORT);
        let annules: Vec<_> = std::fs::read_dir(&dossier)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("import.annule-") && n.ends_with(".toml"))
            .collect();
        assert_eq!(annules.len(), 1, "{annules:?}");
        assert!(!dossier.join(FICHIER_TRACE).exists());
        assert!(!annulation_possible(&c));
        assert_eq!(ancien_emplacement(&c), None);
        // La copie de travail de la configuration est supprimée.
        assert!(
            !noms_dans(&dossier)
                .iter()
                .any(|n| n.starts_with(DEBUT_COPIE_SYNC)),
            "{:?}",
            noms_dans(&dossier)
        );
        // `yabridgectl sync` lancé avec le HOME de l'utilisateur et l'ancien préfixe…
        let trace = std::fs::read_to_string(d.path().join("yabridgectl.trace")).unwrap();
        let champs: Vec<&str> = trace.trim().split('|').collect();
        assert_eq!(champs[0], c.home.to_str().unwrap());
        assert_eq!(champs[2], ancien(&c).to_str().unwrap());
        assert_eq!(champs[3], "sync");
        // … sans jamais pouvoir écrire dans sa configuration.
        assert_ne!(champs[1], c.home.join(".config").to_str().unwrap());
        assert_eq!(
            std::fs::read_to_string(config_utilisateur(&c)).unwrap(),
            CONFIG_UTILISATEUR
        );
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].action, Action::AnnulationImport);
    }

    /// Relecture finale I2 : après l'annulation, les points créés sur l'environnement importé
    /// décrivent `~/.wine-mao`, rendu à l'utilisateur ; un environnement créé ensuite dans
    /// `c.prefixe` n'a rien à voir avec eux. Ils sont périmés dès l'annulation.
    #[test]
    fn annuler_perime_les_points_de_l_environnement_importe() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        let point = crate::restauration::creer(&c, "avant une installation").unwrap();
        annuler(&c, &ProgressionMuette).unwrap();
        let liste = crate::restauration::lister(&c).unwrap();
        assert!(liste.points.is_empty(), "{liste:?}");
        assert_eq!(liste.perimes.len(), 1, "{liste:?}");
        assert_eq!(liste.perimes[0].id, point.id);
    }

    #[test]
    fn annuler_avec_un_renommage_rate_remet_le_lien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        let op = Injections {
            renommer: &|_, _| Err(std::io::Error::other("renommage impossible")),
            ..reelles()
        };
        let r = annuler_dans(&c, &ProgressionMuette, &op);
        assert!(matches!(r, Err(Erreur::Io(_))), "{r:?}");
        assert_eq!(std::fs::read_link(ancien(&c)).unwrap(), c.prefixe);
        assert!(c.prefixe.join("system.reg").is_file());
        assert_eq!(lire_trace(&c).unwrap().etat, EtatImport::Fait);
        assert!(annulation_possible(&c));
    }

    #[test]
    fn annuler_refuse_une_trace_qui_designe_un_autre_chemin() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        let mut trace = lire_trace(&c).unwrap();
        trace.ancien = d.path().join("ailleurs");
        ecrire_trace(&c, &trace).unwrap();
        let r = annuler(&c, &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::PasNotreLien(_))), "{r:?}");
        assert!(c.prefixe.join("system.reg").is_file());
    }

    #[test]
    fn annuler_sans_configuration_avertit() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        std::fs::remove_file(config_utilisateur(&c)).unwrap();
        let notes = Notes::default();
        annuler(&c, &notes).unwrap();
        assert!(!d.path().join("yabridgectl.trace").exists());
        let messages = notes.0.lock().unwrap();
        assert!(
            messages.iter().any(|m| m.contains("yabridgectl sync")),
            "{messages:?}"
        );
    }

    #[test]
    fn annuler_refuse_si_le_lien_a_ete_remplace() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        std::fs::remove_file(ancien(&c)).unwrap();
        std::fs::create_dir(ancien(&c)).unwrap();
        assert!(!annulation_possible(&c));
        let r = annuler(&c, &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::PasNotreLien(_))), "{r:?}");
        assert!(c.prefixe.join("system.reg").is_file());
        assert!(est_un_vrai_dossier(&ancien(&c)));
    }

    #[test]
    fn reprise_import_coupe_avant_le_lien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        interruption(&c, true, false, EtatImport::EnCours);
        assert!(!annulation_possible(&c));
        assert!(reprendre_interruption(&c).unwrap().is_some());
        assert_eq!(std::fs::read_link(ancien(&c)).unwrap(), c.prefixe);
        assert_eq!(lire_trace(&c).unwrap().etat, EtatImport::Fait);
        assert!(annulation_possible(&c));
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].action, Action::Import);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
        let cle = &entrees[0]
            .details_neutre
            .as_ref()
            .expect("détails neutres")
            .cle;
        assert!(historique::CLES_CONNUES.contains(&cle.as_str()), "{cle}");
        // Une seconde reprise n'a plus rien à faire.
        assert_eq!(reprendre_interruption(&c).unwrap(), None);
    }

    /// Crée un point de restauration sur un environnement de WiMao qui disparaît ensuite (comme
    /// après une annulation d'import ou une suppression à la main) ; rend son identifiant.
    fn point_d_un_ancien_prefixe(c: &Chemins) -> String {
        std::fs::create_dir_all(c.drive_c()).unwrap();
        for nom in REG {
            std::fs::write(c.prefixe.join(nom), "ancien").unwrap();
        }
        let id = crate::restauration::creer(c, "avant").unwrap().id;
        std::fs::remove_dir_all(&c.prefixe).unwrap();
        id
    }

    /// Identifiants des points périmés, et nombre de points encore utilisables.
    fn perimes_et_valables(c: &Chemins) -> (Vec<String>, usize) {
        let liste = crate::restauration::lister(c).unwrap();
        let perimes = liste.perimes.into_iter().map(|p| p.id).collect();
        (perimes, liste.points.len())
    }

    /// L'import installe un autre environnement dans `c.prefixe` : les points d'un environnement
    /// précédent ne le décrivent pas.
    #[test]
    fn importer_perime_les_points_d_un_ancien_environnement() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let id = point_d_un_ancien_prefixe(&c);
        importer(&c, &ProgressionMuette).unwrap();
        assert_eq!(perimes_et_valables(&c), (vec![id], 0));
    }

    /// Même règle quand l'import (ou l'annulation) coupé est terminé par la reprise.
    #[test]
    fn les_reprises_terminees_periment_les_points() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let id = point_d_un_ancien_prefixe(&c);
        interruption(&c, true, false, EtatImport::EnCours);
        reprendre_interruption(&c).unwrap().unwrap();
        assert_eq!(perimes_et_valables(&c), (vec![id.clone()], 0));

        // Un point sur l'environnement importé, puis une annulation coupée après le renommage.
        let autre = crate::restauration::creer(&c, "avant une installation")
            .unwrap()
            .id;
        std::fs::remove_file(ancien(&c)).unwrap();
        std::fs::rename(&c.prefixe, ancien(&c)).unwrap();
        let mut trace = lire_trace(&c).unwrap();
        trace.etat = EtatImport::AnnulationEnCours;
        ecrire_trace(&c, &trace).unwrap();
        reprendre_interruption(&c).unwrap().unwrap();
        let (mut perimes, valables) = perimes_et_valables(&c);
        perimes.sort();
        let mut attendus = vec![id, autre];
        attendus.sort();
        assert_eq!((perimes, valables), (attendus, 0));
    }

    #[test]
    fn reprise_import_coupe_apres_le_lien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        interruption(&c, true, true, EtatImport::EnCours);
        assert!(reprendre_interruption(&c).unwrap().is_some());
        assert_eq!(lire_trace(&c).unwrap().etat, EtatImport::Fait);
        assert!(annulation_possible(&c));
    }

    #[test]
    fn reprise_import_coupe_avant_le_deplacement() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        interruption(&c, false, false, EtatImport::EnCours);
        assert!(reprendre_interruption(&c).unwrap().is_some());
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(c.prefixe.symlink_metadata().is_err());
        assert!(lire_trace(&c).is_none());
        assert_eq!(archives(&c, DEBUT_ABANDONNE), 1);
    }

    #[test]
    fn reprise_annulation_coupee_apres_le_retrait_du_lien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        interruption(&c, true, false, EtatImport::AnnulationEnCours);
        assert!(reprendre_interruption(&c).unwrap().is_some());
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(ancien(&c).join("system.reg").is_file());
        assert!(c.prefixe.symlink_metadata().is_err());
        assert!(lire_trace(&c).is_none());
        assert_eq!(archives(&c, DEBUT_ANNULE), 1);
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].action, Action::AnnulationImport);
    }

    #[test]
    fn reprise_annulation_coupee_apres_le_deplacement() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        interruption(&c, false, false, EtatImport::AnnulationEnCours);
        let message = reprendre_interruption(&c).unwrap().unwrap();
        assert!(message.contains("yabridgectl sync"), "{message}");
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(lire_trace(&c).is_none());
        assert_eq!(archives(&c, DEBUT_ANNULE), 1);
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].action, Action::AnnulationImport);
        let cle = &entrees[0]
            .details_neutre
            .as_ref()
            .expect("détails neutres")
            .cle;
        assert!(historique::CLES_CONNUES.contains(&cle.as_str()), "{cle}");
        assert_eq!(entrees[0].details_affiches(), message);
    }

    #[test]
    fn reprise_annulation_coupee_avant_le_retrait_du_lien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        interruption(&c, true, true, EtatImport::AnnulationEnCours);
        assert!(!annulation_possible(&c));
        assert!(reprendre_interruption(&c).unwrap().is_some());
        assert_eq!(lire_trace(&c).unwrap().etat, EtatImport::Fait);
        assert!(annulation_possible(&c));
    }

    #[test]
    fn reprise_ambigue_ne_touche_a_rien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        // Trace « en cours » mais deux vrais dossiers : impossible de savoir lequel garder.
        interruption(&c, false, false, EtatImport::EnCours);
        std::fs::create_dir_all(&c.prefixe).unwrap();
        let r = reprendre_interruption(&c);
        match r {
            Err(Erreur::ImportInterrompu { ancien: a, prefixe }) => {
                assert_eq!(a, ancien(&c));
                assert_eq!(prefixe, c.prefixe);
            }
            autre => panic!("ImportInterrompu attendu, obtenu {autre:?}"),
        }
        assert_eq!(lire_trace(&c).unwrap().etat, EtatImport::EnCours);
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(est_un_vrai_dossier(&c.prefixe));
    }

    #[test]
    fn sans_trace_ou_import_fait_rien_a_reprendre() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        assert_eq!(reprendre_interruption(&c).unwrap(), None);
        importer(&c, &ProgressionMuette).unwrap();
        assert_eq!(reprendre_interruption(&c).unwrap(), None);
    }

    #[test]
    fn trace_sans_etat_vaut_fait() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        interruption(&c, true, true, EtatImport::Fait);
        // Trace d'une version précédente : pas de champ `etat`.
        std::fs::write(
            c.donnees.join(DOSSIER_IMPORT).join(FICHIER_TRACE),
            format!(
                "ancien = \"{}\"\ndate = \"x\"\nsauvegarde = \"/s\"\n",
                ancien(&c).display()
            ),
        )
        .unwrap();
        assert_eq!(reprendre_interruption(&c).unwrap(), None);
        assert!(annulation_possible(&c));
    }

    /// `fsync` qui échoue toujours : simule un disque qui refuse la synchronisation.
    fn fsync_rate(_: &Path) -> std::io::Result<()> {
        Err(std::io::Error::other("fsync impossible"))
    }

    #[test]
    fn fsync_rate_apres_le_deplacement_n_annule_pas_l_import() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let proc = proc_vide(d.path());
        let op = Injections {
            proc: &proc,
            synchroniser: &fsync_rate,
            ..reelles()
        };
        let importe = importer_dans(&c, &ProgressionMuette, &op).unwrap();
        assert!(c.prefixe.join("system.reg").is_file());
        assert_eq!(std::fs::read_link(ancien(&c)).unwrap(), c.prefixe);
        assert_eq!(lire_trace(&c).unwrap().etat, EtatImport::Fait);
        assert!(importe.sauvegarde.join("system.reg").is_file());
        assert!(annulation_possible(&c));
    }

    #[test]
    fn fsync_rate_apres_la_remise_en_place_dit_que_tout_est_remis() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let proc = proc_vide(d.path());
        let op = Injections {
            proc: &proc,
            creer_lien: &|_, _| Err(std::io::Error::other("lien impossible")),
            synchroniser: &fsync_rate,
            ..reelles()
        };
        let message = importer_dans(&c, &ProgressionMuette, &op)
            .unwrap_err()
            .to_string();
        // Le retour a eu lieu : le message ne prétend pas que l'environnement est ailleurs.
        assert!(!message.contains(c.prefixe.to_str().unwrap()), "{message}");
        assert!(message.contains("lien impossible"), "{message}");
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(c.prefixe.symlink_metadata().is_err());
        assert_eq!(
            noms_dans(&c.donnees.join(DOSSIER_IMPORT)),
            Vec::<String>::new()
        );
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].resultat, ResultatAction::Echec);
        assert!(!entrees[0].details.contains(c.prefixe.to_str().unwrap()));
    }

    #[test]
    fn fsync_rate_pendant_l_annulation_ne_la_fait_pas_echouer() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        importer(&c, &ProgressionMuette).unwrap();
        let op = Injections {
            synchroniser: &fsync_rate,
            ..reelles()
        };
        annuler_dans(&c, &ProgressionMuette, &op).unwrap();
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(c.prefixe.symlink_metadata().is_err());
        assert!(lire_trace(&c).is_none());
        assert_eq!(archives(&c, DEBUT_ANNULE), 1);
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees[0].action, Action::AnnulationImport);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
    }

    #[test]
    fn trace_en_cours_impossible_refuse_l_import_sans_rien_deplacer() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        // Un dossier à la place de `import.toml` : l'écriture atomique de la trace échoue.
        std::fs::create_dir_all(c.donnees.join(DOSSIER_IMPORT).join(FICHIER_TRACE)).unwrap();
        let r = importer(&c, &ProgressionMuette);
        assert!(r.is_err(), "{r:?}");
        assert!(est_un_vrai_dossier(&ancien(&c)));
        assert!(ancien(&c).join("system.reg").is_file());
        assert!(c.prefixe.symlink_metadata().is_err());
        // La petite sauvegarde a été effacée.
        assert!(!noms_dans(&c.donnees.join(DOSSIER_IMPORT))
            .iter()
            .any(|n| n.starts_with(DEBUT_SAUVEGARDE)));
    }
}
