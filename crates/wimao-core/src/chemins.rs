//! Emplacements sur disque utilisés par WiMao.
//!
//! Centralise toutes les décisions « où est quoi » (données, environnement Wine,
//! Wine lui-même, yabridge) pour qu'aucun autre module ne lise `HOME` ou ne
//! code un chemin en dur. Utilisé par toutes les crates. Voir `ARCHITECTURE.md`
//! (section « Emplacements sur disque »).
//!
//! Ne touche jamais à `~/.wine-mao` ni à `~/.config/yabridgectl` : ce sont les
//! installations de l'utilisateur, WiMao a les siennes sous `<donnees>`.

use crate::erreur::{Erreur, Resultat};
use crate::i18n::{tr, trf};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Variable d'environnement qui remplace le dossier de données.
const VAR_DONNEES: &str = "WIMAO_DONNEES";
/// Variable d'environnement qui désigne le dossier de Wine.
const VAR_WINE_DIR: &str = "WIMAO_WINE_DIR";
/// Variable d'environnement qui désigne le dossier de yabridge.
const VAR_YABRIDGE_DIR: &str = "WIMAO_YABRIDGE_DIR";

/// Wine installé par le paquet WiMao.
const WINE_PAQUET: &str = "/opt/wimao/wine";
/// Wine de développement (voir `NOTES.md`), utilisé tant que le paquet n'existe pas.
const WINE_DEVELOPPEMENT: &str = "/opt/wine-mao-v2";
/// yabridge installé par le paquet WiMao.
const YABRIDGE_PAQUET: &str = "/opt/wimao/yabridge";
/// yabridge de développement, livré avec le Wine de développement.
const YABRIDGE_DEVELOPPEMENT: &str = "/opt/wine-mao-v2/yabridge";

/// Tous les emplacements dont WiMao a besoin, calculés une fois au démarrage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chemins {
    /// Données de WiMao (`~/.local/share/wimao` par défaut).
    pub donnees: PathBuf,
    /// Environnement Wine (« préfixe ») : `<donnees>/prefixe`.
    pub prefixe: PathBuf,
    /// Réglages de l'utilisateur (`~/.config/wimao`).
    pub config: PathBuf,
    /// Cache, par exemple les téléchargements (`~/.cache/wimao`).
    pub cache: PathBuf,
    /// Dossier personnel de l'utilisateur.
    pub home: PathBuf,
    /// Dossier d'installation de Wine (contient `bin/`).
    pub wine_dir: PathBuf,
    /// Dossier d'installation de yabridge.
    pub yabridge_dir: PathBuf,
    /// Vrai quand tout ce que WiMao écrit est isolé dans un dossier temporaire
    /// (tests, voir [`Chemins::pour_test`]) ; faux en usage réel.
    ///
    /// En isolation, les commandes Wine reçoivent aussi `XDG_DATA_HOME`,
    /// `XDG_CONFIG_HOME` et `XDG_CACHE_HOME` sous [`Chemins::home`] et
    /// `winemenubuilder.exe=d` : sans cela, Wine crée des entrées de menu et des
    /// icônes dans le vrai `~/.local/share` de la personne qui lance les tests
    /// (incident réel, voir `NOTES.md`). En usage réel on garde `winemenubuilder` :
    /// les gestionnaires d'éditeurs (Native Access, Arturia Software Center…)
    /// doivent apparaître dans le menu du bureau. Voir [`crate::wine::preparer_environnement`].
    pub isole: bool,
}

impl Chemins {
    /// Calcule les chemins depuis l'environnement réel du processus.
    ///
    /// Erreurs : [`Erreur::Introuvable`] si `HOME` est absent ou vide, même quand
    /// `WIMAO_DONNEES` est défini : les réglages et le cache en dépendent ;
    /// [`Erreur::NonPrisEnCharge`] si `HOME` ou `WIMAO_DONNEES` est un chemin relatif,
    /// qui ferait écrire WiMao dans le dossier courant (différent à chaque lancement).
    pub fn depuis_env() -> Resultat<Chemins> {
        Chemins::depuis(|nom| std::env::var_os(nom), |p| p.exists())
    }

    /// Cœur de [`Chemins::depuis_env`], avec l'environnement et le test d'existence
    /// injectés : les tests n'ont ainsi pas à modifier l'environnement global
    /// (dangereux quand les tests tournent en parallèle).
    pub(crate) fn depuis(
        env: impl Fn(&str) -> Option<OsString>,
        existe: impl Fn(&Path) -> bool,
    ) -> Resultat<Chemins> {
        let home = env("HOME")
            .filter(|h| !h.is_empty())
            .map(PathBuf::from)
            .ok_or_else(pas_de_home)?;
        exiger_absolu("HOME", &home)?;
        let donnees = match env(VAR_DONNEES) {
            Some(d) => PathBuf::from(d),
            None => home.join(".local/share/wimao"),
        };
        exiger_absolu(VAR_DONNEES, &donnees)?;
        Ok(Chemins {
            prefixe: donnees.join("prefixe"),
            config: home.join(".config/wimao"),
            cache: home.join(".cache/wimao"),
            wine_dir: dossier_outil(&env, &existe, VAR_WINE_DIR, WINE_PAQUET, WINE_DEVELOPPEMENT),
            yabridge_dir: dossier_outil(
                &env,
                &existe,
                VAR_YABRIDGE_DIR,
                YABRIDGE_PAQUET,
                YABRIDGE_DEVELOPPEMENT,
            ),
            donnees,
            home,
            isole: false,
        })
    }

