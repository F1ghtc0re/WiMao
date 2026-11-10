//! Applications ajoutées par un installeur : lecture des raccourcis `.lnk` du menu Démarrer
//! de Wine et liste enregistrée dans `<données>/applications.toml`.
//!
//! Certains installeurs (BLEASS Plugins Installer…) n'installent pas de plugin : ils posent un
//! programme et un raccourci dans le menu Démarrer. WiMao relève les raccourcis avant et après
//! une installation, retient les nouveaux qui visent un vrai programme, et les range dans une
//! liste que l'interface montre dans « Gestionnaires » (ouverture par `gestionnaire::ouvrir`
//! grâce à [`Application::vers_recette`]).
//!
//! - [`cible_raccourci`] : lecteur du format MS-SHLLINK, écrit à la main (pas de dépendance) ;
//! - [`releve_raccourcis`] / [`nouvelles_applications`] : détection (spec § 3.1) ;
//! - [`lire_applications`], [`enregistrer_applications`], [`ajouter_applications`],
//!   [`retirer_application`], [`applications_presentes`] : la liste enregistrée (spec § 3.2).
//!
//! Utilise `chemins_windows` (traduction des chemins), `recette` (recette en mémoire) et
//! `wimao-core` (chemins, erreurs, journal, écriture atomique, date). Utilisé par
//! `installation` (détection), `wimao-cli` et `wimao` (liste, ouverture, retrait).
//!
//! Sécurité : un `.lnk` est écrit par n'importe quel installeur ; le lecteur ne panique jamais,
//! ne lit jamais hors du tampon (`get`, arithmétique vérifiée) et refuse plus de 1 Mo.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use wimao_core::i18n::trf;
use wimao_core::outils::date_du_jour;
use wimao_core::restauration::ecrire_atomique;
use wimao_core::{journal, Chemins, Erreur, Resultat};

use crate::chemins_windows::{self, utilisateur_windows};
use crate::recette::{Recette, TypeRecette};

/// Taille maximale d'un `.lnk` lu (1 Mo) : un vrai raccourci pèse quelques centaines d'octets.
pub(crate) const TAILLE_MAX_RACCOURCI: usize = 1 << 20;
/// Taille de l'en-tête `ShellLinkHeader` (`HeaderSize` vaut toujours 0x4C).
const TAILLE_ENTETE: usize = 0x4C;
/// CLSID `00021401-0000-0000-C000-000000000046` d'un raccourci, tel qu'écrit dans le fichier.
pub(crate) const CLSID_LIEN: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];
/// `LinkFlags` bit 0 : une `LinkTargetIDList` suit l'en-tête.
const A_UNE_LISTE_ID: u32 = 1;
/// `LinkFlags` bit 1 : un `LinkInfo` suit.
const A_UN_LINK_INFO: u32 = 1 << 1;
/// `LinkInfoFlags` bit 0 : le chemin local (`LocalBasePath`) est présent.
const CHEMIN_LOCAL_PRESENT: u32 = 1;
/// Taille minimale de l'en-tête du `LinkInfo` (champs ANSI seulement).
const ENTETE_LINK_INFO_MIN: usize = 0x1C;
/// À partir de cette taille d'en-tête, les décalages Unicode existent.
const ENTETE_LINK_INFO_UNICODE: usize = 0x24;

/// Fichier de la liste, dans les données de WiMao.
const FICHIER_APPLICATIONS: &str = "applications.toml";
/// Menu Démarrer commun à tous les utilisateurs, sous `drive_c`.
const MENU_COMMUN: &str = "ProgramData/Microsoft/Windows/Start Menu/Programs";
/// Menu Démarrer d'un utilisateur, sous `drive_c/users/<nom>`.
const MENU_UTILISATEUR: &str = "AppData/Roaming/Microsoft/Windows/Start Menu/Programs";
/// Dossier du menu dont les raccourcis se lancent au démarrage de Windows : jamais une application.
const DOSSIER_DEMARRAGE: &str = "startup";
/// Préfixes (en minuscules) des noms de raccourci qui désinstallent.
const PREFIXES_DESINSTALLATION: [&str; 2] = ["uninstall", "désinstaller"];
/// Préfixes (en minuscules) des noms d'exécutable qui désinstallent (`unins000.exe`, `uninstall.exe`).
const PREFIXES_EXE_DESINSTALLATION: [&str; 2] = ["unins", "uninstall"];

// ───────────────────────── Lecture d'un raccourci `.lnk` ─────────────────────────

/// Lit un entier little-endian de 16 bits à `decalage` ; `None` si le tampon est trop court.
fn lire_u16(octets: &[u8], decalage: usize) -> Option<u16> {
    let fin = decalage.checked_add(2)?;
    Some(u16::from_le_bytes(
        octets.get(decalage..fin)?.try_into().ok()?,
    ))
}

