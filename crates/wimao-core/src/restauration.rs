//! Points de restauration de l'environnement Wine (le « préfixe »).
//!
//! Un point garde l'état du registre (`system.reg`, `user.reg`, `userdef.reg`) et la
//! liste des fichiers présents. Revenir en arrière remet les trois `.reg` et supprime
//! les fichiers apparus depuis (une installation ratée, par exemple). Les fichiers
//! modifiés ou supprimés depuis ne sont PAS restaurés : seul le registre l'est.
//!
//! Utilise [`Chemins`], [`Wine::arreter`], [`Erreur`], `import::noms_du_prefixe` (plugins
//! en cours sous tous les noms du préfixe) et `diagnostic::poser_verrou_installation` (verrou
//! pendant un retour) ; utilisé par l'installation et la
//! session de gestionnaire de `wimao-recettes` (point avant chaque installeur ou session), par
//! `diagnostic::reparer` (point « repair ») et par la CLI et l'interface (liste, retour en
//! arrière). `revenir` écrit l'action `Retour` dans l'[`historique`] (une seule entrée, une fois
//! le retour engagé : un point périmé, un plugin en cours ou un verrou d'installation tenu
//! ailleurs sont refusés sans rien écrire).
//!
//! # Disposition sur disque
//!
//! ```text
//! <donnees>/restauration/<id>/
//!     motif.txt      pourquoi le point existe (texte libre)
//!     system.reg, user.reg, userdef.reg
//!     fichiers.txt   entrées du préfixe, chemins relatifs, octets bruts, chacune
//!                    terminée par un octet NUL ; un dossier se termine par `/`
//!                    avant le NUL (un nom de fichier ne peut pas finir par `/`)
//! ```
//!
//! `fichiers.txt` est écrit EN DERNIER, de façon atomique (`.part`, `sync_all`, renommage) comme
//! les autres fichiers, et se termine par une marque `\x01WIMAO-FIN:<nombre d'entrées>`. Un point
//! n'est complet que si les trois `.reg` sont là et que la liste porte une marque valide et contient
//! les `.reg` et `drive_c/` : sinon [`lister`] le met dans `incomplets` et [`revenir`] le refuse
//! avant de toucher à quoi que ce soit (une liste vide ferait effacer tout le préfixe).
//! Séparateur NUL plutôt que saut de ligne : un nom de fichier Linux peut contenir
//! n'importe quel octet sauf `/` et NUL, y compris un saut de ligne.
//!
//! # Sécurité
//!
//! - **Logiciel de musique** : arrêter le `wineserver` ferait planter les plugins
//!   ouverts. [`creer`] et [`revenir`] refusent donc de s'exécuter (erreur
//!   [`Erreur::PluginsEnCours`]) tant qu'un `yabridge-host` utilise ce préfixe.
//! - **Liens symboliques** : `drive_c/users/<utilisateur>/Documents` etc. pointent
//!   souvent vers le vrai dossier personnel. Les liens ne sont jamais suivis : ils
//!   sont listés comme des entrées ordinaires, et supprimés (le lien seul) s'ils
//!   sont apparus depuis. On ne supprime donc jamais rien hors du préfixe.
//!
//! # Identifiants et heure
//!
//! `AAAA-MM-JJ-HHhMMmSS` en heure LOCALE (lue avec `localtime_r` de la libc, déjà
//! une dépendance : pas de crate de dates en plus). Deux points dans la même seconde
//! reçoivent le suffixe `-2`, `-3`… Limite connue : au changement d'heure d'automne,
//! une heure se répète et l'ordre « plus récent d'abord » peut y être faussé.

use crate::chemins::Chemins;
use crate::diagnostic::poser_verrou_installation;
use crate::erreur::{Erreur, Resultat};
use crate::historique::{self, Action, ResultatAction, TexteNeutre};
use crate::i18n::{tr, trf};
use crate::import;
use crate::journal;
use crate::pontage::retirer_liens_casses;
use crate::processus::processus_du_prefixe_dans;
use crate::wine::Wine;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Nombre de points conservés dans la file « installation » (installations et sessions) ; les
/// plus anciens sont supprimés par [`creer`].
const POINTS_CONSERVES: usize = 10;
/// Nombre de points conservés dans la file « réparation » (motif [`MOTIF_REPARATION`]). File
/// séparée : plusieurs réparations de suite ne doivent pas chasser les points d'installation,
/// qui sont ceux qu'on veut retrouver après une installation ratée.
const POINTS_REPARATION_CONSERVES: usize = 3;
/// Fichier vide qui marque un point « périmé » : le préfixe n'est plus celui que le point décrit
/// (sauvegarde restaurée, import ou annulation de l'import, préfixe neuf : voir
/// [`perimer_tous_les_points`] ; ou retour vers un point plus ancien : voir [`revenir`]).
const FICHIER_PERIME: &str = "perime";
/// Sous-dossier de `<donnees>` qui contient les points.
const DOSSIER_RESTAURATION: &str = "restauration";
/// Fichiers de registre sauvegardés (Wine ne les écrit qu'à l'arrêt du serveur, voir NOTES.md).
const FICHIERS_REG: [&str; 3] = ["system.reg", "user.reg", "userdef.reg"];
/// Fichier du motif dans un point.
const FICHIER_MOTIF: &str = "motif.txt";
/// Fichier de la liste des entrées (écrit en dernier : marque un point complet).
const FICHIER_LISTE: &str = "fichiers.txt";
/// Marque, en fin de chemin dans `fichiers.txt`, une entrée qui est un dossier.
const MARQUE_DOSSIER: u8 = b'/';
/// Début de la dernière entrée de `fichiers.txt`, suivi du nombre d'entrées qui la
/// précèdent. L'octet 0x01 la distingue d'un nom de fichier réel.
const MARQUE_FIN: &[u8] = b"\x01WIMAO-FIN:";
/// Dossier que tout préfixe Wine contient (voir `lire_liste`).
const DOSSIER_DISQUE_C: &str = "drive_c";
/// Essais de noms libres pour un point (collision avec un autre processus de WiMao).
const ESSAIS_MAXIMAUX: u32 = 1000;
/// Séparateur des chemins dans `fichiers.txt`.
const SEPARATEUR: u8 = 0;
/// Dossier virtuel du système de fichiers des processus Linux.
const RACINE_PROC: &str = "/proc";
/// Fragment de ligne de commande qui désigne le processus hôte des plugins yabridge.
const NOM_HOTE_YABRIDGE: &str = "yabridge-host";
/// Nom du programme serveur de Wine.
const NOM_WINESERVER: &str = "wineserver";

/// Motif enregistré pour un point créé avant une réparation (clé stable, en anglais,
/// identique dans les points déjà enregistrés) ; affiché traduit par [`libelle_motif`].
pub const MOTIF_REPARATION: &str = "repair";
/// Début du motif d'un point créé avant une installation : `install <fichier>`.
const PREFIXE_MOTIF_INSTALLATION: &str = "install ";

/// Motif à donner à [`creer`] avant l'installation de `fichier` (nom du fichier seul).
/// Le motif est une clé stable, jamais un texte traduit : un point créé en français
/// s'affiche ensuite dans la langue courante (voir [`libelle_motif`]).
pub fn motif_installation(fichier: &str) -> String {
    format!("{PREFIXE_MOTIF_INSTALLATION}{fichier}")
}

/// Début du motif d'un point créé à l'ouverture d'un gestionnaire : `session <nom>`.
const PREFIXE_MOTIF_SESSION: &str = "session ";

/// Motif à donner à [`creer`] à l'ouverture d'une session du gestionnaire `nom` (nom de la
/// recette). Clé stable, comme [`motif_installation`].
pub fn motif_session(nom: &str) -> String {
    format!("{PREFIXE_MOTIF_SESSION}{nom}")
}

/// Texte traduit d'un motif enregistré, pour l'affichage. Les motifs connus
/// ([`MOTIF_REPARATION`], [`motif_installation`], [`motif_session`]) sont traduits ; tout autre
/// motif (point créé par une version future ou à la main) est rendu tel quel.
pub fn libelle_motif(motif: &str) -> String {
    if motif == MOTIF_REPARATION {
        return tr("before a repair");
    }
    if let Some(nom) = motif.strip_prefix(PREFIXE_MOTIF_SESSION) {
        return trf("before the {name} session", &[("name", nom)]);
    }
    match motif.strip_prefix(PREFIXE_MOTIF_INSTALLATION) {
        Some(fichier) => trf("before installing {file}", &[("file", fichier)]),
        None => motif.to_string(),
    }
}

