//! Sauvegarde et restauration de l'environnement Wine (plugins, licences, bibliothèques).
//!
//! Comportement repris de `scripts/sauvegarde-prefixe.sh`, éprouvé à la main :
//!
//! - refus tant qu'un plugin tourne ([`Erreur::PluginsEnCours`]) ou qu'une
//!   installation / réparation tient le verrou ([`Erreur::InstallationEnCours`]) ; le
//!   verrou `installation.lock` est tenu pendant toute la sauvegarde et la restauration,
//!   puis le `wineserver` est arrêté (aucun programme Windows ne doit écrire pendant la copie) ;
//! - place libre de la destination vérifiée **avant** (taille du préfixe hors
//!   exclusions, contre `statvfs`) ;
//! - exclus : `Temp`, `Downloads` et `dxvk` des utilisateurs Windows (réinstallables) ;
//! - dossier `WiMao-sauvegarde`, archive `wimao-AAAA-MM-JJ-HHhMM.tar.zst` (l'heure
//!   évite d'écraser une sauvegarde précédente : une sauvegarde existante est un refus) ;
//! - disque FAT32 (4 Go maximum par fichier) : archive découpée en morceaux
//!   `.part000`, `.part001`… de 3900 Mo.
//!
//! # Écarts voulus avec le script
//!
//! - Le découpage et les empreintes sont faits **en Rust** : `tar -cf - | zstd` sont deux
//!   processus enfants (même groupe), la sortie de `zstd` est lue ici, découpée, écrite et
//!   hachée (SHA-256) au fil de l'eau. Pas de `split` ni de `sha256sum` : on contrôle les
//!   erreurs d'écriture (disque plein, débranché) et les codes de sortie de `tar` **et**
//!   de `zstd` (un `tar` en échec = sauvegarde en échec, ce que `| zstd` d'un script sans
//!   `pipefail` cache).
//! - `<archive>.sha256` (format de `sha256sum`, vérifiable par `sha256sum -c`) est écrit
//!   **en dernier**, atomiquement, seulement si tout a réussi. En cas d'erreur, les
//!   morceaux déjà écrits sont supprimés et `tar`/`zstd` sont tués : on ne laisse jamais
//!   une archive qui ressemble à une sauvegarde complète sans l'être.
//! - FAT32 est reconnu par `statfs` (`f_type` = `MSDOS_SUPER_MAGIC`) plutôt que par
//!   `findmnt` : pas d'outil externe de plus, et la réponse porte sur le dossier même.
//! - L'archive contient `prefixe/…` quel que soit le nom réel du dossier (transformation
//!   de noms de `tar`), pour que la restauration ne dépende pas de l'ancien emplacement.
//!
//! # Restauration
//!
//! [`restaurer`] vérifie d'abord les empreintes de **tous** les morceaux, puis extrait dans
//! `<données>/prefixe.restauration` (supprimé s'il restait d'une tentative ratée), et ne
//! renomme qu'après une extraction complète et réussie : `prefixe` devient
//! `prefixe.ancien-<date>` (conservé, jamais supprimé) et `prefixe.restauration` devient
//! `prefixe`. Une restauration qui échoue laisse donc l'environnement actuel intact.
//!
//! **Sécurité de l'extraction.** `tar` GNU retire le `/` initial des noms absolus et
//! **refuse** (code 2) les membres contenant `..`, sauf option `-P` que nous n'utilisons
//! jamais ; `--strip-components=1` n'est appliqué qu'après ces contrôles et
//! `--no-same-owner` évite de recréer des propriétaires. Un test le vérifie avec une
//! archive piégée. Le fichier `.sha256` est à côté de l'archive : il protège contre la
//! corruption, pas contre une archive volontairement malveillante venue d'ailleurs.
//!
//! Utilise `tar` et `zstd` (outils du système), `restauration`, `diagnostic` (verrou),
//! `wine`, `telechargement`, `historique` ; utilisé par la CLI et l'interface.
//!
//! **Historique.** [`sauvegarder`] et [`restaurer`] écrivent chacune une entrée (`Sauvegarde`,
//! `Restauration`) dès que l'opération est engagée, c'est-à-dire une fois le verrou pris : un refus
//! antérieur (destination invalide, plugin en cours, verrou déjà pris) n'écrit rien. Le sujet est
//! le nom de l'archive, `wimao-<date>.tar.zst` (prévue, pour une sauvegarde qui échoue), et non
//! celui de son fichier `.sha256`.

use crate::chemins::Chemins;
use crate::diagnostic::poser_verrou_installation;
use crate::erreur::{Erreur, Resultat};
use crate::historique::{self, Action, ResultatAction};
use crate::i18n::{tr, trf};
use crate::outils::heure_locale;
use crate::progression::Progression;
use crate::restauration::{ecrire_atomique, perimer_tous_les_points, plugins_en_cours};
use crate::telechargement::{empreinte, en_hexadecimal};
use crate::wine::Wine;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use walkdir::WalkDir;

/// Dossier créé sur la destination.
const DOSSIER_SAUVEGARDE: &str = "WiMao-sauvegarde";
/// Taille d'un morceau sur FAT32 (3900 Mo, sous la limite de 4 Go par fichier).
pub const TAILLE_MORCEAU_FAT32: u64 = 3900 * 1024 * 1024;
/// `f_type` de `statfs` pour FAT (`MSDOS_SUPER_MAGIC`, aussi vfat).
const MAGIE_FAT: i64 = 0x4d44;
/// Nom de l'environnement dans l'archive.
const NOM_DANS_ARCHIVE: &str = "prefixe";
/// Taille du tampon de lecture/écriture.
const TAMPON: usize = 1024 * 1024;
/// Octets de la sortie d'erreur de `tar`/`zstd` gardés pour les messages.
const ERREUR_MAX: usize = 16 * 1024;
/// Dossier d'extraction d'une restauration (frère de `prefixe`, même système de fichiers).
const EXTRACTION: &str = "prefixe.restauration";
/// Extension du fichier d'empreintes écrit à côté de l'archive (`<archive>.sha256`).
const EXTENSION_SHA256: &str = ".sha256";

/// Fabrique de fichiers de sortie : ouvre (en création exclusive) le fichier à écrire.
/// Injectée pour simuler un disque plein ou débranché dans les tests.
type Ouvrir<'a> = &'a mut dyn FnMut(&Path) -> std::io::Result<Box<dyn Write>>;

// ---------------------------------------------------------------------------------------
// Sauvegarde
// ---------------------------------------------------------------------------------------

/// Sauvegarde l'environnement dans `destination/WiMao-sauvegarde/` et rend le chemin du
/// fichier `.sha256` (à donner à [`restaurer`]). Découpe en morceaux si `destination`
/// est en FAT32. Voir l'en-tête du module pour le détail.
///
/// Erreurs : [`Erreur::PluginsEnCours`], [`Erreur::InstallationEnCours`],
/// [`Erreur::Introuvable`] (`tar`/`zstd` absents), [`Erreur::Sauvegarde`] (destination
/// absente ou dans les données de WiMao, déjà existante, place insuffisante, écriture
/// impossible), [`Erreur::Commande`]
/// (`tar` ou `zstd` en échec), [`Erreur::Io`].
pub fn sauvegarder(c: &Chemins, destination: &Path, p: &dyn Progression) -> Resultat<PathBuf> {
    // Avant `est_fat`, qui échouerait sur une destination absente avec un message obscur.
    verifier_destination(c, destination)?;
    let taille = if est_fat(destination)? {
        Some(TAILLE_MORCEAU_FAT32)
    } else {
        None
    };
    sauvegarder_avec_taille(c, destination, p, taille, ouvrir_fichier)
}

/// Cœur de [`sauvegarder`] : `taille_morceau` (`None` = un seul fichier) et la fabrique
/// de fichiers de sortie `ouvrir` sont injectés pour les tests.
pub(crate) fn sauvegarder_avec_taille(
    c: &Chemins,
    destination: &Path,
    p: &dyn Progression,
    taille_morceau: Option<u64>,
    mut ouvrir: impl FnMut(&Path) -> std::io::Result<Box<dyn Write>>,
) -> Resultat<PathBuf> {
    verifier_destination(c, destination)?;
    if plugins_en_cours(c)? {
        return Err(Erreur::PluginsEnCours);
    }
    // Gardé jusqu'à la sortie (Drop) : aucune installation ne démarre pendant la copie.
    let _verrou = poser_verrou_installation(c)?;
    let dossier = destination.join(DOSSIER_SAUVEGARDE);
    let archive = dossier.join(format!("wimao-{}.tar.zst", horodatage_sauvegarde()?));
    let sha = chemin_sha256(&archive);
    // L'opération est engagée : une seule entrée d'historique, réussite ou échec, dont le
    // sujet est le nom de l'archive (celle qui était prévue si la sauvegarde n'a pas abouti).
    let resultat = (|| {
        refuser_si_existe(&archive, taille_morceau.is_some())?;
        p.etape(&tr("Checking the free space…"));
        verifier_place(&c.prefixe, destination)?;
        p.etape(&tr("Stopping Wine…"));
        Wine::nouveau(c).arreter()?;
        std::fs::create_dir_all(&dossier)?;
        p.etape(&tr("Backing up the Wine environment (several minutes)…"));
        ecrire_sauvegarde(c, &archive, &sha, taille_morceau, &mut ouvrir)
    })();
    noter_historique(c, Action::Sauvegarde, &sha, resultat.as_ref().err());
    resultat.map(|()| sha)
}

