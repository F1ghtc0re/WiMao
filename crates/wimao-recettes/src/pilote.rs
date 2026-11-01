//! Pilote : relance en silencieux les installeurs qu'un gestionnaire d'éditeur ouvre avec
//! une interface.
//!
//! Un gestionnaire (inMusic Software Center…) télécharge un installeur dans un dossier connu
//! (par exemple `{telechargements}\inMusic`) et le lance avec sa fenêtre. Pendant que la
//! session du gestionnaire tourne, le pilote observe les processus de l'environnement Wine :
//! tout `.exe` situé sous un de ces dossiers et lancé sans option silencieuse est fermé, puis
//! relancé en silencieux ([`crate::detection::arguments_silencieux`]).
//!
//! Règles de sécurité :
//! - seuls les processus du préfixe de WiMao sont vus (`wimao_core::processus`) ;
//! - un installeur de type inconnu n'est jamais arrêté : on ne sait pas le relancer ;
//! - le `wineserver` n'est **jamais** arrêté ici : le gestionnaire tourne dedans.
//! - les processus du gestionnaire de la session ne sont pas montrés au pilote
//!   (`gestionnaire::processus_pour_le_pilote`) : il ne peut donc pas les fermer, même s'ils
//!   citent le chemin d'un installeur.
//!
//! Utilise : `wimao_core::processus` (liste et arrêt), `detection` (type de l'installeur),
//! `chemins_windows` (traduction vers Linux), `installation` (commande `wine` sans arrêt).
//! Utilisé par : la session de gestionnaire (`gestionnaire.rs`, `Session::tick`).

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use wimao_core::processus::{arreter_processus, ProcessusWine, PROGRAMMES_SYSTEME_WINE};
use wimao_core::{journal, Chemins, Erreur, Wine};

use crate::chemins_windows::vers_linux;
use crate::detection::{detecter, TypeInstalleur};
use crate::installation::{arguments_wine, dossier_de_lancement, lancer_wine_sans_arret};

/// Options qui rendent un installeur silencieux : un processus qui en porte une est déjà
/// silencieux, le pilote n'y touche pas. Comparaison sans casse, sur l'argument entier.
pub(crate) const ARGUMENTS_SILENCIEUX: &[&str] =
    &["/verysilent", "/silent", "-quiet", "/quiet", "/s", "/qn"];

/// Un installeur interactif repéré dans les processus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalleurPilote {
    /// Numéro d'un processus qui porte cet installeur.
    pub pid: u32,
    /// Chemin Windows du fichier, tel qu'il figure dans la ligne de commande.
    pub fichier_windows: String,
    /// Nom de fichier (`Install-Hype.exe`).
    pub nom: String,
}

/// Ce que le pilote a fait d'un installeur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueRelance {
    /// Fermé puis relancé en silencieux.
    Relance {
        /// Nom de fichier de l'installeur.
        nom: String,
        /// Type reconnu.
        type_: TypeInstalleur,
        /// Code de sortie, `None` si le délai a été dépassé.
        code: Option<i32>,
    },
    /// Déjà relancé dans cette session : fermé sans nouvelle relance.
    Refermee {
        /// Nom de fichier de l'installeur.
        nom: String,
    },
    /// Type inconnu : laissé tel quel.
    Inconnu {
        /// Nom de fichier de l'installeur.
        nom: String,
    },
}