/// Lit un entier little-endian de 32 bits à `decalage` ; `None` si le tampon est trop court.
fn lire_u32(octets: &[u8], decalage: usize) -> Option<u32> {
    let fin = decalage.checked_add(4)?;
    Some(u32::from_le_bytes(
        octets.get(decalage..fin)?.try_into().ok()?,
    ))
}

/// Texte ANSI terminé par 0 qui commence à `debut` dans `zone`. L'ASCII est gardé tel quel ; tout
/// autre octet (Windows-1252 étendu) devient `U+FFFD`. `None` sans terminateur dans la zone.
fn texte_ansi(zone: &[u8], debut: usize) -> Option<String> {
    let reste = zone.get(debut..)?;
    let fin = reste.iter().position(|&o| o == 0)?;
    Some(
        reste[..fin]
            .iter()
            .map(|&o| {
                if o.is_ascii() {
                    char::from(o)
                } else {
                    char::REPLACEMENT_CHARACTER
                }
            })
            .collect(),
    )
}

/// Texte UTF-16 little-endian terminé par un mot nul qui commence à `debut` dans `zone`.
/// `None` sans terminateur dans la zone.
fn texte_unicode(zone: &[u8], debut: usize) -> Option<String> {
    let reste = zone.get(debut..)?;
    let mut mots = Vec::new();
    let (paires, _) = reste.as_chunks::<2>();
    for &paire in paires {
        let mot = u16::from_le_bytes(paire);
        if mot == 0 {
            return Some(String::from_utf16_lossy(&mots));
        }
        mots.push(mot);
    }
    None
}

/// Chemin Windows de la cible d'un raccourci `.lnk` (format MS-SHLLINK), ou `None` si le fichier
/// est abîmé, trop gros (plus de 1 Mo) ou sans chemin local.
///
/// Déroulé : en-tête de 76 octets (taille et CLSID vérifiés), saut de la `LinkTargetIDList` si
/// présente, puis lecture du `LinkInfo` : le chemin Unicode est préféré (en-tête >= 0x24), à
/// défaut le chemin ANSI. Les décalages du `LinkInfo` sont relatifs à son début et ne sortent
/// jamais de sa zone.
pub fn cible_raccourci(octets: &[u8]) -> Option<String> {
    if octets.len() > TAILLE_MAX_RACCOURCI
        || lire_u32(octets, 0)? != TAILLE_ENTETE as u32
        || octets.get(4..20)? != CLSID_LIEN
    {
        return None;
    }
    let drapeaux = lire_u32(octets, 20)?;
    let mut position = TAILLE_ENTETE;
    if drapeaux & A_UNE_LISTE_ID != 0 {
        let taille_liste = usize::from(lire_u16(octets, position)?);
        position = position.checked_add(2)?.checked_add(taille_liste)?;
    }
    if drapeaux & A_UN_LINK_INFO == 0 {
        return None;
    }
    let taille_info = usize::try_from(lire_u32(octets, position)?).ok()?;
    let info = octets.get(position..position.checked_add(taille_info)?)?;
    let taille_entete = usize::try_from(lire_u32(info, 4)?).ok()?;
    if taille_entete < ENTETE_LINK_INFO_MIN || taille_entete > info.len() {
        return None;
    }
    if lire_u32(info, 8)? & CHEMIN_LOCAL_PRESENT == 0 {
        return None;
    }
    let unicode = if taille_entete >= ENTETE_LINK_INFO_UNICODE {
        let decalage = usize::try_from(lire_u32(info, 28)?).ok()?;
        // Décalage nul : le champ est absent, on retombe sur l'ANSI. Non nul mais hors de la
        // zone : le fichier est abîmé, on ne devine pas.
        if decalage == 0 {
            None
        } else {
            Some(texte_unicode(info, decalage)?)
        }
    } else {
        None
    };
    let chemin = match unicode {
        Some(c) => c,
        None => texte_ansi(info, usize::try_from(lire_u32(info, 16)?).ok()?)?,
    };
    (!chemin.is_empty()).then_some(chemin)
}

// ───────────────────────── Détection des nouvelles applications ─────────────────────────

/// Un programme ajouté par un installeur, tel qu'enregistré dans `applications.toml`
/// (une table `[[application]]` par programme).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    /// Nom affiché : celui du raccourci, sans `.lnk`.
    pub nom: String,
    /// Chemin Windows de l'exécutable (cible du raccourci).
    pub programme: String,
    /// Chemin du `.lnk`, relatif au préfixe.
    pub raccourci: PathBuf,
    /// Date d'ajout (`AAAA-MM-JJ`).
    pub ajoutee: String,
}

