//! Diagnostic et réparation de l'environnement Wine, et verrou d'installation.
//!
//! - [`diagnostiquer`] regarde l'environnement **sans rien modifier** et liste les
//!   [`Probleme`]s trouvés ;
//! - [`reparer`] corrige une liste de problèmes (après un point de restauration) et écrit
//!   l'action `Reparation` dans l'[`historique`] (rien n'est écrit si elle est refusée avant) ;
//! - [`message_probleme`] donne le texte traduit d'un problème, pour l'interface ;
//! - [`poser_verrou_installation`] / [`installation_en_cours`] : le fichier verrou
//!   `<données>/installation.lock` que `wimao-recettes::installation::installer` pose
//!   pendant une installation, pour que le diagnostic ne prenne pas le `wineserver`
//!   de l'installeur pour un Wine bloqué.
//!
//! Utilise [`crate::correctifs`] (vérifier, retirer, appliquer), [`crate::pontage`],
//! [`crate::inventaire`], [`crate::restauration`] (point avant réparation, détection des
//! processus dans `/proc`) et [`crate::wine`]. Les textes d'erreur bruts, eux, sont
//! expliqués par [`crate::regles`], qui désigne le [`Probleme`] à réparer.

use crate::chemins::Chemins;
use crate::correctifs::{dxvk::Dxvk, mono::Mono, msvcp::Msvcp140, Correctif};
use crate::environnement::{creer_environnement, dossiers_relies, remplacer_dossiers_relies};
use crate::erreur::{Erreur, Resultat};
use crate::historique::{self, Action, ResultatAction};
use crate::i18n::{tr, trf, trn};
use crate::inventaire::Inventaire;
use crate::pontage::{
    bundle_linux, configuration, emplacement_lien_yabridge, etat_lien_yabridge, ponter,
    EtatLienYabridge,
};
use crate::progression::Progression;
use crate::restauration::{self, plugins_en_cours, wineserver_actif, MOTIF_REPARATION};
use crate::wine::Wine;
use std::path::{Path, PathBuf};

/// Nom du fichier verrou, dans le dossier de données.
const FICHIER_VERROU: &str = "installation.lock";
/// Fichier dont la présence prouve que l'environnement Wine existe (comme `environnement.rs`).
const FICHIER_PREFIXE_PRET: &str = "system.reg";
/// Dossier virtuel des processus Linux : `/proc/<pid>` existe tant que le processus vit.
const RACINE_PROC: &str = "/proc";

/// Un défaut de l'environnement Wine que WiMao sait réparer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probleme {
    /// L'environnement Wine n'existe pas (ou n'a jamais été initialisé).
    EnvironnementAbsent,
    /// Wine Mono (.NET) n'est pas installé : les installeurs .NET échouent
    /// (NOTES.md, Mini Grand et Velvet).
    MonoAbsent,
    /// DXVK n'est pas en place : interfaces Direct2D/D3D11 qui scintillent (NOTES.md, Serum 2).
    DxvkAbsent,
    /// La `msvcp140.dll` de Microsoft n'est pas en place (NOTES.md, Maschine 3).
    Msvcp140Absent,
    /// Autant de plugins de l'inventaire ne sont ni pontés ni écartés (`blacklist`).
    PontageIncomplet(usize),
    /// Un `wineserver` tourne pour cet environnement alors qu'aucun plugin ni aucune
    /// installation de WiMao ne l'utilise : il est resté bloqué.
    WineBloque,
    /// Des dossiers personnels de l'environnement (`Documents`, `Desktop`…) sont des
    /// liens vers ceux de Linux : les installeurs y déposeraient leurs bibliothèques
    /// hors de l'environnement (NOTES.md). `wineboot -u`, que Wine relance après sa
    /// mise à jour, les recrée ; voir [`crate::environnement::dossiers_relies`].
    DossiersRelies,
    /// `~/.local/share/yabridge` ne mène pas au yabridge de WiMao (`attendu`, c'est-à-dire
    /// [`Chemins::yabridge_dir`]) : il vise un autre yabridge, un dossier disparu, ou c'est un
    /// vrai dossier. Les « chainloaders » des plugins pontés (`~/.vst3/yabridge`…) y cherchent
    /// yabridge : le logiciel de musique risque d'utiliser un autre yabridge que celui de WiMao,
    /// ou de ne pas en trouver.
    ///
    /// **Simple avertissement** ([`Probleme::est_reparable`] est faux) : WiMao ne modifie jamais ce
    /// lien, qui appartient à la personne (elle peut l'avoir réglé exprès) ; le message donne la
    /// commande à lancer soi-même. `etat` n'est jamais `Absent` ni `Bon` (ce n'est pas un problème :
    /// WiMao crée le lien lui-même quand il manque).
    LienYabridge {
        /// Emplacement du lien (`~/.local/share/yabridge`, ou sous `XDG_DATA_HOME`).
        lien: PathBuf,
        /// Le yabridge de WiMao, vers lequel le lien devrait mener.
        attendu: PathBuf,
        /// Ce qu'on a trouvé à la place.
        etat: EtatLienYabridge,
    },
}

impl Probleme {
    /// Vrai si [`reparer`] sait corriger ce problème. Faux pour un simple avertissement
    /// ([`Probleme::LienYabridge`]) : il est montré, mais ni la réparation, ni le point de
    /// restauration, ni l'arrêt de Wine, ni la question sur les programmes ouverts ne le concernent.
    pub fn est_reparable(&self) -> bool {
        !matches!(self, Probleme::LienYabridge { .. })
    }

    /// Le problème de ce nom (celui du fichier `regles.toml`), sinon `None`.
    /// `PontageIncomplet` n'a pas de forme texte : il porte un nombre calculé.
    pub fn depuis_nom(nom: &str) -> Option<Probleme> {
        Some(match nom {
            "EnvironnementAbsent" => Probleme::EnvironnementAbsent,
            "MonoAbsent" => Probleme::MonoAbsent,
            "DxvkAbsent" => Probleme::DxvkAbsent,
            "Msvcp140Absent" => Probleme::Msvcp140Absent,
            "WineBloque" => Probleme::WineBloque,
            "DossiersRelies" => Probleme::DossiersRelies,
            _ => return None,
        })
    }

    /// Le correctif qui répare ce problème, s'il s'agit d'un correctif.
    fn correctif(&self) -> Option<Box<dyn Correctif>> {
        match self {
            Probleme::MonoAbsent => Some(Box::new(Mono)),
            Probleme::DxvkAbsent => Some(Box::new(Dxvk)),
            Probleme::Msvcp140Absent => Some(Box::new(Msvcp140)),
            _ => None,
        }
    }

