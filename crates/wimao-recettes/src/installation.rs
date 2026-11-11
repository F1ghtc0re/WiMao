//! Parcours complet d'installation d'un plugin : l'orchestrateur de WiMao.
//!
//! Assemble les briques déjà faites, sans rien réimplémenter :
//! - [`crate::catalogue`] et [`crate::detection`] : quelle recette, quel type d'installeur ;
//! - [`crate::archive`] : les archives (installeur à l'intérieur, ou plugins à copier) ;
//! - `wimao_core` : environnement Wine, point de restauration, inventaire des plugins,
//!   pontage yabridge et test d'ouverture.
//!
//! Utilisé par `wimao-cli` (commande d'installation) et `wimao` (assistant graphique).
//!
//! # Déroulé de [`installer`]
//!
//! 1. recette ([`Catalogue::trouver`]), ou recette donnée par [`installer_recette`] qui
//!    obtient d'abord l'installeur (`obtenir_installeur`) ;
//! 2. type d'installeur : la méthode de la recette, sinon [`detecter`] ; un installeur
//!    **inconnu** en mode non interactif s'arrête là (`interactif_necessaire = true`, sans
//!    point de restauration, environnement ni registre : on ne devine pas d'options) ;
//! 3. verrou `<données>/installation.lock` (gardé jusqu'à la fin, même en cas d'erreur) :
//!    il dit au diagnostic que le `wineserver` actif est celui de l'installeur ;
//! 4. [`creer_environnement`] seulement si `prefixe/system.reg` est absent (un
//!    environnement existant mais incomplet relève de « Réparer », pas d'ici) ;
//! 5. point de restauration (refusé si un plugin tourne dans un logiciel de musique) ;
//! 6. relevé des plugins « avant » ;
//! 7. installation : archive extraite, méthode sans Wine (`extraire-7z`, `squirrel`), ou
//!    installeur lancé par Wine (délai de 30 min en silencieux, 6 h en interactif) ;
//! 8. dépendances de la recette (installeurs lancés en silence), puis réglages
//!    (`appliquer_reglages` : registre et dossiers) ;
//! 9. relevé « après » ; les plugins **nouveaux** sont la différence, les plugins **mis à
//!    jour** ceux dont le binaire a changé ([`Inventaire::modifies`] : date ou taille) ;
//! 10. pontage de l'inventaire complet, puis test d'ouverture de chaque plugin nouveau ou
//!     mis à jour et ponté ;
//! 11. entrée `Installation` dans l'historique (`wimao_core::historique`), écrite une seule fois
//!     dès que le point existe (étapes 6 à 10 comprises) : `Reussi`, ou `Echec` avec la cause.
//!     Une erreur avant le point (fichier absent, verrou pris, plugin en cours) n'écrit rien.
//!
//! Sans plugin mais avec un nouveau raccourci du menu Démarrer vers un programme (lanceur
//! d'éditeur), le programme est enregistré comme application
//! ([`RapportInstallation::applications`], `crate::applications`) et ce n'est pas un échec.
//!
//! Quand rien de nouveau ni de modifié n'apparaît après une installation silencieuse
//! (installeur qui a échoué, qui ignore les options, ou délai dépassé), ce n'est pas une
//! erreur : [`RapportInstallation::interactif_necessaire`] vaut `true` et l'interface
//! propose d'ouvrir l'installeur normalement ; le code de sortie de l'installeur
//! ([`RapportInstallation::code_installeur`]) aide alors à comprendre. Une recette de
//! **gestionnaire** d'éditeur n'installe aucun plugin : ce conseil ne lui est jamais donné.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use wimao_core::diagnostic::{poser_verrou_installation, programmes_a_nommer_avant_reparer};
use wimao_core::historique::{self, Action, ResultatAction, TexteNeutre};
use wimao_core::i18n::{langue_courante, tr, trf};
use wimao_core::pontage::{bundle_linux, ponter, EtatPontage};
use wimao_core::processus::programmes_ouverts;
use wimao_core::restauration::{self, PointRestauration};
use wimao_core::wine::SURCHARGE_SANS_DEBOGUEUR;
use wimao_core::{
    creer_environnement, journal, lancer, mettre_a_jour, tester_plugin, Chemins, Erreur,
    Inventaire, PluginWindows, Probleme, Progression, Resultat, ResultatTest, Sortie, Wine,
};

use crate::applications::{
    ajouter_applications, nouvelles_applications, programmes_des_gestionnaires, releve_raccourcis,
};
use crate::archive::{installer_archive, ContenuArchive};
use crate::catalogue::{dossiers_recettes, Catalogue};
use crate::chemins_windows::{
    remplacer_variables, resoudre_motif, utilisateur_windows, vers_linux,
};
use crate::detection::{arguments_silencieux, detecter, TypeInstalleur};
use crate::extraction::{installer_extraire_7z, installer_squirrel};
use crate::obtention::obtenir_installeur;
use crate::recette::{consigne, Dependance, Methode, Recette, TypeRecette, CONSIGNE_ACTIVATION};

/// Durée maximale d'un installeur silencieux (aussi celle d'une relance du pilote). Large :
/// certains installeurs de banques de sons copient plusieurs Gio ; au-delà, on tue tout et on
/// propose l'installation normale.
pub(crate) const DELAI_INSTALLATION: Duration = Duration::from_secs(30 * 60);
/// Durée maximale d'un installeur interactif. Bien plus longue : l'utilisateur lit, choisit
/// des options et peut installer une grosse banque de sons ; on ne doit pas lui tuer
/// l'installeur au milieu. Le délai reste fini (6 h) pour qu'un installeur oublié, fenêtre
/// cachée derrière une autre, ne bloque pas WiMao indéfiniment.
const DELAI_INSTALLATION_INTERACTIF: Duration = Duration::from_secs(6 * 60 * 60);
/// Codes de sortie d'une dépendance qui comptent comme une réussite en plus de 0 :
/// InstallAware rend 100 ou 110 quand l'installation a réussi mais demande un redémarrage
/// (le NTKDaemon de Native Instruments, NOTES.md).
const CODES_REUSSITE_DEPENDANCE: [i32; 2] = [100, 110];

/// Choix de l'utilisateur pour une installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OptionsInstallation {
    /// Lancer l'installeur sans options silencieuses : sa fenêtre s'affiche et
    /// l'utilisateur clique lui-même (installeur inconnu, ou second essai).
    pub interactif: bool,
    /// Tester l'ouverture de chaque nouveau plugin ponté (Xvfb + Carla).
    pub tester: bool,
}

/// Ce qu'une installation a produit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RapportInstallation {
    /// Point de restauration créé juste avant l'installation (pour revenir en arrière) ;
    /// `None` quand rien n'a été lancé (installeur inconnu en mode non interactif).
    pub point: Option<PointRestauration>,
    /// Nom de la recette utilisée, s'il y en a une.
    pub recette: Option<String>,
    /// Type d'installeur retenu (celui de l'installeur interne pour une archive qui en contient un).
    pub methode: TypeInstalleur,
    /// Chaque plugin nouveau ou mis à jour, son état de pontage et, si demandé et
    /// possible, le résultat du test d'ouverture (`None` : pas testé, ou bundle Linux
    /// introuvable). Les mis à jour sont aussi dans [`RapportInstallation::mis_a_jour`].
    pub plugins: Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>,
    /// Chemins des plugins de [`RapportInstallation::plugins`] qui existaient déjà avant
    /// l'installation et dont le binaire a changé (mise à jour) ; les autres sont nouveaux.
    pub mis_a_jour: BTreeSet<PathBuf>,
    /// Aucun plugin nouveau ni mis à jour après une installation silencieuse : l'interface
    /// doit proposer « Ouvrir l'installeur normalement ».
    pub interactif_necessaire: bool,
    /// Code de sortie de l'installeur lancé par Wine ; `None` s'il n'y en a pas (archive
    /// de plugins, délai dépassé, rien lancé). Un code non nul n'est pas une erreur en soi
    /// (voir `lancer_installeur`) mais explique souvent un « aucun plugin ».
    pub code_installeur: Option<i32>,
    /// Recette de type gestionnaire : aucun plugin n'est attendu, « aucun plugin » n'est
    /// donc pas un échec et aucun conseil « installation normale » n'est donné.
    pub gestionnaire: bool,
    /// Schéma du `lien-connexion` de la recette (par exemple `native-access`), que
    /// l'appelant déclarera après confirmation de l'utilisateur (`wimao_core::liens`) ;
    /// la déclaration n'est jamais faite ici (voir `apres_installation::traiter_le_lien`).
    pub lien_a_declarer: Option<String>,
    /// Consigne `activation` de la recette dans la langue courante (activer une licence,
    /// entrer un numéro de série…), à afficher après l'installation ; `None` s'il n'y en a
    /// pas ou si rien n'a été installé.
    pub consigne_activation: Option<String>,
    /// Noms des applications enregistrées dans `applications.toml` par cette installation :
    /// l'installeur n'a ajouté **aucun plugin** mais un programme avec un raccourci du menu
    /// Démarrer (BLEASS Plugins Installer, par exemple). Vide dans tous les autres cas ;
    /// quand elle est remplie, `interactif_necessaire` vaut `false`.
    pub applications: Vec<String>,
}

impl RapportInstallation {
    /// Vrai si `plugin` a été mis à jour (il existait déjà), faux s'il est nouveau.
    pub fn est_mis_a_jour(&self, plugin: &PluginWindows) -> bool {
        self.mis_a_jour.contains(&plugin.chemin)
    }
}