impl Application {
    /// Recette de gestionnaire en mémoire pour ouvrir ce programme avec `gestionnaire::ouvrir` :
    /// ni arguments, ni pilote, ni consigne ; à la fermeture, on arrête l'exécutable lui-même.
    pub fn vers_recette(&self) -> Recette {
        let exe = nom_de_fichier_windows(&self.programme).to_string();
        Recette::gestionnaire_minimal(&self.nom, &self.programme, vec![exe])
    }
}

/// Nom de fichier d'un chemin Windows (coupe sur `\` et `/`).
fn nom_de_fichier_windows(chemin: &str) -> &str {
    chemin.rsplit(['\\', '/']).next().unwrap_or(chemin)
}

/// Forme comparable d'un chemin Windows : minuscules, `/` remplacé par `\`.
fn normaliser(chemin: &str) -> String {
    chemin.to_lowercase().replace('/', "\\")
}

/// Tous les `.lnk` (extension sans tenir compte des majuscules) des deux menus Démarrer du
/// préfixe : celui de tous les utilisateurs et celui de l'utilisateur Windows de WiMao.
/// Chemins **relatifs au préfixe** ; un dossier absent est ignoré, les liens symboliques ne
/// sont pas suivis.
pub fn releve_raccourcis(c: &Chemins) -> BTreeSet<PathBuf> {
    let drive_c = c.drive_c();
    let menus = [
        drive_c.join(MENU_COMMUN),
        drive_c
            .join("users")
            .join(utilisateur_windows(c))
            .join(MENU_UTILISATEUR),
    ];
    let mut trouves = BTreeSet::new();
    for menu in menus {
        for entree in WalkDir::new(&menu).into_iter().flatten() {
            let est_lnk = entree
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"));
            if entree.file_type().is_file() && est_lnk {
                if let Ok(relatif) = entree.path().strip_prefix(&c.prefixe) {
                    trouves.insert(relatif.to_path_buf());
                }
            }
        }
    }
    trouves
}

/// Contenu d'un raccourci (au plus [`TAILLE_MAX_RACCOURCI`] + 1 octets lus) ; `None` si illisible.
fn lire_borne(chemin: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut octets = Vec::new();
    std::fs::File::open(chemin)
        .ok()?
        .take(TAILLE_MAX_RACCOURCI as u64 + 1)
        .read_to_end(&mut octets)
        .ok()?;
    Some(octets)
}

/// Une application si le raccourci `relatif` passe tous les filtres de la spec § 3.1, sinon `None`.
fn application_du_raccourci(
    c: &Chemins,
    relatif: &Path,
    connus: &BTreeSet<String>,
) -> Option<Application> {
    // Pas dans un dossier « StartUp ».
    let au_demarrage = relatif.components().any(|p| {
        p.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(DOSSIER_DEMARRAGE)
    });
    if au_demarrage {
        return None;
    }
    // Nom du raccourci : pas un désinstalleur.
    let nom = relatif.file_stem()?.to_string_lossy().into_owned();
    let nom_min = nom.to_lowercase();
    if PREFIXES_DESINSTALLATION
        .iter()
        .any(|p| nom_min.starts_with(p))
    {
        return None;
    }
    // Cible : un .exe qui n'est pas un désinstalleur, déjà connu, ni absent du préfixe.
    let lien = c.prefixe.join(relatif);
    if !std::fs::symlink_metadata(&lien).ok()?.is_file() {
        return None;
    }
    let programme = cible_raccourci(&lire_borne(&lien)?)?;
    let exe = nom_de_fichier_windows(&programme).to_lowercase();
    let desinstalleur = PREFIXES_EXE_DESINSTALLATION
        .iter()
        .any(|p| exe.starts_with(p));
    if !exe.ends_with(".exe") || desinstalleur || connus.contains(&normaliser(&programme)) {
        return None;
    }
    let existe = chemins_windows::vers_linux(&c.drive_c(), &programme)
        .ok()
        .is_some_and(|p| p.is_file());
    existe.then(|| Application {
        nom,
        programme,
        raccourci: relatif.to_path_buf(),
        ajoutee: date_du_jour(),
    })
}

/// Applications nées entre deux relevés : les raccourcis de `apres` absents de `avant` qui
/// passent les filtres de la spec § 3.1 (hors StartUp, ni désinstalleur, cible `.exe` présente
/// dans le préfixe, programme inconnu). La condition « aucun plugin ajouté » est vérifiée par
/// l'appelant. `programmes_connus` : chemins Windows des programmes des recettes de gestionnaire
/// (comparés sans tenir compte des majuscules ni de `/` ou `\`). Un même programme n'est
/// retenu qu'une fois.
pub fn nouvelles_applications(
    c: &Chemins,
    avant: &BTreeSet<PathBuf>,
    apres: &BTreeSet<PathBuf>,
    programmes_connus: &[String],
) -> Vec<Application> {
    let connus: BTreeSet<String> = programmes_connus.iter().map(|p| normaliser(p)).collect();
    let mut vues = BTreeSet::new();
    apres
        .difference(avant)
        .filter_map(|relatif| application_du_raccourci(c, relatif, &connus))
        .filter(|a| vues.insert(normaliser(&a.programme)))
        .collect()
}