/// Le motif enregistré sous forme de texte neutre pour l'historique (traduit à l'affichage,
/// comme [`libelle_motif`]) ; `None` pour un motif inconnu, qui n'a pas de traduction.
pub fn motif_neutre(motif: &str) -> Option<TexteNeutre> {
    if motif == MOTIF_REPARATION {
        return Some(TexteNeutre::nouveau(historique::CLE_AVANT_REPARATION, &[]));
    }
    if let Some(nom) = motif.strip_prefix(PREFIXE_MOTIF_SESSION) {
        return Some(TexteNeutre::nouveau(
            historique::CLE_AVANT_SESSION,
            &[("name", nom)],
        ));
    }
    motif
        .strip_prefix(PREFIXE_MOTIF_INSTALLATION)
        .map(|fichier| {
            TexteNeutre::nouveau(historique::CLE_AVANT_INSTALLATION, &[("file", fichier)])
        })
}

/// File de conservation d'un point : chaque file a son propre quota (voir [`creer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilePoints {
    /// Points d'installation et de session : `POINTS_CONSERVES` gardés.
    Installation,
    /// Points créés avant une réparation : `POINTS_REPARATION_CONSERVES` gardés.
    Reparation,
}

/// Un point de restauration enregistré sur disque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointRestauration {
    /// Identifiant `AAAA-MM-JJ-HHhMMmSS`, éventuellement suivi de `-2`, `-3`…
    pub id: String,
    /// Raison de la création, telle que donnée à [`creer`] (clé stable : afficher
    /// [`libelle_motif`]).
    pub motif: String,
    /// Dossier du point : `<donnees>/restauration/<id>`.
    pub dossier: PathBuf,
}

impl PointRestauration {
    /// File à laquelle appartient le point, d'après son motif.
    pub fn file(&self) -> FilePoints {
        if self.motif == MOTIF_REPARATION {
            FilePoints::Reparation
        } else {
            FilePoints::Installation
        }
    }
}

/// Résultat de [`lister`] : les points utilisables et ceux qu'il a fallu écarter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListePoints {
    /// Points complets, du plus récent au plus ancien.
    pub points: Vec<PointRestauration>,
    /// Points complets mais périmés (ils ne correspondent plus à l'environnement : voir
    /// `FICHIER_PERIME`, [`perimer_tous_les_points`] et [`revenir`]), du plus récent au plus ancien. On les montre mais [`revenir`] les
    /// refuse. Ils comptent dans le quota de leur file.
    pub perimes: Vec<PointRestauration>,
    /// Dossiers de `restauration/` sans `fichiers.txt` (création interrompue) :
    /// inutilisables, mais signalés pour que l'interface puisse les montrer ou les
    /// supprimer. Jamais supprimés automatiquement.
    pub incomplets: Vec<PathBuf>,
}

/// Dossier qui contient tous les points : `<donnees>/restauration`.
fn dossier_points(c: &Chemins) -> PathBuf {
    c.donnees.join(DOSSIER_RESTAURATION)
}

/// Dit si un plugin tourne actuellement sur l'environnement de `c`.
///
/// Cherche un processus dont la ligne de commande contient `yabridge-host` et dont
/// la variable `WINEPREFIX` vaut un des noms de l'environnement
/// ([`import::noms_du_prefixe`] : `c.prefixe`, et après un import l'ancien `~/.wine-mao`, sous
/// lequel tournent les plugins pontés avant l'import). Lecture de `/proc` faite par
/// [`crate::processus`]. Un processus d'un autre utilisateur (dont `/proc/<pid>/environ` est
/// illisible) est ignoré : il ne peut pas utiliser notre préfixe.
/// Erreur : [`Erreur::Io`] si `/proc` est illisible.
pub fn plugins_en_cours(c: &Chemins) -> Resultat<bool> {
    plugins_en_cours_selon(Path::new(RACINE_PROC), c)
}

/// [`plugins_en_cours`] avec la racine de `/proc` injectée (tests).
pub(crate) fn plugins_en_cours_selon(proc: &Path, c: &Chemins) -> Resultat<bool> {
    plugins_en_cours_dans(proc, &import::noms_du_prefixe(c))
}

/// Cœur de [`plugins_en_cours`] : un `yabridge-host` tourne-t-il avec un `WINEPREFIX` égal à un
/// des chemins de `noms` ? Aussi utilisé par `import` pour l'environnement `~/.wine-mao` avant
/// qu'il ne soit importé.
pub(crate) fn plugins_en_cours_dans(proc: &Path, noms: &[PathBuf]) -> Resultat<bool> {
    Ok(processus_du_prefixe_dans(proc, noms)?
        .iter()
        .any(|p| p.ligne().contains(NOM_HOTE_YABRIDGE)))
}

/// Dit si le `wineserver` de ce préfixe est actif (utilisé par `diagnostic`).
///
/// Même méthode que [`plugins_en_cours`] (sous tous les noms de l'environnement), mais le
/// programme recherché est `wineserver`. Erreur : [`Erreur::Io`] si `/proc` est illisible.
pub(crate) fn wineserver_actif(c: &Chemins) -> Resultat<bool> {
    wineserver_actif_selon(Path::new(RACINE_PROC), c)
}

/// [`wineserver_actif`] avec la racine de `/proc` injectée (tests).
pub(crate) fn wineserver_actif_selon(proc: &Path, c: &Chemins) -> Resultat<bool> {
    Ok(
        processus_du_prefixe_dans(proc, &import::noms_du_prefixe(c))?
            .iter()
            .any(|p| {
                // Le nom du programme est le premier argument : comparer seulement lui évite
                // de prendre pour un serveur un éditeur de texte dont un argument
                // contiendrait « wineserver ».
                let premier = p.arguments.first().map(String::as_str).unwrap_or("");
                Path::new(premier).file_name() == Some(OsStr::new(NOM_WINESERVER))
            }),
    )
}

/// Refuse de continuer si un plugin tourne sur l'environnement de `c`, sous l'un de ses noms
/// ([`Erreur::PluginsEnCours`]).
fn exiger_aucun_plugin(proc: &Path, c: &Chemins) -> Resultat<()> {
    if plugins_en_cours_selon(proc, c)? {
        return Err(Erreur::PluginsEnCours);
    }
    Ok(())
}

/// Crée un point de restauration pour le préfixe de `c`.
///
/// Étapes : vérifie qu'aucun plugin ne tourne, arrête le `wineserver` (il n'écrit les
/// `.reg` qu'à l'arrêt), copie les trois `.reg`, relève les entrées du préfixe, puis
/// supprime les points au-delà des 10 plus récents. Si une étape échoue, le point
/// partiel est supprimé.
///
/// Erreurs : [`Erreur::PluginsEnCours`] ; [`Erreur::Introuvable`] si un `.reg` manque
/// (préfixe non initialisé) ; [`Erreur::Io`] ; erreurs de [`Wine::arreter`].
pub fn creer(c: &Chemins, motif: &str) -> Resultat<PointRestauration> {
    creer_dans(c, motif, &identifiant_local()?, Path::new(RACINE_PROC))
}

/// Cœur de [`creer`] avec l'identifiant de base et la racine de `/proc` injectés.
fn creer_dans(c: &Chemins, motif: &str, base_id: &str, proc: &Path) -> Resultat<PointRestauration> {
    exiger_aucun_plugin(proc, c)?;
    Wine::nouveau(c).arreter()?;
    let (id, dossier) = reserver_dossier(c, base_id)?;
    if let Err(e) = remplir_point(c, motif, &dossier) {
        let _ = std::fs::remove_dir_all(&dossier); // point partiel inutile
        return Err(e);
    }
    elaguer(c)?;
    Ok(PointRestauration {
        id,
        motif: motif.to_string(),
        dossier,
    })
}

/// Crée le dossier du point. `create_dir` (pas `create_dir_all`) échoue si le dossier
/// existe : c'est ce qui garantit qu'un point n'en écrase jamais un autre.
///
/// Le suffixe (`-2`, `-3`…) part de « le plus grand déjà présent pour cette seconde,
/// plus un », pas de 2 : l'élagage supprime d'abord le plus ancien (celui sans suffixe),
/// et un point neuf qui reprendrait ce nom libéré serait classé le plus ancien, donc
/// supprimé à son tour au prochain élagage.
fn reserver_dossier(c: &Chemins, base_id: &str) -> Resultat<(String, PathBuf)> {
    let racine = dossier_points(c);
    std::fs::create_dir_all(&racine)?;
    let mut numero = plus_grand_numero(&racine, base_id)?.map_or(1, |n| n + 1);
    for _ in 0..ESSAIS_MAXIMAUX {
        let id = if numero == 1 {
            base_id.to_string()
        } else {
            format!("{base_id}-{numero}")
        };
        let dossier = racine.join(&id);
        match std::fs::create_dir(&dossier) {
            Ok(()) => return Ok((id, dossier)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => numero += 1,
            Err(e) => return Err(e.into()),
        }
    }
    Err(Erreur::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        trf(
            "no free name found for the restore point {id}",
            &[("id", base_id)],
        ),
    )))
}