/// Délai laissé à un installeur interactif pour se fermer avant `SIGKILL`.
const DELAI_FERMETURE: Duration = Duration::from_secs(10);
/// Séparateur de chemin Windows, auquel on ramène `/` pour comparer.
const SEPARATEUR_WINDOWS: char = '\\';
/// Fin du nom d'un programme Windows (comparée en minuscules).
const EXTENSION_EXE: &str = ".exe";
/// Début de l'option de journal d'Inno Setup (`/LOG=<fichier>`).
const OPTION_JOURNAL_INNO: &str = "/LOG=";
/// Option de journal de WiX Burn (`-log <fichier>`).
const OPTION_JOURNAL_WIXBURN: &str = "-log";
/// Début du nom des journaux écrits par le pilote, à la racine de `C:`.
const PREFIXE_JOURNAL: &str = r"C:\wimao-pilote-";
/// Titre des événements du pilote dans le journal de WiMao.
const EVENEMENT_JOURNAL: &str = "pilote : installeur relancé en silencieux";
/// Événement du journal : installeur laissé tel quel (type inconnu ou fichier illisible).
const EVENEMENT_INCONNU: &str = "pilote : installeur laissé tel quel";
/// Événement du journal : échec du traitement d'un installeur.
const EVENEMENT_ECHEC: &str = "pilote : échec sur un installeur";
/// Début (minuscules) du nom des programmes qu'on n'arrête jamais : `wineserver` porte tout
/// l'environnement, `yabridge-host` un plugin ouvert dans le logiciel de musique (comme dans
/// `plugins_en_cours` de `wimao-core`).
const PREFIXES_PROTEGES: [&str; 2] = ["wineserver", "yabridge-host"];
/// Fin (minuscules) du fichier temporaire que lance Inno Setup.
const EXTENSION_TMP: &str = ".tmp";
/// Dossier temporaire de Windows tel qu'il apparaît dans un chemin normalisé (`normaliser` :
/// minuscules, `\`) : Inno Setup y extrait son `.tmp` (`…\AppData\Local\Temp\is-XXXX\`).
const DOSSIER_TEMPORAIRE: &str = r"\temp\";

/// Résultat du traitement d'un installeur par `Pilote::agir`.
struct Traitement {
    /// Ce qui est à signaler (`None` : rien).
    issue: Option<IssueRelance>,
    /// Vrai si des processus ont été arrêtés (ou qu'on a tenté de le faire) : le relevé de
    /// `/proc` du passage n'est plus sûr, le passage s'arrête.
    processus_touches: bool,
}

impl Traitement {
    /// Installeur laissé tel quel (aucun processus touché).
    fn sans_arret(issue: Option<IssueRelance>) -> Traitement {
        Traitement {
            issue,
            processus_touches: false,
        }
    }

    /// Installeur fermé (et peut-être relancé) : le relevé n'est plus sûr.
    fn avec_arret(issue: Option<IssueRelance>) -> Traitement {
        Traitement {
            issue,
            processus_touches: true,
        }
    }
}

/// Une relance silencieuse prête à lancer.
struct Relance {
    /// Clé de l'installeur (chemin Windows normalisé).
    cle: String,
    /// Nom de fichier de l'installeur.
    nom: String,
    /// Type reconnu.
    type_: TypeInstalleur,
    /// Dossier de travail de la relance (`installation::dossier_de_lancement`).
    dossier: PathBuf,
    /// Arguments de `wine`.
    args: Vec<OsString>,
}

/// Mémoire du pilote pour une session. Les ensembles sont indexés par le chemin Windows du
/// fichier, normalisé (`normaliser` : sans casse, `\` et `/` unifiés) : deux installeurs
/// de même nom dans deux dossiers sont deux installeurs.
#[derive(Debug, Default)]
pub(crate) struct Pilote {
    /// Installeurs déjà relancés en silencieux. Un installeur de cet ensemble qui
    /// réapparaît avec une interface : la relance n'a pas suffi, on le ferme sans relancer.
    traites: HashSet<String>,
    /// Installeurs de type inconnu (ou illisibles) déjà signalés. Séparés de `traites` : un
    /// installeur inconnu n'est **jamais** fermé (on ne sait pas le relancer), même revu.
    /// Un `.exe` reconnu comme archive (auto-extractible) y finit aussi : `arguments_wine`
    /// ne sait pas le lancer en silence, il ressort donc en [`IssueRelance::Inconnu`].
    inconnus: HashSet<String>,
    /// Installeurs dont un échec a déjà été écrit au journal : une fois suffit, le pilote
    /// repasse toutes les deux secondes.
    echecs_notes: HashSet<String>,
}