/// Programmes (chemins Windows) des recettes de gestionnaire de `recettes`, **résolus** dans le
/// préfixe : à passer à [`nouvelles_applications`] comme `programmes_connus`, pour qu'un
/// raccourci vers un gestionnaire déjà installé ne devienne jamais une application.
///
/// Les `programme` des recettes contiennent des variables (`{appdata-local}`) et des jokers
/// (`app-*`, dossier de version) alors que la cible d'un raccourci est un chemin précis :
/// les variables sont remplacées, puis le joker est résolu sur le disque (la version la plus
/// haute). Le texte non résolu est gardé aussi (gestionnaire pas encore installé : inoffensif).
/// Les recettes qui ne sont pas des gestionnaires sont ignorées.
pub fn programmes_des_gestionnaires<'a>(
    c: &Chemins,
    recettes: impl IntoIterator<Item = &'a Recette>,
) -> Vec<String> {
    let drive_c = c.drive_c();
    let utilisateur = utilisateur_windows(c);
    let mut programmes = Vec::new();
    for g in recettes
        .into_iter()
        .filter(|r| r.type_ == TypeRecette::Gestionnaire)
        .filter_map(|r| r.gestionnaire.as_ref())
    {
        let texte = chemins_windows::remplacer_variables(&g.programme, &utilisateur);
        // Chemin réel (casse du disque, joker remplacé), reconverti en chemin Windows.
        let resolu = chemins_windows::resoudre_motif(&drive_c, &texte)
            .ok()
            .and_then(|p| p.strip_prefix(&drive_c).ok().map(chemin_windows_depuis));
        programmes.push(texte);
        programmes.extend(resolu);
    }
    programmes
}

/// `C:\a\b.exe` pour le chemin relatif `a/b.exe` de `drive_c`.
fn chemin_windows_depuis(relatif: &Path) -> String {
    let composants: Vec<String> = relatif
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    format!("C:\\{}", composants.join("\\"))
}

// ───────────────────────── Liste enregistrée ─────────────────────────

/// Contenu du fichier `applications.toml` : la table `[[application]]`.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FichierApplications {
    /// Une entrée par application.
    #[serde(default)]
    application: Vec<Application>,
}

/// Chemin de `applications.toml`.
fn fichier(c: &Chemins) -> PathBuf {
    c.donnees.join(FICHIER_APPLICATIONS)
}

/// Lit la liste : fichier absent = liste vide ; illisible = erreur (ni écrasé ni effacé).
fn lire_strict(c: &Chemins) -> Resultat<Vec<Application>> {
    let chemin = fichier(c);
    let texte = match std::fs::read_to_string(&chemin) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(erreur_lecture(&chemin, &e.to_string())),
    };
    toml::from_str::<FichierApplications>(&texte)
        .map(|f| f.application)
        .map_err(|e| erreur_lecture(&chemin, &e.to_string()))
}

/// Erreur « fichier illisible » (message déjà traduit ailleurs).
fn erreur_lecture(chemin: &Path, cause: &str) -> Erreur {
    Erreur::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        trf(
            "Cannot read {path}: {error}",
            &[("path", &chemin.display().to_string()), ("error", cause)],
        ),
    ))
}

/// Liste enregistrée. Jamais d'erreur bloquante : fichier absent = vide ; fichier illisible =
/// noté dans le journal et vide, **sans** être modifié.
pub fn lire_applications(c: &Chemins) -> Vec<Application> {
    lire_strict(c).unwrap_or_else(|e| {
        journal::noter("applications.toml illisible", &e.to_string());
        Vec::new()
    })
}

/// Remplace la liste enregistrée (écriture atomique : fichier temporaire puis renommage).
///
/// # Erreurs
/// `Erreur::Io` si le dossier ou le fichier ne peut pas être écrit.
pub fn enregistrer_applications(c: &Chemins, liste: &[Application]) -> Resultat<()> {
    let contenu = FichierApplications {
        application: liste.to_vec(),
    };
    let texte = toml::to_string(&contenu).map_err(|e| {
        Erreur::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        ))
    })?;
    std::fs::create_dir_all(&c.donnees)?;
    ecrire_atomique(&fichier(c), texte.as_bytes())
}