/// Écrit l'entrée d'historique d'une sauvegarde ou d'une restauration : `Reussi` si `erreur`
/// est `None`, sinon `Echec` avec la cause en détail. Le sujet est le nom de l'archive
/// ([`nom_de_l_archive`]), celui que l'utilisateur reconnaît sur son disque.
fn noter_historique(c: &Chemins, action: Action, sha256: &Path, erreur: Option<&Erreur>) {
    let sujet = nom_de_l_archive(sha256);
    match erreur {
        None => historique::ajouter(c, action, &sujet, ResultatAction::Reussi, None, ""),
        Some(e) => historique::ajouter(
            c,
            action,
            &sujet,
            ResultatAction::Echec,
            None,
            &e.to_string(),
        ),
    }
}

/// Nom de l'archive décrite par le fichier d'empreintes `sha256` : son nom de fichier sans
/// l'extension `.sha256` (`wimao-2026-11-01-06h57.tar.zst.sha256` donne
/// `wimao-2026-11-01-06h57.tar.zst`, que l'archive soit en un seul fichier ou en morceaux
/// `.partNNN`). Un fichier choisi par l'utilisateur sans cette extension garde son nom entier.
fn nom_de_l_archive(sha256: &Path) -> String {
    let nom = sha256
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    match nom.strip_suffix(EXTENSION_SHA256) {
        Some(archive) if !archive.is_empty() => archive.to_string(),
        _ => nom,
    }
}

/// Écrit l'archive puis le `.sha256` ; supprime tout ce qui a été écrit en cas d'erreur.
fn ecrire_sauvegarde(
    c: &Chemins,
    archive: &Path,
    sha: &Path,
    taille_morceau: Option<u64>,
    ouvrir: Ouvrir,
) -> Resultat<()> {
    let mut decoupeur = Decoupeur::nouveau(archive, taille_morceau, ouvrir);
    let resultat = produire_archive(c, &mut decoupeur).and_then(|()| {
        let lignes: String = decoupeur
            .termines
            .iter()
            .map(|(nom, hex)| format!("{hex}  {nom}\n"))
            .collect();
        ecrire_atomique(sha, lignes.as_bytes())
    });
    if resultat.is_err() {
        // Ni morceau ni empreinte : rien qui ressemble à une sauvegarde complète.
        let temporaire = nom_part(sha);
        for f in decoupeur
            .tentes
            .iter()
            .map(PathBuf::as_path)
            .chain([sha, &temporaire])
        {
            // Fichier peut-être jamais créé (échec avant) : « absent » n'est pas un
            // problème, et une autre erreur ne doit pas masquer la cause réelle.
            let _ = std::fs::remove_file(f);
        }
    }
    resultat
}

/// `<chemin>.part` (fichier temporaire d'`ecrire_atomique`).
fn nom_part(chemin: &Path) -> PathBuf {
    let mut n = chemin.as_os_str().to_os_string();
    n.push(".part");
    PathBuf::from(n)
}

/// Lance `tar | zstd`, lit la sortie de `zstd` dans le fil courant et la donne au
/// découpeur ; vérifie à la fin les codes de sortie des deux programmes.
fn produire_archive(c: &Chemins, decoupeur: &mut Decoupeur) -> Resultat<()> {
    let (mut chaine, mut sortie) = demarrer_compression(c)?;
    let mut tampon = vec![0u8; TAMPON];
    loop {
        let n = sortie.read(&mut tampon)?;
        if n == 0 {
            break;
        }
        // En cas d'erreur, `chaine` est abandonnée : son Drop tue tar et zstd.
        decoupeur.ecrire(&tampon[..n])?;
    }
    decoupeur.terminer()?;
    chaine.attendre()
}

/// Démarre `tar` puis `zstd` ; rend la chaîne de processus et la sortie de `zstd`.
fn demarrer_compression(c: &Chemins) -> Resultat<(Chaine, ChildStdout)> {
    let (lecteur, ecrivain) = std::io::pipe()?;
    let mut tar = Command::new("tar");
    tar.arg("-C")
        .arg(&c.prefixe)
        .args(["--no-wildcards-match-slash", "--anchored"])
        .args(EXCLUSIONS_TAR.iter().map(|e| format!("--exclude=./{e}")))
        // Nom `prefixe/` dans l'archive. `flags=rhS` : noms des fichiers (`r`) ET cibles des
        // liens durs (`h`, sinon l'archive dirait « prefixe/b lien vers ./dur » et un
        // `tar -xf` manuel échouerait), mais jamais les cibles des liens symboliques (`S` :
        // « ../x » doit rester « ../x »).
        .arg(format!("--transform=flags=rhS;s,^\\.,{NOM_DANS_ARCHIVE},"))
        .args(["-cf", "-", "."])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(ecrivain.try_clone()?)
        .process_group(0);
    let mut enfant_tar = lancer_outil(&mut tar, "tar")?;
    let pgid = enfant_tar.id() as libc::pid_t;
    let entree_zstd = enfant_tar.stdout.take().map(Stdio::from);
    // `chaine` possède déjà tar : si zstd ne démarre pas, tar est tué par son Drop.
    let mut chaine = Chaine::nouvelle(pgid, lecteur);
    chaine.enfants.push(("tar", enfant_tar));
    let mut zstd = Command::new("zstd");
    zstd.args(["-T0", "-3", "-q", "-c"])
        .stdin(entree_zstd.unwrap_or_else(Stdio::null))
        .stdout(Stdio::piped())
        .stderr(ecrivain)
        .process_group(pgid);
    let mut enfant_zstd = lancer_outil(&mut zstd, "zstd")?;
    let sortie = enfant_zstd
        .stdout
        .take()
        .ok_or_else(|| Erreur::Io(std::io::Error::other(tr("zstd has no output"))))?;
    chaine.enfants.push(("zstd", enfant_zstd));
    Ok((chaine, sortie))
}

/// Exclusions (relatives au préfixe) : réinstallables ou inutiles. `*` = un utilisateur.
const EXCLUSIONS_TAR: [&str; 3] = [
    "drive_c/users/*/AppData/Local/Temp",
    "drive_c/users/*/Downloads",
    "drive_c/users/*/AppData/Local/dxvk",
];

/// Lance une commande ; un outil absent donne une erreur claire plutôt qu'« os error 2 ».
fn lancer_outil(cmd: &mut Command, nom: &str) -> Resultat<Child> {
    cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Erreur::Introuvable(trf(
                "The tool {name} is required for backups but is not installed",
                &[("name", nom)],
            ))
        } else {
            Erreur::Io(e)
        }
    })
}

// ---------------------------------------------------------------------------------------
// Chaîne de processus enfants
// ---------------------------------------------------------------------------------------

/// Processus enfants d'une chaîne (`tar | zstd`) : même groupe, tués ensemble si on
/// abandonne, codes de sortie vérifiés à la fin.
struct Chaine {
    enfants: Vec<(&'static str, Child)>,
    /// Groupe de processus (identifiant de l'enfant qui en est le chef).
    pgid: libc::pid_t,
    /// Vrai quand tous les enfants ont été attendus (plus rien à tuer).
    fini: bool,
    /// Lecteur de la sortie d'erreur commune des enfants.
    erreurs: Option<std::thread::JoinHandle<String>>,
}

impl Chaine {
    /// Chaîne vide dont les enfants écrivent leurs erreurs dans `lecteur`.
    fn nouvelle(pgid: libc::pid_t, lecteur: std::io::PipeReader) -> Chaine {
        Chaine {
            enfants: Vec::new(),
            pgid,
            fini: false,
            erreurs: Some(lire_erreurs(lecteur)),
        }
    }