impl Pilote {
    /// Pilote neuf : aucun installeur traité.
    pub(crate) fn new() -> Pilote {
        Pilote::default()
    }

    /// Traite les installeurs interactifs présents dans `processus` (voir le module et
    /// [`IssueRelance`]). `dossiers_windows` : dossiers surveillés (variables déjà
    /// remplacées) ; `delai` : durée maximale de chaque relance silencieuse.
    ///
    /// **Un seul installeur est fermé par passage.** Pourquoi : `processus` est un relevé de
    /// `/proc` pris au début du passage, et une relance silencieuse peut durer jusqu'à
    /// 30 minutes. Après elle, les numéros de processus du relevé peuvent désigner d'autres
    /// programmes (un numéro libéré est réutilisé par Linux) : fermer le deuxième installeur
    /// avec ce vieux relevé pourrait tuer un inconnu. Dès qu'un installeur a été fermé (relancé
    /// ou refermé), le passage s'arrête ; le suivant (2 s plus tard) traite le reste avec un
    /// relevé neuf. Les installeurs laissés tels quels (type inconnu) ne comptent pas : on ne
    /// les touche pas.
    ///
    /// Le `wineserver` n'est jamais arrêté. Une relance qui dépasse le délai n'est pas une
    /// erreur (`code: None`).
    ///
    /// Aucune erreur n'est renvoyée : un échec sur un installeur (fichier illisible, arrêt
    /// refusé, Wine absent) est écrit au journal de WiMao et n'empêche pas de traiter les
    /// autres. Fichier illisible : traité comme `Inconnu` (laissé tel quel). Arrêt refusé :
    /// l'installeur n'est pas mémorisé, le passage suivant réessaiera.
    pub(crate) fn agir(
        &mut self,
        c: &Chemins,
        processus: &[ProcessusWine],
        dossiers_windows: &[String],
        delai: Duration,
    ) -> Vec<IssueRelance> {
        let mut issues = Vec::new();
        for installeur in installeurs_a_piloter(processus, dossiers_windows) {
            let traitement = self.traiter(c, processus, &installeur, delai);
            issues.extend(traitement.issue);
            if traitement.processus_touches {
                break; // le relevé n'est plus sûr : le reste au passage suivant
            }
        }
        issues
    }

    /// Traite un installeur. L'issue vaut `None` s'il n'y a rien à dire (inconnu déjà
    /// signalé, échec écrit au journal).
    fn traiter(
        &mut self,
        c: &Chemins,
        processus: &[ProcessusWine],
        installeur: &InstalleurPilote,
        delai: Duration,
    ) -> Traitement {
        let nom = installeur.nom.clone();
        let cle = normaliser(&installeur.fichier_windows);
        if self.inconnus.contains(&cle) {
            return Traitement::sans_arret(None); // déjà signalé, laissé tel quel
        }
        let pids = pids_de(processus, &installeur.fichier_windows);
        if self.traites.contains(&cle) {
            return Traitement::avec_arret(match arreter_processus(&pids, DELAI_FERMETURE) {
                Ok(()) => Some(IssueRelance::Refermee { nom }),
                Err(e) => self.noter_echec(cle, &nom, &e),
            });
        }
        let fichier = match vers_linux(&c.drive_c(), &installeur.fichier_windows) {
            Ok(f) => f,
            Err(e) => return Traitement::sans_arret(self.laisser_inconnu(cle, nom, Some(&e))),
        };
        let type_ = match detecter(&fichier) {
            Ok(t) => t,
            Err(e) => return Traitement::sans_arret(self.laisser_inconnu(cle, nom, Some(&e))),
        };
        let Some(mut args) = arguments_wine(type_, &fichier, false) else {
            return Traitement::sans_arret(self.laisser_inconnu(cle, nom, None));
        };
        args.extend(arguments_journal(type_, &nom));
        let relance = Relance {
            cle,
            nom,
            type_,
            dossier: dossier_de_lancement(type_, &fichier).to_path_buf(),
            args,
        };
        Traitement::avec_arret(self.fermer_et_relancer(c, &pids, relance, delai))
    }