/// Ajoute `nouvelles` à la liste, sans doublon de `programme` (comparaison sans tenir compte
/// des majuscules ni de `/` ou `\`). Refuse de toucher à un fichier illisible : on ne perd
/// pas la liste de la personne.
///
/// # Erreurs
/// `Erreur::Io` si le fichier existant est illisible ou si l'écriture échoue.
pub fn ajouter_applications(c: &Chemins, nouvelles: Vec<Application>) -> Resultat<()> {
    let mut liste = lire_strict(c)?;
    let mut changee = false;
    for nouvelle in nouvelles {
        let cle = normaliser(&nouvelle.programme);
        if !liste.iter().any(|a| normaliser(&a.programme) == cle) {
            liste.push(nouvelle);
            changee = true;
        }
    }
    if changee {
        enregistrer_applications(c, &liste)?;
    }
    Ok(())
}

/// Retire de la liste l'application `nom` (la ligne du fichier seulement : le programme et son
/// raccourci restent sur le disque). Renvoie `false` si aucune ne porte ce nom.
///
/// # Erreurs
/// `Erreur::Io` si le fichier est illisible ou ne peut pas être écrit.
pub fn retirer_application(c: &Chemins, nom: &str) -> Resultat<bool> {
    let mut liste = lire_strict(c)?;
    let avant = liste.len();
    liste.retain(|a| a.nom != nom);
    if liste.len() == avant {
        return Ok(false);
    }
    enregistrer_applications(c, &liste)?;
    Ok(true)
}

