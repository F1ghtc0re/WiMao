//! Session d'un gestionnaire d'éditeur (Native Access, Arturia Software Center…) : ouverture,
//! surveillance périodique, fermeture, et relais des liens de connexion.
//!
//! Une session suit la spec du sous-projet 2, sections 4.2 à 4.4 :
//! - [`ouvrir`] pose le verrou d'opération de WiMao, crée un point de restauration, applique les
//!   réglages de la recette, démarre ses services, relève les plugins puis lance le programme
//!   du gestionnaire (sans l'attendre) ;
//! - [`Session::tick`] est appelé régulièrement par l'interface : il fait travailler le pilote
//!   (relance en silencieux des installeurs interactifs) toutes les [`INTERVALLE_PILOTE`] et
//!   ponte les plugins apparus toutes les `INTERVALLE_RELEVE` ;
//! - [`Session::fermer`] arrête le gestionnaire, ponte ce qui reste, teste les plugins de la
//!   session et libère le verrou ;
//! - [`relayer_lien`] transmet au gestionnaire un lien de connexion (`native-access://…`)
//!   reçu du navigateur ; il ne prend ni verrou ni journal : c'est une commande très courte,
//!   lancée par le bureau pendant qu'une session tourne (elle tient déjà le verrou).
//!
//! La [`Session`] **possède** tout ce dont elle a besoin (copies des chemins et de la recette) :
//! elle n'emprunte rien et peut donc passer dans un fil de travail de l'interface.
//!
//! # Pontage « au fil de l'eau »
//! Un plugin nouveau ou modifié (par rapport au relevé d'ouverture) n'est ponté que quand sa
//! [`Signature`] (date et taille) est la même qu'au relevé précédent : il n'est plus en cours
//! d'écriture (`plugins_stables`). À la fermeture, tout ce qui reste est ponté, stable ou non.
//! Les tests d'ouverture attendent la fin de la session (ils lancent chacun un hôte yabridge).
//!
//! Utilise : `installation` (réglages, pontage, test), `pilote`, `chemins_windows`,
//! `wimao_core` (verrou, restauration, inventaire, processus, journal, historique, Wine).
//! [`Session::fermer`] écrit l'action `Gestionnaire` dans l'historique (point de session compris).
//! Utilisé par : `wimao-cli` (`gestionnaire ouvrir`, `relayer-lien`) et `wimao` (fil et écran
//! de session).

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use wimao_core::diagnostic::{poser_verrou_installation, VerrouInstallation};
use wimao_core::historique::{self, Action, ResultatAction};
use wimao_core::i18n::{tr, trf};
use wimao_core::inventaire::{signature, Signature};
use wimao_core::pontage::EtatPontage;
use wimao_core::processus::{arreter_processus, processus_du_prefixe, ProcessusWine};
use wimao_core::restauration::{self, PointRestauration};
use wimao_core::{
    creer_environnement, expliquer, journal, lancer, Chemins, Erreur, Inventaire, PluginWindows,
    Progression, Resultat, ResultatTest, Wine,
};

use crate::catalogue::Catalogue;
use crate::chemins_windows::{remplacer_variables, resoudre_motif, utilisateur_windows};
use crate::installation::{
    appliquer_reglages, est_installe, ponter_plugins, tester_plugins, DELAI_INSTALLATION,
};
use crate::pilote::{est_intouchable, IssueRelance, Pilote};
use crate::recette::{Gestionnaire, Recette};

/// Intervalle entre deux passages du pilote (spec 4.3 : « toutes les 2 secondes »).
pub const INTERVALLE_PILOTE: Duration = Duration::from_secs(2);
/// Intervalle entre deux relevés de plugins (spec 4.3 : « toutes les 60 secondes »). Il couvre
/// aussi le daemon de Native Access, qui continue d'installer après la fermeture de la fenêtre.
pub(crate) const INTERVALLE_RELEVE: Duration = Duration::from_secs(60);

/// Délai de `wine net start <service>` : un démarrage de service prend quelques secondes.
const DELAI_SERVICE: Duration = Duration::from_secs(60);
/// Délai laissé aux processus du gestionnaire pour se fermer avant `SIGKILL` (spec 4.4).
const DELAI_FERMETURE: Duration = Duration::from_secs(10);
/// Sous-dossier des journaux de sortie des programmes lancés, dans les données de WiMao.
const DOSSIER_JOURNAUX: &str = "journaux";
/// Journal commun des liens relayés (`<données>/journaux/liens.log`).
const JOURNAL_LIENS: &str = "liens.log";
/// Séparateur entre le schéma d'un lien et le reste (`native-access://…`).
const SEPARATEUR_SCHEMA: &str = "://";
/// Fragments (minuscules) du nom d'un processus d'installation ou de téléchargement
/// (spec 4.4, étape 1).
const FRAGMENTS_INSTALLEUR: [&str; 3] = ["setup", "install", "msiexec"];
/// Texte que `wine net start` écrit quand le service tourne déjà : ce n'est pas une erreur.
const SERVICE_DEJA_DEMARRE: &str = "already";
/// Titre de l'événement du journal pour le bilan d'une session.
const EVENEMENT_BILAN: &str = "session : bilan";
/// Titre de l'événement du journal quand l'arrêt des processus du gestionnaire échoue.
const EVENEMENT_ARRET_ECHEC: &str = "session : arrêt du gestionnaire en échec";

/// Ce qui s'est passé pendant un [`Session::tick`], pour l'affichage de la session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvenementSession {
    /// Le pilote a traité un installeur (« Jura installé en silencieux »).
    Relance(IssueRelance),
    /// Un plugin nouveau ou modifié vient d'être ponté.
    Ponte(PluginWindows),
}

/// Texte d'un événement de session, pour la ligne de la CLI et le toast de l'interface (une
/// seule source pour les deux) : « Plugin bridged: … », « … installed silently (exit code 0) »…
pub fn texte_evenement(evenement: &EvenementSession) -> String {
    match evenement {
        EvenementSession::Ponte(plugin) => trf("Plugin bridged: {name}", &[("name", &plugin.nom)]),
        EvenementSession::Relance(IssueRelance::Relance { nom, code, .. }) => match code {
            Some(code) => trf(
                "{name} installed silently (exit code {code})",
                &[("name", nom), ("code", &code.to_string())],
            ),
            None => trf(
                "{name} ran silently but did not finish in time",
                &[("name", nom)],
            ),
        },
        EvenementSession::Relance(IssueRelance::Refermee { nom }) => {
            trf("{name} was already run silently: closed", &[("name", nom)])
        }
        EvenementSession::Relance(IssueRelance::Inconnu { nom }) => {
            trf("{name}: unknown installer, left as it is", &[("name", nom)])
        }
    }
}

/// Bilan d'une session fermée par [`Session::fermer`] (même forme que celui d'une installation).
#[derive(Debug, Clone)]
pub struct BilanSession {
    /// Point de restauration créé à l'ouverture.
    pub point: PointRestauration,
    /// Plugins nouveaux ou modifiés pendant la session, leur état de pontage et, si le test a
    /// été demandé et que le plugin est ponté, son résultat.
    pub plugins: Vec<(PluginWindows, EtatPontage, Option<ResultatTest>)>,
    /// Chemins des plugins qui existaient à l'ouverture et dont le binaire a changé.
    pub mis_a_jour: BTreeSet<PathBuf>,
    /// Tout ce que le pilote a fait pendant la session.
    pub relances: Vec<IssueRelance>,
    /// Messages traduits des règles d'erreurs (`wimao_core::regles::expliquer`) qui ont
    /// reconnu une ligne du journal du gestionnaire écrite pendant la session (« Native Access
    /// has no download folder: use Repair »…), sans doublon : à montrer en avertissement.
    pub avertissements: Vec<String>,
}