    /// Attend tous les enfants puis vérifie leurs codes. Si plusieurs échouent, l'un est
    /// la cause et les autres des ricochets : un programme tué par un signal (typiquement
    /// `tar` frappé par SIGPIPE parce que `zstd` est mort) n'est que la conséquence, donc
    /// on désigne d'abord un échec avec code de sortie, dans l'ordre de lancement
    /// (en amont d'abord : `zstd` corrompu avant `tar` qui manque de données).
    fn attendre(&mut self) -> Resultat<()> {
        let mut echecs = Vec::new();
        for (nom, enfant) in &mut self.enfants {
            let statut = enfant.wait()?;
            if !statut.success() {
                echecs.push((*nom, statut.code()));
            }
        }
        let echec = echecs
            .iter()
            .find(|(_, code)| code.is_some())
            .or(echecs.first())
            .copied();
        self.fini = true;
        let texte = self
            .erreurs
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default();
        match echec {
            None => Ok(()),
            Some((programme, code)) => Err(Erreur::Commande {
                programme: programme.to_string(),
                code,
                sortie: texte,
            }),
        }
    }
}

impl Drop for Chaine {
    fn drop(&mut self) {
        if !self.fini {
            // SAFETY: `kill` ne touche pas à la mémoire ; `-pgid` vise le groupe créé
            // pour cette chaîne, et un échec (groupe déjà fini) est sans conséquence.
            unsafe {
                libc::kill(-self.pgid, libc::SIGKILL);
            }
        }
        // Récupère les enfants (pas de zombie). Après SIGKILL, ne bloque pas.
        for (_, enfant) in &mut self.enfants {
            let _ = enfant.wait();
        }
    }
}

/// Lit la sortie d'erreur des enfants dans un fil (sinon un enfant bavard se bloquerait) ;
/// ne garde que le début, assez pour un message.
fn lire_erreurs(mut lecteur: std::io::PipeReader) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut gardes = Vec::new();
        let mut bloc = [0u8; 4096];
        while let Ok(n) = lecteur.read(&mut bloc) {
            if n == 0 {
                break;
            }
            if gardes.len() < ERREUR_MAX {
                gardes.extend_from_slice(&bloc[..n]);
            }
        }
        String::from_utf8_lossy(&gardes).into_owned()
    })
}

// ---------------------------------------------------------------------------------------
// Découpage et empreintes
// ---------------------------------------------------------------------------------------

/// Morceau en cours d'écriture.
struct Morceau {
    sortie: Box<dyn Write>,
    hash: Sha256,
    ecrit: u64,
    chemin: PathBuf,
}

/// Reçoit le flux compressé, l'écrit en morceaux de `taille` octets au plus (ou en un
/// seul fichier) et calcule l'empreinte de chaque fichier au fil de l'eau.
struct Decoupeur<'a> {
    archive: PathBuf,
    taille: Option<u64>,
    ouvrir: Ouvrir<'a>,
    courant: Option<Morceau>,
    /// (nom de fichier, empreinte) des fichiers terminés, dans l'ordre.
    termines: Vec<(String, String)>,
    /// Tous les fichiers que nous avons créés : à supprimer en cas d'échec.
    tentes: Vec<PathBuf>,
}