/// Plus grand numéro de suffixe des points de `racine` créés à la seconde `base_id`
/// (1 pour le point sans suffixe) ; `None` s'il n'y en a aucun.
fn plus_grand_numero(racine: &Path, base_id: &str) -> Resultat<Option<u32>> {
    let mut maximum = None;
    for entree in std::fs::read_dir(racine)? {
        let nom = entree?.file_name().to_string_lossy().into_owned();
        let (base, numero) = cle_de_tri(&nom);
        if base == base_id {
            maximum = maximum.max(Some(numero));
        }
    }
    Ok(maximum)
}

/// Écrit le contenu d'un point. Chaque fichier est écrit de façon atomique
/// ([`ecrire_atomique`]) et `fichiers.txt`, qui marque un point complet, en dernier.
fn remplir_point(c: &Chemins, motif: &str, dossier: &Path) -> Resultat<()> {
    ecrire_atomique(&dossier.join(FICHIER_MOTIF), motif.as_bytes())?;
    for nom in FICHIERS_REG {
        let source = c.prefixe.join(nom);
        copier_atomique(&source, &dossier.join(nom)).map_err(|e| contexte_reg(&source, e))?;
    }
    let releve = relever_prefixe(&c.prefixe)?;
    let nombre = releve.fichiers.len() + releve.dossiers.len();
    let mut liste = Vec::new();
    for (relatif, marque) in releve
        .fichiers
        .iter()
        .map(|f| (f, None))
        .chain(releve.dossiers.iter().map(|d| (d, Some(MARQUE_DOSSIER))))
    {
        liste.extend_from_slice(relatif.as_os_str().as_bytes());
        liste.extend(marque);
        liste.push(SEPARATEUR);
    }
    // Marque de fin : une liste coupée (courant, disque plein) ne la porte pas.
    liste.extend_from_slice(MARQUE_FIN);
    liste.extend_from_slice(nombre.to_string().as_bytes());
    liste.push(SEPARATEUR);
    ecrire_atomique(&dossier.join(FICHIER_LISTE), &liste)
}

/// Écrit `octets` dans `chemin` sans jamais laisser un fichier à moitié écrit :
/// écriture dans `<chemin>.part`, `sync_all` (données sur le disque), `rename`
/// (atomique sur un même système de fichiers), puis synchronisation du dossier pour
/// que le renommage lui-même survive à une coupure de courant.
pub(crate) fn ecrire_atomique(chemin: &Path, octets: &[u8]) -> Resultat<()> {
    let temporaire = nom_temporaire(chemin);
    let mut fichier = std::fs::File::create(&temporaire)?;
    std::io::Write::write_all(&mut fichier, octets)?;
    finaliser(fichier, &temporaire, chemin)
}

/// Comme [`ecrire_atomique`] pour une copie de `source` (erreur d'E/S brute : l'appelant
/// sait quel fichier est la source).
fn copier_atomique(source: &Path, destination: &Path) -> std::io::Result<()> {
    let temporaire = nom_temporaire(destination);
    std::fs::copy(source, &temporaire)?;
    let fichier = std::fs::File::open(&temporaire)?;
    finaliser(fichier, &temporaire, destination).map_err(|e| match e {
        Erreur::Io(io) => io,
        autre => std::io::Error::other(autre.to_string()),
    })
}

/// `<chemin>.part`, dans le même dossier (condition d'un `rename` atomique).
fn nom_temporaire(chemin: &Path) -> PathBuf {
    let mut nom = chemin.as_os_str().to_os_string();
    nom.push(".part");
    PathBuf::from(nom)
}

/// Fin commune : `sync_all`, renommage, synchronisation du dossier parent.
fn finaliser(fichier: std::fs::File, temporaire: &Path, chemin: &Path) -> Resultat<()> {
    fichier.sync_all()?;
    drop(fichier);
    std::fs::rename(temporaire, chemin)?;
    if let Some(parent) = chemin.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// Erreur lisible pour un `.reg` impossible à copier (le message de `io::Error` seul
/// ne dit pas quel fichier).
fn contexte_reg(fichier: &Path, e: std::io::Error) -> Erreur {
    if e.kind() == std::io::ErrorKind::NotFound {
        Erreur::Introuvable(trf(
            "{path} (has the Wine environment been created?)",
            &[("path", &fichier.display().to_string())],
        ))
    } else {
        Erreur::Io(e)
    }
}

/// Contenu du préfixe à un instant donné, en chemins relatifs triés par octets.
struct Releve {
    /// Fichiers ET liens symboliques (un lien, même vers un dossier, est une entrée
    /// ordinaire : on ne le suit pas et on ne descend pas dedans).
    fichiers: Vec<PathBuf>,
    /// Vrais dossiers. Gardés pour ne jamais supprimer un dossier qui existait déjà
    /// (par exemple `Program Files` vide) sous prétexte qu'il se vide.
    dossiers: Vec<PathBuf>,
}

/// Relève le préfixe sans suivre aucun lien symbolique.
fn relever_prefixe(prefixe: &Path) -> Resultat<Releve> {
    let mut releve = Releve {
        fichiers: Vec::new(),
        dossiers: Vec::new(),
    };
    for entree in WalkDir::new(prefixe).min_depth(1).follow_links(false) {
        let entree = entree.map_err(std::io::Error::from)?;
        let Ok(relatif) = entree.path().strip_prefix(prefixe) else {
            continue;
        };
        if entree.file_type().is_dir() {
            releve.dossiers.push(relatif.to_path_buf());
        } else {
            releve.fichiers.push(relatif.to_path_buf());
        }
    }
    for liste in [&mut releve.fichiers, &mut releve.dossiers] {
        liste.sort_by(|a, b| a.as_os_str().as_bytes().cmp(b.as_os_str().as_bytes()));
    }
    Ok(releve)
}

/// Contenu de `fichiers.txt`, déjà décodé.
struct ListeEnregistree {
    /// Chemins relatifs (octets bruts) des fichiers et liens.
    fichiers: HashSet<Vec<u8>>,
    /// Chemins relatifs (octets bruts, sans la marque `/`) des dossiers.
    dossiers: HashSet<Vec<u8>>,
}

fn lire_liste(point: &PointRestauration) -> Resultat<ListeEnregistree> {
    let brut = std::fs::read(point.dossier.join(FICHIER_LISTE)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Erreur::Introuvable(fichier_du_point(FICHIER_LISTE, point))
        } else {
            Erreur::Io(e)
        }
    })?;
    let corrompu = || {
        Erreur::Introuvable(trf(
            "{file} of the restore point {id} (incomplete or damaged)",
            &[("file", FICHIER_LISTE), ("id", &point.id)],
        ))
    };
    // Une liste valide finit par NUL, porte la marque de fin avec le bon nombre
    // d'entrées, et contient de quoi reconnaître un préfixe Wine (voir `verifier_point`).
    let sans_fin = brut.strip_suffix(&[SEPARATEUR]).ok_or_else(corrompu)?;
    let mut noms: Vec<&[u8]> = sans_fin.split(|&o| o == SEPARATEUR).collect();
    let marque = noms.pop().and_then(|n| n.strip_prefix(MARQUE_FIN));
    let declare = marque
        .and_then(|n| std::str::from_utf8(n).ok())
        .and_then(|n| n.parse::<usize>().ok());
    if declare != Some(noms.len()) || noms.iter().any(|n| n.is_empty()) {
        return Err(corrompu());
    }
    let mut liste = ListeEnregistree {
        fichiers: HashSet::new(),
        dossiers: HashSet::new(),
    };
    for nom in noms {
        match nom.strip_suffix(&[MARQUE_DOSSIER]) {
            Some(dossier) => liste.dossiers.insert(dossier.to_vec()),
            None => liste.fichiers.insert(nom.to_vec()),
        };
    }
    // Garde-fou : un préfixe Wine a toujours ces éléments ; sans eux la liste est
    // fausse et `revenir` effacerait tout.
    let reconnu = FICHIERS_REG
        .iter()
        .all(|r| liste.fichiers.contains(r.as_bytes()))
        && liste.dossiers.contains(DOSSIER_DISQUE_C.as_bytes());
    if !reconnu {
        return Err(corrompu());
    }
    Ok(liste)
}