// ---------------------------------------------------------------------------------------
// Décision de chaque tour de session, commune à la CLI et à l'interface
// ---------------------------------------------------------------------------------------

/// Ce que la personne demande à la boucle de session : bouton de l'interface, ou Ctrl+C dans
/// la CLI (le premier vaut [`Commande::Terminer`], le second [`Commande::ArreterQuandMeme`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commande {
    /// « I'm done » (1er Ctrl+C dans la CLI) : finir la session, après les installeurs en
    /// cours.
    Terminer,
    /// « Stop anyway » (2e Ctrl+C dans la CLI) : finir sans attendre les installeurs.
    ArreterQuandMeme,
    /// Fenêtre de l'interface fermée : quitter sans fermer le gestionnaire (spec 4.4). La CLI
    /// ne l'envoie jamais.
    Abandonner,
}

/// Où en est la boucle de session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Surveillance normale : pilote et pontage.
    Surveillance,
    /// « I'm done » reçu, des installeurs tournent : on attend qu'ils finissent.
    Attente,
}

/// Décision d'un tour de boucle (voir [`suite`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suite {
    /// Encore un tour.
    Continuer,
    /// Passer en [`Phase::Attente`] et prévenir la personne (installeurs encore en cours).
    Attendre,
    /// Fermer la session (`Session::fermer`).
    Fermer,
    /// Quitter sans fermer le gestionnaire.
    Abandonner,
}

/// Décide de la suite d'un tour, d'après la `phase`, la `commande` reçue pendant l'attente
/// précédente, l'état du gestionnaire (`ferme`) et les `installeurs` encore en cours (`None` :
/// jamais lus avec succès ; comptent comme en cours, pour ne pas fermer sans attendre).
///
/// Fonction pure, seule source de la décision pour la CLI (`gestionnaire ouvrir`) et
/// l'interface (fil de session) : fin automatique quand le gestionnaire est fermé et qu'aucun
/// installeur ne tourne ; « I'm done » attend les installeurs ; « Stop anyway » n'attend plus ;
/// l'abandon l'emporte sur tout. Pendant l'attente, l'appelant continue d'appeler
/// [`Session::tick`] (le pilote et le pontage travaillent encore).
pub fn suite(
    phase: Phase,
    commande: Option<Commande>,
    ferme: bool,
    installeurs: Option<&[String]>,
) -> Suite {
    let aucun = installeurs.is_some_and(|liste| liste.is_empty());
    match (commande, phase) {
        (Some(Commande::Abandonner), _) => Suite::Abandonner,
        (Some(Commande::ArreterQuandMeme), _) => Suite::Fermer,
        _ if aucun && (ferme || phase == Phase::Attente) => Suite::Fermer,
        (Some(Commande::Terminer), _) if aucun => Suite::Fermer,
        (Some(Commande::Terminer), Phase::Surveillance) => Suite::Attendre,
        _ => Suite::Continuer,
    }
}

// Vérifié à la compilation : la session doit pouvoir passer dans un fil de l'interface.
const _: fn() = || {
    fn est_send<T: Send>() {}
    est_send::<Session>();
};

/// Une session de gestionnaire ouverte. Voir la documentation du module.
///
/// Détruire la session sans appeler [`Session::fermer`] libère le verrou (`Drop`) mais laisse
/// le gestionnaire ouvert, ce qui est voulu quand WiMao est fermé en pleine session (spec 4.4).
pub struct Session {
    /// Chemins de WiMao (copie : la session n'emprunte rien).
    chemins: Chemins,
    /// Recette du gestionnaire (copie).
    recette: Recette,
    /// Verrou d'opération, libéré quand la session est détruite.
    _verrou: VerrouInstallation,
    /// Point de restauration créé à l'ouverture.
    point: PointRestauration,
    /// Relevé des plugins à l'ouverture : référence pour « nouveau » et « modifié ».
    ouverture: Inventaire,
    /// Mémoire du pilote (installeurs déjà relancés).
    pilote: Pilote,
    /// Processus `wine` qui a lancé le gestionnaire.
    enfant: Option<Child>,
    /// Signatures du relevé précédent (au début, celles de l'ouverture).
    signatures_precedentes: BTreeMap<PathBuf, Signature>,
    /// Signature de chaque plugin déjà ponté pendant la session : un plugin n'est pas repassé
    /// au pontage tant que sa signature ne change pas.
    signatures_pontees: BTreeMap<PathBuf, Signature>,
    /// Plugins pontés pendant la session et leur état (un seul par chemin).
    plugins: Vec<(PluginWindows, EtatPontage)>,
    /// Plugins pontés qui existaient à l'ouverture.
    mis_a_jour: BTreeSet<PathBuf>,
    /// Relances du pilote, dans l'ordre.
    relances: Vec<IssueRelance>,
    /// Instant du dernier relevé (l'ouverture compte comme un relevé).
    dernier_releve: Instant,
    /// Instant du dernier passage du pilote ; `None` tant qu'il n'a pas travaillé.
    dernier_pilote: Option<Instant>,
    /// Journal de sortie du gestionnaire (`<données>/journaux/<recette>.log`).
    journal: PathBuf,
    /// Taille du journal à l'ouverture : seule la partie écrite pendant la session est lue
    /// par les règles d'erreurs à la fermeture.
    debut_journal: u64,
}