/// Installe `fichier` (installeur ou archive) dans l'environnement Wine de WiMao.
///
/// Voir la documentation du module pour le déroulé complet. Erreurs : celles des briques
/// appelées ; en particulier [`Erreur::PluginsEnCours`] et [`Erreur::Introuvable`]
/// si `yabridgectl` manque. Un délai dépassé n'est **pas** une erreur (voir le rapport).
pub fn installer(
    c: &Chemins,
    fichier: &Path,
    catalogue: &Catalogue,
    options: OptionsInstallation,
    p: &dyn Progression,
) -> Resultat<RapportInstallation> {
    let delai = delai_installeur(options.interactif);
    installer_avec_delai(c, fichier, catalogue.trouver(fichier), options, p, delai)
}

/// Installe une recette connue : le gestionnaire d'un éditeur, ou tout plugin à adresse
/// de téléchargement.
///
/// `fichier` absent : l'installeur est d'abord obtenu par `obtenir_installeur`
/// (téléchargement vérifié par empreinte). Le parcours est ensuite celui de [`installer`],
/// mais avec **cette** recette (pas de recherche dans le catalogue) : méthodes sans Wine,
/// dépendances et réglages compris.
///
/// Erreurs : celles d'`obtenir_installeur` et de [`installer`], plus celles des
/// dépendances ([`Erreur::MotifAmbigu`], [`Erreur::Introuvable`], [`Erreur::Commande`]).
pub fn installer_recette(
    c: &Chemins,
    r: &Recette,
    fichier: Option<&Path>,
    options: OptionsInstallation,
    p: &dyn Progression,
) -> Resultat<RapportInstallation> {
    let obtenu;
    let fichier = match fichier {
        Some(f) => f,
        None => {
            obtenu = obtenir_installeur(c, r, p)?;
            &obtenu
        }
    };
    let delai = delai_installeur(options.interactif);
    installer_avec_delai(c, fichier, Some(r), options, p, delai)
}

/// Délai maximal de l'installeur : long en interactif, voir [`DELAI_INSTALLATION_INTERACTIF`].
fn delai_installeur(interactif: bool) -> Duration {
    if interactif {
        DELAI_INSTALLATION_INTERACTIF
    } else {
        DELAI_INSTALLATION
    }
}

/// Comme [`installer`] avec la recette déjà choisie et le délai de l'installeur : les
/// tests le réduisent.
///
/// Point d'entrée commun de toutes les installations (`installer`, `installer_recette`) : le
/// fichier est vérifié **avant tout** (verrou, création de l'environnement, point de
/// restauration). Sans cela, un `--fichier` mal tapé créait d'abord l'environnement Wine
/// (téléchargement de DXVK compris) avant d'échouer.
pub(crate) fn installer_avec_delai(
    c: &Chemins,
    fichier: &Path,
    recette: Option<&Recette>,
    options: OptionsInstallation,
    p: &dyn Progression,
    delai: Duration,
) -> Resultat<RapportInstallation> {
    verifier_fichier(fichier)?;
    let type_ = type_retenu(recette, fichier)?;
    if type_ == TypeInstalleur::Inconnu && !options.interactif {
        // Sans options silencieuses connues, rien ne peut être lancé : inutile de créer
        // un point de restauration, un environnement ou d'écrire dans le registre.
        p.etape(&tr("Unknown installer type: it has to be opened normally"));
        return Ok(rapport_a_la_main(recette, type_));
    }
    // Gardé jusqu'à la sortie de la fonction (Drop), succès, erreur ou panique.
    let _verrou = poser_verrou_installation(c)?;

    if !c.prefixe.join("system.reg").exists() {
        creer_environnement(c, p)?;
    }
    p.etape(&tr("Creating a restore point…"));
    let motif = restauration::motif_installation(&nom_du_fichier(fichier));
    let point = restauration::creer(c, &motif)?;
    let id_point = point.id.clone();
    // Le point existe : l'installation est engagée. Tout ce qui suit est dans la fermeture, et
    // l'historique s'écrit après elle, en un seul endroit, quelle que soit l'issue (un refus
    // d'avant le point, ci-dessus, n'écrit rien).
    let resultat: Resultat<RapportInstallation> = (|| {
        // Après le point de restauration (qui garde l'état d'avant), avant le relevé `avant` :
        // un `wineboot -u` tardif ne doit pas se déclencher au milieu de l'installeur.
        p.etape(&tr("Updating the Wine environment…"));
        mettre_a_jour(c, p)?;
        let avant = Inventaire::relever(&c.drive_c())?;
        // Raccourcis du menu Démarrer juste avant l'installeur : la différence avec le relevé
        // d'après désigne les programmes qu'il a ajoutés.
        let raccourcis_avant = releve_raccourcis(c);

        let issue = installer_le_fichier(c, fichier, type_, recette, options, p, delai)?;
        if let Some(r) = recette {
            verifier_gestionnaire_installe(c, r, issue.code)?;
            installer_dependances(c, r, p)?;
            appliquer_reglages(c, r, p)?;
        }
        let gestionnaire = est_gestionnaire(recette);

        p.etape(&tr("Looking for the installed plugins…"));
        let apres = Inventaire::relever(&c.drive_c())?;
        let modifies = apres.modifies(&avant);
        let mis_a_jour: BTreeSet<PathBuf> = modifies.iter().map(|m| m.chemin.clone()).collect();
        // Un plugin mis à jour suit le même chemin qu'un nouveau : pontage, test, rapport.
        let mut a_traiter = apres.nouveaux(&avant);
        a_traiter.extend(modifies);
        // Aucun plugin mais un programme nouveau (lanceur d'éditeur) : il devient une application.
        let applications = if a_traiter.is_empty() && !gestionnaire {
            enregistrer_nouvelles_applications(c, recette, &raccourcis_avant)
        } else {
            Vec::new()
        };
        // Un gestionnaire n'installe aucun plugin : « aucun plugin » est son état normal.
        // Un lanceur enregistré est un résultat : pas besoin de rouvrir l'installeur.
        let interactif_necessaire = issue.silencieux
            && a_traiter.is_empty()
            && !issue.vide
            && !gestionnaire
            && applications.is_empty();
        // Le conseil n'a de sens qu'après une installation silencieuse : pas en interactif
        // (l'utilisateur l'a faite lui-même), pas pour une archive de plugins copiés.
        if interactif_necessaire {
            p.etape(&message_aucun_plugin(issue.code));
        }
        let plugins = if a_traiter.is_empty() {
            Vec::new()
        } else {
            ponter_et_tester(c, &apres, a_traiter, options.tester, p)?
        };
        Ok(RapportInstallation {
            point: Some(point),
            recette: recette.map(|r| r.nom.clone()),
            methode: issue.type_,
            plugins,
            mis_a_jour,
            interactif_necessaire,
            code_installeur: issue.code,
            gestionnaire,
            lien_a_declarer: lien_a_declarer(recette),
            consigne_activation: recette
                .and_then(|r| consigne(r, CONSIGNE_ACTIVATION, &langue_courante())),
            applications,
        })
    })();
    let sujet = recette.map_or_else(|| nom_du_fichier(fichier), |r| r.nom.clone());
    match &resultat {
        Ok(rapport) => {
            // Détail neutre : le pluriel est choisi à l'affichage, dans la langue d'alors.
            let details = TexteNeutre::nouveau(
                historique::CLE_PLUGINS_TRAITES,
                &[("n", &rapport.plugins.len().to_string())],
            );
            historique::ajouter_details_neutres(
                c,
                Action::Installation,
                &sujet,
                ResultatAction::Reussi,
                Some(&id_point),
                &details,
            );
        }
        Err(e) => historique::ajouter(
            c,
            Action::Installation,
            &sujet,
            ResultatAction::Echec,
            Some(&id_point),
            &e.to_string(),
        ),
    }
    resultat
}

/// Enregistre dans `applications.toml` les programmes nés pendant l'installation (raccourcis
/// absents de `avant`) et renvoie leurs noms. À n'appeler que si l'installeur n'a ajouté aucun
/// plugin et n'est pas un gestionnaire.
///
/// Les gestionnaires déjà connus (recette courante et catalogue, programmes résolus à ce
/// moment : le dossier de version existe désormais) ne sont jamais enregistrés. Un échec
/// d'écriture est noté dans le journal et rend une liste vide : l'installation, elle, a réussi.
fn enregistrer_nouvelles_applications(
    c: &Chemins,
    recette: Option<&Recette>,
    avant: &BTreeSet<PathBuf>,
) -> Vec<String> {
    let apres = releve_raccourcis(c);
    let catalogue = Catalogue::charger(&dossiers_recettes(c)).0;
    let connus = programmes_des_gestionnaires(c, catalogue.0.iter().chain(recette));
    let nouvelles = nouvelles_applications(c, avant, &apres, &connus);
    if nouvelles.is_empty() {
        return Vec::new();
    }
    let noms: Vec<String> = nouvelles.iter().map(|a| a.nom.clone()).collect();
    match ajouter_applications(c, nouvelles) {
        Ok(()) => noms,
        Err(e) => {
            journal::noter("applications.toml non écrit", &e.to_string());
            Vec::new()
        }
    }
}

/// Vérifie que `fichier` est un fichier existant.
///
/// Erreur : [`Erreur::Introuvable`] avec le chemin (même texte que la ligne de commande).
fn verifier_fichier(fichier: &Path) -> Resultat<()> {
    if fichier.is_file() {
        return Ok(());
    }
    Err(Erreur::Introuvable(trf(
        "the file {path} does not exist",
        &[("path", &fichier.display().to_string())],
    )))
}