    /// Ferme l'installeur interactif (`pids`) puis le relance en silencieux.
    fn fermer_et_relancer(
        &mut self,
        c: &Chemins,
        pids: &[u32],
        relance: Relance,
        delai: Duration,
    ) -> Option<IssueRelance> {
        let Relance {
            cle,
            nom,
            type_,
            dossier,
            args,
        } = relance;
        if let Err(e) = arreter_processus(pids, DELAI_FERMETURE) {
            return self.noter_echec(cle, &nom, &e); // pas mémorisé : réessayé au passage suivant
        }
        self.traites.insert(cle.clone());
        let code = match lancer_wine_sans_arret(&Wine::nouveau(c), &args, &dossier, delai) {
            Ok(sortie) => Some(sortie.code),
            Err(Erreur::Delai { .. }) => None,
            Err(e) => return self.noter_echec(cle, &nom, &e),
        };
        journal::noter_info(
            EVENEMENT_JOURNAL,
            &format!("{nom} ({type_:?}), code {code:?}"),
        );
        Some(IssueRelance::Relance { nom, type_, code })
    }

    /// Écrit au journal l'échec du traitement de `nom` (une seule fois par installeur) ;
    /// rend `None` (rien à signaler).
    fn noter_echec(&mut self, cle: String, nom: &str, erreur: &Erreur) -> Option<IssueRelance> {
        if self.echecs_notes.insert(cle) {
            journal::noter(EVENEMENT_ECHEC, &format!("{nom} : {erreur}"));
        }
        None
    }

    /// Mémorise l'installeur comme inconnu, l'écrit au journal (une seule fois) et le
    /// signale. `cause` : l'erreur de lecture qui empêche de le reconnaître, s'il y en a une.
    fn laisser_inconnu(
        &mut self,
        cle: String,
        nom: String,
        cause: Option<&Erreur>,
    ) -> Option<IssueRelance> {
        let detail = cause.map_or_else(|| "type inconnu".to_string(), |e| e.to_string());
        journal::noter(EVENEMENT_INCONNU, &format!("{nom} : {detail}"));
        self.inconnus.insert(cle);
        Some(IssueRelance::Inconnu { nom })
    }
}

/// Arguments qui font écrire un journal à l'installeur (`C:\wimao-pilote-<nom>.log`), pour
/// comprendre un échec : Inno `/LOG=…`, WiX Burn `-log …`. Rien pour les autres types.
fn arguments_journal(type_: TypeInstalleur, nom: &str) -> Vec<OsString> {
    let sans_extension = nom_sans_extension(nom);
    let journal = format!("{PREFIXE_JOURNAL}{sans_extension}.log");
    match type_ {
        TypeInstalleur::Inno => vec![format!("{OPTION_JOURNAL_INNO}{journal}").into()],
        TypeInstalleur::WixBurn => vec![OPTION_JOURNAL_WIXBURN.into(), journal.into()],
        _ => Vec::new(),
    }
}

/// Nom de fichier sans son extension `.exe` (casse ignorée) ; inchangé sans `.exe`.
fn nom_sans_extension(nom: &str) -> &str {
    let coupe = nom.len().saturating_sub(EXTENSION_EXE.len());
    match nom.get(coupe..) {
        Some(fin) if fin.eq_ignore_ascii_case(EXTENSION_EXE) => &nom[..coupe],
        _ => nom,
    }
}

/// Nom de fichier (après le dernier `\` ou `/`) sans sa dernière extension, en minuscules :
/// `C:\X\Install-Hype.tmp` donne `install-hype`.
fn racine_du_nom(chemin: &str) -> String {
    let nom = chemin.rsplit(['\\', '/']).next().unwrap_or(chemin);
    let sans = nom.rsplit_once('.').map_or(nom, |(debut, _)| debut);
    sans.to_lowercase()
}