/// Ouvre la session du gestionnaire décrit par `r` (spec 4.2).
///
/// Dans l'ordre : le gestionnaire doit être installé ; verrou ; environnement créé s'il manque ;
/// point de restauration `session <nom>` ; réglages de la recette ; services ; relevé des
/// plugins ; lancement du programme (dossier de travail = celui du programme, sorties ajoutées à
/// `<données>/journaux/<nom>.log`, entrée vide, sans attendre sa fin).
///
/// Erreurs : [`Erreur::Introuvable`] (recette sans gestionnaire, programme absent, Wine absent),
/// [`Erreur::InstallationEnCours`], [`Erreur::PluginsEnCours`], [`Erreur::Commande`] (service qui
/// ne démarre pas), [`Erreur::MotifAmbigu`], [`Erreur::Io`]. En cas d'erreur, le verrou est
/// libéré et aucun programme n'a été lancé.
pub fn ouvrir(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<Session> {
    let gestionnaire = gestionnaire_de(r)?;
    if !est_installe(c, r) {
        return Err(Erreur::Introuvable(trf(
            "{program} (install the manager first)",
            &[("program", &gestionnaire.programme)],
        )));
    }
    // Gardé jusqu'à la destruction de la session : « Réparer », « Sauvegarder »… sont refusés.
    let verrou = poser_verrou_installation(c)?;
    if !c.prefixe.join("system.reg").exists() {
        creer_environnement(c, p)?;
    }
    p.etape(&tr("Creating a restore point…"));
    let point = restauration::creer(c, &restauration::motif_session(&r.nom))?;
    appliquer_reglages(c, r, p)?;
    demarrer_services(c, r, p)?;
    let ouverture = Inventaire::relever(&c.drive_c())?;

    p.etape(&trf("Opening {name}…", &[("name", &r.nom)]));
    let journal = dossier_journaux(c).join(nom_du_journal(&r.nom));
    // Absent (première ouverture) : 0, tout ce qui sera écrit compte.
    let debut_journal = std::fs::metadata(&journal).map_or(0, |m| m.len());
    let enfant = lancer_gestionnaire(c, gestionnaire, &[], &journal)?;
    Ok(Session {
        chemins: c.clone(),
        recette: r.clone(),
        _verrou: verrou,
        point,
        signatures_precedentes: ouverture.1.clone(),
        ouverture,
        pilote: Pilote::new(),
        enfant: Some(enfant),
        signatures_pontees: BTreeMap::new(),
        plugins: Vec::new(),
        mis_a_jour: BTreeSet::new(),
        relances: Vec::new(),
        dernier_releve: Instant::now(),
        dernier_pilote: None,
        journal,
        debut_journal,
    })
}

impl Session {
    /// Travail périodique de la session ; à appeler souvent (chaque seconde suffit) avec
    /// l'instant courant. Fait le pilote s'il y a des dossiers `pilote` et que
    /// [`INTERVALLE_PILOTE`] s'est écoulé, puis le relevé et le pontage des plugins stables
    /// quand `INTERVALLE_RELEVE` s'est écoulé.
    ///
    /// Les relances du pilote sont synchrones : un appel peut durer le temps d'une relance
    /// (jusqu'à 30 minutes). Erreurs : lecture de `/proc`, relevé, pontage (la prochaine
    /// échéance réessaie). Si le pilote et le relevé échouent tous deux, la première erreur
    /// est rendue (les événements du passage sont alors perdus).
    pub fn tick(
        &mut self,
        maintenant: Instant,
        p: &dyn Progression,
    ) -> Resultat<Vec<EvenementSession>> {
        let mut evenements = Vec::new();
        // Une erreur du pilote ne doit pas retarder le pontage de l'échéance (ni
        // l'inverse) : on fait les deux, puis on rend la première erreur.
        let mut premiere_erreur = None;
        if self.pilote_du(maintenant) {
            self.dernier_pilote = Some(maintenant);
            match self.faire_le_pilote() {
                Ok(faits) => evenements.extend(faits),
                Err(e) => premiere_erreur = Some(e),
            }
        }
        if maintenant.duration_since(self.dernier_releve) >= INTERVALLE_RELEVE {
            self.dernier_releve = maintenant;
            match self.relever_et_ponter(p) {
                Ok(pontes) => evenements.extend(pontes.into_iter().map(EvenementSession::Ponte)),
                Err(e) => {
                    premiere_erreur.get_or_insert(e);
                }
            }
        }
        premiere_erreur.map_or(Ok(evenements), Err)
    }

    /// Relevé périodique : ponte les plugins stables et mémorise les signatures du relevé.
    /// Rend les plugins qui viennent d'être pontés.
    fn relever_et_ponter(&mut self, p: &dyn Progression) -> Resultat<Vec<PluginWindows>> {
        let releve = Inventaire::relever(&self.chemins.drive_c())?;
        let pontes = self.ponter_candidats(&releve, true, p)?;
        self.signatures_precedentes = releve.1;
        Ok(pontes)
    }

    /// Vrai si le pilote doit travailler à `maintenant`.
    fn pilote_du(&self, maintenant: Instant) -> bool {
        !self.pilote_surveille().is_empty()
            && self
                .dernier_pilote
                .is_none_or(|dernier| maintenant.duration_since(dernier) >= INTERVALLE_PILOTE)
    }

    /// Dossiers Windows surveillés par le pilote (variables remplacées).
    fn pilote_surveille(&self) -> Vec<String> {
        let Some(g) = self.recette.gestionnaire.as_ref() else {
            return Vec::new();
        };
        let utilisateur = utilisateur_windows(&self.chemins);
        g.pilote
            .iter()
            .map(|d| remplacer_variables(d, &utilisateur))
            .collect()
    }

    /// Un passage du pilote sur les processus actuels, hors processus du gestionnaire
    /// ([`processus_pour_le_pilote`]) ; mémorise ses relances.
    fn faire_le_pilote(&mut self) -> Resultat<Vec<EvenementSession>> {
        let caches = self
            .recette
            .gestionnaire
            .as_ref()
            .map(noms_caches_au_pilote)
            .unwrap_or_default();
        let processus = processus_pour_le_pilote(processus_du_prefixe(&self.chemins)?, &caches);
        let dossiers = self.pilote_surveille();
        let issues = self
            .pilote
            .agir(&self.chemins, &processus, &dossiers, DELAI_INSTALLATION);
        self.relances.extend(issues.iter().cloned());
        Ok(issues.into_iter().map(EvenementSession::Relance).collect())
    }

    /// Ponte les plugins nouveaux ou modifiés du `releve` qui ne l'ont pas déjà été avec leur
    /// signature actuelle. Avec `seulement_stables`, ceux qui ont encore changé depuis le relevé
    /// précédent attendent. Rend ceux dont l'état de pontage est [`EtatPontage::Ponte`].
    fn ponter_candidats(
        &mut self,
        releve: &Inventaire,
        seulement_stables: bool,
        p: &dyn Progression,
    ) -> Resultat<Vec<PluginWindows>> {
        let modifies = releve.modifies(&self.ouverture);
        let mut candidats = releve.nouveaux(&self.ouverture);
        let chemins_modifies: BTreeSet<PathBuf> =
            modifies.iter().map(|m| m.chemin.clone()).collect();
        candidats.extend(modifies);
        candidats.retain(|c| self.signatures_pontees.get(&c.chemin) != releve.1.get(&c.chemin));
        if seulement_stables {
            candidats = plugins_stables(&self.signatures_precedentes, &candidats);
        }
        if candidats.is_empty() {
            return Ok(Vec::new()); // inutile de lancer yabridgectl pour rien
        }
        let mut nouveaux_pontes = Vec::new();
        for (plugin, etat) in ponter_plugins(&self.chemins, releve, candidats, p)? {
            if let Some(signature) = releve.1.get(&plugin.chemin) {
                self.signatures_pontees
                    .insert(plugin.chemin.clone(), *signature);
            }
            if chemins_modifies.contains(&plugin.chemin) {
                self.mis_a_jour.insert(plugin.chemin.clone());
            }
            if etat == EtatPontage::Ponte {
                nouveaux_pontes.push(plugin.clone());
            }
            self.plugins
                .retain(|(deja, _)| deja.chemin != plugin.chemin);
            self.plugins.push((plugin, etat));
        }
        Ok(nouveaux_pontes)
    }

    /// Vrai quand le gestionnaire est fermé : le processus lancé est terminé **et** aucun
    /// processus de l'environnement n'est un processus du gestionnaire au sens de l'arrêt
    /// (`est_processus_du_gestionnaire` : un argument au nom exact d'un de `noms_a_fermer`,
    /// hors processus intouchables). Une instance peut survivre au processus lancé (NOTES.md),
    /// et les fils Electron citent l'exécutable ailleurs qu'en premier argument : la même
    /// règle que la fermeture évite de croire fermé un gestionnaire que l'arrêt viserait encore.
    ///
    /// Erreurs : lecture de `/proc`, état du processus lancé.
    pub fn gestionnaire_ferme(&mut self) -> Resultat<bool> {
        if let Some(enfant) = self.enfant.as_mut() {
            if enfant.try_wait()?.is_none() {
                return Ok(false);
            }
        }
        let noms = self.noms_a_fermer();
        let processus = processus_du_prefixe(&self.chemins)?;
        Ok(!processus
            .iter()
            .any(|pr| est_processus_du_gestionnaire(pr, &noms)))
    }

    /// Noms des installeurs ou téléchargements en cours, hors gestionnaire (voir
    /// `noms_d_installeurs`). L'interface les affiche et attend avant de fermer.
    ///
    /// Erreur : lecture de `/proc`.
    pub fn installeurs_en_cours(&self) -> Resultat<Vec<String>> {
        let processus = processus_du_prefixe(&self.chemins)?;
        Ok(noms_d_installeurs(&processus, &self.noms_a_fermer()))
    }

    /// Noms des processus du gestionnaire (`fermer` de la recette, sinon son programme).
    fn noms_a_fermer(&self) -> Vec<String> {
        self.recette
            .gestionnaire
            .as_ref()
            .map(Gestionnaire::noms_a_fermer)
            .unwrap_or_default()
    }

    /// Termine la session (spec 4.4, étapes 2 à 5) : arrête les processus du gestionnaire
    /// (10 s avant `SIGKILL`), fait un dernier relevé et ponte tout ce qui reste, teste les
    /// plugins de la session si `tester`, passe la partie du journal du gestionnaire écrite
    /// pendant la session aux règles d'erreurs (`BilanSession::avertissements`), écrit le bilan
    /// dans le journal. Le verrou est libéré
    /// quand la session est détruite à la fin de l'appel, même en cas d'erreur. Les services ne
    /// sont pas arrêtés.
    ///
    /// Erreurs : relevé et pontage. Une erreur d'arrêt des processus est seulement notée au
    /// journal : la fermeture continue.
    pub fn fermer(mut self, tester: bool, p: &dyn Progression) -> Resultat<BilanSession> {
        // Une seule entrée d'historique par session, écrite à la fermeture (le point de session
        // existe depuis `ouvrir`) : `Reussi`, ou `Echec` avec la cause.
        let resultat = self.terminer(tester, p);
        let (issue, details) = match &resultat {
            Ok(_) => (ResultatAction::Reussi, String::new()),
            Err(e) => (ResultatAction::Echec, e.to_string()),
        };
        historique::ajouter(
            &self.chemins,
            Action::Gestionnaire,
            &self.recette.nom,
            issue,
            Some(&self.point.id),
            &details,
        );
        resultat
    }

    /// Corps de [`Session::fermer`] : tout sauf l'écriture dans l'historique.
    fn terminer(&mut self, tester: bool, p: &dyn Progression) -> Resultat<BilanSession> {
        p.etape(&trf("Closing {name}…", &[("name", &self.recette.nom)]));
        // Un échec ici (lecture de /proc, signal refusé) ne doit pas empêcher de ponter ce
        // que la session a installé : il est noté au journal et la fermeture continue.
        if let Err(e) = self.arreter_le_gestionnaire() {
            journal::noter(
                EVENEMENT_ARRET_ECHEC,
                &format!("{} : {e}", self.recette.nom),
            );
        }
        p.etape(&tr("Looking for the installed plugins…"));
        let releve = Inventaire::relever(&self.chemins.drive_c())?;
        self.ponter_candidats(&releve, false, p)?;
        let plugins = std::mem::take(&mut self.plugins);
        let plugins = if tester {
            tester_plugins(&self.chemins, plugins, p)
        } else {
            plugins.into_iter().map(|(g, e)| (g, e, None)).collect()
        };
        let bilan = BilanSession {
            point: self.point.clone(),
            plugins,
            mis_a_jour: std::mem::take(&mut self.mis_a_jour),
            relances: std::mem::take(&mut self.relances),
            avertissements: expliquer_journal(&lire_depuis(&self.journal, self.debut_journal)),
        };
        noter_bilan(&self.recette.nom, &bilan);
        Ok(bilan)
    }
}

impl Session {
    /// Arrête les processus du gestionnaire (jamais ceux de [`est_intouchable`]) puis confie
    /// le processus lancé à un fil qui l'attend, pour qu'il ne reste pas zombie. Rend la
    /// première erreur (lecture de `/proc`, signal refusé) ; le fil est confié dans tous les cas.
    fn arreter_le_gestionnaire(&mut self) -> Resultat<()> {
        let noms = self.noms_a_fermer();
        let resultat = processus_du_prefixe(&self.chemins).and_then(|processus| {
            let pids: Vec<u32> = processus
                .iter()
                .filter(|pr| est_processus_du_gestionnaire(pr, &noms))
                .map(|pr| pr.pid)
                .collect();
            arreter_processus(&pids, DELAI_FERMETURE)
        });
        if let Some(enfant) = self.enfant.take() {
            attendre_en_arriere_plan(enfant);
        }
        resultat
    }
}

/// Confie `enfant` à un fil qui l'attend : le processus terminé est récolté (pas de zombie)
/// sans que l'appelant bloque. Le fil ne retient rien.
fn attendre_en_arriere_plan(mut enfant: Child) {
    std::thread::spawn(move || {
        let _ = enfant.wait();
    });
}

/// Noms d'exécutables cachés au pilote : ceux de `fermer` (ou, à défaut, le programme, voir
/// [`Gestionnaire::noms_a_fermer`]) **et** toujours le nom de fichier de `programme`, qui ne
/// figure pas forcément dans un `fermer` renseigné. Sans doublon (casse ignorée).
pub(crate) fn noms_caches_au_pilote(g: &Gestionnaire) -> Vec<String> {
    let programme = g
        .programme
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&g.programme);
    let mut noms: Vec<String> = Vec::new();
    for nom in g.noms_a_fermer().into_iter().chain([programme.to_string()]) {
        if !contient_sans_casse(&noms, &nom) {
            noms.push(nom);
        }
    }
    noms
}