/// « Aucun nouveau plugin » ; cite le code de sortie de l'installeur s'il est non nul
/// (constaté : MiniGrand, WiX Burn, rend 67 avec `-quiet` sans rien installer, NOTES.md).
fn message_aucun_plugin(code: Option<i32>) -> String {
    match code {
        Some(code) if code != 0 => trf(
            "No new or updated plugin found (the installer returned code {code}): it may need to be opened normally",
            &[("code", &code.to_string())],
        ),
        _ => tr("No new or updated plugin found: the installer may need to be opened normally"),
    }
}

/// Rapport d'un installeur qu'on n'a pas lancé : l'utilisateur doit l'ouvrir normalement.
fn rapport_a_la_main(recette: Option<&Recette>, type_: TypeInstalleur) -> RapportInstallation {
    RapportInstallation {
        point: None,
        recette: recette.map(|r| r.nom.clone()),
        methode: type_,
        plugins: Vec::new(),
        mis_a_jour: BTreeSet::new(),
        interactif_necessaire: true,
        code_installeur: None,
        gestionnaire: est_gestionnaire(recette),
        lien_a_declarer: lien_a_declarer(recette),
        // Rien n'a été installé : rien à activer.
        consigne_activation: None,
        applications: Vec::new(),
    }
}

/// Vrai si la recette est celle d'un gestionnaire d'éditeur.
fn est_gestionnaire(recette: Option<&Recette>) -> bool {
    recette.is_some_and(|r| r.type_ == TypeRecette::Gestionnaire)
}

/// Schéma du `lien-connexion` de la recette, s'il y en a un.
fn lien_a_declarer(recette: Option<&Recette>) -> Option<String> {
    recette.and_then(|r| r.installation.lien_connexion.clone())
}

/// Ce qu'a fait l'étape d'installation, pour décider de la suite.
struct Issue {
    /// Type d'installeur finalement utilisé.
    type_: TypeInstalleur,
    /// L'installation était silencieuse (ou n'a pas pu être lancée) : s'il ne reste
    /// rien de nouveau, c'est à l'utilisateur de la faire à la main.
    silencieux: bool,
    /// L'archive ne contenait rien d'utilisable : pas de plugin attendu, rien à proposer.
    vide: bool,
    /// Code de sortie de l'installeur, s'il a été lancé et s'est terminé dans le délai.
    code: Option<i32>,
}

/// Type d'installeur : la méthode explicite de la recette, sinon la reconnaissance du fichier.
fn type_retenu(recette: Option<&Recette>, fichier: &Path) -> Resultat<TypeInstalleur> {
    type_retenu_methode(
        recette.map_or(Methode::Auto, |r| r.installation.methode),
        fichier,
    )
}

/// Type d'installeur pour une `methode` : explicite, sinon reconnu dans `fichier`.
fn type_retenu_methode(methode: Methode, fichier: &Path) -> Resultat<TypeInstalleur> {
    Ok(match methode {
        Methode::Inno => TypeInstalleur::Inno,
        Methode::Msi => TypeInstalleur::Msi,
        Methode::Wix => TypeInstalleur::WixBurn,
        Methode::Nsis => TypeInstalleur::Nsis,
        Methode::Installaware => TypeInstalleur::InstallAware,
        // Sans Wine, l'installeur n'est pas exécuté : on le traite comme une archive
        // (que `7z` ouvre), sans lire ses marqueurs.
        Methode::Extraire7z | Methode::Squirrel => TypeInstalleur::Archive,
        Methode::Auto => detecter(fichier)?,
    })
}

/// Installe le fichier : une archive est d'abord ouverte, un installeur est lancé.
fn installer_le_fichier(
    c: &Chemins,
    fichier: &Path,
    type_: TypeInstalleur,
    recette: Option<&Recette>,
    options: OptionsInstallation,
    p: &dyn Progression,
    delai: Duration,
) -> Resultat<Issue> {
    if let Some(r) = recette.filter(|r| sans_wine(r.installation.methode)) {
        return installer_sans_wine(c, fichier, r, p);
    }
    if type_ != TypeInstalleur::Archive {
        return lancer_installeur(c, fichier, type_, options, p, delai);
    }
    match installer_archive(c, fichier, p)? {
        // L'installeur sorti de l'archive suit le même parcours, sans nouveau point.
        ContenuArchive::Installeur(interne) => {
            let type_interne = type_retenu(recette, &interne)?;
            lancer_installeur(c, &interne, type_interne, options, p, delai)
        }
        ContenuArchive::PluginsCopies(_) => Ok(Issue {
            type_,
            silencieux: false,
            vide: false,
            code: None,
        }),
        ContenuArchive::Vide => {
            p.etape(&tr("The archive contains nothing usable"));
            Ok(Issue {
                type_,
                silencieux: false,
                vide: true,
                code: None,
            })
        }
    }
}

/// Les méthodes `extraire-7z` et `squirrel` installent sans exécuter l'installeur.
fn sans_wine(methode: Methode) -> bool {
    matches!(methode, Methode::Extraire7z | Methode::Squirrel)
}

/// Installation sans Wine : `extraire-7z` ou `squirrel` (voir `extraction.rs`). Ni
/// installeur lancé ni code de sortie ; ce n'est pas « silencieux » au sens d'un installeur
/// qui aurait pu échouer sans rien dire : toute erreur est remontée.
fn installer_sans_wine(
    c: &Chemins,
    fichier: &Path,
    r: &Recette,
    p: &dyn Progression,
) -> Resultat<Issue> {
    p.etape(&trf(
        "Extracting {name}…",
        &[("name", &nom_du_fichier(fichier))],
    ));
    let utilisateur = utilisateur_windows(c);
    match r.installation.methode {
        Methode::Squirrel => {
            installer_squirrel(c, fichier, &utilisateur)?;
        }
        _ => installer_extraire_7z(c, fichier, &r.installation.extraire, &utilisateur)?,
    }
    Ok(Issue {
        type_: TypeInstalleur::Archive,
        silencieux: false,
        vide: false,
        code: None,
    })
}

/// Lance l'installeur dans Wine puis arrête le `wineserver`.
///
/// - Le fichier est donné à Wine comme chemin Linux absolu (Wine accepte les chemins Unix) ;
///   le répertoire courant est le dossier de l'installeur, car certains installeurs
///   cherchent leurs fichiers à côté de lui (voir `NOTES.md`, installeur MiniGrand).
/// - Pas de `WINEDLLOVERRIDES="mscoree,mshtml="` ici, contrairement à `wineboot` : Wine Mono
///   est installé dans l'environnement et les installeurs .NET en ont besoin.
/// - Un code de sortie non nul n'est pas une erreur : bien des installeurs ont réussi malgré
///   lui. Seule la présence de plugins nouveaux en juge.
/// - Le délai dépassé tue le groupe de processus ([`lancer`]) et signale seulement
///   « à faire à la main ».
/// - `Wine::arreter` ensuite : il termine les processus restés derrière l'installeur et
///   fait écrire les fichiers `.reg` (utile au point de restauration suivant).
fn lancer_installeur(
    c: &Chemins,
    fichier: &Path,
    type_: TypeInstalleur,
    options: OptionsInstallation,
    p: &dyn Progression,
    delai: Duration,
) -> Resultat<Issue> {
    let absolu = std::path::absolute(fichier)?;
    let Some(args) = arguments_wine(type_, &absolu, options.interactif) else {
        // Installeur inconnu sans fenêtre autorisée : on ne devine pas d'options.
        p.etape(&tr("Unknown installer type: it has to be opened normally"));
        return Ok(Issue {
            type_,
            silencieux: true,
            vide: false,
            code: None,
        });
    };
    let nom = nom_du_fichier(fichier);
    p.etape(&if options.interactif {
        trf("Running {name} (its window will open)…", &[("name", &nom)])
    } else {
        trf("Running {name} silently…", &[("name", &nom)])
    });
    let code = match lancer_dans_wine(c, &args, dossier_de_lancement(type_, &absolu), delai) {
        Ok(sortie) => Some(sortie.code),
        Err(Erreur::Delai { .. }) => None,
        Err(autre) => return Err(autre),
    };
    Ok(Issue {
        type_,
        silencieux: !options.interactif,
        vide: false,
        code,
    })
}

/// Lance `wine <args>` depuis `dossier`, puis arrête le `wineserver` **dans tous les cas**
/// (réussite, erreur ou délai dépassé) : il termine les processus restés derrière
/// l'installeur et fait écrire les `.reg`. Rend la sortie, ou l'erreur de [`lancer`]
/// (dont [`Erreur::Delai`]) ; à défaut d'erreur de lancement, celle de l'arrêt.
fn lancer_dans_wine(
    c: &Chemins,
    args: &[OsString],
    dossier: &Path,
    delai: Duration,
) -> Resultat<Sortie> {
    let wine = Wine::nouveau(c);
    let resultat = lancer_wine_sans_arret(&wine, args, dossier, delai);
    let arret = wine.arreter();
    let sortie = resultat?;
    arret?;
    Ok(sortie)
}

/// Lance `wine <args>` depuis `dossier` avec un délai, **sans** arrêter le `wineserver`.
///
/// Partie commune à [`lancer_dans_wine`] (qui arrête ensuite le `wineserver`) et au pilote
/// ([`crate::pilote`]) : celui-ci relance un installeur pendant que le gestionnaire tourne
/// dans le même `wineserver`, qu'il ne faut donc surtout pas arrêter.
/// Erreurs : celles de [`lancer`] (dont [`Erreur::Delai`]) et de `Wine::commande`.
pub(crate) fn lancer_wine_sans_arret(
    wine: &Wine,
    args: &[OsString],
    dossier: &Path,
    delai: Duration,
) -> Resultat<Sortie> {
    let mut cmd = wine.commande_avec("wine", &[SURCHARGE_SANS_DEBOGUEUR])?;
    cmd.args(args).current_dir(dossier);
    lancer(&mut cmd, delai)
}