/// Texte traduit « <fichier> du point <id> » pour [`Erreur::Introuvable`].
fn fichier_du_point(fichier: &str, point: &PointRestauration) -> String {
    trf(
        "{file} of the restore point {id}",
        &[("file", fichier), ("id", &point.id)],
    )
}

/// Vérifie qu'un point est utilisable : ses trois `.reg` sont présents et sa liste est
/// valide. Rend la liste décodée. Utilisé par [`lister`] et par [`revenir`] (avant
/// de toucher à quoi que ce soit).
fn verifier_point(point: &PointRestauration) -> Resultat<ListeEnregistree> {
    for nom in FICHIERS_REG {
        if !point.dossier.join(nom).is_file() {
            return Err(Erreur::Introuvable(fichier_du_point(nom, point)));
        }
    }
    lire_liste(point)
}

/// Supprime, dans chaque file, les points au-delà du quota : `POINTS_CONSERVES` pour
/// l'installation, `POINTS_REPARATION_CONSERVES` pour la réparation. Dans une file les points
/// complets **et** périmés comptent ensemble, du plus récent au plus ancien : un point périmé
/// ne sert plus mais ne doit pas rester indéfiniment.
fn elaguer(c: &Chemins) -> Resultat<()> {
    let liste = lister(c)?;
    let mut tous: Vec<PointRestauration> = liste.points.into_iter().chain(liste.perimes).collect();
    tous.sort_by_key(|p| std::cmp::Reverse(cle_de_tri(&p.id)));
    for (file, quota) in [
        (FilePoints::Installation, POINTS_CONSERVES),
        (FilePoints::Reparation, POINTS_REPARATION_CONSERVES),
    ] {
        for ancien in tous.iter().filter(|p| p.file() == file).skip(quota) {
            std::fs::remove_dir_all(&ancien.dossier)?;
        }
    }
    Ok(())
}

/// Marque périmés tous les points complets non déjà périmés ; rend combien l'ont été.
///
/// À appeler (par [`perimer_tous_les_points`]) quand l'environnement change d'identité : les
/// `.reg` et la liste de fichiers d'un point décrivent l'ancien, y revenir mélangerait deux
/// environnements. Le marquage est un fichier vide
/// `FICHIER_PERIME` dans le dossier du point ; rien d'autre n'est modifié. Les points
/// incomplets sont laissés tels quels. Erreur : [`Erreur::Io`].
pub fn marquer_perimes(c: &Chemins) -> Resultat<usize> {
    let points = lister(c)?.points;
    for point in &points {
        std::fs::write(point.dossier.join(FICHIER_PERIME), b"")?;
    }
    Ok(points.len())
}

/// L'environnement de WiMao vient de changer d'identité : périme tous les points (voir
/// [`marquer_perimes`]) ; un échec est seulement noté au journal technique avec `cause`.
///
/// Seul appel du moteur pour ce cas, à faire chaque fois que `c.prefixe` cesse de désigner
/// l'environnement que les points décrivent :
/// - une sauvegarde restaurée a remplacé le préfixe ([`crate::sauvegarde::restaurer`]) ;
/// - l'annulation de l'import a rendu l'environnement à `~/.wine-mao` ([`crate::import::annuler`]) ;
/// - un import vient d'installer un autre environnement ([`crate::import::importer`]) ;
/// - un préfixe neuf va être créé ([`crate::environnement::creer_environnement`]).
///
/// Sans cela, « Revenir à avant ceci » remettrait le registre d'un environnement sur un autre
/// et supprimerait tous les fichiers absents de l'ancienne liste. Ne fait jamais échouer
/// l'opération qui l'appelle : elle a déjà eu lieu (ou est sur le point d'avoir lieu), et un
/// point resté valable par erreur est encore refusé s'il ne correspond plus (`revenir` vérifie
/// le point, l'aperçu montre le nombre de fichiers supprimés).
pub fn perimer_tous_les_points(c: &Chemins, cause: &str) {
    if let Err(e) = marquer_perimes(c) {
        journal::noter(
            "marquer les points de restauration périmés",
            &format!("{cause} : {e}"),
        );
    }
}

/// Marque périmés les points complets plus récents que `point` (ordre de [`cle_de_tri`]) ;
/// rend combien l'ont été. Appelée par [`revenir`] : après un retour vers `point`, les fichiers
/// apparus depuis ont disparu, et les points créés ensuite (installation déjà défaite, par
/// exemple) remettraient les réglages d'un environnement qui n'existe plus. `point` lui-même
/// reste valable. Toutes les files sont concernées : elles partagent le même préfixe.
/// Erreur : [`Erreur::Io`].
fn marquer_perimes_apres(c: &Chemins, point: &PointRestauration) -> Resultat<usize> {
    let limite = cle_de_tri(&point.id);
    let plus_recents: Vec<PointRestauration> = lister(c)?
        .points
        .into_iter()
        .filter(|p| cle_de_tri(&p.id) > limite)
        .collect();
    for p in &plus_recents {
        std::fs::write(p.dossier.join(FICHIER_PERIME), b"")?;
    }
    Ok(plus_recents.len())
}

/// Liste les points, du plus récent au plus ancien, et signale les dossiers incomplets.
///
/// Un dossier sans `fichiers.txt` (création interrompue) ne peut pas servir : il va
/// dans [`ListePoints::incomplets`] au lieu d'être ignoré en silence. Aucun point
/// (dossier absent) donne une liste vide. Un point complet qui contient `FICHIER_PERIME` va
/// dans [`ListePoints::perimes`]. Erreur : [`Erreur::Io`].
pub fn lister(c: &Chemins) -> Resultat<ListePoints> {
    let mut liste = ListePoints {
        points: Vec::new(),
        perimes: Vec::new(),
        incomplets: Vec::new(),
    };
    let entrees = match std::fs::read_dir(dossier_points(c)) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(liste),
        Err(e) => return Err(e.into()),
    };
    for entree in entrees {
        let entree = entree?;
        let dossier = entree.path();
        if !entree.file_type()?.is_dir() {
            continue;
        }
        let id = entree.file_name().to_string_lossy().into_owned();
        // Un motif illisible n'empêche pas de revenir en arrière.
        let motif = std::fs::read_to_string(dossier.join(FICHIER_MOTIF)).unwrap_or_default();
        let point = PointRestauration { id, motif, dossier };
        if verifier_point(&point).is_err() {
            liste.incomplets.push(point.dossier);
            continue;
        }
        if point.dossier.join(FICHIER_PERIME).exists() {
            liste.perimes.push(point);
        } else {
            liste.points.push(point);
        }
    }
    for groupe in [&mut liste.points, &mut liste.perimes] {
        groupe.sort_by_key(|p| std::cmp::Reverse(cle_de_tri(&p.id)));
    }
    liste.incomplets.sort();
    Ok(liste)
}

/// Clé de tri chronologique : l'identifiant de base (l'ordre alphabétique est
/// l'ordre du temps) puis le numéro de suffixe, comparé comme nombre (`-10` après `-9`).
fn cle_de_tri(id: &str) -> (String, u32) {
    // La base fait toujours 19 caractères ASCII : AAAA-MM-JJ-HHhMMmSS.
    match id.split_at_checked(19) {
        Some((base, suffixe)) => {
            let numero = suffixe
                .strip_prefix('-')
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            (base.to_string(), numero)
        }
        None => (id.to_string(), 1),
    }
}

/// Liste exacte des entrées que [`revenir`] supprimerait, en chemins complets.
///
/// Ce sont les fichiers et liens symboliques du préfixe absents de `fichiers.txt`.
/// Un lien symbolique est listé lui-même (seul le lien serait supprimé, jamais sa
/// cible). L'interface la montre pour demander confirmation. Ne modifie rien.
/// Erreurs : [`Erreur::Introuvable`] si `fichiers.txt` manque ; [`Erreur::Io`].
pub fn apercu_retour(c: &Chemins, point: &PointRestauration) -> Resultat<Vec<PathBuf>> {
    let connus = lire_liste(point)?;
    Ok(relever_prefixe(&c.prefixe)?
        .fichiers
        .into_iter()
        .filter(|relatif| !connus.fichiers.contains(relatif.as_os_str().as_bytes()))
        .map(|relatif| c.prefixe.join(relatif))
        .collect())
}