/// Processus que le pilote peut voir : tous, sauf ceux du gestionnaire de la session (un
/// argument au nom exact d'un de `noms_gestionnaire`, voir [`cite_un_programme`] et
/// [`noms_caches_au_pilote`]). Les fils d'un gestionnaire Electron citent le même exécutable :
/// ils sont cachés aussi.
///
/// Pourquoi : le pilote ferme tout processus dont un argument est le chemin d'un installeur
/// surveillé. Un gestionnaire qui a reçu ce chemin dans sa ligne de commande (le faux
/// gestionnaire du test d'intégration `tests/gestionnaire.rs`, ou un vrai qui ouvre un fichier
/// téléchargé) serait fermé avec l'installeur, et la session finirait sous l'utilisateur.
/// Constaté en écrivant ce test d'intégration : le gestionnaire était tué dès le premier
/// passage.
pub(crate) fn processus_pour_le_pilote(
    processus: Vec<ProcessusWine>,
    noms_gestionnaire: &[String],
) -> Vec<ProcessusWine> {
    processus
        .into_iter()
        .filter(|pr| !cite_un_programme(pr, noms_gestionnaire))
        .collect()
}

/// Plugins de `candidats` dont la signature actuelle (lue sur le disque) est **égale** à celle
/// du relevé `precedent` : ils n'ont pas bougé entre deux relevés, l'installeur a fini de les
/// écrire. Un plugin absent du relevé précédent attend le suivant ; un plugin dont la signature
/// ne se lit pas aussi.
pub(crate) fn plugins_stables(
    precedent: &BTreeMap<PathBuf, Signature>,
    candidats: &[PluginWindows],
) -> Vec<PluginWindows> {
    candidats
        .iter()
        .filter(|c| match (precedent.get(&c.chemin), signature(c)) {
            (Some(avant), Ok(maintenant)) => *avant == maintenant,
            _ => false,
        })
        .cloned()
        .collect()
}

/// Noms des programmes de `processus` qui ressemblent à un installeur ou à un téléchargement :
/// leur nom contient `setup`, `install` ou `msiexec` (casse ignorée), sauf ceux de `exclus`
/// (casse ignorée aussi). Sans l'exclusion, `PA-InstallationManager.exe` (le gestionnaire de
/// Plugin Alliance) compterait, puisqu'il contient « install ». Chaque nom n'est rendu qu'une fois.
pub(crate) fn noms_d_installeurs(processus: &[ProcessusWine], exclus: &[String]) -> Vec<String> {
    let mut noms: Vec<String> = Vec::new();
    for nom in processus.iter().filter_map(ProcessusWine::nom_programme) {
        let minuscules = nom.to_lowercase();
        let ressemble = FRAGMENTS_INSTALLEUR.iter().any(|f| minuscules.contains(f));
        if ressemble && !contient_sans_casse(exclus, &nom) && !noms.contains(&nom) {
            noms.push(nom);
        }
    }
    noms
}