    /// Ordre de réparation : arrêter un Wine bloqué, créer l'environnement, remplacer
    /// les dossiers reliés (avant tout installeur de correctif, qui pourrait y écrire),
    /// poser les correctifs, et seulement ensuite ponter (le pontage lance Wine).
    fn rang(&self) -> u8 {
        match self {
            Probleme::WineBloque => 0,
            Probleme::EnvironnementAbsent => 1,
            Probleme::DossiersRelies => 2,
            Probleme::MonoAbsent | Probleme::DxvkAbsent | Probleme::Msvcp140Absent => 3,
            Probleme::PontageIncomplet(_) => 4,
            // Jamais réparé : `reparer` l'écarte avant de trier. Le rang n'a pas d'effet.
            Probleme::LienYabridge { .. } => 5,
        }
    }

    /// Vrai si [`creer_environnement`] règle déjà ce problème quand il crée
    /// l'environnement (correctifs et dossiers reliés) : inutile de le refaire.
    fn regle_par_la_creation(&self) -> bool {
        self.correctif().is_some() || *self == Probleme::DossiersRelies
    }
}

/// Texte traduit d'un problème, pour l'interface et la ligne de commande.
pub fn message_probleme(p: &Probleme) -> String {
    match p {
        Probleme::EnvironnementAbsent => tr("The Wine environment does not exist yet"),
        Probleme::MonoAbsent => tr("Wine Mono (.NET support) is not installed"),
        Probleme::DxvkAbsent => tr("DXVK (graphics libraries) is not installed"),
        Probleme::Msvcp140Absent => tr("The Microsoft msvcp140 library is not installed"),
        Probleme::PontageIncomplet(n) => trn(
            "{n} plugin is not bridged to Linux yet",
            "{n} plugins are not bridged to Linux yet",
            *n as u64,
            &[],
        ),
        Probleme::WineBloque => {
            tr("Wine seems stuck: its server is running but no plugin or installation uses it")
        }
        Probleme::DossiersRelies => tr(
            "Some folders of the Wine environment (Documents, Desktop…) lead to your personal folders: installers would fill them",
        ),
        Probleme::LienYabridge {
            lien,
            attendu,
            etat,
        } => message_lien_yabridge(lien, attendu, etat),
    }
}

/// Texte de [`Probleme::LienYabridge`] : ce qui se passe, puis la commande qui y remédie. Sans
/// alarme (c'est un avertissement, parfois voulu), sans rien faire à la place de la personne.
///
/// - lien vers un autre yabridge ou cassé : `ln -sfn` remplace le lien (`-n` : sans descendre dans
///   le dossier visé, `-f` : en écrasant l'ancien lien) ;
/// - vrai dossier : `ln -sfn` créerait le lien *dans* ce dossier ; il faut d'abord le déplacer.
fn message_lien_yabridge(lien: &Path, attendu: &Path, etat: &EtatLienYabridge) -> String {
    let (lien_sh, attendu_sh) = (pour_shell(lien), pour_shell(attendu));
    let (lien_txt, attendu_txt) = (lien.display().to_string(), attendu.display().to_string());
    match etat {
        EtatLienYabridge::Ailleurs(cible) => trf(
            "The plugins in your music software use another yabridge than WiMao's: {link} leads to {current}, not to {expected}. To use WiMao's, run: ln -sfn {expected_sh} {link_sh}",
            &[
                ("link", &lien_txt),
                ("current", &cible.display().to_string()),
                ("expected", &attendu_txt),
                ("expected_sh", &attendu_sh),
                ("link_sh", &lien_sh),
            ],
        ),
        EtatLienYabridge::Casse(cible) => trf(
            "The plugins in your music software cannot find yabridge: {link} leads to {current}, which no longer exists. To use WiMao's ({expected}), run: ln -sfn {expected_sh} {link_sh}",
            &[
                ("link", &lien_txt),
                ("current", &cible.display().to_string()),
                ("expected", &attendu_txt),
                ("expected_sh", &attendu_sh),
                ("link_sh", &lien_sh),
            ],
        ),
        // `Absent` et `Bon` ne sont pas des problèmes : texte neutre, jamais montré en pratique.
        EtatLienYabridge::Dossier | EtatLienYabridge::Absent | EtatLienYabridge::Bon => trf(
            "The plugins in your music software use the yabridge in the folder {link}, not WiMao's ({expected}). To use WiMao's, move that folder away, then run: ln -s {expected_sh} {link_sh}",
            &[
                ("link", &lien_txt),
                ("expected", &attendu_txt),
                ("expected_sh", &attendu_sh),
                ("link_sh", &lien_sh),
            ],
        ),
    }
}

/// Chemin écrit pour être collé tel quel dans un terminal : tel quel s'il ne contient que des
/// caractères sans effet pour le shell, sinon entre apostrophes (`'` interne : `'\''`).
fn pour_shell(chemin: &Path) -> String {
    let texte = chemin.display().to_string();
    let sur = !texte.is_empty()
        && texte
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+@:,=~".contains(c));
    if sur {
        texte
    } else {
        format!("'{}'", texte.replace('\'', "'\\''"))
    }
}

// ---------------------------------------------------------------------------------------
// Verrou d'installation
// ---------------------------------------------------------------------------------------

/// Programmes Windows ouverts que [`reparer`] fermerait, à nommer à la personne **avant**
/// de réparer : ceux de l'environnement dès que `problemes` n'est pas vide, sinon rien. Liste
/// vide : on peut réparer sans demander.
///
/// Pourquoi « dès qu'il y a un problème », et pas seulement pour [`Probleme::WineBloque`] :
/// **toute** réparation arrête le `wineserver`, donc tout ce qui tourne dans l'environnement.
/// [`reparer`] crée d'abord un point de restauration (`restauration::creer` arrête Wine pour
/// copier un registre complet) et termine par `Wine::arreter` (pour que le registre soit
/// écrit). Un gestionnaire resté ouvert (WiMao fermé pendant une session) serait donc fermé
/// sans prévenir par une simple réparation du pontage.
///
/// Partagée par la ligne de commande et l'interface, qui posent chacune la question ; si la
/// personne refuse, **toute** la réparation est annulée.
/// Erreur : [`Erreur::Io`] si `/proc` est illisible.
pub fn programmes_a_nommer_avant_reparer(
    c: &Chemins,
    problemes: &[Probleme],
) -> Resultat<Vec<String>> {
    programmes_a_nommer_dans(Path::new(RACINE_PROC), c, problemes)
}