/// Arguments de `wine` pour installer `fichier` (chemin absolu), ou `None` si on ne sait
/// pas le faire sans fenêtre (type inconnu en mode silencieux).
///
/// MSI : `msiexec /i <fichier>` (suivi de `/qn /norestart` en silencieux). Autres types :
/// `<fichier>` suivi des options silencieuses. En interactif, jamais d'options.
pub(crate) fn arguments_wine(
    type_: TypeInstalleur,
    fichier: &Path,
    interactif: bool,
) -> Option<Vec<OsString>> {
    if interactif {
        return Some(match type_ {
            TypeInstalleur::Msi => vec!["msiexec".into(), "/i".into(), fichier.into()],
            _ => vec![fichier.into()],
        });
    }
    let silencieux = arguments_silencieux(type_, fichier)?;
    if type_ == TypeInstalleur::Msi {
        return Some(silencieux); // déjà `msiexec /i <fichier> /qn /norestart`
    }
    let mut args = vec![OsString::from(fichier)];
    args.extend(silencieux);
    Some(args)
}

/// Applique les réglages de la recette (`[correctifs]`) : valeurs de registre, puis
/// dossiers à créer (chemins Windows traduits par `vers_linux`, variables admises).
///
/// Idempotent : on peut le rappeler à volonté ([`reparer_gestionnaires`]). Les `services`
/// de la recette ne sont pas démarrés ici : la session et [`reparer_gestionnaires`] s'en
/// chargent.
///
/// Erreurs : celles de `Wine::reg_ajouter`, [`Erreur::NonPrisEnCharge`] pour un chemin
/// Windows qu'on ne sait pas traduire, [`Erreur::Io`].
pub(crate) fn appliquer_reglages(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<()> {
    appliquer_registre(c, r, p)?;
    let utilisateur = utilisateur_windows(c);
    for dossier in &r.correctifs.dossiers {
        let chemin = vers_linux(&c.drive_c(), &remplacer_variables(dossier, &utilisateur))?;
        std::fs::create_dir_all(chemin)?;
    }
    Ok(())
}

/// Vrai si la recette est celle d'un gestionnaire déjà installé : son `programme` se
/// résout (`resoudre_motif`). Toute autre recette : `false` (un plugin se reconnaît à
/// l'inventaire, pas à un fichier). Une erreur de résolution (motif ambigu, chemin non
/// traduisible) compte comme « pas installé ».
pub fn est_installe(c: &Chemins, r: &Recette) -> bool {
    let Some(g) = r
        .gestionnaire
        .as_ref()
        .filter(|_| est_gestionnaire(Some(r)))
    else {
        return false;
    };
    let programme = remplacer_variables(&g.programme, &utilisateur_windows(c));
    resoudre_motif(&c.drive_c(), &programme).is_ok()
}

/// Réapplique les réglages de chaque gestionnaire du `catalogue` qui est installé, puis
/// redémarre ses `services` (après une restauration ou une réparation de l'environnement, le
/// registre peut avoir perdu ces valeurs, et l'arrêt de Wine a arrêté les services : sans le
/// NTKDaemon, Native Access n'installe plus rien).
///
/// Les services sont démarrés par la même fonction que l'ouverture d'une session
/// (`gestionnaire::demarrer_services` : un service déjà démarré n'est pas une erreur).
/// Un gestionnaire en échec n'empêche pas de traiter les suivants.
///
/// Erreurs : la première de `appliquer_reglages` ou du démarrage d'un service.
pub fn reparer_gestionnaires(
    c: &Chemins,
    catalogue: &Catalogue,
    p: &dyn Progression,
) -> Resultat<()> {
    let mut premiere_erreur = None;
    for r in catalogue.0.iter().filter(|r| est_installe(c, r)) {
        let resultat = appliquer_reglages(c, r, p)
            .and_then(|()| crate::gestionnaire::demarrer_services(c, r, p));
        if let Err(e) = resultat {
            premiere_erreur.get_or_insert(e);
        }
    }
    premiere_erreur.map_or(Ok(()), Err)
}

/// Programmes ouverts à nommer avant « Réparer » (ligne de commande et interface) : la règle
/// du noyau ([`programmes_a_nommer_avant_reparer`] : dès qu'un problème est à réparer), plus
/// le cas où il n'y a aucun problème mais où [`reparer_gestionnaires`], que « Réparer » lance
/// toujours, arrêtera Wine : un gestionnaire installé a des valeurs de registre à réécrire
/// (`appliquer_registre` arrête Wine pour que le registre soit écrit).
///
/// Erreur : [`Erreur::Io`] si `/proc` est illisible.
pub fn programmes_a_nommer_avant_reparation(
    c: &Chemins,
    catalogue: &Catalogue,
    problemes: &[Probleme],
) -> Resultat<Vec<String>> {
    if reglages_arretent_wine(c, catalogue) {
        return programmes_ouverts(c);
    }
    programmes_a_nommer_avant_reparer(c, problemes)
}

/// Vrai si [`reparer_gestionnaires`] arrêtera Wine : un gestionnaire installé du `catalogue`
/// a des valeurs de registre.
fn reglages_arretent_wine(c: &Chemins, catalogue: &Catalogue) -> bool {
    catalogue
        .0
        .iter()
        .any(|r| !r.correctifs.registre.is_empty() && est_installe(c, r))
}

/// Après l'installation principale d'un gestionnaire, vérifie que son `programme` existe.
///
/// Sans cela, un installeur silencieux qui a échoué sans rien écrire (code non nul, options
/// ignorées) serait annoncé « installé » : l'erreur cite le motif cherché et le code de
/// sortie de l'installeur pour aider à corriger la recette. Sans effet pour les autres
/// recettes (leur résultat se juge à l'inventaire des plugins).
///
/// Erreur : [`Erreur::Introuvable`] si le programme n'est pas là.
fn verifier_gestionnaire_installe(c: &Chemins, r: &Recette, code: Option<i32>) -> Resultat<()> {
    let Some(g) = r.gestionnaire.as_ref() else {
        return Ok(());
    };
    if !est_gestionnaire(Some(r)) || est_installe(c, r) {
        return Ok(());
    }
    Err(Erreur::Introuvable(match code.filter(|code| *code != 0) {
        Some(code) => trf(
            "{program} after running the installer (exit code {code})",
            &[("program", &g.programme), ("code", &code.to_string())],
        ),
        None => trf(
            "{program} after running the installer",
            &[("program", &g.programme)],
        ),
    }))
}

/// Lance en silence, dans l'ordre, les installeurs `[[dependances]]` de la recette.
///
/// Toujours silencieuses, elles ont le délai de [`DELAI_INSTALLATION`] même quand
/// l'installation principale est interactive (6 h). Le fichier est résolu par `resoudre_motif` ; son type vient de la méthode, ou de
/// [`detecter`] pour `auto`. Pour une dépendance InstallAware, les codes de [`CODES_REUSSITE_DEPENDANCE`] valent 0 ; tout autre
/// code non nul est [`Erreur::Commande`] (contrairement à l'installeur principal, dont seul
/// le résultat sur les plugins juge : une dépendance manquante casserait le gestionnaire).
fn installer_dependances(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<()> {
    let utilisateur = utilisateur_windows(c);
    for dependance in &r.dependances {
        let chemin = remplacer_variables(&dependance.fichier, &utilisateur);
        let fichier = resoudre_motif(&c.drive_c(), &chemin)?;
        lancer_dependance(c, dependance, &fichier, p, DELAI_INSTALLATION)?;
    }
    Ok(())
}

/// Lance une dépendance dans Wine (voir [`installer_dependances`]).
fn lancer_dependance(
    c: &Chemins,
    dependance: &Dependance,
    fichier: &Path,
    p: &dyn Progression,
    delai: Duration,
) -> Resultat<()> {
    let type_ = type_retenu_methode(dependance.methode, fichier)?;
    let nom = nom_du_fichier(fichier);
    let Some(args) = arguments_wine(type_, fichier, false) else {
        return Err(Erreur::NonPrisEnCharge(trf(
            "Dependency {name}: no silent options known for this installer type",
            &[("name", &nom)],
        )));
    };
    p.etape(&trf("Running {name} silently…", &[("name", &nom)]));
    let sortie = lancer_dans_wine(c, &args, dossier_de_lancement(type_, fichier), delai)?;
    let reussite_installaware =
        type_ == TypeInstalleur::InstallAware && CODES_REUSSITE_DEPENDANCE.contains(&sortie.code);
    if sortie.code != 0 && !reussite_installaware {
        return Err(Erreur::Commande {
            programme: format!("wine {nom}"),
            code: Some(sortie.code),
            sortie: sortie.texte,
        });
    }
    Ok(())
}

/// Écrit dans le registre les valeurs de la recette (section `[correctifs]`, `registre`).
fn appliquer_registre(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<()> {
    if r.correctifs.registre.is_empty() {
        return Ok(());
    }
    p.etape(&tr("Applying the recipe settings…"));
    let utilisateur = utilisateur_windows(c);
    let wine = Wine::nouveau(c);
    for entree in &r.correctifs.registre {
        let donnee = remplacer_variables(&entree.donnee, &utilisateur);
        wine.reg_ajouter(&entree.cle, &entree.valeur, &donnee)?;
    }
    wine.arreter()
}

/// Ponte l'inventaire complet puis teste chaque nouveau plugin qui n'est pas écarté.
fn ponter_et_tester(
    c: &Chemins,
    inventaire: &Inventaire,
    nouveaux: Vec<PluginWindows>,
    tester: bool,
    p: &dyn Progression,
) -> Resultat<Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>> {
    let pontes = ponter_plugins(c, inventaire, nouveaux, p)?;
    if tester {
        Ok(tester_plugins(c, pontes, p))
    } else {
        Ok(pontes.into_iter().map(|(g, e)| (g, e, None)).collect())
    }
}

/// Ponte l'inventaire complet et rend l'état de pontage de chacun des `a_traiter`.
///
/// Réutilisée par la session de gestionnaire (`gestionnaire.rs`). Erreurs : celles de
/// `wimao_core::pontage::ponter` (`yabridgectl` absent, par exemple).
pub(crate) fn ponter_plugins(
    c: &Chemins,
    apres: &Inventaire,
    a_traiter: Vec<PluginWindows>,
    p: &dyn Progression,
) -> Resultat<Vec<(PluginWindows, EtatPontage)>> {
    p.etape(&tr("Bridging the plugins to Linux…"));
    let mut etats = ponter(c, apres)?;
    let mut pontes = Vec::new();
    for plugin in a_traiter {
        let Some(etat) = etats.remove(&plugin.chemin) else {
            continue; // le pontage rend un état par plugin de l'inventaire : ne devrait pas arriver
        };
        pontes.push((plugin, etat));
    }
    Ok(pontes)
}

/// Teste l'ouverture de ceux des `plugins` qui sont pontés ; `None` pour les autres.
///
/// Réutilisée par la session de gestionnaire (`gestionnaire.rs`).
pub(crate) fn tester_plugins(
    c: &Chemins,
    plugins: Vec<(PluginWindows, EtatPontage)>,
    p: &dyn Progression,
) -> Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)> {
    plugins
        .into_iter()
        .map(|(plugin, etat)| {
            let test = if etat == EtatPontage::Ponte {
                tester_le_plugin(c, &plugin, p)
            } else {
                None
            };
            (plugin, etat, test)
        })
        .collect()
}

/// Teste l'ouverture du plugin ponté ; `None` si son bundle Linux n'existe pas.
fn tester_le_plugin(
    c: &Chemins,
    plugin: &PluginWindows,
    p: &dyn Progression,
) -> Option<ResultatTest> {
    let bundle: PathBuf = bundle_linux(c, plugin)?;
    p.etape(&trf("Testing {name}…", &[("name", &plugin.nom)]));
    Some(tester_plugin(c, &bundle, plugin.format))
}

/// Nom de fichier sans dossier, pour les messages et le motif du point de restauration.
fn nom_du_fichier(fichier: &Path) -> String {
    fichier
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

/// Dossier qui contient `fichier` (`/` en dernier recours).
fn dossier_de(fichier: &Path) -> &Path {
    fichier.parent().unwrap_or(Path::new("/"))
}

/// Dossier de travail pour lancer l'installeur `fichier` de type `type_`.
///
/// En général le dossier de l'installeur (certains cherchent leurs fichiers à côté d'eux).
/// **Sauf pour un MSI** : `/`. Le chemin du MSI est donné en entier à `msiexec`, qui n'a pas
/// besoin du dossier de travail ; or `msiexec` de Wine plante (« page fault », code 5) avec
/// certains MSI quand ce dossier dépasse environ 125 caractères (vu à la vérification réelle
/// avec l'Installation Manager de Plugin Alliance, NOTES.md).
pub(crate) fn dossier_de_lancement(type_: TypeInstalleur, fichier: &Path) -> &Path {
    if type_ == TypeInstalleur::Msi {
        Path::new("/")
    } else {
        dossier_de(fichier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wimao_core::ProgressionMuette;

    #[test]
    fn un_msi_se_lance_depuis_la_racine_les_autres_depuis_leur_dossier() {
        // `msiexec` de Wine plante si le dossier de travail est trop long (NOTES.md).
        let msi = Path::new("/un/dossier/tres/long/PA-InstallationManager-v1.4.0.msi");
        assert_eq!(
            dossier_de_lancement(TypeInstalleur::Msi, msi),
            Path::new("/")
        );
        let inno = Path::new("/un/dossier/Setup.exe");
        assert_eq!(
            dossier_de_lancement(TypeInstalleur::Inno, inno),
            Path::new("/un/dossier")
        );
    }

    /// Environnement de test : faux Wine, faux yabridgectl, faux préfixe, tout dans un
    /// dossier temporaire. Aucun accès au vrai `HOME`, aucune fenêtre.
    struct Banc {
        racine: tempfile::TempDir,
        c: Chemins,
    }

    /// Faux `wine` : consigne chaque appel dans `appels.log` (avec le dossier courant) puis
    /// agit selon les arguments. `/VERYSILENT` (Inno) crée (ou réécrit) un plugin, `/S`
    /// (NSIS) ne fait rien, `-quiet` (WiX Burn) ne fait rien et rend 67 (comme MiniGrand,
    /// NOTES.md), `/s` (InstallAware) dort ; `reg` réussit.
    const FAUX_WINE: &str = include_str!("../tests/faux-wine.sh");

    use crate::test_utils::ecrire_executable;

    fn echantillon(nom: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/echantillons")
            .join(nom)
    }

    impl Banc {
        fn nouveau() -> Banc {
            let racine = tempfile::tempdir().unwrap();
            let mut c = Chemins::pour_test(racine.path());
            c.wine_dir = racine.path().join("wine");
            c.yabridge_dir = racine.path().join("yabridge");
            ecrire_executable(&c.wine_dir.join("bin/wine"), FAUX_WINE);
            ecrire_executable(&c.wine_dir.join("bin/wineserver"), "#!/bin/sh\nexit 0\n");
            // Faux `wineboot` (lancé par `mettre_a_jour` avant l'installeur) : consigne ses
            // arguments dans `wineboot.log`, à part de `appels.log` que les tests comparent.
            ecrire_executable(
                &c.wine_dir.join("bin/wineboot"),
                "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/wineboot.log\"\nexit 0\n",
            );
            ecrire_executable(&c.yabridge_dir.join("yabridgectl"), "#!/bin/sh\nexit 0\n");
            for reg in ["system.reg", "user.reg", "userdef.reg"] {
                std::fs::create_dir_all(&c.prefixe).unwrap();
                std::fs::write(c.prefixe.join(reg), "WINE REGISTRY\n").unwrap();
            }
            std::fs::create_dir_all(c.drive_c()).unwrap();
            Banc { racine, c }
        }

        /// Copie un échantillon d'installeur sous `nom` dans un dossier de téléchargements.
        fn installeur(&self, echantillon_: &str, nom: &str) -> PathBuf {
            let dossier = self.racine.path().join("telechargements");
            std::fs::create_dir_all(&dossier).unwrap();
            let cible = dossier.join(nom);
            std::fs::copy(echantillon(echantillon_), &cible).unwrap();
            cible
        }

        fn appels(&self) -> String {
            std::fs::read_to_string(self.c.wine_dir.join("bin/appels.log")).unwrap_or_default()
        }

        fn lancer(
            &self,
            fichier: &Path,
            catalogue: &Catalogue,
            options: OptionsInstallation,
            delai: Duration,
        ) -> Resultat<RapportInstallation> {
            installer_avec_delai(
                &self.c,
                fichier,
                catalogue.trouver(fichier),
                options,
                &ProgressionMuette,
                delai,
            )
        }
    }

    const SANS_OPTIONS: OptionsInstallation = OptionsInstallation {
        interactif: false,
        tester: false,
    };
    const DELAI_COURT: Duration = Duration::from_secs(5);

    fn catalogue_vide() -> Catalogue {
        Catalogue(Vec::new())
    }

    #[test]
    fn fichier_absent_echoue_avant_de_rien_creer() {
        // Préfixe absent : sans la vérification d'abord, l'environnement serait créé (et
        // DXVK téléchargé) avant de découvrir que le fichier n'existe pas.
        let racine = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(racine.path());
        let absent = racine.path().join("absent.exe");
        let cat = catalogue_vide();
        let r = installer(&c, &absent, &cat, SANS_OPTIONS, &ProgressionMuette);
        assert!(
            matches!(&r, Err(Erreur::Introuvable(m)) if m.contains("absent.exe")),
            "{r:?}"
        );
        assert!(!c.prefixe.exists(), "aucun environnement créé");
        assert!(
            !c.donnees.join("installation.lock").exists(),
            "aucun verrou"
        );
    }

    fn recette(banc: &Banc, extra: &str) -> Catalogue {
        let fichier = banc.racine.path().join("recette.toml");
        std::fs::write(
            &fichier,
            format!("nom = \"Essai\"\nreconnaitre = [\"*.exe\"]\n{extra}"),
        )
        .unwrap();
        Catalogue(vec![crate::recette::charger(&fichier).unwrap()])
    }

    #[test]
    fn l_environnement_est_mis_a_jour_avant_l_installeur() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        banc.lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        let journal = std::fs::read_to_string(banc.c.wine_dir.join("bin/wineboot.log")).unwrap();
        assert_eq!(journal.trim(), "-u");
    }

    #[test]
    fn la_consigne_d_activation_est_dans_le_rapport() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let cat = recette(
            &banc,
            "[consignes]\nactivation.fr = \"Activez.\"\nactivation.en = \"Activate.\"\n\
             activation.es = \"Active.\"\n",
        );
        let r = banc.lancer(&exe, &cat, SANS_OPTIONS, DELAI_COURT).unwrap();
        // Aucun test de cette crate ne change la langue : l'anglais.
        assert_eq!(r.consigne_activation.as_deref(), Some("Activate."));
        let sans = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert_eq!(sans.consigne_activation, None);
    }

    #[test]
    fn installeur_inno_silencieux_donne_un_plugin_ponte() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert_eq!(r.methode, TypeInstalleur::Inno);
        assert_eq!(r.recette, None);
        assert!(!r.interactif_necessaire);
        assert_eq!(r.plugins.len(), 1);
        assert_eq!(r.plugins[0].0.nom, "Faux");
        assert_eq!(r.plugins[0].1, EtatPontage::Ponte);
        assert_eq!(r.plugins[0].2, None, "tester = false : pas de test");
        let appels = banc.appels();
        assert!(appels.contains("/VERYSILENT"), "{appels}");
        // Chemin Linux absolu, lancé depuis le dossier de l'installeur.
        let dossier = exe.parent().unwrap().display().to_string();
        assert!(
            appels.contains(&format!("{dossier} | {}", exe.display())),
            "{appels}"
        );
        assert!(r.point.unwrap().dossier.exists());
    }

    #[test]
    fn plugin_mis_a_jour_est_ponte_teste_et_signale() {
        let banc = Banc::nouveau();
        // Version précédente déjà installée (contenu plus court que celui du faux Wine).
        let vst3 = banc.c.drive_c().join("Program Files/Common Files/VST3");
        std::fs::create_dir_all(&vst3).unwrap();
        std::fs::write(vst3.join("Faux.vst3"), "v1").unwrap();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(
            !r.interactif_necessaire,
            "une mise à jour n'est pas un échec"
        );
        assert_eq!(r.plugins.len(), 1);
        assert_eq!(r.plugins[0].0.nom, "Faux");
        assert_eq!(r.plugins[0].1, EtatPontage::Ponte);
        assert!(r.est_mis_a_jour(&r.plugins[0].0));
        assert_eq!(r.code_installeur, Some(0));
    }

    #[test]
    fn plugin_nouveau_n_est_pas_marque_mis_a_jour() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(!r.est_mis_a_jour(&r.plugins[0].0));
        assert!(r.mis_a_jour.is_empty());
    }

    #[test]
    fn aucun_plugin_avec_code_non_nul_cite_le_code() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("wixburn.bin", "Setup.exe");
        let muet = Enregistreuse::default();
        let r =
            installer_avec_delai(&banc.c, &exe, None, SANS_OPTIONS, &muet, DELAI_COURT).unwrap();
        assert!(r.interactif_necessaire);
        assert_eq!(r.code_installeur, Some(67));
        let messages = muet.0.lock().unwrap().join("\n");
        assert!(messages.contains("67"), "{messages}");
    }

    /// Prépare dans le dossier du faux Wine les raccourcis que `tests/faux-wine.sh` posera
    /// (`lanceur.lnk` vers le lanceur, `desinstalleur.lnk` vers `unins000.exe`).
    fn preparer_raccourcis(banc: &Banc) {
        let bin = banc.c.wine_dir.join("bin");
        let lnk = |cible: &str| crate::applications::tests::fabriquer_lnk(cible.as_bytes(), None);
        std::fs::write(
            bin.join("lanceur.lnk"),
            lnk(r"C:\Program Files\BLEASS\BLEASS Plugins Installer.exe"),
        )
        .unwrap();
        std::fs::write(
            bin.join("desinstalleur.lnk"),
            lnk(r"C:\Program Files\BLEASS\unins000.exe"),
        )
        .unwrap();
    }

    #[test]
    fn lanceur_sans_plugin_devient_application() {
        let banc = Banc::nouveau();
        preparer_raccourcis(&banc);
        let exe = banc.installeur("nsis.bin", "lanceur.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(r.plugins.is_empty());
        assert_eq!(r.applications, ["BLEASS Plugins Installer"]);
        assert!(!r.interactif_necessaire);
        let liste = crate::applications::lire_applications(&banc.c);
        assert_eq!(liste.len(), 1);
        assert_eq!(
            liste[0].programme,
            r"C:\Program Files\BLEASS\BLEASS Plugins Installer.exe"
        );
        let texte = std::fs::read_to_string(banc.c.donnees.join("applications.toml")).unwrap();
        assert!(texte.contains("BLEASS Plugins Installer"), "{texte}");
    }

    #[test]
    fn installeur_de_plugin_ne_cree_pas_d_application() {
        let banc = Banc::nouveau();
        preparer_raccourcis(&banc);
        let exe = banc.installeur("inno.bin", "melange.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert_eq!(r.plugins.len(), 1);
        assert!(r.applications.is_empty());
        assert!(!banc.c.donnees.join("applications.toml").exists());
    }

    #[test]
    fn sans_raccourci_reste_interactif_necessaire() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("nsis.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(r.interactif_necessaire);
        assert!(r.applications.is_empty());
        assert!(!banc.c.donnees.join("applications.toml").exists());
    }

    #[test]
    fn echec_d_ecriture_de_la_liste_n_echoue_pas_l_installation() {
        let banc = Banc::nouveau();
        preparer_raccourcis(&banc);
        // Un dossier à la place du fichier : la lecture comme l'écriture échouent.
        std::fs::create_dir_all(banc.c.donnees.join("applications.toml")).unwrap();
        let exe = banc.installeur("nsis.bin", "lanceur.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(r.applications.is_empty());
        assert!(r.interactif_necessaire, "rien d'enregistré : comme avant");
    }

    #[test]
    fn installeur_qui_ne_cree_rien_demande_l_installation_normale() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("nsis.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(r.interactif_necessaire);
        assert!(r.plugins.is_empty());
        assert!(banc.appels().contains(" /S"), "{}", banc.appels());
    }

    #[test]
    fn delai_depasse_tue_wine_et_demande_l_installation_normale() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("installaware.bin", "Setup.exe");
        let r = banc
            .lancer(
                &exe,
                &catalogue_vide(),
                SANS_OPTIONS,
                Duration::from_secs(1),
            )
            .unwrap();
        assert!(r.interactif_necessaire);
        let pid = std::fs::read_to_string(banc.c.wine_dir.join("bin/pid")).unwrap();
        assert!(
            !Path::new(&format!("/proc/{}", pid.trim())).exists(),
            "le faux wine tourne encore"
        );
    }

    #[test]
    fn inconnu_en_silencieux_n_est_pas_lance() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inconnu.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(r.interactif_necessaire);
        assert!(!banc.appels().contains("Setup.exe"));
    }

    #[test]
    fn inconnu_en_interactif_est_lance_sans_arguments() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inconnu.bin", "Setup.exe");
        let options = OptionsInstallation {
            interactif: true,
            tester: false,
        };
        let r = banc
            .lancer(&exe, &catalogue_vide(), options, DELAI_COURT)
            .unwrap();
        assert!(
            !r.interactif_necessaire,
            "l'utilisateur est déjà aux commandes"
        );
        let appels = banc.appels();
        assert!(
            appels.trim_end().ends_with(&exe.display().to_string()),
            "{appels}"
        );
    }

    /// Fabrique un installeur 7z qui contient `$PLUGINSDIR/app-64.7z` (lui-même contenant
    /// `Faux.exe`) : même fixture que les tests d'`extraction.rs`.
    fn installeur_7z(banc: &Banc) -> PathBuf {
        let ecrire_7z = |source: &Path, archive: &Path| {
            let ok = std::process::Command::new("7z")
                .arg("a")
                .arg(archive)
                .arg(".")
                .current_dir(source)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "7z a a échoué");
        };
        let interne = banc.racine.path().join("interne");
        std::fs::create_dir_all(&interne).unwrap();
        std::fs::write(interne.join("Faux.exe"), "MZ").unwrap();
        let externe = banc.racine.path().join("externe");
        std::fs::create_dir_all(externe.join("$PLUGINSDIR")).unwrap();
        ecrire_7z(&interne, &externe.join("$PLUGINSDIR/app-64.7z"));
        let setup = banc.racine.path().join("setup.exe");
        ecrire_7z(&externe, &setup);
        setup
    }

    /// Recette de gestionnaire `extraire-7z` avec un lien de connexion.
    const GESTIONNAIRE_7Z: &str = "type = \"gestionnaire\"\n\
        [installation]\nmethode = \"extraire-7z\"\nlien-connexion = \"wimao-test\"\n\
        [[installation.extraire]]\narchive = \"$PLUGINSDIR/app-64.7z\"\n\
        vers = 'C:\\Program Files\\Faux'\n\
        [gestionnaire]\nprogramme = 'C:\\Program Files\\Faux\\Faux.exe'\n";

    #[test]
    fn gestionnaire_extrait_sans_message_aucun_plugin() {
        if which_7z().is_none() {
            eprintln!("7z absent : test ignoré");
            return;
        }
        let banc = Banc::nouveau();
        let setup = installeur_7z(&banc);
        let cat = recette(&banc, GESTIONNAIRE_7Z);
        let r = banc
            .lancer(&setup, &cat, SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(r.gestionnaire);
        assert!(!r.interactif_necessaire);
        assert_eq!(r.lien_a_declarer.as_deref(), Some("wimao-test"));
        assert!(r.plugins.is_empty());
        assert!(banc
            .c
            .drive_c()
            .join("Program Files/Faux/Faux.exe")
            .is_file());
        assert_eq!(banc.appels(), "", "extraction sans Wine");
    }

    #[test]
    fn reglages_creent_les_dossiers() {
        let banc = Banc::nouveau();
        let cat = recette(
            &banc,
            "[correctifs]\ndossiers = ['{telechargements}\\NI']\n",
        );
        appliquer_reglages(&banc.c, &cat.0[0], &ProgressionMuette).unwrap();
        let utilisateur = utilisateur_windows(&banc.c);
        assert!(banc
            .c
            .drive_c()
            .join("users")
            .join(utilisateur)
            .join("Downloads/NI")
            .is_dir());
    }

    #[test]
    fn est_installe_suit_le_programme() {
        let banc = Banc::nouveau();
        let cat = recette(
            &banc,
            "type = \"gestionnaire\"\n[gestionnaire]\nprogramme = 'C:\\Faux\\app-*\\Faux.exe'\n",
        );
        let r = &cat.0[0];
        assert!(!est_installe(&banc.c, r));
        ecrire_executable(&banc.c.drive_c().join("Faux/app-1.0/Faux.exe"), "MZ");
        assert!(est_installe(&banc.c, r));
        // Une recette qui n'est pas un gestionnaire n'est jamais « installée » ici.
        assert!(!est_installe(&banc.c, &recette(&banc, "").0[0]));
    }

    #[test]
    fn reparer_gestionnaires_reapplique_les_reglages_des_installes() {
        let banc = Banc::nouveau();
        let cat = recette(
            &banc,
            "type = \"gestionnaire\"\n[correctifs]\ndossiers = ['C:\\Dossier']\n\
             [gestionnaire]\nprogramme = 'C:\\Faux\\Faux.exe'\n",
        );
        reparer_gestionnaires(&banc.c, &cat, &ProgressionMuette).unwrap();
        assert!(!banc.c.drive_c().join("Dossier").exists(), "pas installé");
        ecrire_executable(&banc.c.drive_c().join("Faux/Faux.exe"), "MZ");
        reparer_gestionnaires(&banc.c, &cat, &ProgressionMuette).unwrap();
        assert!(banc.c.drive_c().join("Dossier").is_dir());
    }

    #[test]
    fn reparer_gestionnaires_redemarre_les_services_des_installes() {
        let banc = Banc::nouveau();
        let cat = recette(
            &banc,
            "type = \"gestionnaire\"\n[correctifs]\nservices = [\"Svc\"]\n\
             [gestionnaire]\nprogramme = 'C:\\Faux\\Faux.exe'\n",
        );
        reparer_gestionnaires(&banc.c, &cat, &ProgressionMuette).unwrap();
        assert!(!banc.appels().contains("net start"), "pas installé");
        ecrire_executable(&banc.c.drive_c().join("Faux/Faux.exe"), "MZ");
        reparer_gestionnaires(&banc.c, &cat, &ProgressionMuette).unwrap();
        assert!(banc.appels().contains("net start Svc"), "{}", banc.appels());
    }

    #[test]
    fn des_reglages_de_registre_installes_arretent_wine() {
        let banc = Banc::nouveau();
        let registre = "type = \"gestionnaire\"\n[correctifs]\n\
             registre = [{ cle = 'HKCU\\X', valeur = \"V\", donnee = \"D\" }]\n\
             [gestionnaire]\nprogramme = 'C:\\Faux\\Faux.exe'\n";
        let cat = recette(&banc, registre);
        assert!(!reglages_arretent_wine(&banc.c, &cat), "pas installé");
        ecrire_executable(&banc.c.drive_c().join("Faux/Faux.exe"), "MZ");
        assert!(reglages_arretent_wine(&banc.c, &cat));
        let sans_registre = recette(
            &banc,
            "type = \"gestionnaire\"\n[gestionnaire]\nprogramme = 'C:\\Faux\\Faux.exe'\n",
        );
        assert!(!reglages_arretent_wine(&banc.c, &sans_registre));
    }

    #[test]
    fn gestionnaire_dont_l_installeur_n_a_rien_cree_est_une_erreur() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("nsis.bin", "Setup.exe");
        ecrire_executable(&banc.c.drive_c().join("deps/Dep.exe"), "MZ");
        // `/S` (NSIS) réussit sans rien créer ; la dépendance ne doit pas être lancée.
        let cat = recette(
            &banc,
            "type = \"gestionnaire\"\n[installation]\nmethode = \"nsis\"\n\
             [[dependances]]\nfichier = 'C:\\deps\\Dep.exe'\nmethode = \"nsis\"\n\
             [gestionnaire]\nprogramme = 'C:\\Faux\\Faux.exe'\n",
        );
        let r = banc.lancer(&exe, &cat, SANS_OPTIONS, DELAI_COURT);
        assert!(
            matches!(&r, Err(Erreur::Introuvable(m)) if m.contains("Faux.exe")),
            "{r:?}"
        );
        assert!(!banc.appels().contains("Dep.exe"), "{}", banc.appels());
    }

    #[test]
    fn une_installation_reussie_ajoute_une_entree_avec_son_point() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        let entrees = historique::lire(&banc.c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].action, Action::Installation);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
        // Sans recette, le sujet est le nom du fichier.
        assert_eq!(entrees[0].sujet, "Setup.exe");
        assert_eq!(entrees[0].point, r.point.map(|pt| pt.id));
        // Un plugin traité (aucun test ne change la langue : l'anglais).
        assert_eq!(entrees[0].details, "1 plugin processed");
        // Le détail est neutre (le sujet, un nom de fichier, ne l'est pas).
        let neutre = entrees[0].details_neutre.as_ref().expect("détails neutres");
        assert!(historique::CLES_CONNUES.contains(&neutre.cle.as_str()));
        assert_eq!(neutre.valeurs["n"], "1");
        assert!(entrees[0].sujet_neutre.is_none());
    }

    #[test]
    fn le_sujet_de_l_entree_est_le_nom_de_la_recette() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let cat = recette(&banc, "");
        banc.lancer(&exe, &cat, SANS_OPTIONS, DELAI_COURT).unwrap();
        let entrees = historique::lire(&banc.c).unwrap();
        assert_eq!(entrees[0].sujet, "Essai", "{entrees:?}");
    }

    #[test]
    fn une_erreur_apres_le_point_ajoute_une_entree_echec() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("nsis.bin", "Setup.exe");
        // Le gestionnaire annoncé n'est pas créé par l'installeur : erreur après le point.
        let cat = recette(
            &banc,
            "type = \"gestionnaire\"\n[installation]\nmethode = \"nsis\"\n\
             [gestionnaire]\nprogramme = 'C:\\Faux\\Faux.exe'\n",
        );
        banc.lancer(&exe, &cat, SANS_OPTIONS, DELAI_COURT)
            .unwrap_err();
        let entrees = historique::lire(&banc.c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].resultat, ResultatAction::Echec);
        assert_eq!(entrees[0].sujet, "Essai");
        assert!(entrees[0].point.is_some(), "le point existe : {entrees:?}");
        assert!(entrees[0].details.contains("Faux.exe"), "{entrees:?}");
    }

    #[test]
    fn un_refus_avant_le_point_n_ecrit_rien() {
        let banc = Banc::nouveau();
        // Fichier absent : refusé avant le verrou et le point.
        let absent = banc.racine.path().join("absent.exe");
        banc.lancer(&absent, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap_err();
        // Préfixe incomplet : le point lui-même est refusé.
        std::fs::remove_file(banc.c.prefixe.join("user.reg")).unwrap();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        banc.lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap_err();
        assert!(historique::lire(&banc.c).unwrap().is_empty());
    }

    #[test]
    fn codes_100_et_110_acceptes_seulement_pour_installaware() {
        let banc = Banc::nouveau();
        ecrire_executable(&banc.c.drive_c().join("deps/Dep.exe"), "MZ");
        let dep = |methode| Dependance {
            fichier: r"C:\deps\Dep.exe".into(),
            methode,
        };
        let fichier = banc.c.drive_c().join("deps/Dep.exe");
        // Faux wine : le fichier se terminant par `.exe` avec `/s` dort ; on s'appuie donc
        // sur le script de code ci-dessous, qui rend 100 pour tout appel.
        let wine = banc.c.wine_dir.join("bin/wine");
        ecrire_executable(&wine, "#!/bin/sh\nexit 100\n");
        let lancer =
            |m| lancer_dependance(&banc.c, &dep(m), &fichier, &ProgressionMuette, DELAI_COURT);
        assert!(lancer(Methode::Installaware).is_ok());
        let r = lancer(Methode::Nsis);
        assert!(
            matches!(
                r,
                Err(Erreur::Commande {
                    code: Some(100),
                    ..
                })
            ),
            "{r:?}"
        );
    }

    #[test]
    fn delai_depasse_dans_une_dependance_arrete_quand_meme_le_wineserver() {
        let banc = Banc::nouveau();
        let journal = banc.racine.path().join("wineserver.log");
        ecrire_executable(
            &banc.c.wine_dir.join("bin/wineserver"),
            &format!("#!/bin/sh\necho arret >> {}\n", journal.display()),
        );
        ecrire_executable(&banc.c.drive_c().join("deps/Dep.exe"), "MZ");
        let dep = Dependance {
            fichier: r"C:\deps\Dep.exe".into(),
            methode: Methode::Installaware, // le faux wine dort avec `/s`
        };
        let fichier = banc.c.drive_c().join("deps/Dep.exe");
        let r = lancer_dependance(
            &banc.c,
            &dep,
            &fichier,
            &ProgressionMuette,
            Duration::from_millis(300),
        );
        assert!(matches!(r, Err(Erreur::Delai { .. })), "{r:?}");
        let arrets = std::fs::read_to_string(journal).unwrap_or_default();
        assert!(arrets.contains("arret"), "wineserver non arrêté");
    }

    #[test]
    fn dependance_ambigue_refusee() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        for nom in ["Dep 1.exe", "Dep 2.exe"] {
            ecrire_executable(&banc.c.drive_c().join("deps").join(nom), "MZ");
        }
        let cat = recette(
            &banc,
            "[installation]\nmethode = \"inno\"\n\
             [[dependances]]\nfichier = 'C:\\deps\\Dep *.exe'\nmethode = \"nsis\"\n",
        );
        let r = banc.lancer(&exe, &cat, SANS_OPTIONS, DELAI_COURT);
        assert!(matches!(r, Err(Erreur::MotifAmbigu { .. })), "{r:?}");
    }

    #[test]
    fn dependance_est_lancee_en_silencieux_et_code_inattendu_est_une_erreur() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        ecrire_executable(&banc.c.drive_c().join("deps/Dep.exe"), "MZ");
        // Faux wine : `-quiet` (WiX) rend 67, code inattendu pour une dépendance.
        let wix = recette(
            &banc,
            "[installation]\nmethode = \"inno\"\n\
             [[dependances]]\nfichier = 'C:\\deps\\Dep.exe'\nmethode = \"wix\"\n",
        );
        let r = banc.lancer(&exe, &wix, SANS_OPTIONS, DELAI_COURT);
        assert!(
            matches!(r, Err(Erreur::Commande { code: Some(67), .. })),
            "{r:?}"
        );
        // NSIS (`/S`) réussit : la dépendance est lancée avec ses options silencieuses.
        let nsis = recette(
            &banc,
            "[installation]\nmethode = \"inno\"\n\
             [[dependances]]\nfichier = 'C:\\deps\\Dep.exe'\nmethode = \"nsis\"\n",
        );
        banc.lancer(&exe, &nsis, SANS_OPTIONS, DELAI_COURT).unwrap();
        assert!(banc.appels().contains("Dep.exe /S"), "{}", banc.appels());
    }

    #[test]
    fn la_recette_impose_sa_methode_et_ecrit_le_registre() {
        let banc = Banc::nouveau();
        // Le fichier est reconnu comme inconnu, la recette dit « inno ».
        let exe = banc.installeur("inconnu.bin", "Setup.exe");
        let cat = recette(
            &banc,
            "[installation]\nmethode = \"inno\"\n\
             [[correctifs.registre]]\ncle = 'HKCU\\Software\\Essai'\nvaleur = \"Dossier\"\n\
             donnee = '{telechargements}'\n",
        );
        let r = banc.lancer(&exe, &cat, SANS_OPTIONS, DELAI_COURT).unwrap();
        assert_eq!(r.methode, TypeInstalleur::Inno);
        assert_eq!(r.recette.as_deref(), Some("Essai"));
        assert_eq!(r.plugins.len(), 1);
        let appels = banc.appels();
        assert!(
            appels.contains("reg add HKCU\\Software\\Essai /v Dossier /t REG_SZ /d C:\\users\\"),
            "{appels}"
        );
        assert!(appels.contains("\\Downloads /f"), "{appels}");
    }

    #[test]
    fn point_de_restauration_refuse_quand_le_prefixe_est_incomplet() {
        let banc = Banc::nouveau();
        std::fs::remove_file(banc.c.prefixe.join("user.reg")).unwrap();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let r = banc.lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT);
        assert!(r.is_err());
        assert_eq!(banc.appels(), "", "l'installeur ne doit pas être lancé");
    }

    #[test]
    fn archive_de_plugins_est_copiee_et_pontee() {
        if which_7z().is_none() {
            eprintln!("7z absent : test ignoré");
            return;
        }
        let banc = Banc::nouveau();
        let source = banc.racine.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("Zip.vst3"), "plugin").unwrap();
        let archive = banc.racine.path().join("Plugin.7z");
        let ok = std::process::Command::new("7z")
            .arg("a")
            .arg(&archive)
            .arg(source.join("Zip.vst3"))
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok);
        let r = banc
            .lancer(&archive, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert_eq!(r.methode, TypeInstalleur::Archive);
        assert!(!r.interactif_necessaire);
        assert_eq!(r.plugins.len(), 1);
        assert_eq!(r.plugins[0].0.nom, "Zip");
        assert_eq!(banc.appels(), "", "aucun installeur à lancer");
    }

    fn which_7z() -> Option<()> {
        std::process::Command::new("7z").output().ok().map(|_| ())
    }

    /// Progression qui garde les messages reçus.
    #[derive(Default)]
    struct Enregistreuse(std::sync::Mutex<Vec<String>>);

    impl Progression for Enregistreuse {
        fn etape(&self, message: &str) {
            if let Ok(mut v) = self.0.lock() {
                v.push(message.to_string());
            }
        }
    }

    const INTERACTIF: OptionsInstallation = OptionsInstallation {
        interactif: true,
        tester: false,
    };

    #[test]
    fn inconnu_en_silencieux_ne_cree_ni_point_ni_environnement() {
        let banc = Banc::nouveau();
        std::fs::remove_dir_all(&banc.c.prefixe).unwrap();
        let exe = banc.installeur("inconnu.bin", "Setup.exe");
        let r = banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        assert!(r.interactif_necessaire);
        assert!(r.point.is_none(), "aucun point de restauration");
        assert!(r.plugins.is_empty());
        assert!(!banc.c.prefixe.exists(), "aucun environnement créé");
        assert!(!banc.c.donnees.join("restauration").exists());
        assert_eq!(banc.appels(), "", "ni installeur ni registre");
    }

    #[test]
    fn inconnu_en_silencieux_n_applique_pas_le_registre_de_la_recette() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("inconnu.bin", "Setup.exe");
        let cat = recette(
            &banc,
            "[[correctifs.registre]]\ncle = 'HKCU\\Software\\Essai'\nvaleur = \"A\"\ndonnee = \"B\"\n",
        );
        let r = banc.lancer(&exe, &cat, SANS_OPTIONS, DELAI_COURT).unwrap();
        assert!(r.interactif_necessaire && r.point.is_none());
        assert_eq!(banc.appels(), "", "aucune écriture de registre");
    }

    #[test]
    fn message_installeur_normal_seulement_apres_une_installation_silencieuse() {
        let banc = Banc::nouveau();
        let exe = banc.installeur("nsis.bin", "Setup.exe");
        let muet = Enregistreuse::default();
        installer_avec_delai(&banc.c, &exe, None, SANS_OPTIONS, &muet, DELAI_COURT).unwrap();
        let messages = muet.0.lock().unwrap().join("\n");
        assert!(messages.contains("No new or updated plugin"), "{messages}");
        assert!(!messages.contains("code"), "code 0 : non cité ; {messages}");

        // En interactif, l'utilisateur a tout fait lui-même : pas de conseil inutile.
        let interactif = Enregistreuse::default();
        let r = installer_avec_delai(&banc.c, &exe, None, INTERACTIF, &interactif, DELAI_COURT)
            .unwrap();
        assert!(!r.interactif_necessaire);
        let messages = interactif.0.lock().unwrap().join("\n");
        assert!(!messages.contains("No new or updated plugin"), "{messages}");
    }

    #[test]
    fn delai_interactif_bien_plus_long_que_le_delai_silencieux() {
        assert_eq!(delai_installeur(false), DELAI_INSTALLATION);
        assert_eq!(delai_installeur(true), DELAI_INSTALLATION_INTERACTIF);
        assert!(DELAI_INSTALLATION_INTERACTIF >= Duration::from_secs(6 * 3600));
        assert!(DELAI_INSTALLATION_INTERACTIF > DELAI_INSTALLATION);
    }

    #[test]
    fn le_verrou_existe_pendant_l_installation_et_disparait_apres() {
        let banc = Banc::nouveau();
        // Ce faux wine note le contenu du dossier de données pendant qu'il « installe ».
        ecrire_executable(
            &banc.c.wine_dir.join("bin/wine"),
            "#!/bin/sh\nls \"$WINEPREFIX/..\" > \"$(dirname \"$0\")/donnees.log\"\nexit 0\n",
        );
        let exe = banc.installeur("inno.bin", "Setup.exe");
        banc.lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .unwrap();
        let pendant = std::fs::read_to_string(banc.c.wine_dir.join("bin/donnees.log")).unwrap();
        assert!(pendant.contains("installation.lock"), "{pendant}");
        assert!(!banc.c.donnees.join("installation.lock").exists());
    }

    #[test]
    fn le_verrou_disparait_aussi_apres_une_erreur() {
        let banc = Banc::nouveau();
        std::fs::remove_file(banc.c.prefixe.join("user.reg")).unwrap();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        assert!(banc
            .lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT)
            .is_err());
        assert!(!banc.c.donnees.join("installation.lock").exists());
    }

    #[test]
    fn deux_installations_a_la_fois_sont_refusees() {
        let banc = Banc::nouveau();
        let _verrou = wimao_core::poser_verrou_installation(&banc.c).unwrap();
        let exe = banc.installeur("inno.bin", "Setup.exe");
        let r = banc.lancer(&exe, &catalogue_vide(), SANS_OPTIONS, DELAI_COURT);
        assert!(matches!(r, Err(Erreur::InstallationEnCours)), "{r:?}");
        assert_eq!(banc.appels(), "");
    }

    #[test]
    fn arguments_msi_et_autres() {
        let f = Path::new("/t/Setup.msi");
        let msi = arguments_wine(TypeInstalleur::Msi, f, false).unwrap();
        assert_eq!(
            msi[..3],
            ["msiexec", "/i", "/t/Setup.msi"].map(OsString::from)
        );
        assert!(msi.contains(&OsString::from("/qn")));
        let interactif = arguments_wine(TypeInstalleur::Msi, f, true).unwrap();
        assert_eq!(interactif.len(), 3);
        let nsis = arguments_wine(TypeInstalleur::Nsis, Path::new("/t/a.exe"), false).unwrap();
        assert_eq!(nsis, ["/t/a.exe", "/S"].map(OsString::from));
        assert!(arguments_wine(TypeInstalleur::Inconnu, f, false).is_none());
        assert_eq!(
            arguments_wine(TypeInstalleur::Inconnu, f, true),
            Some(vec![OsString::from(f)])
        );
    }
}