/// Relaie un lien de connexion (`native-access://…`) au gestionnaire dans Wine.
///
/// 1. le schéma est le texte avant `://`, en minuscules ; la recette dont `lien_connexion` lui
///    est égal est retenue ([`Erreur::Introuvable`] sinon) ;
/// 2. le programme du gestionnaire est lancé avec ses `arguments` suivis de `url` ;
/// 3. sorties ajoutées à `<données>/journaux/liens.log`, entrée vide, sans attendre la fin.
///
/// Pas de verrou ni de journal de WiMao : le gestionnaire tourne déjà (c'est lui qui reçoit le
/// lien dans son instance ouverte) et cette commande doit rester instantanée. L'`url` peut
/// contenir un jeu d'identification : elle n'est jamais écrite dans un message d'erreur.
///
/// Erreurs : [`Erreur::Introuvable`] (schéma absent ou inconnu, programme absent, Wine absent),
/// [`Erreur::MotifAmbigu`], [`Erreur::Io`].
pub fn relayer_lien(c: &Chemins, catalogue: &Catalogue, url: &str) -> Resultat<()> {
    let Some((schema, _)) = url.split_once(SEPARATEUR_SCHEMA) else {
        return Err(Erreur::Introuvable(tr("a link of the form scheme://…")));
    };
    let schema = schema.to_lowercase();
    let recette = catalogue
        .0
        .iter()
        .find(|r| r.installation.lien_connexion.as_deref() == Some(schema.as_str()))
        .ok_or_else(|| {
            Erreur::Introuvable(trf(
                "a recipe for the link scheme {scheme}",
                &[("scheme", &schema)],
            ))
        })?;
    let journal = dossier_journaux(c).join(JOURNAL_LIENS);
    let enfant = lancer_gestionnaire(c, gestionnaire_de(recette)?, &[url.to_string()], &journal)?;
    // Un fil récolte le processus quand il se termine (sinon il resterait zombie dans une
    // interface qui reste ouverte) ; il ne retient rien et meurt avec un processus court.
    attendre_en_arriere_plan(enfant);
    Ok(())
}

/// Section `[gestionnaire]` de la recette, ou [`Erreur::Introuvable`] si elle n'en a pas.
fn gestionnaire_de(r: &Recette) -> Resultat<&Gestionnaire> {
    r.gestionnaire.as_ref().ok_or_else(|| {
        Erreur::Introuvable(trf(
            "{name}: this recipe has no manager to open",
            &[("name", &r.nom)],
        ))
    })
}

/// Démarre les services de la recette (`wine net start <nom>`). Un service déjà démarré
/// (sortie qui contient « already ») n'est pas une erreur : la session peut être rouverte.
/// Partagée avec `installation::reparer_gestionnaires` (« Réparer » arrête Wine, donc les
/// services).
pub(crate) fn demarrer_services(c: &Chemins, r: &Recette, p: &dyn Progression) -> Resultat<()> {
    let wine = Wine::nouveau(c);
    for service in &r.correctifs.services {
        p.etape(&trf("Starting the {name} service…", &[("name", service)]));
        // « already » est le texte anglais de ERROR_SERVICE_ALREADY_RUNNING : LC_ALL=C empêche
        // une traduction par la langue de l'utilisateur de le faire disparaître.
        let mut cmd = wine.commande("wine")?;
        cmd.args(["net", "start"]).arg(service).env("LC_ALL", "C");
        let sortie = lancer(&mut cmd, DELAI_SERVICE)?;
        let deja = sortie.texte.to_lowercase().contains(SERVICE_DEJA_DEMARRE);
        if sortie.code != 0 && !deja {
            return Err(Erreur::Commande {
                programme: format!("wine net start {service}"),
                code: Some(sortie.code),
                sortie: sortie.texte,
            });
        }
    }
    Ok(())
}

/// Lance le programme du gestionnaire dans Wine **sans l'attendre** et rend le processus.
///
/// - dossier de travail = celui du programme : l'Arturia Software Center l'exige (spec 4.2) ;
/// - arguments de la recette (variables remplacées), puis `supplementaires` ;
/// - sorties ajoutées à `journal`, entrée vide : sans entrée, un programme graphique ne reste
///   pas bloqué à lire un terminal ;
/// - pas d'arrêt du `wineserver` : le gestionnaire tourne dedans ;
/// - **groupe de processus à part** (`process_group(0)`), pour [`ouvrir`] comme pour
///   [`relayer_lien`] : un Ctrl+C dans le terminal envoie `SIGINT` à tout le groupe de premier
///   plan du terminal. Sans groupe à part, le gestionnaire (et le `wine` qui le porte) le
///   recevrait et se fermerait brutalement, alors que pour `wimao-cli gestionnaire ouvrir` ce
///   Ctrl+C veut dire « J'ai fini » : c'est WiMao qui ferme ensuite le gestionnaire
///   proprement (et `relayer-lien` ne doit pas emporter le gestionnaire qui reçoit le lien).
fn lancer_gestionnaire(
    c: &Chemins,
    g: &Gestionnaire,
    supplementaires: &[String],
    journal: &Path,
) -> Resultat<Child> {
    let utilisateur = utilisateur_windows(c);
    let programme = resoudre_motif(
        &c.drive_c(),
        &remplacer_variables(&g.programme, &utilisateur),
    )?;
    let dossier = programme.parent().unwrap_or(Path::new("/"));
    if let Some(parent) = journal.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let sortie = OpenOptions::new().create(true).append(true).open(journal)?;
    let erreurs = sortie.try_clone()?;
    let mut cmd = Wine::nouveau(c).commande("wine")?;
    cmd.arg(&programme)
        .args(
            g.arguments
                .iter()
                .map(|a| remplacer_variables(a, &utilisateur)),
        )
        .args(supplementaires)
        .current_dir(dossier)
        .stdin(Stdio::null())
        .stdout(sortie)
        .stderr(erreurs)
        // Voir la documentation : le Ctrl+C du terminal ne doit pas atteindre le gestionnaire.
        .process_group(0);
    Ok(cmd.spawn()?)
}

/// Dossier des journaux de sortie : `<données>/journaux`.
fn dossier_journaux(c: &Chemins) -> PathBuf {
    c.donnees.join(DOSSIER_JOURNAUX)
}

/// Nom du journal d'un gestionnaire : le nom de la recette en minuscules, tout caractère qui
/// n'est pas une lettre ou un chiffre devenant un tiret (`Native Access` donne
/// `native-access.log`). Le nom vient d'un fichier de recette : il ne doit jamais pouvoir
/// désigner un autre dossier (`/`, `..`).
fn nom_du_journal(nom_recette: &str) -> String {
    let nom: String = nom_recette
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    format!("{nom}.log")
}

/// Vrai si `noms` contient `nom`, sans tenir compte de la casse.
fn contient_sans_casse(noms: &[String], nom: &str) -> bool {
    let nom = nom.to_lowercase();
    noms.iter().any(|n| n.to_lowercase() == nom)
}