impl<'a> Decoupeur<'a> {
    fn nouveau(archive: &Path, taille: Option<u64>, ouvrir: Ouvrir<'a>) -> Decoupeur<'a> {
        Decoupeur {
            archive: archive.to_path_buf(),
            taille,
            ouvrir,
            courant: None,
            termines: Vec::new(),
            tentes: Vec::new(),
        }
    }

    /// Chemin du morceau numéro `n` (l'archive elle-même s'il n'y a pas de découpage).
    fn chemin_du_morceau(&self, n: usize) -> PathBuf {
        match self.taille {
            None => self.archive.clone(),
            Some(_) => {
                let mut nom = self.archive.as_os_str().to_os_string();
                nom.push(format!(".part{n:03}"));
                PathBuf::from(nom)
            }
        }
    }

    /// Écrit `donnees`, en ouvrant un nouveau morceau quand le courant est plein.
    /// Le morceau suivant n'est ouvert que s'il reste des données (jamais de morceau vide).
    fn ecrire(&mut self, mut donnees: &[u8]) -> Resultat<()> {
        while !donnees.is_empty() {
            if self.courant.as_ref().is_some_and(|m| self.plein(m)) {
                self.fermer()?;
            }
            if self.courant.is_none() {
                self.ouvrir_suivant()?;
            }
            let Some(m) = self.courant.as_mut() else {
                return Err(Erreur::Io(std::io::Error::other(tr(
                    "missing backup piece",
                ))));
            };
            let place = self.taille.map_or(u64::MAX, |t| t - m.ecrit);
            let n = donnees
                .len()
                .min(usize::try_from(place).unwrap_or(usize::MAX));
            m.sortie
                .write_all(&donnees[..n])
                .map_err(|e| erreur_ecriture(&m.chemin, &e))?;
            m.hash.update(&donnees[..n]);
            m.ecrit += n as u64;
            donnees = &donnees[n..];
        }
        Ok(())
    }

    fn plein(&self, m: &Morceau) -> bool {
        self.taille.is_some_and(|t| m.ecrit >= t)
    }

    fn ouvrir_suivant(&mut self) -> Resultat<()> {
        let chemin = self.chemin_du_morceau(self.termines.len());
        let sortie = (self.ouvrir)(&chemin).map_err(|e| erreur_ecriture(&chemin, &e))?;
        // Noté seulement une fois créé : on ne supprimera jamais un fichier qui existait
        // déjà (l'ouverture en création exclusive a échoué) et qui n'est pas à nous.
        self.tentes.push(chemin.clone());
        self.courant = Some(Morceau {
            sortie,
            hash: Sha256::new(),
            ecrit: 0,
            chemin,
        });
        Ok(())
    }

    /// Termine le morceau courant : vidage sur disque, puis empreinte.
    fn fermer(&mut self) -> Resultat<()> {
        if let Some(mut m) = self.courant.take() {
            m.sortie
                .flush()
                .map_err(|e| erreur_ecriture(&m.chemin, &e))?;
            let nom = m
                .chemin
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.termines
                .push((nom, en_hexadecimal(&m.hash.finalize())));
        }
        Ok(())
    }

    /// Fin du flux : ferme le dernier morceau.
    fn terminer(&mut self) -> Resultat<()> {
        self.fermer()
    }
}

/// Message clair pour une écriture impossible (disque plein, débranché, droits…).
fn erreur_ecriture(chemin: &Path, e: &std::io::Error) -> Erreur {
    Erreur::Sauvegarde(trf(
        "Cannot write the backup to {path}: {error}. Check that the destination is connected and has free space.",
        &[
            ("path", &chemin.display().to_string()),
            ("error", &e.to_string()),
        ],
    ))
}

/// Fichier de sortie dont `flush` écrit réellement sur le disque (`sync_all`) : une erreur
/// de disque plein ou débranché apparaît à la fin du morceau, pas après coup.
struct FichierSortie(std::fs::File);

impl Write for FichierSortie {
    fn write(&mut self, tampon: &[u8]) -> std::io::Result<usize> {
        self.0.write(tampon)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()?;
        self.0.sync_all()
    }
}

/// Fabrique par défaut : création exclusive (n'écrase jamais un fichier existant).
fn ouvrir_fichier(chemin: &Path) -> std::io::Result<Box<dyn Write>> {
    let f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(chemin)?;
    Ok(Box::new(FichierSortie(f)))
}

// ---------------------------------------------------------------------------------------
// Vérifications préalables
// ---------------------------------------------------------------------------------------

/// `<archive>.sha256`.
fn chemin_sha256(archive: &Path) -> PathBuf {
    let mut n = archive.as_os_str().to_os_string();
    n.push(EXTENSION_SHA256);
    PathBuf::from(n)
}

/// La destination doit être un dossier existant, **hors** des données de WiMao (et donc
/// du préfixe) : une sauvegarde rangée dans ce qu'elle sauvegarde se copierait elle-même
/// et disparaîtrait avec les données qu'elle doit protéger. Les chemins sont comparés
/// une fois les liens résolus (`canonicalize`). Erreur : [`Erreur::Sauvegarde`] (traduite).
fn verifier_destination(c: &Chemins, destination: &Path) -> Resultat<()> {
    if !destination.is_dir() {
        return Err(Erreur::Sauvegarde(trf(
            "The destination {path} does not exist or is not a folder: plug in the disk or choose another folder",
            &[("path", &destination.display().to_string())],
        )));
    }
    let reelle = destination.canonicalize()?;
    // Un dossier encore inexistant ne peut contenir la destination (qui existe).
    let interdits = [&c.donnees, &c.prefixe]
        .into_iter()
        .filter_map(|d| d.canonicalize().ok());
    for interdit in interdits {
        if reelle.starts_with(&interdit) {
            return Err(Erreur::Sauvegarde(trf(
                "The destination {path} is inside WiMao's data folder ({data}): choose another folder, for example on an external disk",
                &[
                    ("path", &destination.display().to_string()),
                    ("data", &c.donnees.display().to_string()),
                ],
            )));
        }
    }
    Ok(())
}

/// `AAAA-MM-JJ-HHhMM` à l'heure locale.
fn horodatage_sauvegarde() -> Resultat<String> {
    let tm = heure_locale()?;
    Ok(format!(
        "{:04}-{:02}-{:02}-{:02}h{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    ))
}

/// Refuse d'écraser une sauvegarde de la même minute (archive, premier morceau ou empreintes).
fn refuser_si_existe(archive: &Path, decoupe: bool) -> Resultat<()> {
    let mut premier = archive.as_os_str().to_os_string();
    premier.push(".part000");
    let deja = archive.exists()
        || chemin_sha256(archive).exists()
        || (decoupe && Path::new(&premier).exists());
    if deja {
        return Err(Erreur::Sauvegarde(trf(
            "The backup {path} already exists: wait a minute or remove it",
            &[("path", &archive.display().to_string())],
        )));
    }
    Ok(())
}

/// Vrai si `chemin` est en dehors de ce que l'on sauvegarde (chemin relatif au préfixe).
fn est_exclu(relatif: &Path) -> bool {
    let p: Vec<&str> = relatif
        .components()
        .map(|c| c.as_os_str().to_str().unwrap_or(""))
        .collect();
    matches!(
        p.as_slice(),
        ["drive_c", "users", _, "Downloads"]
            | ["drive_c", "users", _, "AppData", "Local", "Temp" | "dxvk"]
    )
}

/// Taille (octets) du préfixe hors exclusions, comme `du -sb`. Les liens ne sont pas
/// suivis ; une entrée illisible est ignorée (`tar` signalera le vrai problème).
fn taille_prefixe(prefixe: &Path) -> u64 {
    WalkDir::new(prefixe)
        .into_iter()
        .filter_entry(|e| {
            e.path()
                .strip_prefix(prefixe)
                .map_or(true, |r| !est_exclu(r))
        })
        .filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Erreur claire si la destination n'a pas assez de place pour le préfixe.
fn verifier_place(prefixe: &Path, destination: &Path) -> Resultat<()> {
    controler_place(
        taille_prefixe(prefixe),
        espace_libre(destination)?,
        destination,
    )
}

/// Comparaison pure : il faut strictement plus de place libre que `besoin` (comme le
/// script, `[ libre -gt besoin ]`) ; `libre == besoin` est refusé.
fn controler_place(besoin: u64, libre: u64, destination: &Path) -> Resultat<()> {
    if libre > besoin {
        return Ok(());
    }
    Err(Erreur::Sauvegarde(trf(
        "Not enough free space on {path}: {need} MB needed, {free} MB available",
        &[
            ("path", &destination.display().to_string()),
            ("need", &(besoin / 1_000_000).to_string()),
            ("free", &(libre / 1_000_000).to_string()),
        ],
    )))
}

/// Chemin en chaîne C (erreur si le chemin contient un octet nul).
fn chemin_c(chemin: &Path) -> std::io::Result<CString> {
    CString::new(chemin.as_os_str().as_bytes()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            tr("invalid path (NUL byte)"),
        )
    })
}

/// Octets disponibles pour un utilisateur ordinaire sur le disque de `dossier` (`statvfs`).
fn espace_libre(dossier: &Path) -> std::io::Result<u64> {
    let c = chemin_c(dossier)?;
    // SAFETY: `c` est une chaîne C valide ; `statvfs` n'écrit que dans `s`
    // (structure C, tout à zéro est valide).
    let (r, s) = unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        (libc::statvfs(c.as_ptr(), &mut s), s)
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Les types de `statvfs` varient selon l'architecture : conversions nécessaires ailleurs.
    #[allow(clippy::unnecessary_cast)]
    Ok((s.f_bavail as u64).saturating_mul(s.f_frsize as u64))
}

/// Vrai si `dossier` est sur un système de fichiers FAT (`statfs`, `f_type`).
fn est_fat(dossier: &Path) -> std::io::Result<bool> {
    let c = chemin_c(dossier)?;
    // SAFETY: comme `espace_libre`, avec `statfs`.
    let (r, s) = unsafe {
        let mut s: libc::statfs = std::mem::zeroed();
        (libc::statfs(c.as_ptr(), &mut s), s)
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // `f_type` est un entier de taille variable selon l'architecture.
    #[allow(clippy::unnecessary_cast)]
    Ok(s.f_type as i64 == MAGIE_FAT)
}

// ---------------------------------------------------------------------------------------
// Restauration
// ---------------------------------------------------------------------------------------

/// Restaure l'environnement depuis `archive_sha256` (fichier rendu par [`sauvegarder`],
/// à côté des morceaux). Voir l'en-tête du module : empreintes de tous les morceaux
/// vérifiées d'abord, extraction dans `prefixe.restauration`, échange à la fin seulement,
/// ancien environnement gardé en `prefixe.ancien-<date>`.
///
/// Erreurs : [`Erreur::PluginsEnCours`], [`Erreur::InstallationEnCours`],
/// [`Erreur::Empreinte`] (morceau altéré), [`Erreur::Sauvegarde`] (morceau manquant,
/// liste d'empreintes illisible, archive vide), [`Erreur::Commande`] (`zstd`/`tar` en
/// échec : archive corrompue ou disque plein), [`Erreur::Io`].
pub fn restaurer(c: &Chemins, archive_sha256: &Path, p: &dyn Progression) -> Resultat<()> {
    if plugins_en_cours(c)? {
        return Err(Erreur::PluginsEnCours);
    }
    let _verrou = poser_verrou_installation(c)?;
    // L'opération est engagée : une seule entrée d'historique, réussite ou échec.
    let resultat = restaurer_engagee(c, archive_sha256, p);
    noter_historique(
        c,
        Action::Restauration,
        archive_sha256,
        resultat.as_ref().err(),
    );
    resultat
}

/// Corps de [`restaurer`], une fois le verrou pris (l'historique s'écrit dans l'appelant).
fn restaurer_engagee(c: &Chemins, archive_sha256: &Path, p: &dyn Progression) -> Resultat<()> {
    p.etape(&tr("Verifying the backup checksums…"));
    let fichiers = verifier_empreintes(archive_sha256)?;
    p.etape(&tr("Stopping Wine…"));
    Wine::nouveau(c).arreter()?;
    let extraction = c.donnees.join(EXTRACTION);
    supprimer_tentative(&extraction)?;
    p.etape(&tr("Extracting the backup (several minutes)…"));
    if let Err(e) =
        extraire(&fichiers, &extraction).and_then(|()| verifier_vraisemblable(&extraction))
    {
        // Meilleur effort : un reste éventuel est de toute façon supprimé à la tentative suivante.
        let _ = std::fs::remove_dir_all(&extraction);
        return Err(e);
    }
    p.etape(&tr("Putting the restored environment in place…"));
    echanger(c, &extraction)?;
    // Les points de restauration décrivent l'ancien préfixe : on les périme. La restauration
    // elle-même a réussi, un échec ici est seulement noté au journal.
    perimer_tous_les_points(c, "sauvegarde restaurée");
    Ok(())
}

/// Supprime le dossier d'extraction d'une tentative précédente (lien ou fichier compris :
/// jamais de suivi de lien).
fn supprimer_tentative(extraction: &Path) -> Resultat<()> {
    match std::fs::symlink_metadata(extraction) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(extraction)?,
        Ok(_) => std::fs::remove_file(extraction)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// Lit le `.sha256`, contrôle sa forme (lignes valides, noms sûrs, morceaux sans trou) et la
/// présence de chaque fichier listé (dans le même dossier) : rend (nom, empreinte attendue,
/// chemin), dans l'ordre des morceaux. Ne calcule aucune empreinte (c'est long).
fn lister_pieces(sha256: &Path) -> Resultat<Vec<(String, String, PathBuf)>> {
    let dossier = sha256.parent().unwrap_or(Path::new("."));
    let texte = std::fs::read_to_string(sha256).map_err(|e| {
        Erreur::Sauvegarde(trf(
            "Cannot read {path}: {error}",
            &[
                ("path", &sha256.display().to_string()),
                ("error", &e.to_string()),
            ],
        ))
    })?;
    let attendues = lire_empreintes(&texte)?;
    verifier_suite(attendues.keys())?;
    let mut pieces = Vec::new();
    for (nom, attendue) in attendues {
        let chemin = dossier.join(&nom);
        if !chemin.is_file() {
            return Err(Erreur::Sauvegarde(trf(
                "A backup file is missing: {name}",
                &[("name", &nom)],
            )));
        }
        pieces.push((nom, attendue, chemin));
    }
    Ok(pieces)
}

/// Les fichiers (morceaux) que décrit un `.sha256`, dans l'ordre, après les contrôles de forme
/// et de présence de [`restaurer`] mais **sans** vérifier les empreintes : sert à résumer une
/// sauvegarde avant de la restaurer (nom, taille) sans lire des gigaoctets.
///
/// # Erreurs
/// [`Erreur::Sauvegarde`] : `.sha256` illisible, ligne invalide, nom hors du dossier, morceaux
/// avec un trou, fichier absent.
pub fn pieces_listees(sha256: &Path) -> Resultat<Vec<PathBuf>> {
    Ok(lister_pieces(sha256)?
        .into_iter()
        .map(|(_, _, chemin)| chemin)
        .collect())
}

/// Lit le `.sha256`, vérifie l'empreinte de chaque fichier listé (dans le même dossier) et
/// rend les chemins dans l'ordre des morceaux.
fn verifier_empreintes(sha256: &Path) -> Resultat<Vec<PathBuf>> {
    let mut chemins = Vec::new();
    for (_, attendue, chemin) in lister_pieces(sha256)? {
        let obtenue = empreinte(&chemin)?;
        if obtenue != attendue {
            return Err(Erreur::Empreinte { attendue, obtenue });
        }
        chemins.push(chemin);
    }
    Ok(chemins)
}

/// Analyse un fichier au format `sha256sum` : « empreinte  nom » ou « empreinte *nom ».
/// Rend nom → empreinte, trié par nom. Un nom avec `/` ou `..` est refusé : il ferait
/// lire un fichier hors du dossier de la sauvegarde.
fn lire_empreintes(texte: &str) -> Resultat<BTreeMap<String, String>> {
    let mut resultat = BTreeMap::new();
    for ligne in texte.lines().filter(|l| !l.trim().is_empty()) {
        let invalide = || {
            Erreur::Sauvegarde(trf(
                "Invalid line in the checksum file: {line}",
                &[("line", ligne)],
            ))
        };
        let (hex, reste) = ligne.split_once(' ').ok_or_else(invalide)?;
        let nom = reste
            .strip_prefix(' ')
            .or_else(|| reste.strip_prefix('*'))
            .ok_or_else(invalide)?;
        let hex_valide = hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit());
        let nom_sur = !nom.is_empty() && nom != ".." && nom != "." && !nom.contains('/');
        if !hex_valide || !nom_sur {
            return Err(invalide());
        }
        resultat.insert(nom.to_string(), hex.to_ascii_lowercase());
    }
    if resultat.is_empty() {
        return Err(Erreur::Sauvegarde(tr(
            "The checksum file lists no backup file",
        )));
    }
    Ok(resultat)
}

/// Les morceaux `.part000`, `.part001`… doivent se suivre sans trou (sinon l'archive est
/// incomplète même si chaque morceau listé est intact). Un fichier unique est accepté tel quel.
fn verifier_suite<'a>(noms: impl Iterator<Item = &'a String>) -> Resultat<()> {
    let noms: Vec<&String> = noms.collect();
    if noms.len() == 1 && !noms[0].contains(".part") {
        return Ok(());
    }
    for (i, nom) in noms.iter().enumerate() {
        let attendu = format!(".part{i:03}");
        if !nom.ends_with(&attendu) {
            return Err(Erreur::Sauvegarde(trf(
                "The backup is incomplete: expected a file ending with {suffix}",
                &[("suffix", &attendu)],
            )));
        }
    }
    Ok(())
}

/// Extrait la concaténation des morceaux dans `extraction` : un fil envoie les fichiers à
/// `zstd -dc`, dont la sortie va directement à `tar -x`.
fn extraire(fichiers: &[PathBuf], extraction: &Path) -> Resultat<()> {
    std::fs::create_dir_all(extraction)?;
    let (lecteur, ecrivain) = std::io::pipe()?;
    let mut zstd = Command::new("zstd");
    zstd.args(["-dc", "-q"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(ecrivain.try_clone()?)
        .process_group(0);
    let mut enfant_zstd = lancer_outil(&mut zstd, "zstd")?;
    let pgid = enfant_zstd.id() as libc::pid_t;
    let entree = enfant_zstd.stdin.take();
    let sortie = enfant_zstd.stdout.take().map(Stdio::from);
    let mut chaine = Chaine::nouvelle(pgid, lecteur);
    chaine.enfants.push(("zstd", enfant_zstd));
    let mut tar = Command::new("tar");
    tar.arg("-xf")
        .arg("-")
        .arg("-C")
        .arg(extraction)
        // Le premier élément est `prefixe/` : on le retire pour extraire directement ici.
        // Les contrôles de `tar` (pas de `..`, `/` initial retiré) passent avant.
        .args(["--strip-components=1", "--no-same-owner"])
        .stdin(sortie.unwrap_or_else(Stdio::null))
        .stdout(Stdio::null())
        .stderr(ecrivain)
        .process_group(pgid);
    chaine.enfants.push(("tar", lancer_outil(&mut tar, "tar")?));
    // Les `Command` gardent des copies du tuyau d'erreur : sans les lâcher, le fil de
    // lecture des erreurs ne verrait jamais la fin du flux et `attendre` ne finirait pas.
    drop(tar);
    drop(zstd);
    let envoi = entree.map(|e| envoyer_fichiers(fichiers.to_vec(), e));
    let fin = chaine.attendre();
    // Une erreur de lecture d'un morceau (disque débranché) est la vraie cause du reste.
    if let Some(Err(e)) = envoi.map(|h| h.join()).transpose().map_err(|_| {
        Erreur::Io(std::io::Error::other(tr(
            "the sending thread stopped unexpectedly",
        )))
    })? {
        if e.kind() != std::io::ErrorKind::BrokenPipe {
            return Err(Erreur::Sauvegarde(trf(
                "Cannot read the backup: {error}",
                &[("error", &e.to_string())],
            )));
        }
    }
    fin
}

/// Fil qui copie les fichiers, dans l'ordre, vers l'entrée de `zstd` puis la ferme.
fn envoyer_fichiers(
    fichiers: Vec<PathBuf>,
    mut entree: std::process::ChildStdin,
) -> std::thread::JoinHandle<std::io::Result<()>> {
    std::thread::spawn(move || {
        for f in fichiers {
            std::io::copy(&mut std::fs::File::open(f)?, &mut entree)?;
        }
        Ok(())
    })
}

/// Contrôle de vraisemblance avant l'échange : un environnement Wine a un `drive_c/` et un
/// `system.reg`. Une archive vide, un mauvais fichier ou une archive piégée ne doit pas
/// remplacer l'environnement de l'utilisateur.
fn verifier_vraisemblable(extraction: &Path) -> Resultat<()> {
    if extraction.join("drive_c").is_dir() && extraction.join("system.reg").is_file() {
        return Ok(());
    }
    Err(Erreur::Sauvegarde(tr(
        "The backup does not look like a Wine environment (drive_c or system.reg is missing): nothing was changed",
    )))
}

/// Échange : `prefixe` → `prefixe.ancien-<date>` (conservé), `extraction` → `prefixe`.
/// Si le second renommage échoue, l'ancien environnement est remis en place.
fn echanger(c: &Chemins, extraction: &Path) -> Resultat<()> {
    let ancien = if c.prefixe.symlink_metadata().is_ok() {
        let cible = nom_ancien(c)?;
        // Mêmes droits que l'ancien dossier (l'archive n'a pas restauré celui de la racine).
        if let Ok(m) = std::fs::metadata(&c.prefixe) {
            // Détail de confort : si les droits ne peuvent pas être copiés, ceux par défaut conviennent.
            let _ = std::fs::set_permissions(extraction, m.permissions());
        }
        std::fs::rename(&c.prefixe, &cible)?;
        Some(cible)
    } else {
        None
    };
    if let Err(e) = std::fs::rename(extraction, &c.prefixe) {
        if let Some(a) = &ancien {
            if let Err(e2) = std::fs::rename(a, &c.prefixe) {
                // Pire cas : l'utilisateur doit savoir où sont ses deux dossiers.
                return Err(Erreur::Sauvegarde(trf(
                    "Could not put the restored environment in place ({error}) nor put the previous one back ({error2}). Your previous environment is in {old}; the restored files are in {new}",
                    &[
                        ("error", &e.to_string()),
                        ("error2", &e2.to_string()),
                        ("old", &a.display().to_string()),
                        ("new", &extraction.display().to_string()),
                    ],
                )));
            }
        }
        return Err(e.into());
    }
    Ok(())
}

/// `<données>/prefixe.ancien-AAAA-MM-JJ-HHhMMmSS`, avec `-2`, `-3`… s'il existe déjà.
fn nom_ancien(c: &Chemins) -> Resultat<PathBuf> {
    let tm = heure_locale()?;
    let base = format!(
        "{}.ancien-{:04}-{:02}-{:02}-{:02}h{:02}m{:02}",
        c.prefixe
            .file_name()
            .map_or_else(|| NOM_DANS_ARCHIVE.into(), |n| n.to_string_lossy()),
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    );
    let mut candidat = c.donnees.join(&base);
    let mut n = 2;
    while candidat.symlink_metadata().is_ok() {
        candidat = c.donnees.join(format!("{base}-{n}"));
        n += 1;
    }
    Ok(candidat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progression::ProgressionMuette;
    use std::os::unix::fs::PermissionsExt;

    /// Faux environnement : `wineserver` factice, préfixe avec un plugin aux noms
    /// difficiles (espaces, « ! », accent), un gros fichier incompressible, un lien
    /// symbolique et un dossier exclu.
    fn banc(racine: &Path) -> Chemins {
        let mut c = Chemins::pour_test(racine);
        c.wine_dir = racine.join("wine");
        std::fs::create_dir_all(c.wine_bin()).unwrap();
        let script = c.wine_bin().join("wineserver");
        crate::test_utils::ecrire_executable(
            &script,
            "#!/bin/sh
exit 0
",
        );
        let plugin = c.drive_c().join("Program Files/Xpand!2");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("é.dll"), "contenu de é").unwrap();
        std::fs::write(c.drive_c().join("Program Files/un fichier.txt"), "espaces").unwrap();
        let dl = c.drive_c().join("users/u/Downloads");
        std::fs::create_dir_all(&dl).unwrap();
        std::fs::write(dl.join("gros.bin"), "à exclure").unwrap();
        std::os::unix::fs::symlink("../x", c.prefixe.join("lien")).unwrap();
        std::fs::hard_link(
            c.drive_c().join("Program Files/un fichier.txt"),
            c.drive_c().join("Program Files/dur"),
        )
        .unwrap();
        // Un environnement Wine réel a toujours `system.reg` (contrôle avant l'échange).
        std::fs::write(c.prefixe.join("system.reg"), "reg").unwrap();
        c
    }

    /// 3 Mo d'octets pseudo-aléatoires (incompressibles : l'archive dépasse 1 Mo).
    fn ecrire_incompressible(chemin: &Path) {
        let mut etat: u64 = 0x2545F4914F6CDD1D;
        let octets: Vec<u8> = (0..3 * 1024 * 1024)
            .map(|_| {
                etat ^= etat << 13;
                etat ^= etat >> 7;
                etat ^= etat << 17;
                (etat >> 24) as u8
            })
            .collect();
        std::fs::write(chemin, octets).unwrap();
    }

    fn sauvegarde_1mo(c: &Chemins, dest: &Path) -> Resultat<PathBuf> {
        sauvegarder_avec_taille(
            c,
            dest,
            &ProgressionMuette,
            Some(1024 * 1024),
            ouvrir_fichier,
        )
    }

    fn noms(dossier: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dossier)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    // `serial` : le message vérifié est traduit ; les tests d'i18n changent la langue globale.
    #[serial_test::serial]
    fn sauvegarde_decoupee_verifiable_puis_restauration() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        ecrire_incompressible(&c.drive_c().join("Program Files/Xpand!2/gros-plugin.bin"));
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();

        let sha = sauvegarde_1mo(&c, &dest).unwrap();
        let dossier = dest.join(DOSSIER_SAUVEGARDE);
        assert_eq!(sha.parent().unwrap(), dossier);
        let fichiers = noms(&dossier);
        assert!(
            fichiers
                .iter()
                .filter(|n| n.contains(".tar.zst.part"))
                .count()
                >= 3,
            "{fichiers:?}"
        );
        assert!(fichiers.iter().any(|n| n.ends_with(".tar.zst.sha256")));
        // La commande de référence du script : sha256sum -c.
        let verif = Command::new("sha256sum")
            .arg("-c")
            .arg(sha.file_name().unwrap())
            .current_dir(&dossier)
            .output()
            .unwrap();
        assert!(
            verif.status.success(),
            "{}",
            String::from_utf8_lossy(&verif.stdout)
        );
        // gros.bin (Downloads) absent de l'archive, é.dll présent, nommée « prefixe/… ».
        let liste = lister_archive(&dossier);
        assert!(
            !liste.contains("gros.bin") && liste.contains("Xpand!2/é.dll"),
            "{liste}"
        );
        assert!(
            liste.contains("prefixe/drive_c/Program Files/un fichier.txt"),
            "{liste}"
        );
        // Restauration dans un environnement modifié.
        std::fs::write(c.prefixe.join("temoin.txt"), "ancien").unwrap();
        std::fs::remove_file(c.drive_c().join("Program Files/Xpand!2/é.dll")).unwrap();
        // Un point de restauration pris avant : la restauration le rend périmé.
        for reg in ["system.reg", "user.reg", "userdef.reg"] {
            std::fs::write(c.prefixe.join(reg), "registre").unwrap();
        }
        crate::restauration::creer(&c, "avant").unwrap();
        restaurer(&c, &sha, &ProgressionMuette).unwrap();
        let points = crate::restauration::lister(&c).unwrap();
        assert!(
            points.points.is_empty() && points.perimes.len() == 1,
            "{points:?}"
        );
        let e_dll = c.drive_c().join("Program Files/Xpand!2/é.dll");
        assert_eq!(std::fs::read_to_string(e_dll).unwrap(), "contenu de é");
        assert!(!c.prefixe.join("temoin.txt").exists());
        assert!(!c.drive_c().join("users/u/Downloads/gros.bin").exists());
        assert_eq!(
            std::fs::read_link(c.prefixe.join("lien")).unwrap(),
            Path::new("../x")
        );
        let anciens: Vec<String> = noms(&c.donnees)
            .into_iter()
            .filter(|n| n.starts_with("prefixe.ancien-"))
            .collect();
        assert_eq!(anciens.len(), 1, "{anciens:?}");
        assert!(c.donnees.join(&anciens[0]).join("temoin.txt").exists());
        assert!(!c.donnees.join(EXTRACTION).exists());
    }

    /// Contenu (`tar -t`) de l'archive du dossier : morceaux (ou fichier unique)
    /// concaténés dans l'ordre, passés à `zstd -dc | tar -t`.
    fn lister_archive(dossier: &Path) -> String {
        let mut tout = Vec::new();
        for n in noms(dossier).iter().filter(|n| !n.ends_with(".sha256")) {
            tout.extend(std::fs::read(dossier.join(n)).unwrap());
        }
        let mut zstd = Command::new("zstd")
            .args(["-dc", "-q"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut entree = zstd.stdin.take().unwrap();
        let ecrivain = std::thread::spawn(move || entree.write_all(&tout));
        let tar = Command::new("tar")
            .args(["-t", "--quoting-style=literal"])
            .stdin(Stdio::from(zstd.stdout.take().unwrap()))
            .output()
            .unwrap();
        ecrivain.join().unwrap().unwrap();
        zstd.wait().unwrap();
        String::from_utf8_lossy(&tar.stdout).into_owned()
    }

    #[test]
    fn fichier_unique_sans_decoupage() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let sha =
            sauvegarder_avec_taille(&c, &dest, &ProgressionMuette, None, ouvrir_fichier).unwrap();
        let fichiers = noms(&dest.join(DOSSIER_SAUVEGARDE));
        assert_eq!(fichiers.len(), 2, "{fichiers:?}");
        assert!(fichiers.iter().all(|n| !n.contains(".part")));
        restaurer(&c, &sha, &ProgressionMuette).unwrap();
    }

    #[test]
    fn sauvegarde_reussie_ajoute_une_entree() {
        use crate::historique::{lire, Action, ResultatAction};
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let sha =
            sauvegarder_avec_taille(&c, &dest, &ProgressionMuette, None, ouvrir_fichier).unwrap();
        // Défaut de la vérification réelle : le sujet montré dans l'historique était le nom du
        // `.sha256` (« wimao-…tar.zst.sha256 ») au lieu de celui de l'archive.
        let nom_sha = sha.file_name().unwrap().to_string_lossy().into_owned();
        let nom_archive = nom_sha.strip_suffix(".sha256").unwrap().to_string();
        assert!(nom_archive.ends_with(".tar.zst"), "{nom_archive}");
        let entrees = lire(&c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].action, Action::Sauvegarde);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
        assert_eq!(entrees[0].sujet, nom_archive);

        restaurer(&c, &sha, &ProgressionMuette).unwrap();
        let entrees = lire(&c).unwrap();
        assert_eq!(entrees.len(), 2, "{entrees:?}");
        // La plus récente d'abord.
        assert_eq!(entrees[0].action, Action::Restauration);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
        assert_eq!(entrees[0].sujet, nom_archive);
    }

    #[test]
    fn le_sujet_d_historique_est_le_nom_de_l_archive() {
        let dossier = Path::new("/media/disque/WiMao-sauvegarde");
        assert_eq!(
            nom_de_l_archive(&dossier.join("wimao-2026-11-01-06h57.tar.zst.sha256")),
            "wimao-2026-11-01-06h57.tar.zst"
        );
        // Un fichier d'empreintes choisi par l'utilisateur sans l'extension garde son nom.
        assert_eq!(nom_de_l_archive(&dossier.join("empreintes")), "empreintes");
        assert_eq!(nom_de_l_archive(&dossier.join(".sha256")), ".sha256");
    }

    #[test]
    fn sauvegarde_en_echec_ajoute_une_entree_echec() {
        use crate::historique::{lire, Action, ResultatAction};
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        sauvegarder_avec_taille(&c, &dest, &ProgressionMuette, None, fabrique_pleine(1))
            .unwrap_err();
        let entrees = lire(&c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].action, Action::Sauvegarde);
        assert_eq!(entrees[0].resultat, ResultatAction::Echec);
        // Le nom de l'archive prévue, sans `.sha256`.
        assert!(entrees[0].sujet.ends_with(".tar.zst"), "{entrees:?}");
        assert!(!entrees[0].details.is_empty(), "la cause est attendue");
    }

    #[test]
    fn un_refus_avant_toute_action_n_ecrit_rien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        // Destination inexistante : refus avant l'action.
        sauvegarder_avec_taille(
            &c,
            &d.path().join("absent"),
            &ProgressionMuette,
            None,
            ouvrir_fichier,
        )
        .unwrap_err();
        assert!(crate::historique::lire(&c).unwrap().is_empty());
    }

    /// Fabrique qui échoue (disque plein) à l'ouverture du `n`-ième fichier.
    fn fabrique_pleine(n: usize) -> impl FnMut(&Path) -> std::io::Result<Box<dyn Write>> {
        let mut vus = 0;
        move |chemin| {
            vus += 1;
            if vus == n {
                return Err(std::io::Error::from(std::io::ErrorKind::StorageFull));
            }
            ouvrir_fichier(chemin)
        }
    }

    /// Plus aucun `tar` ou `zstd` dont la ligne de commande vise `prefixe`.
    fn aucun_processus_restant(prefixe: &Path) -> bool {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let sortie = Command::new("pgrep")
            .args(["-af", "^(tar|zstd) "])
            .output()
            .unwrap();
        let t = String::from_utf8_lossy(&sortie.stdout);
        !t.lines()
            .any(|l| l.contains(&prefixe.display().to_string()))
    }

    #[test]
    // `serial` : le message vérifié est traduit ; les tests d'i18n changent la langue globale.
    #[serial_test::serial]
    fn disque_plein_au_deuxieme_morceau_ne_laisse_rien() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        ecrire_incompressible(&c.drive_c().join("Program Files/gros.bin"));
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let r = sauvegarder_avec_taille(
            &c,
            &dest,
            &ProgressionMuette,
            Some(1024 * 1024),
            fabrique_pleine(2),
        );
        let e = r.unwrap_err().to_string();
        assert!(
            e.contains(".part001") && (e.contains("free space") || e.contains("espace")),
            "{e}"
        );
        let restes = noms(&dest.join(DOSSIER_SAUVEGARDE));
        assert!(restes.is_empty(), "{restes:?}");
        assert!(aucun_processus_restant(&c.prefixe));
        // Le verrou est rendu.
        assert!(!crate::installation_en_cours(&c));
    }