/// Cœur de [`programmes_a_nommer_avant_reparer`], avec la racine de `/proc` injectée (tests).
fn programmes_a_nommer_dans(
    proc: &Path,
    c: &Chemins,
    problemes: &[Probleme],
) -> Resultat<Vec<String>> {
    // Un simple avertissement (lien yabridge) n'arrête pas Wine : rien à réparer, rien à nommer.
    if !problemes.iter().any(Probleme::est_reparable) {
        return Ok(Vec::new()); // rien à réparer : Wine ne sera pas arrêté
    }
    crate::processus::programmes_ouverts_selon(proc, c)
}

/// Verrou d'installation posé par [`poser_verrou_installation`] ; le fichier est retiré
/// quand la garde est détruite, y compris en cas d'erreur ou de panique (`Drop`).
#[derive(Debug)]
pub struct VerrouInstallation {
    /// Fichier verrou à supprimer.
    chemin: PathBuf,
}

impl Drop for VerrouInstallation {
    fn drop(&mut self) {
        // Rien à faire de mieux si la suppression échoue : un verrou resté en place
        // est de toute façon ignoré dès que son processus n'existe plus.
        let _ = std::fs::remove_file(&self.chemin);
    }
}

/// Chemin du fichier verrou : `<données>/installation.lock`.
fn chemin_verrou(c: &Chemins) -> PathBuf {
    c.donnees.join(FICHIER_VERROU)
}

/// Vrai si le fichier verrou `chemin` est tenu par un processus vivant. Un fichier
/// illisible ou sans numéro de processus valide, ou dont le processus est mort
/// (installation tuée), est un verrou orphelin : ignoré.
fn verrou_actif(chemin: &Path, proc: &Path) -> bool {
    std::fs::read_to_string(chemin)
        .ok()
        .and_then(|texte| texte.trim().parse::<u32>().ok())
        .is_some_and(|pid| proc.join(pid.to_string()).exists())
}

/// Dit si une installation de WiMao est en cours (verrou tenu par un processus vivant).
pub fn installation_en_cours(c: &Chemins) -> bool {
    verrou_actif(&chemin_verrou(c), Path::new(RACINE_PROC))
}

/// Pose le verrou d'installation (le numéro du processus courant y est écrit).
///
/// Le fichier est créé d'un coup avec son contenu (écriture dans un fichier temporaire
/// puis `hard_link`, qui échoue si la cible existe) : un autre processus ne voit jamais
/// un verrou vide. Un verrou orphelin est remplacé.
///
/// Erreurs : [`Erreur::InstallationEnCours`] si un autre processus vivant tient déjà le
/// verrou ; [`Erreur::Io`].
pub fn poser_verrou_installation(c: &Chemins) -> Resultat<VerrouInstallation> {
    let chemin = chemin_verrou(c);
    std::fs::create_dir_all(&c.donnees)?;
    let temporaire = c
        .donnees
        .join(format!("{FICHIER_VERROU}.{}.tmp", std::process::id()));
    std::fs::write(&temporaire, std::process::id().to_string())?;
    let resultat = lier_verrou(&temporaire, &chemin);
    let _ = std::fs::remove_file(&temporaire);
    resultat.map(|()| VerrouInstallation { chemin })
}