/// Vrai si un argument de la ligne de commande de `pr` est (casse ignorée) l'un des `noms`,
/// après le dernier `\` ou `/`. Plus large que [`ProcessusWine::nom_programme`] : les processus
/// de l'Electron des gestionnaires et leurs enfants citent tous le même exécutable, et une
/// instance survivante rendait la fenêtre noire au lancement suivant (NOTES.md). L'égalité du
/// nom entier évite d'arrêter un processus qui le cite seulement dans un chemin.
fn cite_un_programme(pr: &ProcessusWine, noms: &[String]) -> bool {
    pr.arguments.iter().any(|a| {
        let fichier = a.rsplit(['\\', '/']).next().unwrap_or(a);
        contient_sans_casse(noms, fichier)
    })
}

/// Vrai si `pr` est un processus du gestionnaire à fermer : il cite un des `noms`
/// ([`cite_un_programme`]) et n'est pas [`est_intouchable`] (`wineserver`, hôte yabridge,
/// programme système de Wine). Règle commune à l'arrêt et à [`Session::gestionnaire_ferme`].
fn est_processus_du_gestionnaire(pr: &ProcessusWine, noms: &[String]) -> bool {
    !est_intouchable(pr) && cite_un_programme(pr, noms)
}

/// Taille maximale lue à la fin du journal du gestionnaire : un gestionnaire Electron peut
/// écrire beaucoup pendant une longue session ; les erreurs utiles sont près de la fin.
const TAILLE_MAX_JOURNAL_LU: u64 = 8 * 1024 * 1024;

/// Texte de `fichier` à partir de l'octet `debut` (au plus les [`TAILLE_MAX_JOURNAL_LU`]
/// derniers octets), octets invalides remplacés. Fichier illisible ou plus court que `debut`
/// (journal remplacé entre-temps) : on relit depuis le début, ou rien si illisible.
fn lire_depuis(fichier: &Path, debut: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(fichier) else {
        return String::new();
    };
    let taille = f.metadata().map_or(0, |m| m.len());
    let debut = if debut > taille { 0 } else { debut };
    let debut = debut.max(taille.saturating_sub(TAILLE_MAX_JOURNAL_LU));
    let mut octets = Vec::new();
    if f.seek(SeekFrom::Start(debut)).is_err() || f.read_to_end(&mut octets).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&octets).into_owned()
}

/// Messages des règles d'erreurs (`wimao_core::regles::expliquer`) qui reconnaissent au moins
/// une ligne de `texte`, dans l'ordre d'apparition et sans doublon. Ligne par ligne : deux
/// erreurs différentes dans le même journal donnent deux messages.
pub(crate) fn expliquer_journal(texte: &str) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();
    for (message, _) in texte.lines().filter_map(expliquer) {
        if !messages.contains(&message) {
            messages.push(message);
        }
    }
    messages
}