    #[test]
    fn ecriture_qui_echoue_en_cours_de_morceau_ne_laisse_rien() {
        struct Casse(u32);
        impl Write for Casse {
            fn write(&mut self, t: &[u8]) -> std::io::Result<usize> {
                if self.0 == 0 {
                    return Err(std::io::Error::from_raw_os_error(libc::EIO));
                }
                self.0 -= 1;
                Ok(t.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        ecrire_incompressible(&c.drive_c().join("Program Files/gros.bin"));
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let r = sauvegarder_avec_taille(&c, &dest, &ProgressionMuette, None, |chemin| {
            ouvrir_fichier(chemin)?;
            Ok(Box::new(Casse(1)))
        });
        assert!(matches!(r, Err(Erreur::Sauvegarde(_))));
        assert!(noms(&dest.join(DOSSIER_SAUVEGARDE)).is_empty());
        assert!(aucun_processus_restant(&c.prefixe));
    }

    #[test]
    fn tar_en_echec_est_une_sauvegarde_en_echec() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let illisible = c.drive_c().join("Program Files/secret");
        std::fs::write(&illisible, "x").unwrap();
        std::fs::set_permissions(&illisible, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::File::open(&illisible).is_ok() {
            return; // exécuté en root : le fichier reste lisible, rien à tester.
        }
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let r = sauvegarder_avec_taille(&c, &dest, &ProgressionMuette, None, ouvrir_fichier);
        assert!(
            matches!(r, Err(Erreur::Commande { ref programme, .. }) if programme == "tar"),
            "{r:?}"
        );
        assert!(noms(&dest.join(DOSSIER_SAUVEGARDE)).is_empty());
    }

    #[test]
    fn refus_si_installation_ou_destination_absente() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let _verrou = poser_verrou_installation(&c).unwrap();
        assert!(matches!(
            sauvegarde_1mo(&c, &dest),
            Err(Erreur::InstallationEnCours)
        ));
        match sauvegarde_1mo(&c, &d.path().join("absent")) {
            Err(Erreur::Sauvegarde(texte)) => assert!(texte.contains("absent"), "{texte}"),
            autre => panic!("attendu Sauvegarde, obtenu {autre:?}"),
        }
    }

    #[test]
    fn refus_d_une_destination_dans_les_donnees_ou_le_prefixe() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dans_donnees = c.donnees.join("sauvegardes");
        std::fs::create_dir_all(&dans_donnees).unwrap();
        for dest in [dans_donnees, c.drive_c()] {
            match sauvegarde_1mo(&c, &dest) {
                Err(Erreur::Sauvegarde(_)) => {}
                autre => panic!("{dest:?} : attendu Sauvegarde, obtenu {autre:?}"),
            }
        }
        // Le même dossier atteint par un lien symbolique est aussi refusé.
        let lien = d.path().join("raccourci");
        std::os::unix::fs::symlink(&c.donnees, &lien).unwrap();
        assert!(matches!(
            sauvegarde_1mo(&c, &lien),
            Err(Erreur::Sauvegarde(_))
        ));
        assert!(noms(&c.donnees).iter().all(|n| n != DOSSIER_SAUVEGARDE));
    }

    #[test]
    // `serial` : le message vérifié est traduit ; les tests d'i18n changent la langue globale.
    #[serial_test::serial]
    fn empreinte_alteree_refusee_et_environnement_intact() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let sha = sauvegarde_1mo(&c, &dest).unwrap();
        let archive = sha.with_extension("").with_extension("zst");
        let mut octets = std::fs::read(&archive).unwrap_or_default();
        if octets.is_empty() {
            // découpage éventuel : altérer le premier morceau
            let part = PathBuf::from(format!("{}.part000", archive.display()));
            octets = std::fs::read(&part).unwrap();
            octets[10] ^= 0xff;
            std::fs::write(part, octets).unwrap();
        } else {
            octets[10] ^= 0xff;
            std::fs::write(&archive, octets).unwrap();
        }
        std::fs::write(c.prefixe.join("temoin.txt"), "intact").unwrap();
        let r = restaurer(&c, &sha, &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::Empreinte { .. })), "{r:?}");
        assert!(c.prefixe.join("temoin.txt").exists());
        assert!(!noms(&c.donnees)
            .iter()
            .any(|n| n.contains("ancien") || n == EXTRACTION));
    }

    #[test]
    fn morceau_manquant_ou_suite_incomplete_refuse() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        ecrire_incompressible(&c.drive_c().join("Program Files/gros.bin"));
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let sha = sauvegarde_1mo(&c, &dest).unwrap();
        let dossier = sha.parent().unwrap().to_path_buf();
        let part1 = noms(&dossier)
            .into_iter()
            .find(|n| n.ends_with(".part001"))
            .unwrap();
        std::fs::remove_file(dossier.join(part1)).unwrap();
        assert!(matches!(
            restaurer(&c, &sha, &ProgressionMuette),
            Err(Erreur::Sauvegarde(_))
        ));
        // Liste amputée de ce morceau : trou dans la suite.
        let texte = std::fs::read_to_string(&sha).unwrap();
        let sans: String = texte
            .lines()
            .filter(|l| !l.ends_with(".part001"))
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(&sha, sans).unwrap();
        assert!(matches!(
            restaurer(&c, &sha, &ProgressionMuette),
            Err(Erreur::Sauvegarde(_))
        ));
    }

    #[test]
    fn extraction_ratee_garde_l_ancien_et_nettoie() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        // Archive « valide » pour les empreintes mais qui n'est pas du zstd.
        let dossier = d.path().join("faux");
        std::fs::create_dir_all(&dossier).unwrap();
        std::fs::write(dossier.join("a.tar.zst"), "pas du zstd").unwrap();
        let hex = empreinte(&dossier.join("a.tar.zst")).unwrap();
        std::fs::write(
            dossier.join("a.tar.zst.sha256"),
            format!("{hex}  a.tar.zst\n"),
        )
        .unwrap();
        // Reste d'une tentative précédente : doit être supprimé.
        std::fs::create_dir_all(c.donnees.join(EXTRACTION)).unwrap();
        std::fs::write(c.donnees.join(EXTRACTION).join("vieux"), "x").unwrap();
        let r = restaurer(&c, &dossier.join("a.tar.zst.sha256"), &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::Commande { .. })), "{r:?}");
        assert!(c.drive_c().join("Program Files/Xpand!2/é.dll").exists());
        assert!(!c.donnees.join(EXTRACTION).exists());
        assert!(!noms(&c.donnees).iter().any(|n| n.contains("ancien")));
    }

    #[test]
    fn tar_n_ecrit_pas_hors_du_dossier_d_extraction() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let travail = d.path().join("piege");
        std::fs::create_dir_all(&travail).unwrap();
        std::fs::write(travail.join("f"), "piege").unwrap();
        let absolu = d.path().join("absolu.txt");
        for (nom, cible) in [
            ("dd", "../evade.txt".to_string()),
            ("abs", absolu.display().to_string()),
        ] {
            let tar = travail.join(format!("{nom}.tar"));
            let st = Command::new("tar")
                .args(["-P", "-cf"])
                .arg(&tar)
                .arg(format!("--transform=s,^f$,{cible},"))
                .args(["-C"])
                .arg(&travail)
                .arg("f")
                .output()
                .unwrap();
            assert!(st.status.success());
            let zst = travail.join(format!("{nom}.tar.zst"));
            let z = Command::new("zstd")
                .arg("-q")
                .arg(&tar)
                .arg("-o")
                .arg(&zst)
                .output()
                .unwrap();
            assert!(z.status.success());
            let hex = empreinte(&zst).unwrap();
            let sha = travail.join(format!("{nom}.sha256"));
            std::fs::write(&sha, format!("{hex}  {nom}.tar.zst\n")).unwrap();
            // Archive piégée : refus (`..` rejeté par tar) ou contenu invraisemblable
            // (`/` retiré mais pas de `drive_c`) ; dans les deux cas rien n'est remplacé.
            let r = restaurer(&c, &sha, &ProgressionMuette);
            assert!(r.is_err(), "{nom} : {r:?}");
            assert!(c.drive_c().join("Program Files/Xpand!2/é.dll").exists());
            assert!(!c.donnees.join(EXTRACTION).exists());
        }
        assert!(!c.donnees.join("evade.txt").exists() && !d.path().join("evade.txt").exists());
        assert!(!absolu.exists());
    }

    #[test]
    fn taille_et_exclusions() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let temp = c.drive_c().join("users/u/AppData/Local/Temp");
        std::fs::create_dir_all(&temp).unwrap();
        let avant = taille_prefixe(&c.prefixe);
        std::fs::write(temp.join("t"), vec![0u8; 5000]).unwrap();
        assert_eq!(
            taille_prefixe(&c.prefixe),
            avant,
            "Temp est exclu du calcul"
        );
        std::fs::write(c.drive_c().join("users/u/Documents.txt"), vec![0u8; 5000]).unwrap();
        // Le fichier compte (la taille du dossier qui le contient varie selon le système de fichiers).
        let apres = taille_prefixe(&c.prefixe);
        assert!(
            (avant + 5000..avant + 5200).contains(&apres),
            "{avant} -> {apres}"
        );
        assert!(est_exclu(Path::new("drive_c/users/bob/Downloads")));
        assert!(est_exclu(Path::new("drive_c/users/bob/AppData/Local/dxvk")));
        assert!(!est_exclu(Path::new("drive_c/users/bob/Documents")));
    }

    #[test]
    // `serial` : le message vérifié est traduit ; les tests d'i18n changent la langue globale.
    #[serial_test::serial]
    fn controle_de_la_place_libre() {
        let dest = Path::new("/disque");
        assert!(controler_place(100, 101, dest).is_ok());
        // Limite : libre == besoin est refusé (comme `[ libre -gt besoin ]` du script).
        assert!(matches!(
            controler_place(100, 100, dest),
            Err(Erreur::Sauvegarde(_))
        ));
        let e = controler_place(5_000_000, 1_000_000, dest)
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("/disque") && e.contains('5') && e.contains('1'),
            "{e}"
        );
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        assert!(verifier_place(&c.prefixe, d.path()).is_ok());
    }

    #[test]
    // `serial` : le message vérifié est traduit ; les tests d'i18n changent la langue globale.
    #[serial_test::serial]
    fn refus_d_une_sauvegarde_existante() {
        let d = tempfile::tempdir().unwrap();
        let archive = d.path().join("wimao-2026-01-01-10h00.tar.zst");
        assert!(refuser_si_existe(&archive, true).is_ok());
        for (nom, decoupe) in [
            ("wimao-2026-01-01-10h00.tar.zst", false),
            ("wimao-2026-01-01-10h00.tar.zst.sha256", false),
            ("wimao-2026-01-01-10h00.tar.zst.part000", true),
        ] {
            let f = d.path().join(nom);
            std::fs::write(&f, "x").unwrap();
            let e = refuser_si_existe(&archive, decoupe)
                .unwrap_err()
                .to_string();
            assert!(
                e.contains("existe déjà") || e.contains("already exists"),
                "{e}"
            );
            std::fs::remove_file(f).unwrap();
        }
    }

    #[test]
    fn archive_extraite_a_la_main_avec_tar_simple() {
        // Ce que ferait un utilisateur sans WiMao : `zstd -dc | tar -xf -`.
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dest = d.path().join("disque");
        std::fs::create_dir_all(&dest).unwrap();
        let sha =
            sauvegarder_avec_taille(&c, &dest, &ProgressionMuette, None, ouvrir_fichier).unwrap();
        let dossier = sha.parent().unwrap();
        let sortie = d.path().join("manuel");
        std::fs::create_dir_all(&sortie).unwrap();
        let archive = noms(dossier)
            .into_iter()
            .find(|n| n.ends_with(".tar.zst"))
            .unwrap();
        let mut zstd = Command::new("zstd")
            .arg("-dc")
            .arg(dossier.join(archive))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let tar = Command::new("tar")
            .args(["-xf", "-", "-C"])
            .arg(&sortie)
            .stdin(Stdio::from(zstd.stdout.take().unwrap()))
            .output()
            .unwrap();
        assert!(
            tar.status.success(),
            "{}",
            String::from_utf8_lossy(&tar.stderr)
        );
        zstd.wait().unwrap();
        let racine = sortie.join("prefixe");
        let dur = std::fs::metadata(racine.join("drive_c/Program Files/dur")).unwrap();
        let orig = std::fs::metadata(racine.join("drive_c/Program Files/un fichier.txt")).unwrap();
        assert_eq!(
            std::os::unix::fs::MetadataExt::ino(&dur),
            std::os::unix::fs::MetadataExt::ino(&orig)
        );
        assert_eq!(
            std::fs::read_link(racine.join("lien")).unwrap(),
            Path::new("../x")
        );
    }

    #[test]
    fn archive_invraisemblable_ne_remplace_pas_l_environnement() {
        let d = tempfile::tempdir().unwrap();
        let c = banc(d.path());
        let dossier = d.path().join("autre");
        std::fs::create_dir_all(dossier.join("racine/dossier")).unwrap();
        std::fs::write(dossier.join("racine/dossier/f"), "x").unwrap();
        let tar = dossier.join("a.tar");
        Command::new("tar")
            .arg("-cf")
            .arg(&tar)
            .arg("-C")
            .arg(&dossier)
            .arg("racine")
            .output()
            .unwrap();
        let zst = dossier.join("a.tar.zst");
        Command::new("zstd")
            .arg("-q")
            .arg(&tar)
            .arg("-o")
            .arg(&zst)
            .output()
            .unwrap();
        let sha = dossier.join("a.sha256");
        std::fs::write(&sha, format!("{}  a.tar.zst\n", empreinte(&zst).unwrap())).unwrap();
        let r = restaurer(&c, &sha, &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::Sauvegarde(_))), "{r:?}");
        assert!(c.drive_c().join("Program Files/Xpand!2/é.dll").exists());
        assert!(!c.donnees.join(EXTRACTION).exists());
    }

    #[test]
    fn pieces_listees_rend_les_morceaux_sans_verifier_les_empreintes() {
        let d = tempfile::tempdir().unwrap();
        let h = "a".repeat(64); // fausse empreinte : elle ne doit pas être contrôlée
        std::fs::write(d.path().join("a.tar.zst.part000"), b"x").unwrap();
        std::fs::write(d.path().join("a.tar.zst.part001"), b"y").unwrap();
        let sha = d.path().join("a.sha256");
        std::fs::write(
            &sha,
            format!("{h}  a.tar.zst.part000\n{h} *a.tar.zst.part001\n"),
        )
        .unwrap();
        let pieces = pieces_listees(&sha).unwrap();
        assert_eq!(pieces.len(), 2);
        assert!(pieces[0].ends_with("a.tar.zst.part000"));
        // Un morceau absent : le message du noyau.
        std::fs::remove_file(d.path().join("a.tar.zst.part001")).unwrap();
        let e = pieces_listees(&sha).unwrap_err();
        assert!(matches!(e, Erreur::Sauvegarde(_)), "{e:?}");
    }

    #[test]
    fn lecture_des_empreintes() {
        let h = "a".repeat(64);
        assert!(lire_empreintes(&format!("{h}  a.part000\n{h} *a.part001\n")).is_ok());
        // Mode binaire (`*nom`) : le nom retenu est sans l'étoile.
        let binaire = lire_empreintes(&format!("{h} *a.tar.zst\n")).unwrap();
        assert!(binaire.contains_key("a.tar.zst"));
        assert!(lire_empreintes(&format!("{h}  ../x\n")).is_err());
        assert!(lire_empreintes(&format!("{h}  d/x\n")).is_err());
        assert!(lire_empreintes("zz  x\n").is_err());
        assert!(lire_empreintes("\n").is_err());
    }

    #[test]
    fn fat_est_reconnu_par_statfs_sans_faux_positif() {
        let d = tempfile::tempdir().unwrap();
        assert!(
            !est_fat(d.path()).unwrap(),
            "un dossier temporaire n'est pas en FAT"
        );
        assert!(espace_libre(d.path()).unwrap() > 0);
    }
}