/// Les applications de la liste dont le programme existe encore dans le préfixe, triées par nom
/// sans tenir compte des majuscules. Une application dont le programme a disparu reste dans
/// le fichier et réapparaît si le programme revient (voir [`lire_applications`]).
pub fn applications_presentes(c: &Chemins) -> Vec<Application> {
    let drive_c = c.drive_c();
    let mut liste: Vec<Application> = lire_applications(c)
        .into_iter()
        .filter(|a| {
            chemins_windows::vers_linux(&drive_c, &a.programme)
                .ok()
                .is_some_and(|p| p.is_file())
        })
        .collect();
    liste.sort_by_key(|a| a.nom.to_lowercase());
    liste
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Dossier des vrais raccourcis faits par Wine (sans donnée personnelle).
    fn dossier_raccourcis() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/raccourcis")
    }

    /// Les raccourcis de test et la cible que `strings` y relève.
    const RACCOURCIS_WINE: [(&str, &str); 2] = [
        (
            "Hype.lnk",
            r"C:\Program Files\Air Music Technology\Hype\Hype.exe",
        ),
        (
            "Essential-Keyboards.lnk",
            r"C:\Program Files\VstPlugins\Essential Keyboards\Essential Keyboards.exe",
        ),
    ];

    /// Construit un `.lnk` minimal sans `LinkTargetIDList`, avec un `LinkInfo` qui porte le
    /// chemin en ANSI et, si `unicode` est donné, en Unicode (en-tête de 0x24 octets).
    pub(crate) fn fabriquer_lnk(ansi: &[u8], unicode: Option<&str>) -> Vec<u8> {
        let mut lnk = vec![0u8; 76];
        lnk[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        lnk[4..20].copy_from_slice(&CLSID_LIEN);
        lnk[20..24].copy_from_slice(&2u32.to_le_bytes()); // HasLinkInfo
        let entete: u32 = if unicode.is_some() { 0x24 } else { 0x1C };
        let volume = entete; // un VolumeID bidon de 16 octets
        let base_ansi = volume + 16;
        let mut corps = vec![0u8; 16];
        corps.extend_from_slice(ansi);
        corps.push(0);
        let suffixe = base_ansi + ansi.len() as u32 + 1;
        corps.push(0); // CommonPathSuffix vide
        let base_unicode = suffixe + 1;
        if let Some(u) = unicode {
            for unite in u.encode_utf16() {
                corps.extend_from_slice(&unite.to_le_bytes());
            }
            corps.extend_from_slice(&[0, 0]);
        }
        let taille = entete + corps.len() as u32;
        let mut info = Vec::new();
        info.extend_from_slice(&taille.to_le_bytes());
        info.extend_from_slice(&entete.to_le_bytes());
        info.extend_from_slice(&1u32.to_le_bytes()); // VolumeIDAndLocalBasePath
        info.extend_from_slice(&volume.to_le_bytes());
        info.extend_from_slice(&base_ansi.to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes());
        info.extend_from_slice(&suffixe.to_le_bytes());
        if unicode.is_some() {
            info.extend_from_slice(&base_unicode.to_le_bytes());
            info.extend_from_slice(&0u32.to_le_bytes());
        }
        info.extend_from_slice(&corps);
        lnk.extend_from_slice(&info);
        lnk
    }

    #[test]
    fn cible_des_raccourcis_de_wine() {
        for (fichier, attendu) in RACCOURCIS_WINE {
            let octets = std::fs::read(dossier_raccourcis().join(fichier)).unwrap();
            assert_eq!(
                cible_raccourci(&octets).as_deref(),
                Some(attendu),
                "{fichier}"
            );
        }
    }

    #[test]
    fn cible_unicode_prioritaire() {
        let lnk = fabriquer_lnk(b"C:\\Ansi\\a.exe", Some(r"C:\Unicode\é.exe"));
        assert_eq!(cible_raccourci(&lnk).as_deref(), Some(r"C:\Unicode\é.exe"));
    }

    #[test]
    fn cible_ansi() {
        let lnk = fabriquer_lnk(b"C:\\Ansi\\a.exe", None);
        assert_eq!(cible_raccourci(&lnk).as_deref(), Some(r"C:\Ansi\a.exe"));
        // Un octet hors ASCII est remplacé, jamais une panique.
        let lnk = fabriquer_lnk(b"C:\\Ans\xe9\\a.exe", None);
        assert_eq!(
            cible_raccourci(&lnk).as_deref(),
            Some("C:\\Ans\u{FFFD}\\a.exe")
        );
    }

    #[test]
    fn raccourci_abime_rend_none() {
        assert_eq!(cible_raccourci(&[]), None);
        assert_eq!(cible_raccourci(&[0x4C, 0, 0, 0, 1, 2, 3, 4, 5, 6]), None);
        // CLSID faux.
        let mut lnk = fabriquer_lnk(b"C:\\a.exe", None);
        lnk[10] ^= 0xFF;
        assert_eq!(cible_raccourci(&lnk), None);
        // HeaderSize faux.
        let mut lnk = fabriquer_lnk(b"C:\\a.exe", None);
        lnk[0] = 0x4D;
        assert_eq!(cible_raccourci(&lnk), None);
        // Sans LinkInfo.
        let mut lnk = fabriquer_lnk(b"C:\\a.exe", None);
        lnk[20] = 0;
        assert_eq!(cible_raccourci(&lnk), None);
        // Offsets pointant au-delà du tampon (ANSI, puis Unicode, puis taille de LinkInfo).
        for (decalage, unicode) in [(76 + 16, None), (76 + 28, Some("C:\\a.exe")), (76, None)] {
            let mut lnk = fabriquer_lnk(b"C:\\a.exe", unicode);
            lnk[decalage..decalage + 4].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
            assert_eq!(cible_raccourci(&lnk), None, "décalage {decalage}");
        }
        // IDList annoncée plus grande que le fichier.
        let mut lnk = fabriquer_lnk(b"C:\\a.exe", None);
        lnk[20] = 3;
        lnk.splice(76..76, [0xFF, 0xFF]);
        assert_eq!(cible_raccourci(&lnk), None);
        // Plus de 1 Mo.
        let mut gros = fabriquer_lnk(b"C:\\a.exe", None);
        gros.resize(TAILLE_MAX_RACCOURCI + 1, 0);
        assert_eq!(cible_raccourci(&gros), None);
    }

    #[test]
    fn raccourci_tronque_a_toute_longueur_ne_panique_pas() {
        for (fichier, attendu) in RACCOURCIS_WINE {
            let octets = std::fs::read(dossier_raccourcis().join(fichier)).unwrap();
            for n in 0..=octets.len() {
                let cible = cible_raccourci(&octets[..n]);
                // Une troncature donne None, ou (si elle ne coupe que la fin du LinkInfo,
                // déjà lu) la même cible ; jamais une cible différente.
                assert!(cible.is_none() || cible.as_deref() == Some(attendu), "{n}");
            }
        }
        // Un bruit quelconque derrière un en-tête valide ne fait pas paniquer.
        let base = fabriquer_lnk(b"C:\\a.exe", Some("C:\\a.exe"));
        for i in 76..base.len() {
            for valeur in [0u8, 0xFF, 0x7F] {
                let mut lnk = base.clone();
                lnk[i] = valeur;
                let _ = cible_raccourci(&lnk);
            }
        }
    }

    /// Écrit `contenu` dans `<prefixe>/drive_c/<relatif>` (dossiers créés).
    fn poser(c: &Chemins, relatif: &str, contenu: &[u8]) -> PathBuf {
        let chemin = c.drive_c().join(relatif);
        std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
        std::fs::write(&chemin, contenu).unwrap();
        chemin
    }

    const MENU_COMMUN: &str = "ProgramData/Microsoft/Windows/Start Menu/Programs";

    /// Pose un raccourci du menu commun vers `cible` (et l'exécutable si `existe`) ;
    /// renvoie le chemin du `.lnk` relatif au préfixe.
    fn raccourci(c: &Chemins, sous: &str, cible: &str, existe: bool) -> PathBuf {
        let lnk = fabriquer_lnk(cible.as_bytes(), None);
        poser(c, &format!("{MENU_COMMUN}/{sous}"), &lnk);
        if existe {
            let linux = chemins_windows::vers_linux(&c.drive_c(), cible).unwrap();
            std::fs::create_dir_all(linux.parent().unwrap()).unwrap();
            std::fs::write(linux, b"MZ").unwrap();
        }
        PathBuf::from("drive_c").join(MENU_COMMUN).join(sous)
    }

    #[test]
    fn releve_trouve_les_lnk_des_deux_menus() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let u = utilisateur_windows(&c);
        let a = raccourci(&c, "Éditeur/A.lnk", r"C:\a.exe", false);
        let b = raccourci(&c, "B.LNK", r"C:\b.exe", false);
        poser(&c, &format!("{MENU_COMMUN}/note.txt"), b"x");
        let perso =
            format!("users/{u}/AppData/Roaming/Microsoft/Windows/Start Menu/Programs/C.lnk");
        poser(&c, &perso, &fabriquer_lnk(b"C:\\c.exe", None));
        let releve = releve_raccourcis(&c);
        let attendu: BTreeSet<PathBuf> = [a, b, PathBuf::from("drive_c").join(perso)].into();
        assert_eq!(releve, attendu);
    }

    #[test]
    fn raccourci_d_un_gestionnaire_installe_n_est_pas_une_application() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let u = utilisateur_windows(&c);
        // Recette comme celle d'inMusic : variable et dossier de version à joker.
        let r = Recette::gestionnaire_minimal(
            "inMusic",
            r"{appdata-local}\inmusic_software_center\app-*\inMusic Software Center.exe",
            vec![],
        );
        // Deux versions installées : la plus haute est la cible du raccourci.
        let exe = |v: &str| {
            let rel = format!(
                "users/{u}/AppData/Local/inmusic_software_center/app-{v}/inMusic Software Center.exe"
            );
            poser(&c, &rel, b"MZ");
        };
        exe("1.9.0");
        exe("1.10.0");
        let cible = format!(
            r"C:\users\{u}\AppData\Local\inmusic_software_center\app-1.10.0\inMusic Software Center.exe"
        );
        let lien = raccourci(&c, "inMusic/inMusic Software Center.lnk", &cible, false);
        let connus = programmes_des_gestionnaires(&c, [&r]);
        let apres: BTreeSet<PathBuf> = [lien].into();
        assert!(nouvelles_applications(&c, &BTreeSet::new(), &apres, &connus).is_empty());
        // Sans cette résolution, le raccourci serait pris pour une application.
        assert_eq!(
            nouvelles_applications(&c, &BTreeSet::new(), &apres, &[]).len(),
            1
        );
    }

    #[test]
    fn programmes_ignore_les_recettes_qui_ne_sont_pas_des_gestionnaires() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let mut r = Recette::gestionnaire_minimal("X", r"C:\X\x.exe", vec![]);
        r.type_ = TypeRecette::Installeur;
        assert!(programmes_des_gestionnaires(&c, [&r]).is_empty());
    }

    #[test]
    fn releve_sans_dossier_est_vide() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        assert!(releve_raccourcis(&c).is_empty());
    }

    #[test]
    fn nouvelles_ignore_desinstalleurs_demarrage_absents_connus() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let ancien = raccourci(
            &c,
            "Ancien/Ancien.lnk",
            r"C:\Program Files\Ancien\ancien.exe",
            true,
        );
        let avant: BTreeSet<PathBuf> = [ancien.clone()].into();
        let bon = raccourci(
            &c,
            "BLEASS/BLEASS Plugins Installer.lnk",
            r"C:\Program Files\BLEASS\BLEASS Plugins Installer.exe",
            true,
        );
        let mut apres = avant.clone();
        for (lnk, cible) in [
            ("X/Uninstall X.lnk", r"C:\Program Files\X\x.exe"),
            ("Y/Désinstaller Y.lnk", r"C:\Program Files\Y\y.exe"),
            ("W/W.lnk", r"C:\Program Files\W\unins000.exe"),
            ("V/V.lnk", r"C:\Program Files\V\Uninstall V.exe"),
            ("StartUp/Z.lnk", r"C:\Program Files\Z\z.exe"),
            ("Absent/Absent.lnk", r"C:\Program Files\Absent\absent.exe"),
            ("Connu/Connu.lnk", r"C:\Program Files\Connu\connu.exe"),
            ("Doc/Doc.lnk", r"C:\Program Files\Doc\lisezmoi.txt"),
        ] {
            let existe = !lnk.starts_with("Absent");
            apres.insert(raccourci(&c, lnk, cible, existe));
        }
        apres.insert(bon.clone());
        let connus = vec![r"c:/program files/CONNU/connu.exe".to_string()];
        let trouvees = nouvelles_applications(&c, &avant, &apres, &connus);
        assert_eq!(trouvees.len(), 1, "{trouvees:?}");
        assert_eq!(trouvees[0].nom, "BLEASS Plugins Installer");
        assert_eq!(
            trouvees[0].programme,
            r"C:\Program Files\BLEASS\BLEASS Plugins Installer.exe"
        );
        assert_eq!(trouvees[0].raccourci, bon);
        assert!(!trouvees[0].ajoutee.is_empty());
        assert!(!trouvees.iter().any(|a| a.raccourci == ancien));
    }

    fn app(nom: &str, programme: &str) -> Application {
        Application {
            nom: nom.to_string(),
            programme: programme.to_string(),
            raccourci: PathBuf::from("drive_c/x.lnk"),
            ajoutee: "2026-11-10".to_string(),
        }
    }

    #[test]
    fn fichier_absent_vide() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        assert!(lire_applications(&c).is_empty());
    }

    #[test]
    fn applications_fichier_aller_retour() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        std::fs::create_dir_all(&c.donnees).unwrap();
        let liste = vec![app("A", r"C:\a.exe"), app("B é", r"C:\b b.exe")];
        enregistrer_applications(&c, &liste).unwrap();
        assert_eq!(lire_applications(&c), liste);
        let texte = std::fs::read_to_string(c.donnees.join("applications.toml")).unwrap();
        assert!(texte.contains("[[application]]"), "{texte}");
    }

    #[test]
    fn fichier_illisible_vide_et_conserve() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        std::fs::create_dir_all(&c.donnees).unwrap();
        let fichier = c.donnees.join("applications.toml");
        std::fs::write(&fichier, "ceci n'est [pas du toml").unwrap();
        assert!(lire_applications(&c).is_empty());
        assert_eq!(
            std::fs::read_to_string(&fichier).unwrap(),
            "ceci n'est [pas du toml"
        );
        // Ajouter refuse d'écraser un fichier illisible.
        assert!(ajouter_applications(&c, vec![app("A", r"C:\a.exe")]).is_err());
        assert_eq!(
            std::fs::read_to_string(&fichier).unwrap(),
            "ceci n'est [pas du toml"
        );
    }

    #[test]
    fn pas_de_doublon() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        ajouter_applications(&c, vec![app("A", r"C:\a.exe")]).unwrap();
        ajouter_applications(&c, vec![app("A bis", r"c:/A.EXE"), app("B", r"C:\b.exe")]).unwrap();
        let noms: Vec<String> = lire_applications(&c).into_iter().map(|a| a.nom).collect();
        assert_eq!(noms, ["A", "B"]);
    }

    #[test]
    fn retirer() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        ajouter_applications(&c, vec![app("A", r"C:\a.exe"), app("B", r"C:\b.exe")]).unwrap();
        assert!(retirer_application(&c, "A").unwrap());
        assert!(!retirer_application(&c, "A").unwrap());
        assert_eq!(lire_applications(&c).len(), 1);
    }

    #[test]
    fn programme_disparu_masque_mais_garde() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        let present = r"C:\Program Files\Z\z.exe";
        let absent = r"C:\Program Files\A\a.exe";
        let linux = chemins_windows::vers_linux(&c.drive_c(), present).unwrap();
        std::fs::create_dir_all(linux.parent().unwrap()).unwrap();
        std::fs::write(&linux, b"MZ").unwrap();
        ajouter_applications(
            &c,
            vec![
                app("Zeta", present),
                app("alpha", absent),
                app("Bêta", present.replace('z', "y").as_str()),
            ],
        )
        .unwrap();
        // Seule Zeta existe ; les deux autres restent dans le fichier.
        let noms: Vec<String> = applications_presentes(&c)
            .into_iter()
            .map(|a| a.nom)
            .collect();
        assert_eq!(noms, ["Zeta"]);
        assert_eq!(lire_applications(&c).len(), 3);
        // Tri insensible à la casse.
        let linux2 = chemins_windows::vers_linux(&c.drive_c(), absent).unwrap();
        std::fs::create_dir_all(linux2.parent().unwrap()).unwrap();
        std::fs::write(linux2, b"MZ").unwrap();
        let noms: Vec<String> = applications_presentes(&c)
            .into_iter()
            .map(|a| a.nom)
            .collect();
        assert_eq!(noms, ["alpha", "Zeta"]);
    }

    #[test]
    fn vers_recette_ferme_le_bon_exe() {
        let a = app(
            "BLEASS Plugins Installer",
            r"C:\Program Files\BLEASS\BLEASS Plugins Installer.exe",
        );
        let r = a.vers_recette();
        assert_eq!(r.type_, TypeRecette::Gestionnaire);
        assert_eq!(r.nom, "BLEASS Plugins Installer");
        let g = r.gestionnaire.unwrap();
        assert_eq!(g.programme, a.programme);
        assert!(g.arguments.is_empty() && g.pilote.is_empty());
        assert_eq!(g.noms_a_fermer(), ["BLEASS Plugins Installer.exe"]);
    }
}