/// Vrai pour un processus qu'on ne ferme jamais : `wineserver`, `yabridge-host` (il porte un
/// plugin ouvert dans le logiciel de musique), programmes de [`PROGRAMMES_SYSTEME_WINE`], ou
/// processus déjà silencieux (une relance en cours).
fn est_protege(p: &ProcessusWine) -> bool {
    est_intouchable(p) || est_silencieux(p)
}

/// Vrai pour un processus qu'aucune fermeture de WiMao ne doit viser : `wineserver`,
/// `yabridge-host` (il porte un plugin ouvert dans le logiciel de musique) ou programme de
/// [`PROGRAMMES_SYSTEME_WINE`]. Partagé avec la fermeture de session (`gestionnaire.rs`) :
/// une recette au nom de programme trop générique ne doit pas pouvoir les arrêter.
pub(crate) fn est_intouchable(p: &ProcessusWine) -> bool {
    let systeme = p
        .nom_programme()
        .is_some_and(|n| PROGRAMMES_SYSTEME_WINE.contains(&n.to_lowercase().as_str()));
    let protege = p.arguments.iter().any(|a| {
        let nom = a.rsplit(['\\', '/']).next().unwrap_or(a).to_lowercase();
        PREFIXES_PROTEGES
            .iter()
            .any(|prefixe| nom.starts_with(prefixe))
    });
    systeme || protege
}

/// Pids des processus de l'installeur `fichier_windows`. Seuls comptent les arguments :
/// - en `.exe` : le chemin Windows normalisé doit être **identique** à celui de l'installeur
///   (un autre `Hype.exe`, par exemple dans `Program Files`, n'est pas lui) ;
/// - en `.tmp` : le nom sans extension doit être celui de l'installeur **et** le chemin doit
///   passer par un dossier `Temp` (casse ignorée). Inno Setup relance l'installeur sous la
///   forme `…\Temp\is-XXXX\<nom>.tmp` ; un `Hype.tmp` ailleurs (fichier de travail d'un
///   plugin, d'un autre programme) n'est pas lui.
///
/// Les autres arguments (`Hype.vst3`, `Hype.dll`…) ne comptent pas : ils désignent des
/// fichiers ouverts, pas l'installeur. Nom vide : aucun pid. Processus protégés
/// ([`est_protege`]) : jamais.
fn pids_de(processus: &[ProcessusWine], fichier_windows: &str) -> Vec<u32> {
    let cle = normaliser(fichier_windows);
    let racine = racine_du_nom(fichier_windows);
    if racine.is_empty() {
        return Vec::new();
    }
    let est_installeur = |argument: &String| {
        let forme = normaliser(argument);
        if forme.ends_with(EXTENSION_EXE) {
            forme == cle
        } else if forme.ends_with(EXTENSION_TMP) {
            racine_du_nom(argument) == racine && forme.contains(DOSSIER_TEMPORAIRE)
        } else {
            false
        }
    };
    processus
        .iter()
        .filter(|p| !est_protege(p))
        .filter(|p| p.arguments.iter().any(est_installeur))
        .map(|p| p.pid)
        .collect()
}

/// Chemin en minuscules, `/` ramené à `\` : forme de comparaison.
fn normaliser(chemin: &str) -> String {
    chemin.to_lowercase().replace('/', "\\")
}

/// Installeurs interactifs parmi `processus`. Un argument convient s'il se termine par
/// `.exe` et commence par un des `dossiers_windows` suivi d'un séparateur (comparaison sans
/// casse, `\` et `/` équivalents). Un processus qui porte une option de
/// `ARGUMENTS_SILENCIEUX` est ignoré. Un même fichier compte une seule fois.
pub(crate) fn installeurs_a_piloter(
    processus: &[ProcessusWine],
    dossiers_windows: &[String],
) -> Vec<InstalleurPilote> {
    let dossiers: Vec<String> = dossiers_windows
        .iter()
        .map(|d| {
            format!(
                "{}{SEPARATEUR_WINDOWS}",
                normaliser(d).trim_end_matches(SEPARATEUR_WINDOWS)
            )
        })
        .collect();
    let mut vus = HashSet::new();
    let mut trouves = Vec::new();
    for p in processus.iter().filter(|p| !est_silencieux(p)) {
        for argument in &p.arguments {
            let forme = normaliser(argument);
            let dedans = dossiers.iter().any(|d| forme.starts_with(d.as_str()));
            let nom = argument.rsplit(['\\', '/']).next().unwrap_or(argument);
            // Un nom vide (`dossier\\.exe`) ne désignerait aucun processus : ignoré.
            let nom_vide = nom_sans_extension(nom).is_empty();
            if dedans && !nom_vide && forme.ends_with(EXTENSION_EXE) && vus.insert(forme) {
                trouves.push(InstalleurPilote {
                    pid: p.pid,
                    fichier_windows: argument.clone(),
                    nom: nom.to_string(),
                });
            }
        }
    }
    trouves
}