/// Ce que [`revenir`] a fait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BilanRetour {
    /// Entrées du préfixe supprimées (fichiers et liens apparus depuis le point).
    pub fichiers_supprimes: usize,
    /// Liens cassés retirés des dossiers yabridge (voir [`retirer_liens_casses`]) ; 0 aussi si
    /// ce nettoyage a échoué (l'échec est noté au journal).
    pub liens_retires: usize,
}

/// Remet l'environnement dans l'état de `point` ; rend le [`BilanRetour`].
///
/// `confirmes` est la liste montrée à l'utilisateur ([`apercu_retour`]) et acceptée par
/// lui : seule l'intersection entre elle et l'aperçu recalculé est supprimée. Un fichier
/// apparu après l'aperçu n'est donc jamais supprimé sans confirmation.
///
/// Étapes : vérifie le point (3 `.reg` et liste valide : sinon rien n'est touché) et
/// qu'aucun plugin ne tourne, arrête le `wineserver`, recalcule l'aperçu (Wine a pu
/// écrire pendant l'arrêt), remet les `.reg` (copie temporaire puis renommage), puis
/// supprime les entrées confirmées et les dossiers laissés vides. Aucun lien symbolique
/// n'est traversé : le relevé n'y descend pas, et chaque dossier parent est revérifié
/// juste avant la suppression.
///
/// Un point périmé ([`marquer_perimes`]) est refusé tout de suite, avant même la recherche de
/// plugins. Le verrou d'installation est ensuite pris pour toute la durée du retour
/// ([`Erreur::InstallationEnCours`] s'il est tenu, sans rien toucher) : aucune installation,
/// sauvegarde, import ou annulation ne peut se dérouler en même temps. Une fois les fichiers supprimés, les points plus récents que `point` sont marqués
/// périmés (`marquer_perimes_apres`) et les liens yabridge devenus cassés sont retirés
/// ([`retirer_liens_casses`]) ; l'échec de ce nettoyage n'est qu'une note au journal.
///
/// Une suppression qui échoue n'interrompt pas les autres (un retour à moitié fait est
/// pire que complet) ; à la fin, [`Erreur::RetourIncomplet`] dit combien ont été
/// supprimées et lesquelles ont échoué. Une entrée déjà absente n'est pas un échec.
///
/// Erreurs : [`Erreur::PointPerime`] ; [`Erreur::PluginsEnCours`] ;
/// [`Erreur::InstallationEnCours`] ; [`Erreur::Introuvable`] (point incomplet) ;
/// [`Erreur::Io`] ; [`Erreur::RetourIncomplet`] ; erreurs de [`Wine::arreter`].
pub fn revenir(
    c: &Chemins,
    point: &PointRestauration,
    confirmes: &[PathBuf],
) -> Resultat<BilanRetour> {
    revenir_dans(c, point, confirmes, Path::new(RACINE_PROC))
}

/// Cœur de [`revenir`] avec la racine de `/proc` injectée.
fn revenir_dans(
    c: &Chemins,
    point: &PointRestauration,
    confirmes: &[PathBuf],
    proc: &Path,
) -> Resultat<BilanRetour> {
    // Avant toute autre opération : un point périmé ne doit rien toucher, pas même arrêter Wine.
    if point.dossier.join(FICHIER_PERIME).exists() {
        return Err(Erreur::PointPerime(point.id.clone()));
    }
    exiger_aucun_plugin(proc, c)?;
    // Verrou d'installation, gardé jusqu'à la fin du retour (Drop) : l'import et son annulation
    // déplacent le préfixe entier sous ce verrou. Sans lui, un retour lancé pendant ce temps
    // (depuis la CLI, par exemple) trouverait le préfixe absent, et `retirer_liens_casses`
    // prendrait alors pour cassés tous les liens yabridge vers lui. Refus préalable, comme le
    // plugin en cours : rien n'est touché ni écrit dans l'historique.
    let _verrou = poser_verrou_installation(c)?;
    // À partir d'ici le retour est engagé : une seule entrée d'historique, quelle que soit l'issue.
    let resultat = revenir_engage(c, point, confirmes);
    // Détail neutre en cas de réussite (nombre de fichiers retirés, pluriel géré à l'affichage) ;
    // la cause d'un échec reste du texte libre.
    let (issue, details_neutre, details_libre) = match &resultat {
        Ok(bilan) => (
            ResultatAction::Reussi,
            Some(TexteNeutre::nouveau(
                historique::CLE_FICHIERS_RETIRES,
                &[("n", &bilan.fichiers_supprimes.to_string())],
            )),
            String::new(),
        ),
        Err(e) => (ResultatAction::Echec, None, e.to_string()),
    };
    // Motif connu : sujet neutre ; motif inconnu (point d'une version future) : texte brut.
    match motif_neutre(&point.motif) {
        Some(sujet) => historique::ajouter_neutre(
            c,
            Action::Retour,
            &sujet,
            issue,
            None,
            details_neutre.as_ref(),
            &details_libre,
        ),
        None => historique::ajouter(
            c,
            Action::Retour,
            &libelle_motif(&point.motif),
            issue,
            None,
            &details_neutre.map_or(details_libre, |d| d.en_clair()),
        ),
    }
    resultat
}

/// Le retour proprement dit, une fois les refus préalables passés (point périmé, plugin en
/// cours, verrou d'installation tenu ailleurs). Séparé de [`revenir_dans`] pour que l'historique s'écrive à un seul endroit.
fn revenir_engage(
    c: &Chemins,
    point: &PointRestauration,
    confirmes: &[PathBuf],
) -> Resultat<BilanRetour> {
    let anciens = verifier_point(point)?.dossiers;
    Wine::nouveau(c).arreter()?;
    let autorises: HashSet<&PathBuf> = confirmes.iter().collect();
    let a_supprimer: Vec<PathBuf> = apercu_retour(c, point)?
        .into_iter()
        .filter(|chemin| autorises.contains(chemin))
        .collect();
    for nom in FICHIERS_REG {
        let source = point.dossier.join(nom);
        copier_atomique(&source, &c.prefixe.join(nom)).map_err(|e| contexte_reg(&source, e))?;
    }
    let (mut supprimes, mut echecs) = (0, Vec::new());
    for chemin in a_supprimer {
        match supprimer_entree(&c.prefixe, &chemin) {
            Ok(true) => {
                supprimes += 1;
                retirer_dossiers_vides(&c.prefixe, &chemin, &anciens);
            }
            Ok(false) => {}
            Err(_) => echecs.push(chemin),
        }
    }
    // Les points plus récents décrivent un environnement qui n'existe plus (leurs fichiers
    // viennent d'être supprimés). Même après un retour incomplet, y revenir mélangerait deux
    // états : on les périme. Un échec n'est qu'une note : le retour lui-même est fait.
    if let Err(e) = marquer_perimes_apres(c, point) {
        journal::noter("marquer périmés les points plus récents", &e.to_string());
    }
    // Les plugins supprimés laissent des liens yabridge cassés. Un échec ici ne doit pas
    // masquer que le retour lui-même a réussi : on le note et on compte 0.
    let liens_retires = retirer_liens_casses(c).unwrap_or_else(|e| {
        journal::noter("retirer les liens yabridge cassés", &e.to_string());
        0
    });
    if echecs.is_empty() {
        Ok(BilanRetour {
            fichiers_supprimes: supprimes,
            liens_retires,
        })
    } else {
        Err(Erreur::RetourIncomplet { supprimes, echecs })
    }
}