    /// Chemins pour les tests : tout ce que WiMao écrit est sous `racine`, pour ne
    /// jamais toucher aux vraies données. Wine et yabridge (en lecture seule) sont
    /// résolus normalement. [`Chemins::isole`] est vrai : Wine n'écrit alors ni
    /// menus ni icônes hors de `racine`.
    pub fn pour_test(racine: &Path) -> Chemins {
        let donnees = racine.join("donnees");
        let reel = Chemins::depuis(|nom| std::env::var_os(nom), |p| p.exists());
        let (wine_dir, yabridge_dir) = match reel {
            Ok(c) => (c.wine_dir, c.yabridge_dir),
            Err(_) => (
                PathBuf::from(WINE_DEVELOPPEMENT),
                PathBuf::from(YABRIDGE_DEVELOPPEMENT),
            ),
        };
        Chemins {
            prefixe: donnees.join("prefixe"),
            config: racine.join("config"),
            cache: racine.join("cache"),
            home: racine.join("home"),
            donnees,
            wine_dir,
            yabridge_dir,
            isole: true,
        }
    }

    /// Dossier de données XDG (`XDG_DATA_HOME`) : celui où le bureau cherche les
    /// `.desktop` (`<ce dossier>/applications`).
    ///
    /// En isolation ([`Chemins::isole`]) c'est toujours `<home>/.local/share` : c'est
    /// aussi la valeur que [`crate::wine::preparer_environnement`] pose dans
    /// `XDG_DATA_HOME`, donc ce que les commandes lancées par WiMao verront. Sinon,
    /// la spécification XDG : la variable `XDG_DATA_HOME` si elle est définie et
    /// absolue (une valeur vide ou relative doit être ignorée), sinon
    /// `<home>/.local/share`.
    pub fn xdg_data_home(&self) -> PathBuf {
        self.xdg_data_home_depuis(|nom| std::env::var_os(nom))
    }