/// Vrai si un argument du processus est une option silencieuse connue.
fn est_silencieux(p: &ProcessusWine) -> bool {
    p.arguments
        .iter()
        .any(|a| ARGUMENTS_SILENCIEUX.contains(&a.to_lowercase().as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn processus(pid: u32, arguments: &[&str]) -> ProcessusWine {
        ProcessusWine {
            pid,
            arguments: arguments.iter().map(|a| a.to_string()).collect(),
        }
    }

    const DOSSIER: &str = r"C:\users\u\Downloads\inMusic";

    #[test]
    fn installeur_interactif_selectionne() {
        let p = processus(
            7,
            &[
                "wine",
                r"C:\users\u\Downloads\inMusic\Hype\Install-Hype.exe",
            ],
        );
        let trouves = installeurs_a_piloter(&[p], &[DOSSIER.to_string()]);
        assert_eq!(trouves.len(), 1);
        assert_eq!(trouves[0].nom, "Install-Hype.exe");
        assert_eq!(trouves[0].pid, 7);
    }

    #[test]
    fn installeur_deja_silencieux_ignore() {
        let p = processus(
            7,
            &[
                r"C:\users\u\Downloads\inMusic\Install-Hype.exe",
                "/VERYSILENT",
            ],
        );
        assert!(installeurs_a_piloter(&[p], &[DOSSIER.to_string()]).is_empty());
    }

    #[test]
    fn exe_hors_dossier_ignore() {
        let p = processus(7, &[r"C:\users\u\Downloads\Autre\Setup.exe"]);
        let voisin = processus(8, &[r"C:\users\u\Downloads\inMusicEtPlus\Setup.exe"]);
        assert!(installeurs_a_piloter(&[p, voisin], &[DOSSIER.to_string()]).is_empty());
    }

    #[test]
    fn dossier_compare_sans_casse_et_separateurs() {
        let p = processus(7, &["C:/USERS/u/downloads/INMUSIC/Hype/Setup.EXE"]);
        let trouves = installeurs_a_piloter(&[p], &[r"c:\Users\U\Downloads\inmusic".to_string()]);
        assert_eq!(trouves.len(), 1);
        assert_eq!(trouves[0].nom, "Setup.EXE");
    }

    #[test]
    fn meme_fichier_dans_plusieurs_processus_compte_une_fois() {
        let f = r"C:\users\u\Downloads\inMusic\Setup.exe";
        let liste = [processus(1, &[f]), processus(2, &["wine", f])];
        assert_eq!(
            installeurs_a_piloter(&liste, &[DOSSIER.to_string()]).len(),
            1
        );
    }

    #[test]
    fn installeur_inconnu_n_est_pas_arrete() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let dossier = c.drive_c().join("users/u/Downloads/inMusic");
        std::fs::create_dir_all(&dossier).unwrap();
        // Aucun marqueur d'installeur connu dans ce fichier.
        std::fs::write(dossier.join("Mystere.exe"), "rien de connu").unwrap();
        let mut enfant = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let windows = r"C:\users\u\Downloads\inMusic\Mystere.exe";
        let liste = [processus(enfant.id(), &[windows])];

        let mut pilote = Pilote::new();
        let issues = pilote.agir(&c, &liste, &[DOSSIER.to_string()], Duration::from_secs(5));
        let vivant = std::path::Path::new(&format!("/proc/{}", enfant.id())).exists();
        enfant.kill().unwrap();
        enfant.wait().unwrap();

        assert_eq!(
            issues,
            [IssueRelance::Inconnu {
                nom: "Mystere.exe".into()
            }]
        );
        assert!(vivant, "un installeur inconnu ne doit pas être arrêté");

        // Revu au passage suivant : plus signalé, et toujours pas arrêté.
        let issues = pilote.agir(&c, &liste, &[DOSSIER.to_string()], Duration::from_secs(5));
        assert!(issues.is_empty());
    }

    #[test]
    fn installeur_deja_relance_est_referme_sans_relance() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let mut enfant = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let liste = [processus(
            enfant.id(),
            &[r"C:\users\u\Downloads\inMusic\Hype.exe"],
        )];
        let mut pilote = Pilote::new();
        pilote
            .traites
            .insert(normaliser(r"C:\users\u\Downloads\inMusic\Hype.exe"));
        let issues = pilote.agir(&c, &liste, &[DOSSIER.to_string()], Duration::from_secs(5));
        // Tué par un signal : pas de code de sortie.
        assert_eq!(enfant.wait().unwrap().code(), None);
        assert_eq!(
            issues,
            [IssueRelance::Refermee {
                nom: "Hype.exe".into()
            }]
        );
    }

    #[test]
    fn journal_selon_le_type() {
        let inno = arguments_journal(TypeInstalleur::Inno, "Install-Hype.exe");
        assert_eq!(
            inno,
            [OsString::from(r"/LOG=C:\wimao-pilote-Install-Hype.log")]
        );
        let burn = arguments_journal(TypeInstalleur::WixBurn, "Setup.EXE");
        assert_eq!(
            burn,
            [
                OsString::from("-log"),
                OsString::from(r"C:\wimao-pilote-Setup.log")
            ]
        );
        assert!(arguments_journal(TypeInstalleur::Nsis, "x.exe").is_empty());
    }

    #[test]
    fn nom_sans_extension_ignore_la_casse_et_garde_le_reste() {
        assert_eq!(nom_sans_extension("Setup.EXE"), "Setup");
        assert_eq!(nom_sans_extension(".exe"), "");
        assert_eq!(nom_sans_extension("a.b.exe"), "a.b");
        assert_eq!(nom_sans_extension("sans-extension"), "sans-extension");
    }

    #[test]
    fn pids_de_ne_vise_que_l_installeur() {
        let liste = [
            processus(
                1,
                &[r"C:\Program Files\inMusic\inMusic Software Center.exe"],
            ),
            processus(2, &["wineserver"]),
            processus(3, &[r"C:\windows\system32\services.exe"]),
            processus(
                4,
                &[r"C:\users\u\Downloads\inMusic\Hype.exe", "/VERYSILENT"],
            ),
            processus(5, &[r"C:\users\u\Downloads\inMusic\Hype-Expansion.exe"]),
            processus(6, &[r"C:\users\u\Downloads\inMusic\Hype.exe"]),
            processus(
                7,
                &[r"C:\users\u\AppData\Local\Temp\is-1\HYPE.tmp", "/SL5=x"],
            ),
            processus(8, &[r"C:\Program Files\inMusic\inMusic.exe"]),
        ];
        assert_eq!(
            pids_de(&liste, r"C:\users\u\Downloads\inMusic\Hype.exe"),
            [6, 7]
        );
        // Un installeur nommé comme le dossier ne touche pas le gestionnaire.
        let dl = r"C:\users\u\Downloads\inMusic\inMusic.exe";
        assert!(pids_de(&liste, dl).is_empty());
        assert!(pids_de(&liste, r"C:\users\u\Downloads\inMusic\.exe").is_empty());
    }

    #[test]
    fn fichier_absent_laisse_le_processus_vivant_aux_deux_passages() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let mut enfant = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let liste = [processus(
            enfant.id(),
            &[r"C:\users\u\Downloads\inMusic\Absent.exe"],
        )];
        let mut pilote = Pilote::new();
        let dossiers = [DOSSIER.to_string()];
        let premier = pilote.agir(&c, &liste, &dossiers, Duration::from_secs(5));
        let second = pilote.agir(&c, &liste, &dossiers, Duration::from_secs(5));
        let vivant = std::path::Path::new(&format!("/proc/{}", enfant.id())).exists();
        enfant.kill().unwrap();
        enfant.wait().unwrap();
        assert_eq!(
            premier,
            [IssueRelance::Inconnu {
                nom: "Absent.exe".into()
            }]
        );
        assert!(second.is_empty());
        assert!(vivant);
    }

    #[test]
    fn un_tmp_hors_du_dossier_temporaire_n_est_pas_l_installeur() {
        let hype = r"C:\users\u\Downloads\inMusic\Hype.exe";
        let liste = [
            processus(1, &[r"C:\Program Files\X\Hype.tmp"]),
            processus(2, &[r"C:\users\u\Downloads\inMusic\Hype.tmp"]),
            processus(
                3,
                &[r"C:\users\u\AppData\Local\TEMP\is-1\hype.TMP", "/SL5=x"],
            ),
            processus(4, &[r"C:\Temporaire\Hype.tmp"]),
        ];
        assert_eq!(pids_de(&liste, hype), [3]);
    }

    #[test]
    fn un_seul_installeur_est_ferme_par_passage() {
        // Deux installeurs déjà relancés réapparaissent : le premier passage n'en ferme
        // qu'un (le relevé n'est plus sûr après un arrêt), le suivant ferme l'autre.
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let mut enfants: Vec<_> = (0..2)
            .map(|_| {
                std::process::Command::new("sleep")
                    .arg("30")
                    .spawn()
                    .unwrap()
            })
            .collect();
        let fichiers = [
            r"C:\users\u\Downloads\inMusic\A.exe",
            r"C:\users\u\Downloads\inMusic\B.exe",
        ];
        let liste: Vec<_> = enfants
            .iter()
            .zip(fichiers)
            .map(|(e, f)| processus(e.id(), &[f]))
            .collect();
        let mut pilote = Pilote::new();
        for f in fichiers {
            pilote.traites.insert(normaliser(f));
        }
        let dossiers = [DOSSIER.to_string()];
        let premier = pilote.agir(&c, &liste, &dossiers, Duration::from_secs(5));
        assert_eq!(
            premier,
            [IssueRelance::Refermee {
                nom: "A.exe".into()
            }]
        );
        let b_vivant = std::path::Path::new(&format!("/proc/{}", enfants[1].id())).exists();
        assert!(b_vivant, "B attend le passage suivant");
        // Passage suivant, relevé neuf : A a disparu, B est fermé.
        let second = pilote.agir(&c, &liste[1..], &dossiers, Duration::from_secs(5));
        assert_eq!(
            second,
            [IssueRelance::Refermee {
                nom: "B.exe".into()
            }]
        );
        for enfant in &mut enfants {
            let _ = enfant.kill();
            enfant.wait().unwrap();
        }
    }

    #[test]
    fn pids_de_ignore_plugins_et_autres_dossiers() {
        let hype = r"C:\users\u\Downloads\inMusic\Hype.exe";
        let liste = [
            processus(1, &["yabridge-host.exe", r"C:\VST3\Hype.vst3"]),
            processus(2, &["yabridge-host-32.exe", r"C:\VST3\Hype.dll"]),
            processus(3, &[r"C:\Program Files\X\Hype.exe"]),
            processus(4, &[r"C:\users\u\Downloads\Autre\Hype.exe"]),
            processus(5, &[hype]),
            processus(6, &["C:/USERS/u/downloads/inmusic/hype.EXE"]),
            processus(7, &[r"C:\users\u\Temp\is-AB\Hype.tmp", "/SL5=x"]),
        ];
        assert_eq!(pids_de(&liste, hype), [5, 6, 7]);
    }
}