/// Installe `temporaire` comme verrou `chemin`, en remplaçant un verrou orphelin.
fn lier_verrou(temporaire: &Path, chemin: &Path) -> Resultat<()> {
    for _ in 0..2 {
        match std::fs::hard_link(temporaire, chemin) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if verrou_actif(chemin, Path::new(RACINE_PROC)) {
                    return Err(Erreur::InstallationEnCours);
                }
                std::fs::remove_file(chemin)?; // orphelin : on le remplace
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(Erreur::InstallationEnCours) // un concurrent a repris la place entre-temps
}

// ---------------------------------------------------------------------------------------
// Diagnostic
// ---------------------------------------------------------------------------------------

/// Examine l'environnement et liste ses problèmes, **sans rien modifier**.
///
/// - environnement sans `system.reg` : seul [`Probleme::EnvironnementAbsent`] est rendu
///   (le reste ne peut pas se vérifier, et interroger Wine créerait un environnement) ;
/// - [`Probleme::WineBloque`] : `wineserver` actif pour CE préfixe, mais aucun
///   `yabridge-host` (plugin ouvert), aucune installation WiMao (verrou) et aucun programme
///   Windows ouvert (service, gestionnaire : voir `wine_bloque`) ; vérifié
///   avant tout appel à Wine, qui pourrait lui-même démarrer un serveur ;
/// - un correctif non appliqué donne le problème correspondant. Les vérifications lisent
///   les fichiers `user.reg` et `system.reg` (voir `registre_fichier`) : **aucune commande
///   Wine n'est lancée**. Une DLL de référence ou un fichier de registre absent compte
///   comme « non appliqué » ; une autre erreur d'entrée/sortie remonte à l'appelant ;
/// - [`Probleme::DossiersRelies`] : un dossier personnel de l'environnement est un lien
///   vers Linux (lu sans suivre les liens) ;
/// - [`Probleme::PontageIncomplet`]`(n)` : `n` plugins de l'inventaire ni pontés ni en
///   `blacklist` (doublons VST2, plugins pontés ailleurs) ;
/// - [`Probleme::LienYabridge`] : `~/.local/share/yabridge` vise un autre yabridge que celui de WiMao,
///   un dossier disparu, ou est un vrai dossier. Absent, ce n'est **pas** un problème (WiMao crée le
///   lien au prochain pontage). Avertissement seulement : jamais réparé automatiquement.
///
/// Limite : le `wineserver` n'écrit les `.reg` qu'à son arrêt, donc une valeur ajoutée
/// pendant qu'il tourne n'est vue qu'après son arrêt (voir NOTES.md).
///
/// Erreurs : [`Erreur::Io`] (lecture de `/proc` ou de l'inventaire).
pub fn diagnostiquer(c: &Chemins) -> Resultat<Vec<Probleme>> {
    if !c.prefixe.join(FICHIER_PREFIXE_PRET).exists() {
        return Ok(vec![Probleme::EnvironnementAbsent]);
    }
    let mut problemes = Vec::new();
    if wine_bloque(c)? {
        problemes.push(Probleme::WineBloque);
    }
    let a_verifier = [
        Probleme::MonoAbsent,
        Probleme::DxvkAbsent,
        Probleme::Msvcp140Absent,
    ];
    for probleme in a_verifier {
        let Some(correctif) = probleme.correctif() else {
            continue;
        };
        // `?` : une erreur d'entrée/sortie inattendue doit apparaître avec sa cause, pas
        // être prise pour « non appliqué » (les « absents » normaux sont déjà `false`).
        if !correctif.est_applique(c)? {
            problemes.push(probleme);
        }
    }
    if !dossiers_relies(c)?.is_empty() {
        problemes.push(Probleme::DossiersRelies);
    }
    let non_pontes = plugins_non_pontes(c)?;
    if non_pontes > 0 {
        problemes.push(Probleme::PontageIncomplet(non_pontes));
    }
    match etat_lien_yabridge(c)? {
        EtatLienYabridge::Absent | EtatLienYabridge::Bon => {}
        etat => problemes.push(Probleme::LienYabridge {
            lien: emplacement_lien_yabridge(c),
            attendu: c.yabridge_dir.clone(),
            etat,
        }),
    }
    Ok(problemes)
}

/// Un `wineserver` de ce préfixe tourne sans que rien de légitime l'utilise : ni plugin
/// ouvert, ni installation de WiMao, ni **programme Windows ouvert**
/// ([`crate::processus::programmes_ouverts`]).
///
/// Pourquoi la dernière condition : un service qui tourne volontairement après une session
/// (le NTKDaemon de Native Instruments, démarré par `wine net start`) ou un gestionnaire
/// resté ouvert gardent le `wineserver` en vie. Ce n'est pas un blocage, et le « réparer »
/// fermerait ces programmes sans raison.
fn wine_bloque(c: &Chemins) -> Resultat<bool> {
    Ok(wineserver_actif(c)?
        && !plugins_en_cours(c)?
        && !installation_en_cours(c)
        && crate::processus::programmes_ouverts(c)?.is_empty())
}

/// Nombre de plugins de l'inventaire qui ne sont ni pontés ni écartés par la `blacklist`.
fn plugins_non_pontes(c: &Chemins) -> Resultat<usize> {
    let inventaire = Inventaire::relever(&c.drive_c())?;
    let (config, _) = configuration(c, &inventaire)?;
    Ok(inventaire
        .0
        .iter()
        .filter(|p| !config.blacklist.contains(&p.chemin) && bundle_linux(c, p).is_none())
        .count())
}

// ---------------------------------------------------------------------------------------
// Réparation
// ---------------------------------------------------------------------------------------

/// Répare `problemes` (ceux de [`diagnostiquer`]).
///
/// 1. un point de restauration « repair » est créé d'abord (sauf si l'environnement
///    n'existe pas : rien à sauvegarder) ; [`restauration::creer`] refuse avec
///    [`Erreur::PluginsEnCours`] si un plugin tourne, et `reparer` fait de même dans
///    le cas sans point ;
/// 2. les problèmes sont traités dans l'ordre de `Probleme::rang` : arrêt de Wine,
///    création de l'environnement (qui pose aussi les correctifs), correctifs
///    (`retirer` puis `appliquer`), pontage ;
/// 3. un problème qui échoue n'empêche pas les suivants ; à la fin, une erreur
///    [`Erreur::ReparationIncomplete`] les liste tous.
///
/// Pendant la réparation, le verrou `installation.lock` est tenu (comme par `installer`) :
/// une installation en cours fait échouer la réparation avec [`Erreur::InstallationEnCours`],
/// et une installation lancée pendant la réparation est refusée de la même façon, ce
/// qui évite d'arrêter le `wineserver` d'un installeur.
///
/// Le `wineserver` est arrêté en fin de réparation (les modifications du registre ne
/// sont écrites qu'à son arrêt, voir NOTES.md).
pub fn reparer(c: &Chemins, problemes: &[Probleme], p: &dyn Progression) -> Resultat<()> {
    // Les simples avertissements (lien yabridge) ne se réparent pas : sans autre problème, il n'y a
    // rien à faire (ni point de restauration, ni arrêt de Wine, ni historique).
    let reparables: Vec<Probleme> = problemes
        .iter()
        .filter(|pb| pb.est_reparable())
        .cloned()
        .collect();
    let problemes = &reparables[..];
    if problemes.is_empty() {
        return Ok(());
    }
    // Gardé jusqu'à la sortie de la fonction (Drop), succès, erreur ou panique.
    let _verrou = poser_verrou_installation(c)?;
    let env_absent = problemes.contains(&Probleme::EnvironnementAbsent);
    // Id du point créé avant la réparation (aucun si l'environnement est à refaire : il n'y a
    // alors rien à garder).
    let mut id_point = None;
    if env_absent {
        if plugins_en_cours(c)? {
            return Err(Erreur::PluginsEnCours);
        }
    } else {
        p.etape(&tr("Creating a restore point…"));
        id_point = Some(restauration::creer(c, MOTIF_REPARATION)?.id);
    }
    // La réparation est engagée (les refus ci-dessus n'écrivent rien) : une seule entrée
    // d'historique, réussite ou échec.
    let resultat = reparer_engagee(c, problemes, env_absent, p);
    let (issue, details) = match &resultat {
        Ok(()) => (ResultatAction::Reussi, String::new()),
        Err(e) => (ResultatAction::Echec, e.to_string()),
    };
    historique::ajouter(
        c,
        Action::Reparation,
        &tr("Repair"),
        issue,
        id_point.as_deref(),
        &details,
    );
    resultat
}

/// Corps de [`reparer`] une fois le point créé : corrige les problèmes dans l'ordre, puis arrête
/// le `wineserver`. Séparé pour que l'historique s'écrive à un seul endroit.
fn reparer_engagee(
    c: &Chemins,
    problemes: &[Probleme],
    env_absent: bool,
    p: &dyn Progression,
) -> Resultat<()> {
    let mut a_traiter: Vec<&Probleme> = problemes
        .iter()
        // `creer_environnement` pose déjà les correctifs manquants et remplace les
        // dossiers reliés : inutile de les refaire.
        .filter(|pb| !(env_absent && pb.regle_par_la_creation()))
        .collect();
    a_traiter.sort_by_key(|pb| pb.rang());
    let mut echecs = executer_reparations(&a_traiter, |pb| reparer_un(c, pb, p));
    if let Err(e) = Wine::nouveau(c).arreter() {
        echecs.push(format!("{} : {e}", tr("Stopping Wine")));
    }
    if echecs.is_empty() {
        Ok(())
    } else {
        Err(Erreur::ReparationIncomplete { echecs })
    }
}

/// Exécute `agir` pour chaque problème, sans s'arrêter au premier échec. Rend une ligne
/// « problème : cause » par échec. Séparée de [`reparer`] pour tester la continuation.
fn executer_reparations(
    problemes: &[&Probleme],
    mut agir: impl FnMut(&Probleme) -> Resultat<()>,
) -> Vec<String> {
    problemes
        .iter()
        .filter_map(|pb| {
            agir(pb)
                .err()
                .map(|e| format!("{} : {e}", message_probleme(pb)))
        })
        .collect()
}

/// Répare un seul problème.
fn reparer_un(c: &Chemins, probleme: &Probleme, p: &dyn Progression) -> Resultat<()> {
    p.etape(&trf(
        "Repairing: {problem}",
        &[("problem", &message_probleme(probleme))],
    ));
    if let Some(correctif) = probleme.correctif() {
        correctif.retirer(c)?;
        return correctif.appliquer(c, p);
    }
    match probleme {
        Probleme::EnvironnementAbsent => creer_environnement(c, p),
        Probleme::PontageIncomplet(_) => ponter(c, &Inventaire::relever(&c.drive_c())?).map(|_| ()),
        Probleme::WineBloque => Wine::nouveau(c).arreter(),
        Probleme::DossiersRelies => remplacer_dossiers_relies(c).map(|_| ()),
        // Écarté par `reparer` (avertissement) : WiMao ne modifie jamais un lien existant (voir `pontage::assurer_lien_yabridge`).
        Probleme::LienYabridge { .. } => Ok(()),
        // Traités plus haut (correctifs) : ce bras rend le `match` exhaustif sans joker.
        Probleme::MonoAbsent | Probleme::DxvkAbsent | Probleme::Msvcp140Absent => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progression::ProgressionMuette;
    use serial_test::serial;
    use std::collections::BTreeMap;
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    /// Faux `/proc` avec un seul programme Windows ouvert dans le préfixe de `c`.
    fn faux_proc_avec_programme(proc: &Path, c: &Chemins) {
        let d = proc.join("7");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("cmdline"), "C:\\NA\\Native Access.exe\0").unwrap();
        let env = format!("WINEPREFIX={}\0", c.prefixe.display());
        std::fs::write(d.join("environ"), env).unwrap();
    }

    #[test]
    fn les_programmes_sont_nommes_des_qu_il_y_a_quelque_chose_a_reparer() {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let proc = racine.path().join("proc");
        faux_proc_avec_programme(&proc, &c);
        // Réparer le seul pontage arrête quand même Wine (point de restauration, arrêt
        // final) : le gestionnaire ouvert doit être nommé.
        let pontage = [Probleme::PontageIncomplet(1)];
        let avec = programmes_a_nommer_dans(&proc, &c, &pontage).unwrap();
        assert_eq!(avec, ["Native Access.exe"]);
        let bloque = programmes_a_nommer_dans(&proc, &c, &[Probleme::WineBloque]).unwrap();
        assert_eq!(bloque, ["Native Access.exe"]);
        // Rien à réparer : Wine ne sera pas arrêté, rien à demander.
        assert!(programmes_a_nommer_dans(&proc, &c, &[]).unwrap().is_empty());
    }

    /// Environnement de test : faux Wine (échoue toujours, donc « registre vide »), faux
    /// `wineserver` et faux `yabridgectl`, préfixe minimal, dans un dossier temporaire.
    struct Banc {
        _racine: tempfile::TempDir,
        c: Chemins,
    }

    use crate::test_utils::ecrire_executable as executable;

    impl Banc {
        fn nouveau() -> Banc {
            let racine = tempfile::tempdir().unwrap();
            let mut c = Chemins::pour_test(racine.path());
            c.wine_dir = racine.path().join("wine");
            c.yabridge_dir = racine.path().join("yabridge");
            executable(&c.wine_dir.join("bin/wine"), "#!/bin/sh\nexit 1\n");
            executable(&c.wine_dir.join("bin/wineserver"), "#!/bin/sh\nexit 0\n");
            executable(&c.yabridge_dir.join("yabridgectl"), "#!/bin/sh\nexit 0\n");
            std::fs::create_dir_all(&c.prefixe).unwrap();
            for reg in ["system.reg", "user.reg", "userdef.reg"] {
                std::fs::write(c.prefixe.join(reg), "WINE REGISTRY\n").unwrap();
            }
            std::fs::create_dir_all(c.drive_c()).unwrap();
            Banc { _racine: racine, c }
        }

        /// Dépose un VST3 qui n'a pas de bundle ponté.
        fn plugin_non_ponte(&self) {
            let dossier = self.c.drive_c().join("Program Files/Common Files/VST3");
            std::fs::create_dir_all(&dossier).unwrap();
            std::fs::write(dossier.join("Faux.vst3"), "plugin").unwrap();
        }
    }

    /// Processus factice (`exec -a <nom> sleep`) avec `WINEPREFIX`, tué à la fin du test.
    struct Factice(Child);

    impl Factice {
        fn lancer(nom: &str, prefixe: &Path) -> Factice {
            let enfant = Command::new("bash")
                .arg("-c")
                .arg(format!("exec -a {nom} sleep 60"))
                .env("WINEPREFIX", prefixe)
                .spawn()
                .unwrap();
            // Attend que `exec` ait remplacé la ligne de commande.
            let fin = Instant::now() + Duration::from_secs(5);
            let chemin = format!("/proc/{}/cmdline", enfant.id());
            while Instant::now() < fin {
                let commande = std::fs::read(&chemin).unwrap_or_default();
                if commande.starts_with(nom.as_bytes()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Factice(enfant)
        }
    }

    impl Drop for Factice {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Chemin → contenu de tout fichier sous `racine` (liens non suivis).
    fn instantane(racine: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        walkdir::WalkDir::new(racine)
            .follow_links(false)
            .into_iter()
            .map(|e| e.unwrap())
            .map(|e| {
                let contenu = std::fs::read(e.path()).unwrap_or_default();
                (e.path().to_path_buf(), contenu)
            })
            .collect()
    }

    #[test]
    fn prefixe_absent_donne_environnement_absent() {
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        assert_eq!(
            diagnostiquer(&c).unwrap(),
            vec![Probleme::EnvironnementAbsent]
        );
        assert!(!c.prefixe.exists(), "le diagnostic ne crée rien");
    }

    #[test]
    fn plugin_sans_bundle_donne_pontage_incomplet() {
        let banc = Banc::nouveau();
        banc.plugin_non_ponte();
        let problemes = diagnostiquer(&banc.c).unwrap();
        assert!(
            problemes.contains(&Probleme::PontageIncomplet(1)),
            "{problemes:?}"
        );
        // Faux Wine : aucun correctif n'est en place.
        assert!(problemes.contains(&Probleme::MonoAbsent), "{problemes:?}");
        assert!(!problemes.contains(&Probleme::WineBloque), "{problemes:?}");
    }

    // --- Lien ~/.local/share/yabridge : avertissement, jamais modifié ---

    /// Le problème `LienYabridge` du diagnostic, s'il y en a un.
    fn probleme_lien(banc: &Banc) -> Option<Probleme> {
        diagnostiquer(&banc.c)
            .unwrap()
            .into_iter()
            .find(|p| matches!(p, Probleme::LienYabridge { .. }))
    }

    /// Emplacement du lien dans le banc (dossier personnel isolé du test).
    fn lien_du_banc(banc: &Banc) -> PathBuf {
        let lien = emplacement_lien_yabridge(&banc.c);
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        lien
    }

    #[test]
    fn lien_yabridge_absent_n_est_pas_un_probleme() {
        let banc = Banc::nouveau();
        assert_eq!(probleme_lien(&banc), None);
        // Le diagnostic ne crée rien : c'est le pontage qui pose le lien.
        assert!(std::fs::symlink_metadata(emplacement_lien_yabridge(&banc.c)).is_err());
    }

    #[test]
    fn lien_yabridge_bon_n_est_pas_un_probleme() {
        let banc = Banc::nouveau();
        std::os::unix::fs::symlink(&banc.c.yabridge_dir, lien_du_banc(&banc)).unwrap();
        assert_eq!(probleme_lien(&banc), None);
    }

    #[test]
    #[serial]
    fn lien_yabridge_vers_un_autre_yabridge_est_signale() {
        // Le message est comparé en anglais : langue imposée, et test sérialisé pour que les
        // tests qui changent la langue globale ne le perturbent pas.
        crate::i18n::init_langue(Some("en"));
        let banc = Banc::nouveau();
        let autre = banc._racine.path().join("autre-yabridge");
        std::fs::create_dir_all(&autre).unwrap();
        let lien = lien_du_banc(&banc);
        std::os::unix::fs::symlink(&autre, &lien).unwrap();
        let attendu = Probleme::LienYabridge {
            lien: lien.clone(),
            attendu: banc.c.yabridge_dir.clone(),
            etat: EtatLienYabridge::Ailleurs(autre.clone()),
        };
        assert_eq!(probleme_lien(&banc), Some(attendu.clone()));
        // Message : explique la conséquence et donne la commande, sans rien changer.
        let message = message_probleme(&attendu);
        assert!(
            message.contains("another yabridge than WiMao's"),
            "{message}"
        );
        assert!(message.contains(&autre.display().to_string()), "{message}");
        let commande = format!(
            "ln -sfn {} {}",
            banc.c.yabridge_dir.display(),
            lien.display()
        );
        assert!(message.contains(&commande), "{message}");
        assert_eq!(std::fs::read_link(&lien).unwrap(), autre, "lien intact");
    }

    #[test]
    #[serial]
    fn lien_yabridge_casse_est_signale() {
        // Le message est comparé en anglais : langue imposée, et test sérialisé pour que les
        // tests qui changent la langue globale ne le perturbent pas.
        crate::i18n::init_langue(Some("en"));
        let banc = Banc::nouveau();
        let disparu = banc._racine.path().join("disparu");
        let lien = lien_du_banc(&banc);
        std::os::unix::fs::symlink(&disparu, &lien).unwrap();
        let probleme = probleme_lien(&banc).expect("lien cassé signalé");
        assert!(
            matches!(&probleme, Probleme::LienYabridge { etat: EtatLienYabridge::Casse(c), .. } if *c == disparu),
            "{probleme:?}"
        );
        let message = message_probleme(&probleme);
        assert!(message.contains("cannot find yabridge"), "{message}");
        assert!(message.contains("ln -sfn"), "{message}");
        assert_eq!(std::fs::read_link(&lien).unwrap(), disparu, "lien intact");
    }

    #[test]
    #[serial]
    fn lien_yabridge_vrai_dossier_est_signale_sans_ln_sfn() {
        // Le message est comparé en anglais : langue imposée, et test sérialisé pour que les
        // tests qui changent la langue globale ne le perturbent pas.
        crate::i18n::init_langue(Some("en"));
        let banc = Banc::nouveau();
        let lien = lien_du_banc(&banc);
        std::fs::create_dir_all(&lien).unwrap();
        let probleme = probleme_lien(&banc).expect("vrai dossier signalé");
        assert!(
            matches!(
                &probleme,
                Probleme::LienYabridge {
                    etat: EtatLienYabridge::Dossier,
                    ..
                }
            ),
            "{probleme:?}"
        );
        // `ln -sfn` créerait le lien DANS le dossier : le message demande de le déplacer d'abord.
        let message = message_probleme(&probleme);
        assert!(message.contains("move that folder away"), "{message}");
        assert!(!message.contains("ln -sfn"), "{message}");
        assert!(lien.is_dir() && !lien.is_symlink(), "dossier intact");
    }

    #[test]
    fn un_avertissement_n_est_pas_reparable_et_reparer_n_y_touche_pas() {
        let banc = Banc::nouveau();
        let lien = lien_du_banc(&banc);
        let disparu = banc._racine.path().join("disparu");
        std::os::unix::fs::symlink(&disparu, &lien).unwrap();
        let probleme = probleme_lien(&banc).unwrap();
        assert!(!probleme.est_reparable());
        assert!(Probleme::MonoAbsent.est_reparable());
        // Seul : ni point de restauration, ni historique, ni modification du lien.
        reparer(&banc.c, std::slice::from_ref(&probleme), &ProgressionMuette).unwrap();
        assert_eq!(std::fs::read_link(&lien).unwrap(), disparu);
        assert!(!banc.c.donnees.join("restauration").exists());
        assert!(historique::lire(&banc.c).unwrap().is_empty());
        // Et il ne fait pas nommer de programme à fermer.
        let proc = banc._racine.path().join("proc");
        faux_proc_avec_programme(&proc, &banc.c);
        assert!(programmes_a_nommer_dans(&proc, &banc.c, &[probleme])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn les_chemins_sont_ecrits_pour_le_shell() {
        assert_eq!(
            pour_shell(Path::new("/opt/wimao/yabridge")),
            "/opt/wimao/yabridge"
        );
        assert_eq!(pour_shell(Path::new("/home/a b/x")), "'/home/a b/x'");
        assert_eq!(pour_shell(Path::new("/home/l'un")), "'/home/l'\\''un'");
    }

    #[test]
    fn mono_present_n_est_pas_signale() {
        let banc = Banc::nouveau();
        std::fs::create_dir_all(banc.c.drive_c().join("windows/mono/mono-2.0")).unwrap();
        let problemes = diagnostiquer(&banc.c).unwrap();
        assert!(!problemes.contains(&Probleme::MonoAbsent), "{problemes:?}");
    }

    #[test]
    fn plugin_en_blacklist_n_est_pas_un_pontage_incomplet() {
        let banc = Banc::nouveau();
        // Un VST2 qui a un jumeau VST3 est un doublon : écarté, donc pas « incomplet ».
        banc.plugin_non_ponte();
        let vst2 = banc.c.drive_c().join("Program Files/Common Files/VST2");
        std::fs::create_dir_all(&vst2).unwrap();
        std::fs::write(vst2.join("Faux_x64.dll"), b"VSTPluginMain").unwrap();
        let problemes = diagnostiquer(&banc.c).unwrap();
        assert!(
            problemes.contains(&Probleme::PontageIncomplet(1)),
            "{problemes:?}"
        );
    }

    #[test]
    fn le_diagnostic_ne_modifie_rien_et_ne_lance_pas_wine() {
        let banc = Banc::nouveau();
        banc.plugin_non_ponte();
        // Ce faux wine (et le faux wineserver) laissent un témoin s'ils sont appelés.
        let temoin = banc.c.wine_dir.join("bin/appele");
        for programme in ["wine", "wineserver"] {
            executable(
                &banc.c.wine_dir.join("bin").join(programme),
                &format!("#!/bin/sh\necho appel >> {}\nexit 1\n", temoin.display()),
            );
        }
        let racine = banc.c.donnees.parent().unwrap().to_path_buf();
        let avant = instantane(&racine);
        diagnostiquer(&banc.c).unwrap();
        assert_eq!(instantane(&racine), avant);
        assert!(!temoin.exists(), "le diagnostic a lancé Wine");
    }

    #[test]
    fn registre_en_place_donne_des_correctifs_appliques() {
        let banc = Banc::nouveau();
        std::fs::create_dir_all(banc.c.drive_c().join("windows/mono/mono-2.0")).unwrap();
        let problemes = diagnostiquer(&banc.c).unwrap();
        assert!(!problemes.contains(&Probleme::MonoAbsent), "{problemes:?}");
    }

    #[test]
    fn erreur_d_entree_sortie_inattendue_remonte() {
        let banc = Banc::nouveau();
        // `system.reg` illisible comme fichier : un dossier. Ce n'est pas un « absent ».
        std::fs::remove_file(banc.c.prefixe.join("system.reg")).unwrap();
        std::fs::create_dir(banc.c.prefixe.join("system.reg")).unwrap();
        assert!(matches!(diagnostiquer(&banc.c), Err(Erreur::Io(_))));
    }

    #[test]
    fn wineserver_seul_est_un_wine_bloque() {
        let banc = Banc::nouveau();
        let _serveur = Factice::lancer("wineserver", &banc.c.prefixe);
        assert!(diagnostiquer(&banc.c)
            .unwrap()
            .contains(&Probleme::WineBloque));
    }

    #[test]
    fn wineserver_avec_un_service_ouvert_n_est_pas_bloque() {
        // Le NTKDaemon tourne volontairement après une session de Native Access : le
        // `wineserver` vit pour lui, ce n'est pas un Wine bloqué.
        let banc = Banc::nouveau();
        let _serveur = Factice::lancer("wineserver", &banc.c.prefixe);
        let _service = Factice::lancer("NTKDaemon.exe", &banc.c.prefixe);
        assert!(!diagnostiquer(&banc.c)
            .unwrap()
            .contains(&Probleme::WineBloque));
    }

    #[test]
    fn wineserver_d_un_autre_prefixe_est_ignore() {
        let banc = Banc::nouveau();
        let _serveur = Factice::lancer("wineserver", Path::new("/autre/prefixe"));
        assert!(!diagnostiquer(&banc.c)
            .unwrap()
            .contains(&Probleme::WineBloque));
    }

    #[test]
    fn wineserver_avec_un_plugin_ouvert_n_est_pas_bloque() {
        let banc = Banc::nouveau();
        let _serveur = Factice::lancer("wineserver", &banc.c.prefixe);
        let _hote = Factice::lancer("yabridge-host.exe", &banc.c.prefixe);
        assert!(!diagnostiquer(&banc.c)
            .unwrap()
            .contains(&Probleme::WineBloque));
    }

    #[test]
    fn wineserver_pendant_une_installation_n_est_pas_bloque() {
        let banc = Banc::nouveau();
        let _serveur = Factice::lancer("wineserver", &banc.c.prefixe);
        let verrou = poser_verrou_installation(&banc.c).unwrap();
        assert!(installation_en_cours(&banc.c));
        assert!(!diagnostiquer(&banc.c)
            .unwrap()
            .contains(&Probleme::WineBloque));
        drop(verrou);
        assert!(
            !installation_en_cours(&banc.c),
            "verrou retiré par la garde"
        );
        assert!(diagnostiquer(&banc.c)
            .unwrap()
            .contains(&Probleme::WineBloque));
    }

    #[test]
    fn verrou_orphelin_est_ignore_puis_remplace() {
        let banc = Banc::nouveau();
        std::fs::create_dir_all(&banc.c.donnees).unwrap();
        // Numéro de processus qui ne peut pas exister (au-delà de pid_max).
        std::fs::write(chemin_verrou(&banc.c), "4000000000").unwrap();
        assert!(!installation_en_cours(&banc.c));
        let _verrou = poser_verrou_installation(&banc.c).unwrap();
        assert!(installation_en_cours(&banc.c));
    }

    #[test]
    fn deuxieme_verrou_vivant_est_refuse() {
        let banc = Banc::nouveau();
        let _premier = poser_verrou_installation(&banc.c).unwrap();
        let second = poser_verrou_installation(&banc.c);
        assert!(
            matches!(second, Err(Erreur::InstallationEnCours)),
            "{second:?}"
        );
        assert!(installation_en_cours(&banc.c), "le premier verrou reste");
    }

    #[test]
    fn le_verrou_est_retire_meme_en_cas_de_panique() {
        let banc = Banc::nouveau();
        let c = banc.c.clone();
        let resultat = std::panic::catch_unwind(move || {
            let _verrou = poser_verrou_installation(&c).unwrap();
            panic!("installation qui plante");
        });
        assert!(resultat.is_err());
        assert!(!chemin_verrou(&banc.c).exists());
    }

    #[test]
    fn reparer_cree_un_point_et_ponte() {
        let banc = Banc::nouveau();
        banc.plugin_non_ponte();
        reparer(
            &banc.c,
            &[Probleme::PontageIncomplet(1)],
            &ProgressionMuette,
        )
        .unwrap();
        let liste = restauration::lister(&banc.c).unwrap();
        assert_eq!(liste.points.len(), 1);
        assert_eq!(liste.points[0].motif, "repair");
        assert!(banc.c.donnees.join("yabridgectl").exists(), "pontage lancé");
        // L'historique garde l'action et l'id du point créé.
        let entrees = historique::lire(&banc.c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].action, Action::Reparation);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
        assert_eq!(
            entrees[0].point.as_deref(),
            Some(liste.points[0].id.as_str())
        );
    }

    #[test]
    fn dossiers_relies_detectes_puis_repares_sans_toucher_la_cible() {
        let banc = Banc::nouveau();
        let cible = banc._racine.path().join("vrai-home/Documents");
        std::fs::create_dir_all(&cible).unwrap();
        std::fs::write(cible.join("garde.txt"), "à garder").unwrap();
        let u = banc.c.drive_c().join("users/jeanne");
        std::fs::create_dir_all(&u).unwrap();
        std::os::unix::fs::symlink(&cible, u.join("Documents")).unwrap();
        std::os::unix::fs::symlink(&cible, u.join("Desktop")).unwrap();

        let problemes = diagnostiquer(&banc.c).unwrap();
        assert!(
            problemes.contains(&Probleme::DossiersRelies),
            "{problemes:?}"
        );
        reparer(&banc.c, &[Probleme::DossiersRelies], &ProgressionMuette).unwrap();

        for nom in ["Documents", "Desktop"] {
            let m = std::fs::symlink_metadata(u.join(nom)).unwrap();
            assert!(m.is_dir() && !m.file_type().is_symlink(), "{nom}");
        }
        assert_eq!(
            std::fs::read_to_string(cible.join("garde.txt")).unwrap(),
            "à garder"
        );
        let apres = diagnostiquer(&banc.c).unwrap();
        assert!(!apres.contains(&Probleme::DossiersRelies), "{apres:?}");
    }

    #[test]
    fn reparer_refuse_pendant_une_installation() {
        let banc = Banc::nouveau();
        let verrou = poser_verrou_installation(&banc.c).unwrap();
        let r = reparer(&banc.c, &[Probleme::WineBloque], &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::InstallationEnCours)), "{r:?}");
        assert!(restauration::lister(&banc.c).unwrap().points.is_empty());
        assert!(
            installation_en_cours(&banc.c),
            "le verrou de l'installation reste"
        );
        drop(verrou);
    }

    #[test]
    fn reparer_tient_le_verrou_puis_le_rend() {
        let banc = Banc::nouveau();
        banc.plugin_non_ponte();
        // Le faux yabridgectl (lancé par le pontage) note le contenu du dossier de données.
        let journal = banc.c.yabridge_dir.join("donnees.log");
        executable(
            &banc.c.yabridge_dir.join("yabridgectl"),
            &format!(
                "#!/bin/sh\nls \"{}\" > {}\nexit 0\n",
                banc.c.donnees.display(),
                journal.display()
            ),
        );
        reparer(
            &banc.c,
            &[Probleme::PontageIncomplet(1)],
            &ProgressionMuette,
        )
        .unwrap();
        let pendant = std::fs::read_to_string(journal).unwrap();
        assert!(pendant.contains("installation.lock"), "{pendant}");
        assert!(!installation_en_cours(&banc.c));
    }

    #[test]
    fn reparer_sans_probleme_ne_fait_rien() {
        let banc = Banc::nouveau();
        reparer(&banc.c, &[], &ProgressionMuette).unwrap();
        assert!(restauration::lister(&banc.c).unwrap().points.is_empty());
    }

    #[test]
    fn reparer_refuse_quand_un_plugin_tourne() {
        let banc = Banc::nouveau();
        let _hote = Factice::lancer("yabridge-host.exe", &banc.c.prefixe);
        let r = reparer(&banc.c, &[Probleme::WineBloque], &ProgressionMuette);
        assert!(matches!(r, Err(Erreur::PluginsEnCours)), "{r:?}");
        assert!(restauration::lister(&banc.c).unwrap().points.is_empty());
        // Refusé avant l'action : rien dans l'historique.
        assert!(historique::lire(&banc.c).unwrap().is_empty());
    }

    #[test]
    fn reparer_liste_les_echecs() {
        let banc = Banc::nouveau();
        banc.plugin_non_ponte();
        executable(
            &banc.c.yabridge_dir.join("yabridgectl"),
            "#!/bin/sh\nexit 3\n",
        );
        let r = reparer(
            &banc.c,
            &[Probleme::WineBloque, Probleme::PontageIncomplet(1)],
            &ProgressionMuette,
        );
        match r {
            Err(Erreur::ReparationIncomplete { echecs }) => {
                assert_eq!(echecs.len(), 1, "{echecs:?}");
                assert!(echecs[0].contains("yabridgectl"), "{echecs:?}");
            }
            autre => panic!("{autre:?}"),
        }
        // Le point existe et l'échec est noté avec lui.
        let entrees = historique::lire(&banc.c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].resultat, ResultatAction::Echec);
        assert!(entrees[0].point.is_some());
        assert!(entrees[0].details.contains("yabridgectl"), "{entrees:?}");
    }

    #[test]
    fn un_echec_n_empeche_pas_les_autres_reparations() {
        let (a, b) = (Probleme::MonoAbsent, Probleme::DxvkAbsent);
        let mut essayes = Vec::new();
        let echecs = executer_reparations(&[&a, &b], |pb| {
            essayes.push(pb.clone());
            if *pb == Probleme::MonoAbsent {
                Err(Erreur::Introuvable("x".into()))
            } else {
                Ok(())
            }
        });
        assert_eq!(essayes, vec![a, b], "les deux sont tentés");
        assert_eq!(echecs.len(), 1);
        assert!(echecs[0].ends_with(": x"), "{echecs:?}");
    }

    #[test]
    fn l_ordre_de_reparation_arrete_wine_avant_de_ponter() {
        let mut v = [
            Probleme::PontageIncomplet(2),
            Probleme::MonoAbsent,
            Probleme::WineBloque,
            Probleme::DossiersRelies,
            Probleme::EnvironnementAbsent,
        ];
        v.sort_by_key(|p| p.rang());
        assert_eq!(v[0], Probleme::WineBloque);
        assert_eq!(v[2], Probleme::DossiersRelies, "avant les installeurs");
        assert_eq!(v[4], Probleme::PontageIncomplet(2));
    }

    #[test]
    fn chaque_probleme_a_un_message() {
        for p in [
            Probleme::EnvironnementAbsent,
            Probleme::MonoAbsent,
            Probleme::DxvkAbsent,
            Probleme::Msvcp140Absent,
            Probleme::PontageIncomplet(3),
            Probleme::WineBloque,
            Probleme::DossiersRelies,
        ] {
            assert!(!message_probleme(&p).is_empty());
        }
        assert!(message_probleme(&Probleme::PontageIncomplet(3)).contains('3'));
    }
}