    /// Cœur de [`Chemins::xdg_data_home`], avec l'environnement injecté (les tests ne
    /// modifient pas l'environnement global).
    pub(crate) fn xdg_data_home_depuis(&self, env: impl Fn(&str) -> Option<OsString>) -> PathBuf {
        let par_defaut = self.home.join(".local/share");
        if self.isole {
            return par_defaut;
        }
        env("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or(par_defaut)
    }

    /// Dossier des exécutables de Wine (`<wine_dir>/bin`).
    pub fn wine_bin(&self) -> PathBuf {
        self.wine_dir.join("bin")
    }

    /// Disque `C:` de l'environnement Wine (`<prefixe>/drive_c`).
    pub fn drive_c(&self) -> PathBuf {
        self.prefixe.join("drive_c")
    }
}

/// Erreur renvoyée quand on ne sait pas où ranger les données.
fn pas_de_home() -> Erreur {
    Erreur::Introuvable(tr(
        "home folder: HOME must be set and not empty (WiMao keeps its settings and cache there)",
    ))
}

/// Refuse un chemin relatif venu de la variable `variable` (message traduit).
fn exiger_absolu(variable: &str, chemin: &Path) -> Resultat<()> {
    if chemin.is_absolute() {
        return Ok(());
    }
    Err(Erreur::NonPrisEnCharge(trf(
        "{variable} must be an absolute path (it is \"{path}\")",
        &[
            ("variable", variable),
            ("path", &chemin.display().to_string()),
        ],
    )))
}

/// Choisit le dossier d'un outil : variable d'environnement, sinon emplacement du
/// paquet s'il existe, sinon emplacement de développement.
fn dossier_outil(
    env: &impl Fn(&str) -> Option<OsString>,
    existe: &impl Fn(&Path) -> bool,
    variable: &str,
    paquet: &str,
    developpement: &str,
) -> PathBuf {
    if let Some(v) = env(variable) {
        return PathBuf::from(v);
    }
    if existe(Path::new(paquet)) {
        PathBuf::from(paquet)
    } else {
        PathBuf::from(developpement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    #[test]
    fn xdg_data_home_suit_la_variable_absolue_hors_isolation() {
        let mut c = Chemins::pour_test(Path::new("/tmp/essai"));
        c.isole = false;
        c.home = PathBuf::from("/home/x");
        let avec = |v: &str| c.xdg_data_home_depuis(env_de(&[("XDG_DATA_HOME", v)]));
        assert_eq!(avec("/srv/donnees"), PathBuf::from("/srv/donnees"));
        // Vide ou relative : ignorée (spécification XDG).
        assert_eq!(avec(""), PathBuf::from("/home/x/.local/share"));
        assert_eq!(avec("relatif"), PathBuf::from("/home/x/.local/share"));
        assert_eq!(
            c.xdg_data_home_depuis(env_de(&[])),
            PathBuf::from("/home/x/.local/share")
        );
    }

    #[test]
    fn xdg_data_home_ignore_la_variable_en_isolation() {
        let c = Chemins::pour_test(Path::new("/tmp/essai"));
        assert_eq!(
            c.xdg_data_home_depuis(env_de(&[("XDG_DATA_HOME", "/reel")])),
            c.home.join(".local/share")
        );
    }

    /// Environnement simulé : évite de modifier l'environnement réel (tests en parallèle).
    fn env_de(paires: &[(&str, &str)]) -> impl Fn(&str) -> Option<std::ffi::OsString> {
        let table: HashMap<String, String> = paires
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |nom| table.get(nom).map(Into::into)
    }

    #[test]
    fn donnees_surchargees_donnent_le_prefixe() {
        let env = env_de(&[("HOME", "/h"), ("WIMAO_DONNEES", "/x")]);
        let c = Chemins::depuis(env, |_| false).unwrap();
        assert_eq!(c.prefixe, PathBuf::from("/x/prefixe"));
        assert_eq!(c.drive_c(), PathBuf::from("/x/prefixe/drive_c"));
    }

    #[test]
    fn emplacements_par_defaut_sous_home() {
        let c = Chemins::depuis(env_de(&[("HOME", "/h")]), |_| false).unwrap();
        assert_eq!(c.donnees, PathBuf::from("/h/.local/share/wimao"));
        assert_eq!(c.config, PathBuf::from("/h/.config/wimao"));
        assert_eq!(c.cache, PathBuf::from("/h/.cache/wimao"));
    }

    #[test]
    fn wine_dir_surcharge() {
        let env = env_de(&[("HOME", "/h"), ("WIMAO_WINE_DIR", "/w")]);
        let c = Chemins::depuis(env, |_| true).unwrap();
        assert_eq!(c.wine_bin(), PathBuf::from("/w/bin"));
    }

    #[test]
    fn wine_dir_par_defaut_sans_opt_wimao() {
        let c = Chemins::depuis(env_de(&[("HOME", "/h")]), |_| false).unwrap();
        assert_eq!(c.wine_dir, PathBuf::from("/opt/wine-mao-v2"));
        assert_eq!(c.yabridge_dir, PathBuf::from("/opt/wine-mao-v2/yabridge"));
    }

    #[test]
    fn opt_wimao_prioritaire_quand_present() {
        let existe = |p: &Path| p.starts_with("/opt/wimao");
        let c = Chemins::depuis(env_de(&[("HOME", "/h")]), existe).unwrap();
        assert_eq!(c.wine_dir, PathBuf::from("/opt/wimao/wine"));
        assert_eq!(c.yabridge_dir, PathBuf::from("/opt/wimao/yabridge"));
    }

    #[test]
    fn home_absent_ou_vide_est_une_erreur_meme_avec_donnees() {
        let sans = env_de(&[("WIMAO_DONNEES", "/x")]);
        assert!(Chemins::depuis(sans, |_| false).is_err());
        let vide = env_de(&[("HOME", ""), ("WIMAO_DONNEES", "/x")]);
        assert!(Chemins::depuis(vide, |_| false).is_err());
    }

    #[test]
    fn home_ou_donnees_relatifs_sont_refuses() {
        let relatif = env_de(&[("HOME", "maison")]);
        assert!(matches!(
            Chemins::depuis(relatif, |_| false),
            Err(Erreur::NonPrisEnCharge(_))
        ));
        let donnees = env_de(&[("HOME", "/h"), ("WIMAO_DONNEES", "donnees")]);
        match Chemins::depuis(donnees, |_| false) {
            Err(Erreur::NonPrisEnCharge(texte)) => {
                assert!(texte.contains("WIMAO_DONNEES"), "{texte}")
            }
            autre => panic!("attendu NonPrisEnCharge, obtenu {autre:?}"),
        }
    }

    #[test]
    fn sans_home_ni_donnees_c_est_une_erreur() {
        assert!(Chemins::depuis(env_de(&[]), |_| false).is_err());
    }

    #[test]
    fn pour_test_met_tout_sous_la_racine() {
        let c = Chemins::pour_test(Path::new("/tmp/t"));
        for p in [&c.donnees, &c.prefixe, &c.config, &c.cache, &c.home] {
            assert!(p.starts_with("/tmp/t"), "{p:?}");
        }
        assert!(c.isole);
    }

    #[test]
    fn l_usage_reel_n_est_pas_isole() {
        let c = Chemins::depuis(env_de(&[("HOME", "/h")]), |_| false).unwrap();
        assert!(!c.isole);
    }
}