/// Supprime une entrée du préfixe. `Ok(false)` si elle avait déjà disparu.
/// Refuse (erreur) si un dossier entre le préfixe et l'entrée n'est pas un vrai dossier
/// : `remove_file` suivrait sinon le lien intermédiaire hors du préfixe.
fn supprimer_entree(prefixe: &Path, chemin: &Path) -> std::io::Result<bool> {
    verifier_parents(prefixe, chemin)?;
    match std::fs::remove_file(chemin) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Vérifie, sans suivre de lien, que chaque dossier entre `prefixe` (exclu) et `chemin`
/// (exclu) est un vrai dossier.
fn verifier_parents(prefixe: &Path, chemin: &Path) -> std::io::Result<()> {
    for parent in chemin.ancestors().skip(1) {
        if parent == prefixe || !parent.starts_with(prefixe) {
            break;
        }
        match std::fs::symlink_metadata(parent) {
            Ok(m) if m.file_type().is_dir() => {}
            Ok(_) => {
                return Err(std::io::Error::other(trf(
                    "{path} is not a real folder (symbolic link?)",
                    &[("path", &parent.display().to_string())],
                )))
            }
            // Dossier disparu : l'entrée l'est aussi (traité comme déjà supprimée).
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Supprime les dossiers parents de `chemin`, du plus profond vers le préfixe (exclu),
/// tant qu'ils sont vides ET absents du point (`anciens` : ceux qui existaient déjà
/// restent, même vides). S'arrête au premier dossier non vide : `remove_dir` refuse
/// de supprimer un dossier qui contient quelque chose, c'est la protection.
fn retirer_dossiers_vides(prefixe: &Path, chemin: &Path, anciens: &HashSet<Vec<u8>>) {
    for parent in chemin.ancestors().skip(1) {
        let ancien = parent
            .strip_prefix(prefixe)
            .map_or(true, |rel| anciens.contains(rel.as_os_str().as_bytes()));
        if ancien || std::fs::remove_dir(parent).is_err() {
            break;
        }
    }
}

/// Identifiant de base `AAAA-MM-JJ-HHhMMmSS` à l'heure locale actuelle. Partagé avec
/// `import` (noms de la petite sauvegarde et de la trace annulée).
pub(crate) fn identifiant_local() -> Resultat<String> {
    // SAFETY : `time` accepte un pointeur nul ; `localtime_r` écrit seulement dans
    // `tm` (structure de C, tout à zéro est valide) et lit `maintenant`.
    let tm = unsafe {
        let maintenant = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&maintenant, &mut tm).is_null() {
            return Err(Erreur::Io(std::io::Error::last_os_error()));
        }
        tm
    };
    Ok(format!(
        "{:04}-{:02}-{:02}-{:02}h{:02}m{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{symlink, PermissionsExt};

    /// Faux environnement : préfixe avec les trois `.reg` et un faux `wineserver`
    /// (script qui réussit) pour que `Wine::arreter` ne lance pas le vrai Wine.
    fn faux_environnement(racine: &Path) -> Chemins {
        let mut c = Chemins::pour_test(racine);
        c.wine_dir = racine.join("wine");
        let bin = c.wine_bin();
        std::fs::create_dir_all(&bin).unwrap();
        let script = bin.join("wineserver");
        crate::test_utils::ecrire_executable(
            &script,
            "#!/bin/sh
exit 0
",
        );
        std::fs::create_dir_all(c.drive_c().join("Program Files")).unwrap();
        for nom in FICHIERS_REG {
            std::fs::write(c.prefixe.join(nom), format!("original {nom}")).unwrap();
        }
        c
    }

    /// `/proc` factice : un processus `pid` avec cette ligne de commande et ce prefixe.
    fn faux_proc(proc: &Path, pid: &str, commande: &str, prefixe: &Path) {
        let d = proc.join(pid);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), commande.replace(' ', "\0")).unwrap();
        let env = format!("HOME=/h\0WINEPREFIX={}\0", prefixe.display());
        std::fs::write(d.join("environ"), env).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn les_motifs_connus_sont_traduits_et_les_autres_gardes() {
        crate::i18n::init_langue(Some("fr"));
        assert_eq!(libelle_motif(MOTIF_REPARATION), "avant une réparation");
        assert_eq!(
            libelle_motif(&motif_installation("Serum2 Setup.exe")),
            "avant l'installation de Serum2 Setup.exe"
        );
        // Point enregistré par une version antérieure : même format, même traduction.
        assert_eq!(
            libelle_motif("install a.msi"),
            "avant l'installation de a.msi"
        );
        assert_eq!(
            libelle_motif(&motif_session("Native Access")),
            "avant la session Native Access"
        );
        assert_eq!(libelle_motif("autre chose"), "autre chose");
        crate::i18n::init_langue(Some("en"));
        assert_eq!(libelle_motif("repair"), "before a repair");
    }

    #[test]
    fn revenir_supprime_ce_qui_est_apparu_et_remet_le_registre() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "avant installation").unwrap();
        assert_eq!(point.motif, "avant installation");

        let dossier_x = c.drive_c().join("Program Files/X !");
        std::fs::create_dir_all(&dossier_x).unwrap();
        std::fs::write(dossier_x.join("a.dll"), "x").unwrap();
        std::fs::write(c.prefixe.join("user.reg"), "modifié").unwrap();

        assert_eq!(
            apercu_retour(&c, &point).unwrap(),
            vec![dossier_x.join("a.dll")]
        );
        let apercu = apercu_retour(&c, &point).unwrap();
        assert_eq!(revenir(&c, &point, &apercu).unwrap().fichiers_supprimes, 1);
        assert!(!dossier_x.exists(), "le dossier devenu vide est supprimé");
        assert!(c.drive_c().join("Program Files").is_dir());
        assert_eq!(
            std::fs::read_to_string(c.prefixe.join("user.reg")).unwrap(),
            "original user.reg"
        );
    }

    #[test]
    fn revenir_ajoute_une_entree_retour() {
        use crate::historique::{lire, Action, ResultatAction};
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "avant installation").unwrap();
        let nouveau = c.drive_c().join("Program Files/nouveau.dll");
        std::fs::write(&nouveau, "x").unwrap();
        let apercu = apercu_retour(&c, &point).unwrap();
        revenir(&c, &point, &apercu).unwrap();
        let entrees = lire(&c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].action, Action::Retour);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
        assert_eq!(entrees[0].sujet, libelle_motif(&point.motif));
        // Le retour ne crée aucun point : il n'y a donc rien à rattacher à l'entrée.
        assert_eq!(entrees[0].point, None);
        assert!(!entrees[0].details.is_empty(), "nombre de fichiers attendu");
    }

    #[test]
    fn revenir_ecrit_un_sujet_et_des_details_neutres() {
        use crate::historique::{lire, CLES_CONNUES};
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        // Motif d'installation : connu, donc neutre.
        let point = creer(&c, &motif_installation("Setup.exe")).unwrap();
        std::fs::write(c.drive_c().join("Program Files/nouveau.dll"), "x").unwrap();
        let apercu = apercu_retour(&c, &point).unwrap();
        revenir(&c, &point, &apercu).unwrap();
        let e = &lire(&c).unwrap()[0];
        let sujet = e.sujet_neutre.as_ref().expect("sujet neutre");
        let details = e.details_neutre.as_ref().expect("détails neutres");
        assert!(CLES_CONNUES.contains(&sujet.cle.as_str()), "{sujet:?}");
        assert!(CLES_CONNUES.contains(&details.cle.as_str()), "{details:?}");
        assert_eq!(sujet.valeurs["file"], "Setup.exe");
        assert_eq!(details.valeurs["n"], "1");
        assert_eq!(e.sujet_affiche(), libelle_motif(&point.motif));
    }

    #[test]
    fn motif_inconnu_n_a_pas_de_texte_neutre() {
        assert!(motif_neutre("autre chose").is_none());
        assert!(motif_neutre(MOTIF_REPARATION).is_some());
        assert!(motif_neutre(&motif_session("Native Access")).is_some());
    }

    /// Défaut trouvé à la vérification réelle du sous-projet 3 : après un retour vers un point
    /// ancien, les points plus récents restaient proposés (« Revenir à avant ceci » sur une
    /// installation déjà défaite). Y revenir remettrait les réglages d'un environnement dont les
    /// fichiers ont été supprimés : ils sont désormais périmés, le point utilisé reste valable.
    #[test]
    fn revenir_rend_perimes_les_points_plus_recents() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let ancien = creer(&c, "avant la première installation").unwrap();
        std::fs::write(c.drive_c().join("Program Files/premier.dll"), "x").unwrap();
        let recent = creer(&c, "avant la seconde installation").unwrap();
        std::fs::write(c.drive_c().join("Program Files/second.dll"), "x").unwrap();

        let apercu = apercu_retour(&c, &ancien).unwrap();
        revenir(&c, &ancien, &apercu).unwrap();

        let liste = lister(&c).unwrap();
        let ids = |points: &[PointRestauration]| -> Vec<String> {
            points.iter().map(|p| p.id.clone()).collect()
        };
        assert_eq!(ids(&liste.points), vec![ancien.id.clone()]);
        assert_eq!(ids(&liste.perimes), vec![recent.id.clone()]);
        // Le point utilisé reste valable : y revenir encore une fois est permis.
        revenir(&c, &ancien, &[]).unwrap();
    }

    /// Relecture finale M2 : l'import et son annulation déplacent le préfixe entier sous le
    /// verrou d'installation. Un retour lancé pendant ce temps (autre processus) verrait le
    /// préfixe absent et prendrait tous les liens yabridge pour cassés : il est refusé avant
    /// toute action, sans entrée d'historique.
    #[test]
    fn revenir_pendant_une_operation_verrouillee_est_refuse_sans_rien_toucher() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "avant").unwrap();
        let nouveau = c.drive_c().join("Program Files/nouveau.dll");
        std::fs::write(&nouveau, "x").unwrap();
        std::fs::write(c.prefixe.join("user.reg"), "modifié").unwrap();
        let _verrou = crate::diagnostic::poser_verrou_installation(&c).unwrap();
        let err = revenir(&c, &point, std::slice::from_ref(&nouveau)).unwrap_err();
        assert!(matches!(err, Erreur::InstallationEnCours), "{err:?}");
        assert!(nouveau.exists(), "rien ne doit être supprimé");
        assert_eq!(
            std::fs::read_to_string(c.prefixe.join("user.reg")).unwrap(),
            "modifié"
        );
        assert!(crate::historique::lire(&c).unwrap().is_empty());
    }

    #[test]
    fn un_point_perime_refuse_n_ecrit_rien_dans_l_historique() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "avant").unwrap();
        marquer_perimes(&c).unwrap();
        revenir(&c, &point, &[]).unwrap_err();
        assert!(crate::historique::lire(&c).unwrap().is_empty());
    }

    #[test]
    fn douze_points_donnent_dix() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        for i in 0..12 {
            creer(&c, &format!("point {i}")).unwrap();
        }
        let liste = lister(&c).unwrap();
        assert_eq!(liste.points.len(), 10);
        assert!(liste.incomplets.is_empty());
        // Le plus récent d'abord : le dernier créé est en tête.
        let ids: Vec<_> = liste.points.iter().map(|p| (&p.id, &p.motif)).collect();
        assert_eq!(liste.points[0].motif, "point 11", "{ids:?}");
        assert_eq!(liste.points[9].motif, "point 2");
    }

    #[test]
    fn onze_points_reparation_ne_chassent_aucun_point_installation() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        for i in 0..2 {
            creer(&c, &motif_installation(&format!("setup{i}.exe"))).unwrap();
        }
        for _ in 0..11 {
            creer(&c, MOTIF_REPARATION).unwrap();
        }
        let liste = lister(&c).unwrap();
        let installations = liste
            .points
            .iter()
            .filter(|p| p.file() == FilePoints::Installation)
            .count();
        let reparations = liste
            .points
            .iter()
            .filter(|p| p.file() == FilePoints::Reparation)
            .count();
        assert_eq!((installations, reparations), (2, 3));
    }

    #[test]
    fn points_perimes_sont_listes_a_part() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        creer(&c, "a").unwrap();
        creer(&c, "b").unwrap();
        assert_eq!(marquer_perimes(&c).unwrap(), 2);
        let liste = lister(&c).unwrap();
        assert!(liste.points.is_empty());
        assert_eq!(liste.perimes.len(), 2);
        assert!(liste.incomplets.is_empty());
        // Un second appel ne trouve plus aucun point à marquer.
        assert_eq!(marquer_perimes(&c).unwrap(), 0);
    }

    #[test]
    fn les_perimes_comptent_dans_l_elagage_de_leur_file() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        for i in 0..10 {
            creer(&c, &format!("p{i}")).unwrap();
        }
        marquer_perimes(&c).unwrap();
        // Dix périmés + un neuf : le plus ancien périmé est élagué, le neuf est gardé.
        creer(&c, "neuf").unwrap();
        let liste = lister(&c).unwrap();
        assert_eq!(liste.points.len(), 1);
        assert_eq!(liste.perimes.len(), 9);
        assert!(liste.perimes.iter().all(|p| p.motif != "p0"));
    }

    #[test]
    fn revenir_a_un_point_perime_est_refuse_sans_rien_toucher() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "avant").unwrap();
        let nouveau = c.drive_c().join("Program Files/nouveau.dll");
        std::fs::write(&nouveau, "x").unwrap();
        marquer_perimes(&c).unwrap();
        let err = revenir(&c, &point, std::slice::from_ref(&nouveau)).unwrap_err();
        assert!(matches!(err, Erreur::PointPerime(_)), "{err:?}");
        assert!(nouveau.exists(), "rien ne doit être touché");
    }

    #[test]
    fn revenir_retire_les_liens_casses_vers_le_prefixe() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "avant").unwrap();
        // Un plugin apparaît après le point, avec son lien yabridge.
        let dll = c.drive_c().join("Program Files/VstPlugins/X_x64.dll");
        std::fs::create_dir_all(dll.parent().unwrap()).unwrap();
        std::fs::write(&dll, "x").unwrap();
        let dossier = c.home.join(".vst/yabridge/VstPlugins");
        std::fs::create_dir_all(&dossier).unwrap();
        symlink(&dll, dossier.join("X_x64.dll")).unwrap();
        std::fs::write(dossier.join("X_x64.so"), "pont").unwrap();

        let apercu = apercu_retour(&c, &point).unwrap();
        let bilan = revenir(&c, &point, &apercu).unwrap();
        assert_eq!(bilan.fichiers_supprimes, 1);
        assert_eq!(bilan.liens_retires, 1);
        assert!(!dossier.join("X_x64.so").exists());
    }

    #[test]
    fn meme_seconde_ne_s_ecrase_pas_et_l_ordre_est_numerique() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let proc = d.path().join("proc");
        std::fs::create_dir_all(&proc).unwrap();
        let base = "2026-10-31-12h00m00";
        for i in 0..11 {
            creer_dans(&c, &format!("p{i}"), base, &proc).unwrap();
        }
        let ids: Vec<String> = lister(&c)
            .unwrap()
            .points
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(ids[0], format!("{base}-11"), "{ids:?}");
        assert_eq!(ids[1], format!("{base}-10"), "{ids:?}");
        assert_eq!(ids.len(), 10);
        // Le premier (sans suffixe) a été élagué, pas écrasé.
        assert!(!ids.contains(&base.to_string()));
    }

    #[test]
    fn dossier_incomplet_est_signale() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        creer(&c, "ok").unwrap();
        let casse = dossier_points(&c).join("2000-01-01-00h00m00");
        std::fs::create_dir_all(&casse).unwrap();
        std::fs::write(casse.join(FICHIER_MOTIF), "x").unwrap();
        let liste = lister(&c).unwrap();
        assert_eq!(liste.points.len(), 1);
        assert_eq!(liste.incomplets, vec![casse]);
    }

    #[test]
    fn creer_sans_reg_echoue_et_ne_laisse_rien() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        std::fs::remove_file(c.prefixe.join("userdef.reg")).unwrap();
        assert!(matches!(creer(&c, "x"), Err(Erreur::Introuvable(_))));
        let liste = lister(&c).unwrap();
        assert!(liste.points.is_empty() && liste.incomplets.is_empty());
    }

    #[test]
    fn les_liens_symboliques_ne_sont_jamais_suivis() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        // « Vrai HOME » : hors du préfixe, avec un fichier précieux.
        let dehors = d.path().join("vrai_home");
        std::fs::create_dir_all(&dehors).unwrap();
        std::fs::write(dehors.join("precieux.txt"), "ne pas toucher").unwrap();
        let utilisateur = c.drive_c().join("users/u");
        std::fs::create_dir_all(&utilisateur).unwrap();
        symlink(&dehors, utilisateur.join("Documents")).unwrap();

        let point = creer(&c, "avec liens").unwrap();
        // Après le point : un fichier arrive derrière le lien, et un nouveau lien apparaît.
        std::fs::write(dehors.join("nouveau.txt"), "aussi précieux").unwrap();
        let autre = d.path().join("autre_dehors");
        std::fs::create_dir_all(&autre).unwrap();
        std::fs::write(autre.join("f.txt"), "intact").unwrap();
        symlink(&autre, utilisateur.join("Musique")).unwrap();

        assert_eq!(
            apercu_retour(&c, &point).unwrap(),
            vec![utilisateur.join("Musique")]
        );
        let apercu = apercu_retour(&c, &point).unwrap();
        assert_eq!(revenir(&c, &point, &apercu).unwrap().fichiers_supprimes, 1);
        assert!(std::fs::symlink_metadata(utilisateur.join("Musique")).is_err());
        assert!(std::fs::symlink_metadata(utilisateur.join("Documents")).is_ok());
        for fichier in [
            dehors.join("precieux.txt"),
            dehors.join("nouveau.txt"),
            autre.join("f.txt"),
        ] {
            assert!(fichier.exists(), "{fichier:?} doit exister encore");
        }
    }

    /// Fabrique un point, ajoute un fichier au préfixe et modifie `user.reg` : état
    /// qu'un `revenir` abusif détruirait.
    fn point_et_changements(c: &Chemins) -> (PointRestauration, PathBuf) {
        let point = creer(c, "t").unwrap();
        let nouveau = c.drive_c().join("nouveau.txt");
        std::fs::write(&nouveau, "x").unwrap();
        std::fs::write(c.prefixe.join("user.reg"), "modifié").unwrap();
        (point, nouveau)
    }

    /// Rien ne doit avoir bougé dans le préfixe après un refus.
    fn assert_prefixe_intact(c: &Chemins, nouveau: &Path) {
        assert!(nouveau.exists());
        assert!(c.drive_c().join("Program Files").is_dir());
        assert_eq!(
            std::fs::read_to_string(c.prefixe.join("user.reg")).unwrap(),
            "modifié"
        );
    }

    #[test]
    fn liste_vide_tronquee_ou_sans_reg_est_refusee_sans_rien_toucher() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let (point, nouveau) = point_et_changements(&c);
        let liste = point.dossier.join(FICHIER_LISTE);
        let valide = std::fs::read(&liste).unwrap();
        let sans_reg: Vec<u8> = [b"drive_c/\0".as_slice(), MARQUE_FIN, b"1\0"].concat();
        let cas: [(&str, Vec<u8>); 4] = [
            ("vide", Vec::new()),
            ("tronquée", valide[..valide.len() - 4].to_vec()),
            ("coupée en plein nom", valide[..valide.len() / 2].to_vec()),
            ("sans les .reg", sans_reg),
        ];
        for (nom, contenu) in cas {
            std::fs::write(&liste, contenu).unwrap();
            let r = revenir(&c, &point, std::slice::from_ref(&nouveau));
            assert!(matches!(r, Err(Erreur::Introuvable(_))), "{nom} : {r:?}");
            assert_prefixe_intact(&c, &nouveau);
            assert!(apercu_retour(&c, &point).is_err(), "{nom}");
            let l = lister(&c).unwrap();
            assert!(l.points.is_empty() && l.incomplets.len() == 1, "{nom}");
        }
    }

    #[test]
    fn point_sans_un_reg_est_incomplet_et_refuse() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let (point, nouveau) = point_et_changements(&c);
        std::fs::remove_file(point.dossier.join("userdef.reg")).unwrap();
        assert!(matches!(
            revenir(&c, &point, std::slice::from_ref(&nouveau)),
            Err(Erreur::Introuvable(_))
        ));
        assert_prefixe_intact(&c, &nouveau);
        assert_eq!(lister(&c).unwrap().incomplets.len(), 1);
    }

    #[test]
    fn un_fichier_apparu_apres_l_apercu_est_conserve() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "t").unwrap();
        let confirme = c.drive_c().join("confirme.txt");
        std::fs::write(&confirme, "x").unwrap();
        let apercu = apercu_retour(&c, &point).unwrap();
        assert_eq!(apercu, vec![confirme.clone()]);
        let tardif = c.drive_c().join("tardif.txt");
        std::fs::write(&tardif, "y").unwrap();
        assert_eq!(revenir(&c, &point, &apercu).unwrap().fichiers_supprimes, 1);
        assert!(!confirme.exists());
        assert!(tardif.exists());
    }

    #[test]
    fn dossier_intermediaire_remplace_par_un_lien_est_refuse() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "t").unwrap();
        let dossier = c.drive_c().join("Program Files/X");
        std::fs::create_dir_all(&dossier).unwrap();
        std::fs::write(dossier.join("a.dll"), "x").unwrap();
        let apercu = apercu_retour(&c, &point).unwrap();
        // Entre l'aperçu et `revenir`, le dossier devient un lien vers l'extérieur.
        let dehors = d.path().join("dehors");
        std::fs::create_dir_all(&dehors).unwrap();
        std::fs::write(dehors.join("a.dll"), "précieux").unwrap();
        std::fs::remove_dir_all(&dossier).unwrap();
        symlink(&dehors, &dossier).unwrap();
        let _ = revenir(&c, &point, &apercu);
        assert!(dehors.join("a.dll").exists());
        // Et la vérification des parents refuse directement le chemin abusif.
        assert!(verifier_parents(&c.prefixe, &dossier.join("a.dll")).is_err());
    }

    #[test]
    fn echec_de_suppression_continue_et_rapporte() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "t").unwrap();
        // Dossier non modifiable : on ne peut pas y supprimer b.txt (sauf root).
        let verrouille = c.drive_c().join("verrou");
        std::fs::create_dir_all(&verrouille).unwrap();
        std::fs::write(verrouille.join("b.txt"), "x").unwrap();
        let libre = c.drive_c().join("libre.txt");
        std::fs::write(&libre, "x").unwrap();
        let apercu = apercu_retour(&c, &point).unwrap();
        std::fs::set_permissions(&verrouille, std::fs::Permissions::from_mode(0o555)).unwrap();
        let r = revenir(&c, &point, &apercu);
        std::fs::set_permissions(&verrouille, std::fs::Permissions::from_mode(0o755)).unwrap();
        if unsafe { libc::geteuid() } == 0 {
            return; // root ignore les permissions : le test n'a pas de sens
        }
        match r {
            Err(Erreur::RetourIncomplet { supprimes, echecs }) => {
                assert_eq!(supprimes, 1);
                assert_eq!(echecs, vec![verrouille.join("b.txt")]);
            }
            autre => panic!("{autre:?}"),
        }
        assert!(!libre.exists(), "les autres suppressions ont eu lieu");
    }

    #[test]
    fn nom_avec_saut_de_ligne() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let bizarre = c.drive_c().join("a\nb.txt");
        std::fs::write(&bizarre, "x").unwrap();
        let point = creer(&c, "nl").unwrap();
        assert!(apercu_retour(&c, &point).unwrap().is_empty());
        let autre = c.drive_c().join("a");
        std::fs::write(&autre, "y").unwrap();
        assert_eq!(apercu_retour(&c, &point).unwrap(), vec![autre]);
    }

    #[test]
    fn detection_des_plugins_en_cours() {
        let d = tempfile::tempdir().unwrap();
        let proc = d.path().join("proc");
        let prefixe = Path::new("/donnees/prefixe");
        std::fs::create_dir_all(&proc).unwrap();
        assert!(!plugins_en_cours_dans(&proc, &[prefixe.to_path_buf()]).unwrap());
        // Autre commande sur notre préfixe, et yabridge sur un autre préfixe : pas concernés.
        faux_proc(&proc, "10", "wine explorer.exe", prefixe);
        faux_proc(
            &proc,
            "11",
            "wine yabridge-host.exe",
            Path::new("/autre/prefixe"),
        );
        // Processus illisible (ni cmdline ni environ) et entrée non numérique : ignorés.
        std::fs::create_dir_all(proc.join("12")).unwrap();
        std::fs::write(proc.join("uptime"), "1").unwrap();
        assert!(!plugins_en_cours_dans(&proc, &[prefixe.to_path_buf()]).unwrap());
        faux_proc(&proc, "13", "wine yabridge-host.exe.so", prefixe);
        assert!(plugins_en_cours_dans(&proc, &[prefixe.to_path_buf()]).unwrap());
    }

    #[test]
    fn plugin_en_cours_bloque_creer_et_revenir_sans_rien_toucher() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let point = creer(&c, "ok").unwrap();
        let proc = d.path().join("proc");
        faux_proc(&proc, "5", "yabridge-host.exe", &c.prefixe);
        std::fs::write(c.prefixe.join("user.reg"), "modifié").unwrap();

        let e = creer_dans(&c, "x", "2026-01-01-00h00m00", &proc).unwrap_err();
        assert!(matches!(e, Erreur::PluginsEnCours));
        assert!(matches!(
            revenir_dans(&c, &point, &[], &proc),
            Err(Erreur::PluginsEnCours)
        ));
        assert_eq!(lister(&c).unwrap().points.len(), 1);
        assert_eq!(
            std::fs::read_to_string(c.prefixe.join("user.reg")).unwrap(),
            "modifié"
        );
    }

    #[test]
    fn identifiant_au_bon_format() {
        let id = identifiant_local().unwrap();
        assert_eq!(id.len(), 19, "{id}");
        assert_eq!(&id[10..11], "-");
        assert_eq!(&id[13..14], "h");
        assert_eq!(&id[16..17], "m");
    }

    #[test]
    fn nom_non_utf8_est_conserve() {
        let d = tempfile::tempdir().unwrap();
        let c = faux_environnement(d.path());
        let nom = std::ffi::OsString::from_vec(vec![b'n', 0xFF, b'.', b'x']);
        std::fs::write(c.drive_c().join(&nom), "x").unwrap();
        let point = creer(&c, "octets").unwrap();
        assert!(apercu_retour(&c, &point).unwrap().is_empty());
    }
}