/// Écrit le bilan de la session au journal de WiMao.
fn noter_bilan(nom: &str, bilan: &BilanSession) {
    journal::noter_info(
        EVENEMENT_BILAN,
        &format!(
            "{nom} : {} plugin(s) traités, {} mis à jour, {} relance(s) du pilote, {} erreur(s) \
             reconnue(s) dans le journal du gestionnaire",
            bilan.plugins.len(),
            bilan.mis_a_jour.len(),
            bilan.relances.len(),
            bilan.avertissements.len()
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use wimao_core::inventaire::Format;
    use wimao_core::ProgressionMuette;

    /// Faux `wine` commun avec les tests d'installation : consigne ses appels et réussit.
    const FAUX_WINE: &str = include_str!("../tests/faux-wine.sh");

    use crate::test_utils::ecrire_executable;

    /// Environnement de test : faux Wine et yabridgectl, faux préfixe, tout en temporaire.
    fn banc() -> (tempfile::TempDir, Chemins) {
        let racine = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(racine.path());
        c.wine_dir = racine.path().join("wine");
        c.yabridge_dir = racine.path().join("yabridge");
        ecrire_executable(&c.wine_dir.join("bin/wine"), FAUX_WINE);
        ecrire_executable(&c.wine_dir.join("bin/wineserver"), "#!/bin/sh\nexit 0\n");
        ecrire_executable(&c.yabridge_dir.join("yabridgectl"), "#!/bin/sh\nexit 0\n");
        std::fs::create_dir_all(&c.prefixe).unwrap();
        for reg in ["system.reg", "user.reg", "userdef.reg"] {
            std::fs::write(c.prefixe.join(reg), "WINE REGISTRY\n").unwrap();
        }
        std::fs::create_dir_all(c.drive_c()).unwrap();
        (racine, c)
    }

    /// Recette de gestionnaire dont le programme est `C:\Faux\Faux.exe`.
    fn recette_faux(racine: &Path, extra: &str) -> Recette {
        let fichier = racine.join("faux.toml");
        std::fs::write(
            &fichier,
            format!(
                "nom = \"Faux Gestionnaire\"\ntype = \"gestionnaire\"\nreconnaitre = [\"*.exe\"]\n\
                 [installation]\nlien-connexion = \"faux-lien\"\n\
                 [gestionnaire]\nprogramme = 'C:\\Faux\\Faux.exe'\narguments = [\"--a\"]\n{extra}"
            ),
        )
        .unwrap();
        crate::recette::charger(&fichier).unwrap()
    }

    fn plugin_de(dossier: &Path, octets: usize) -> PluginWindows {
        let chemin = dossier.join("Essai.vst3");
        std::fs::write(&chemin, vec![b'x'; octets]).unwrap();
        PluginWindows {
            chemin,
            format: Format::Vst3,
            nom: "Essai".into(),
        }
    }

    fn appels(c: &Chemins) -> String {
        std::fs::read_to_string(c.wine_dir.join("bin/appels.log")).unwrap_or_default()
    }

    // Aucun test de cette crate ne change la langue : sans catalogue, les textes sont en anglais.
    #[test]
    fn texte_evenement_cite_le_nom_et_le_code() {
        let relance = EvenementSession::Relance(IssueRelance::Relance {
            nom: "Jura.exe".into(),
            type_: crate::detection::TypeInstalleur::Inno,
            code: Some(3),
        });
        assert_eq!(
            texte_evenement(&relance),
            "Jura.exe installed silently (exit code 3)"
        );
        let inconnu = EvenementSession::Relance(IssueRelance::Inconnu {
            nom: "X.exe".into(),
        });
        assert_eq!(
            texte_evenement(&inconnu),
            "X.exe: unknown installer, left as it is"
        );
    }

    #[test]
    fn plugin_dont_la_signature_change_n_est_pas_ponte() {
        let d = tempfile::tempdir().unwrap();
        let plugin = plugin_de(d.path(), 20);
        let mut precedent = BTreeMap::new();
        precedent.insert(
            plugin.chemin.clone(),
            Signature {
                modifie: None,
                taille: 10,
                change: None,
            },
        );
        assert!(plugins_stables(&precedent, std::slice::from_ref(&plugin)).is_empty());
        precedent.insert(plugin.chemin.clone(), signature(&plugin).unwrap());
        assert_eq!(
            plugins_stables(&precedent, std::slice::from_ref(&plugin)),
            vec![plugin]
        );
    }

    #[test]
    fn plugin_nouveau_sans_releve_precedent_attend() {
        let d = tempfile::tempdir().unwrap();
        let plugin = plugin_de(d.path(), 20);
        assert!(plugins_stables(&BTreeMap::new(), &[plugin]).is_empty());
    }

    fn processus(nom: &str) -> ProcessusWine {
        ProcessusWine {
            pid: 1,
            arguments: vec![format!(r"C:\X\{nom}")],
        }
    }

    #[test]
    fn installeurs_en_cours_exclut_le_gestionnaire() {
        let liste = [
            processus("PA-InstallationManager.exe"),
            processus("Setup PC.exe"),
            processus("msiexec.exe"),
        ];
        let exclus = vec!["PA-InstallationManager.exe".to_string()];
        assert_eq!(
            noms_d_installeurs(&liste, &exclus),
            vec!["Setup PC.exe", "msiexec.exe"]
        );
    }

    #[test]
    fn le_gestionnaire_est_cite_par_un_argument_au_nom_exact() {
        let noms = vec!["Native Access.exe".to_string()];
        let enfant = ProcessusWine {
            pid: 2,
            arguments: vec![r"C:\NI\native access.EXE".into(), "--type=renderer".into()],
        };
        assert!(cite_un_programme(&enfant, &noms));
        assert!(!cite_un_programme(
            &processus("Native Access.exe.bak"),
            &noms
        ));
        assert!(!cite_un_programme(&processus("Autre.exe"), &noms));
    }

    #[test]
    fn la_fermeture_ne_vise_ni_wine_ni_yabridge() {
        let noms = vec!["services.exe".to_string(), "Native Access.exe".to_string()];
        let hote = ProcessusWine {
            pid: 3,
            arguments: vec!["yabridge-host.exe".into(), "Z:/x/Native Access.exe".into()],
        };
        // Cités par le nom, mais intouchables : jamais retenus par `fermer`.
        assert!(cite_un_programme(&hote, &noms) && est_intouchable(&hote));
        assert!(!est_processus_du_gestionnaire(&hote, &noms));
        let systeme = processus("services.exe");
        assert!(cite_un_programme(&systeme, &noms) && est_intouchable(&systeme));
        assert!(!est_processus_du_gestionnaire(&systeme, &noms));
        assert!(!est_intouchable(&processus("Native Access.exe")));
        // Fils Electron : l'exécutable n'est pas le premier argument, il compte quand même
        // (pour l'arrêt comme pour « fermé ? »).
        let fils = ProcessusWine {
            pid: 4,
            arguments: vec!["--type=gpu".into(), r"C:\NI\Native Access.exe".into()],
        };
        assert!(est_processus_du_gestionnaire(&fils, &noms));
    }

    #[test]
    fn le_pilote_ne_voit_pas_le_gestionnaire_qui_cite_l_installeur() {
        // Le gestionnaire a reçu le chemin de l'installeur en argument (le faux gestionnaire
        // du test d'intégration, ou un vrai qui lance un fichier qu'il a téléchargé) : le
        // pilote ne doit voir que l'installeur, sinon il fermerait le gestionnaire avec lui.
        let installeur = r"C:\users\u\Downloads\FauxEditeur\test-nsis.exe";
        let gestionnaire = ProcessusWine {
            pid: 10,
            arguments: vec![
                "/tmp/p/drive_c/Program Files/FauxEditeur/FauxGestionnaire.exe".into(),
                installeur.into(),
            ],
        };
        let lance = ProcessusWine {
            pid: 11,
            arguments: vec![installeur.into()],
        };
        let noms = vec!["FauxGestionnaire.exe".to_string()];
        let vus = processus_pour_le_pilote(vec![gestionnaire, lance.clone()], &noms);
        assert_eq!(vus, [lance]);
    }

    #[test]
    fn le_pilote_voit_les_installeurs_et_pas_les_fils_du_gestionnaire() {
        let noms = vec!["Native Access.exe".to_string()];
        // Nom voisin : un installeur à piloter, il reste visible.
        let installeur = ProcessusWine {
            pid: 1,
            arguments: vec![r"C:\users\u\Downloads\NI\Native Access Installer.exe".into()],
        };
        // Casse différente : c'est bien le gestionnaire.
        let gestionnaire = ProcessusWine {
            pid: 2,
            arguments: vec![r"C:\Program Files\NI\NATIVE ACCESS.EXE".into()],
        };
        // Fils Electron : le chemin de l'exe figure en argument, après d'autres options.
        let fils = ProcessusWine {
            pid: 3,
            arguments: vec![
                "--type=renderer".into(),
                r"C:\Program Files\NI\Native Access.exe".into(),
            ],
        };
        let vus = processus_pour_le_pilote(vec![installeur.clone(), gestionnaire, fils], &noms);
        assert_eq!(vus, [installeur]);
    }

    #[test]
    fn le_programme_est_cache_meme_si_fermer_ne_le_cite_pas() {
        let g = Gestionnaire {
            programme: r"C:\Program Files\Ed\Centre.exe".into(),
            arguments: Vec::new(),
            pilote: Vec::new(),
            fermer: vec!["Aide.exe".into(), "aide.EXE".into(), "centre.exe".into()],
        };
        // Doublons retirés sans tenir compte de la casse ; programme déjà cité : pas rajouté.
        assert_eq!(noms_caches_au_pilote(&g), ["Aide.exe", "centre.exe"]);
        let sans_programme = Gestionnaire {
            fermer: vec!["Aide.exe".into()],
            ..g
        };
        let noms = noms_caches_au_pilote(&sans_programme);
        assert_eq!(noms, ["Aide.exe", "Centre.exe"]);
        let centre = ProcessusWine {
            pid: 4,
            arguments: vec![r"C:\Program Files\Ed\Centre.exe".into()],
        };
        assert!(processus_pour_le_pilote(vec![centre], &noms).is_empty());
    }

    #[test]
    fn le_journal_du_gestionnaire_est_explique_ligne_par_ligne() {
        let texte = "démarrage\n\
            [main] ERROR_CODE_DOWNLOAD_DIR_MISSING\n\
            autre ligne\n\
            Error: open EBADF\n\
            ERROR_CODE_DOWNLOAD_DIR_MISSING encore\n";
        let messages = expliquer_journal(texte);
        assert_eq!(messages.len(), 2, "{messages:?}");
        assert!(messages[0].contains("download folder"), "{messages:?}");
        assert!(messages[1].contains("sign-in link"), "{messages:?}");
        assert!(expliquer_journal("tout va bien\n").is_empty());
    }

    #[test]
    fn seule_la_partie_ecrite_pendant_la_session_est_lue() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("na.log");
        std::fs::write(&f, "ERROR_CODE_DOWNLOAD_DIR_MISSING (session précédente)\n").unwrap();
        let debut = std::fs::metadata(&f).unwrap().len();
        let mut ajout = std::fs::OpenOptions::new().append(true).open(&f).unwrap();
        std::io::Write::write_all(&mut ajout, b"nouvelle ligne\n").unwrap();
        assert_eq!(lire_depuis(&f, debut), "nouvelle ligne\n");
        // Journal remplacé (plus court que le début noté) : relu depuis le début.
        std::fs::write(&f, "court\n").unwrap();
        assert_eq!(lire_depuis(&f, debut), "court\n");
        assert_eq!(lire_depuis(&d.path().join("absent.log"), 0), "");
    }

    #[test]
    fn le_nom_du_journal_ne_peut_pas_sortir_du_dossier() {
        assert_eq!(nom_du_journal("Native Access"), "native-access.log");
        assert_eq!(nom_du_journal("../Évil/x"), "---évil-x.log");
    }

    #[test]
    fn ouvrir_un_gestionnaire_absent_est_introuvable() {
        let (racine, c) = banc();
        let r = recette_faux(racine.path(), "");
        match ouvrir(&c, &r, &ProgressionMuette) {
            Err(Erreur::Introuvable(q)) => assert!(q.contains(r"C:\Faux\Faux.exe"), "{q}"),
            autre => panic!("{:?}", autre.map(|_| ())),
        }
        // Le verrou se trouve dans le dossier des données (`Chemins::pour_test` le place sous
        // `racine/donnees`) ; l'erreur survient avant sa pose, il ne doit donc pas exister.
        assert!(!c.donnees.join("installation.lock").exists());
    }

    #[test]
    fn un_service_en_echec_libere_le_verrou() {
        let (racine, c) = banc();
        ecrire_executable(&c.drive_c().join("Faux/Faux.exe"), "MZ");
        // Faux wine dédié : `net start` échoue sans « already ».
        ecrire_executable(
            &c.wine_dir.join("bin/wine"),
            "#!/bin/sh\necho boum\nexit 2\n",
        );
        let r = recette_faux(racine.path(), "[correctifs]\nservices = [\"Svc\"]\n");
        match ouvrir(&c, &r, &ProgressionMuette) {
            Err(Erreur::Commande { code, .. }) => assert_eq!(code, Some(2)),
            autre => panic!("{:?}", autre.map(|_| ())),
        }
        assert!(
            !c.donnees.join("installation.lock").exists(),
            "verrou libéré"
        );
    }

    #[test]
    fn relayer_un_schema_inconnu_est_introuvable() {
        let (_racine, c) = banc();
        let r = relayer_lien(&c, &Catalogue(vec![]), "inconnu://x");
        assert!(matches!(r, Err(Erreur::Introuvable(_))));
        let r = relayer_lien(&c, &Catalogue(vec![]), "pas-un-lien");
        assert!(matches!(r, Err(Erreur::Introuvable(_))));
    }

    #[test]
    fn relayer_un_lien_lance_le_programme_avec_l_adresse() {
        let (racine, c) = banc();
        ecrire_executable(&c.drive_c().join("Faux/Faux.exe"), "MZ");
        let catalogue = Catalogue(vec![recette_faux(racine.path(), "")]);
        relayer_lien(&c, &catalogue, "Faux-Lien://connexion?jeton=1").unwrap();
        for _ in 0..50 {
            if appels(&c).contains("jeton") {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let appels = appels(&c);
        assert!(
            appels.contains("Faux.exe --a Faux-Lien://connexion?jeton=1"),
            "{appels}"
        );
        assert!(c.donnees.join("journaux/liens.log").exists());
    }

    #[test]
    fn le_gestionnaire_est_lance_dans_son_propre_groupe_de_processus() {
        let (racine, c) = banc();
        ecrire_executable(&c.drive_c().join("Faux/Faux.exe"), "MZ");
        // Faux wine qui note son groupe de processus (champ 5 de /proc/self/stat).
        let temoin = c.wine_dir.join("bin/groupe");
        ecrire_executable(
            &c.wine_dir.join("bin/wine"),
            &format!(
                "#!/bin/sh\ncut -d' ' -f5 /proc/$$/stat > {}\n",
                temoin.display()
            ),
        );
        let r = recette_faux(racine.path(), "");
        let journal = c.donnees.join("journaux/essai.log");
        let mut enfant =
            lancer_gestionnaire(&c, gestionnaire_de(&r).unwrap(), &[], &journal).unwrap();
        let pid = enfant.id();
        enfant.wait().unwrap();
        let groupe = std::fs::read_to_string(&temoin).unwrap();
        // Chef de son propre groupe : le Ctrl+C du terminal (groupe de WiMao) ne l'atteint pas.
        assert_eq!(groupe.trim(), pid.to_string());
    }

    #[test]
    fn session_complete_avec_faux_wine() {
        let (racine, c) = banc();
        ecrire_executable(&c.drive_c().join("Faux/Faux.exe"), "MZ");
        let r = recette_faux(racine.path(), "[correctifs]\nservices = [\"Svc\"]\n");
        let mut session = ouvrir(&c, &r, &ProgressionMuette).unwrap();
        assert!(c.donnees.join("installation.lock").exists(), "verrou posé");
        assert!(c.donnees.join("journaux/faux-gestionnaire.log").exists());
        while !session.gestionnaire_ferme().unwrap() {
            std::thread::sleep(Duration::from_millis(50));
        }
        let appels = appels(&c);
        assert!(appels.contains("net start Svc"), "{appels}");
        assert!(appels.contains("Faux.exe --a"), "{appels}");

        // Un plugin apparaît : le premier relevé l'attend, le second le ponte.
        let dossier = c.drive_c().join("Program Files/Common Files/VST3");
        std::fs::create_dir_all(&dossier).unwrap();
        plugin_de(&dossier, 20);
        let debut = Instant::now();
        let t1 = session
            .tick(debut + INTERVALLE_RELEVE, &ProgressionMuette)
            .unwrap();
        assert!(t1.is_empty(), "{t1:?}");
        let t2 = session
            .tick(debut + INTERVALLE_RELEVE * 2, &ProgressionMuette)
            .unwrap();
        assert!(matches!(t2.as_slice(), [EvenementSession::Ponte(p)] if p.nom == "Essai"));

        // Le gestionnaire a écrit une erreur connue dans son journal pendant la session.
        let journal = c.donnees.join("journaux/faux-gestionnaire.log");
        let mut ajout = OpenOptions::new().append(true).open(&journal).unwrap();
        std::io::Write::write_all(&mut ajout, b"ERROR_CODE_DOWNLOAD_DIR_MISSING\n").unwrap();

        let bilan = session.fermer(false, &ProgressionMuette).unwrap();
        assert_eq!(bilan.plugins.len(), 1);
        assert!(bilan.mis_a_jour.is_empty());
        assert_eq!(bilan.avertissements.len(), 1, "{:?}", bilan.avertissements);
        assert!(
            !c.donnees.join("installation.lock").exists(),
            "verrou libéré"
        );
        // Une entrée de session, avec l'id du point créé à l'ouverture (le seul de la file).
        let entrees = historique::lire(&c).unwrap();
        assert_eq!(entrees.len(), 1, "{entrees:?}");
        assert_eq!(entrees[0].action, Action::Gestionnaire);
        assert_eq!(entrees[0].resultat, ResultatAction::Reussi);
        assert_eq!(entrees[0].sujet, r.nom);
        let points = restauration::lister(&c).unwrap().points;
        assert_eq!(entrees[0].point.as_deref(), Some(points[0].id.as_str()));
    }

    fn noms(liste: &[&str]) -> Vec<String> {
        liste.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn la_session_finit_seule_quand_le_gestionnaire_est_ferme_sans_installeur() {
        let s = Phase::Surveillance;
        assert_eq!(suite(s, None, false, Some(&[])), Suite::Continuer);
        assert_eq!(suite(s, None, true, Some(&[])), Suite::Fermer);
        // Fermé, mais un téléchargement continue (daemon de Native Access) : on attend.
        assert_eq!(
            suite(s, None, true, Some(&noms(&["Setup.exe"]))),
            Suite::Continuer
        );
    }

    #[test]
    fn je_termine_attend_les_installeurs_puis_ferme() {
        let installeurs = noms(&["Setup.exe"]);
        let s = Phase::Surveillance;
        assert_eq!(
            suite(s, Some(Commande::Terminer), false, Some(&[])),
            Suite::Fermer
        );
        assert_eq!(
            suite(s, Some(Commande::Terminer), false, Some(&installeurs)),
            Suite::Attendre
        );
        let a = Phase::Attente;
        assert_eq!(suite(a, None, false, Some(&installeurs)), Suite::Continuer);
        assert_eq!(suite(a, None, false, Some(&[])), Suite::Fermer);
        assert_eq!(
            suite(
                a,
                Some(Commande::ArreterQuandMeme),
                false,
                Some(&installeurs)
            ),
            Suite::Fermer
        );
    }

    #[test]
    fn l_abandon_l_emporte_sur_tout() {
        for phase in [Phase::Surveillance, Phase::Attente] {
            assert_eq!(
                suite(phase, Some(Commande::Abandonner), true, Some(&[])),
                Suite::Abandonner
            );
        }
    }
}
