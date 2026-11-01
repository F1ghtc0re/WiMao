//! Commandes Wine : construction de l'environnement, registre, arrêt du serveur.
//!
//! Utilise [`Chemins`] (où est Wine, où est l'environnement) et [`lancer`]
//! (délai maximal, groupe de processus tué en entier). Utilisé par les modules
//! d'initialisation, d'installation, de pontage et de vérification : aucun autre
//! fichier ne doit construire de commande `wine` à la main, pour que les variables
//! d'environnement soient toujours identiques.

use crate::chemins::Chemins;
use crate::erreur::{Erreur, Resultat};
use crate::i18n::trf;
use crate::outils::{lancer, Sortie};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Délai d'une commande de registre. Le premier appel peut démarrer le
/// `wineserver` (plusieurs secondes sur une machine lente).
const DELAI_REGISTRE: Duration = Duration::from_secs(60);
/// Délai de `wineserver -k` et de `wineserver -w` : arrêter un serveur chargé
/// de processus lourds peut prendre un moment, mais jamais minutes.
const DELAI_ARRET: Duration = Duration::from_secs(60);
/// Code de sortie de `wine reg query` quand la clé ou la valeur n'existe pas.
/// Les messages de `reg` sont traduits selon la langue du système : on ne peut
/// pas s'appuyer sur leur texte, donc seul ce code compte (voir [`Wine::reg_lire`]).
const CODE_REGISTRE_ABSENT: i32 = 1;
/// Séparateur de colonnes de la sortie de `reg query` (quatre espaces).
const SEPARATEUR_REG: &str = "    ";
/// Empêche Wine de proposer l'installation de Mono et de Gecko : leurs fenêtres
/// bloqueraient `wineboot` et `yabridgectl sync` (NOTES.md : « wineboot -u avec
/// WINEDLLOVERRIDES=… »). Mono est installé ensuite par son correctif, Gecko ne sert
/// à aucun plugin. Seule définition de cette valeur dans le code : passer par
/// [`surcharges_dll`] ou [`preparer_environnement`].
pub const SURCHARGES_SANS_FENETRES: &str = "mscoree,mshtml=";
/// Désactive le débogueur de Wine pour les installeurs. Sans lui, un programme qui plante
/// s'arrête aussitôt avec un code d'erreur ; avec lui, Wine ouvre la fenêtre « Le programme a
/// rencontré un problème sérieux », qui attend un clic : une installation silencieuse restait
/// alors bloquée jusqu'au délai maximal (30 min). WiMao affiche de toute façon sa propre erreur
/// (vu à la vérification réelle : `msiexec` de Plugin Alliance, NOTES.md).
pub const SURCHARGE_SANS_DEBOGUEUR: &str = "winedbg.exe=d";
/// Désactive `winemenubuilder`, qui crée entrées de menu, icônes et associations de
/// fichiers dans `XDG_DATA_HOME` / `XDG_CONFIG_HOME`. Posée **seulement** en isolation
/// ([`Chemins::isole`]) : en usage réel les gestionnaires d'éditeurs doivent
/// apparaître dans le menu du bureau (Ruling 15, incident décrit dans NOTES.md).
const SURCHARGE_SANS_MENUS: &str = "winemenubuilder.exe=d";
/// Séparateur des surcharges dans `WINEDLLOVERRIDES` (documentation de Wine).
const SEPARATEUR_SURCHARGES: &str = ";";

/// Accès à Wine pour l'environnement décrit par un [`Chemins`].
///
/// Ne possède rien : emprunte les chemins, donc très bon marché à créer.
#[derive(Debug, Clone, Copy)]
pub struct Wine<'a> {
    /// Emplacements (Wine, environnement, `HOME`) utilisés pour toutes les commandes.
    pub chemins: &'a Chemins,
}

impl<'a> Wine<'a> {
    /// Crée un accès à Wine pour ces chemins.
    pub fn nouveau(chemins: &'a Chemins) -> Self {
        Wine { chemins }
    }

    /// Prépare la commande `programme` (`wine`, `wineserver`, `wineboot`…) de notre Wine.
    ///
    /// Variables posées : `WINEPREFIX` (notre environnement, jamais `~/.wine`),
    /// `WINEDEBUG=-all` (sans cela Wine noie la sortie de traces et ralentit),
    /// `PATH` avec le dossier de Wine en tête (les scripts de Wine appellent
    /// `wineserver` par son nom), puis celles de [`preparer_environnement`]
    /// (`HOME`, et en isolation `XDG_*` et `WINEDLLOVERRIDES`). L'appelant ajoute
    /// arguments et variables supplémentaires, puis passe par [`lancer`].
    /// Pour des surcharges de DLL, utiliser [`Wine::commande_avec`] : poser soi-même
    /// `WINEDLLOVERRIDES` effacerait celles de l'isolation.
    ///
    /// Erreur : [`Erreur::Introuvable`] si `<wine_dir>/bin/<programme>` n'existe pas.
    /// Il n'y a volontairement **aucun repli** sur le `wine` du `PATH` : un autre Wine
    /// (celui de la distribution) n'a pas les correctifs de WiMao (NOTES.md) et
    /// donnerait des pannes incompréhensibles dans l'environnement.
    pub fn commande(&self, programme: &str) -> Resultat<Command> {
        self.commande_avec(programme, &[])
    }

    /// Comme [`Wine::commande`], avec des surcharges de DLL (`WINEDLLOVERRIDES`)
    /// fusionnées à celles de l'isolation, par exemple
    /// `&[SURCHARGES_SANS_FENETRES]` pour `wineboot`. Erreurs : comme [`Wine::commande`].
    pub fn commande_avec(&self, programme: &str, surcharges: &[&str]) -> Resultat<Command> {
        let bin = self.chemins.wine_bin();
        // Toujours le chemin complet : ne dépend pas du PATH du parent.
        let complet = bin.join(programme);
        if !complet.exists() {
            return Err(wine_introuvable(&complet));
        }
        let mut cmd = Command::new(complet);
        cmd.env("WINEPREFIX", &self.chemins.prefixe)
            .env("WINEDEBUG", "-all")
            .env("PATH", chemin_avec_wine_en_tete(bin));
        preparer_environnement(&mut cmd, self.chemins, surcharges);
        Ok(cmd)
    }

    /// Lance `wine <args>` et attend au plus `delai`.
    ///
    /// Un code de sortie non nul n'est pas une erreur ici (voir [`lancer`]) :
    /// l'appelant lit [`Sortie::code`]. Erreurs : [`Erreur::Introuvable`] (Wine
    /// absent), [`Erreur::Delai`], [`Erreur::Io`].
    pub fn executer(&self, args: &[&OsStr], delai: Duration) -> Resultat<Sortie> {
        lancer(self.commande("wine")?.args(args), delai)
    }

    /// Écrit une valeur texte (`REG_SZ`) dans le registre de l'environnement.
    ///
    /// `cle` s'écrit comme pour `reg` (`HKCU\Software\Wine\DllOverrides`) ; `/f`
    /// évite la question de confirmation si la valeur existe déjà (la commande
    /// bloquerait sinon). Erreurs : [`Erreur::Commande`] si `reg` échoue.
    pub fn reg_ajouter(&self, cle: &str, valeur: &str, donnee: &str) -> Resultat<()> {
        let args = [
            "reg", "add", cle, "/v", valeur, "/t", "REG_SZ", "/d", donnee, "/f",
        ];
        self.reg_verifie(&args)
    }

    /// Lit une valeur du registre ; `None` si la clé ou la valeur n'existe pas.
    ///
    /// `wine reg query` rend le code 1 dans les deux cas « absent », mais aussi
    /// pour d'autres échecs. Comme le texte des messages dépend de la langue du
    /// système, on ne peut pas les distinguer : tout code 1 donne `None`. C'est
    /// sans danger ici car les arguments sont construits par WiMao (pas de
    /// syntaxe invalide) ; tout autre code non nul est une [`Erreur::Commande`].
    ///
    /// Renvoie la donnée telle qu'affichée par `reg` (pour un `REG_DWORD`, par
    /// exemple `0x1`). Limite : `reg` écrit dans la page de code de la console,
    /// pas en UTF-8 ; une donnée non ASCII revient donc altérée (constaté avec
    /// Wine v2). WiMao ne lit que des valeurs ASCII (chemins de DLL, drapeaux).
    pub fn reg_lire(&self, cle: &str, valeur: &str) -> Resultat<Option<String>> {
        let args = ["reg", "query", cle, "/v", valeur];
        let sortie = self.reg(&args)?;
        match sortie.code {
            0 => Ok(extraire_valeur(&sortie.texte, valeur)),
            CODE_REGISTRE_ABSENT => Ok(None),
            _ => Err(erreur_commande("wine", &args, sortie)),
        }
    }

    /// Supprime une valeur du registre. Erreurs : [`Erreur::Commande`] si elle
    /// n'existe pas ou si `reg` échoue.
    pub fn reg_supprimer(&self, cle: &str, valeur: &str) -> Resultat<()> {
        self.reg_verifie(&["reg", "delete", cle, "/v", valeur, "/f"])
    }

    /// Arrête le `wineserver` de l'environnement et attend qu'il ait fini.
    ///
    /// `wineserver -k` demande l'arrêt ; `wineserver -w` attend la fin de tous les
    /// processus Wine (sans cela, la commande suivante pourrait trouver un
    /// serveur à moitié arrêté). Aucun serveur en marche n'est pas une erreur :
    /// `-k` rend alors un code non nul, ignoré. Erreurs : [`Erreur::Delai`],
    /// [`Erreur::Introuvable`] (`wineserver` absent), [`Erreur::Io`], ou
    /// [`Erreur::Commande`] si l'attente `-w` échoue.
    pub fn arreter(&self) -> Resultat<()> {
        lancer(self.commande("wineserver")?.arg("-k"), DELAI_ARRET)?;
        let attente = lancer(self.commande("wineserver")?.arg("-w"), DELAI_ARRET)?;
        if attente.code != 0 {
            return Err(erreur_commande("wineserver", &["-w"], attente));
        }
        Ok(())
    }

    /// Lance `wine <args>` avec le délai du registre.
    fn reg(&self, args: &[&str]) -> Resultat<Sortie> {
        let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
        self.executer(&args, DELAI_REGISTRE)
    }

    /// Comme [`Wine::reg`] mais tout code non nul est une erreur.
    fn reg_verifie(&self, args: &[&str]) -> Resultat<()> {
        let sortie = self.reg(args)?;
        if sortie.code != 0 {
            return Err(erreur_commande("wine", args, sortie));
        }
        Ok(())
    }
}

/// Pose les variables communes à **tout** processus qui fait tourner notre Wine
/// (commandes [`Wine`], `yabridgectl sync`, Carla et `yabridge-host` du test de
/// plugin) : seul endroit où elles sont décidées.
///
/// - `HOME` = [`Chemins::home`] : identique au vrai dossier personnel en usage
///   réel, dossier temporaire en test.
/// - En isolation ([`Chemins::isole`]) : `XDG_DATA_HOME`, `XDG_CONFIG_HOME` et
///   `XDG_CACHE_HOME` sous `HOME`. Poser `HOME` ne suffit pas : si la personne qui
///   lance les tests a défini ces variables, Wine écrirait ses menus chez elle.
/// - `WINEDLLOVERRIDES` = `surcharges` demandées, plus `winemenubuilder.exe=d` en
///   isolation (voir [`surcharges_dll`]) ; non posée si la liste est vide.
///
/// Un appelant qui doit imposer un autre `XDG_CONFIG_HOME` (yabridgectl, isolé de
/// `~/.config/yabridgectl`) le pose **après** cet appel.
pub fn preparer_environnement(cmd: &mut Command, c: &Chemins, surcharges: &[&str]) {
    cmd.env("HOME", &c.home);
    if c.isole {
        cmd.env("XDG_DATA_HOME", c.xdg_data_home())
            .env("XDG_CONFIG_HOME", c.home.join(".config"))
            .env("XDG_CACHE_HOME", c.home.join(".cache"));
    }
    if let Some(valeur) = surcharges_dll(c, surcharges) {
        cmd.env("WINEDLLOVERRIDES", valeur);
    }
}

/// Valeur de `WINEDLLOVERRIDES` : les `demandees`, puis `winemenubuilder.exe=d` en
/// isolation, séparées par `;`. `None` si rien n'est à surcharger (la variable n'est
/// alors pas posée du tout).
///
/// ```
/// use wimao_core::chemins::Chemins;
/// use wimao_core::wine::{surcharges_dll, SURCHARGES_SANS_FENETRES};
/// let c = Chemins::pour_test(std::path::Path::new("/tmp/essai"));
/// assert_eq!(
///     surcharges_dll(&c, &[SURCHARGES_SANS_FENETRES]).as_deref(),
///     Some("mscoree,mshtml=;winemenubuilder.exe=d")
/// );
/// ```
pub fn surcharges_dll(c: &Chemins, demandees: &[&str]) -> Option<String> {
    let mut toutes: Vec<&str> = demandees.to_vec();
    if c.isole {
        toutes.push(SURCHARGE_SANS_MENUS);
    }
    (!toutes.is_empty()).then(|| toutes.join(SEPARATEUR_SURCHARGES))
}

/// `PATH` du processus lancé : dossier de Wine d'abord, puis le `PATH` actuel.
fn chemin_avec_wine_en_tete(bin: PathBuf) -> std::ffi::OsString {
    let mut elements = vec![bin];
    if let Some(actuel) = std::env::var_os("PATH") {
        elements.extend(std::env::split_paths(&actuel));
    }
    // `join_paths` ne échoue que si un élément contient `:` ; dans ce cas on se
    // rabat sur le PATH actuel plutôt que de paniquer.
    std::env::join_paths(elements).unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_default())
}

/// Erreur [`Erreur::Introuvable`] pour un programme de Wine absent : dit quel fichier
/// manque et comment y remédier (texte traduit).
fn wine_introuvable(programme: &Path) -> Erreur {
    Erreur::Introuvable(trf(
        "WiMao's Wine ({path}): install the WiMao Wine package or set WIMAO_WINE_DIR",
        &[("path", &programme.display().to_string())],
    ))
}

/// Erreur [`Erreur::Commande`] pour `<programme> <args>` (par exemple `wine reg add …`
/// ou `wineserver -w`) avec la sortie capturée.
fn erreur_commande(programme: &str, args: &[&str], sortie: Sortie) -> Erreur {
    Erreur::Commande {
        programme: format!("{programme} {}", args.join(" ")),
        code: Some(sortie.code),
        sortie: sortie.texte,
    }
}

/// Extrait la donnée de la ligne `    <valeur>    REG_xx    <donnée>` de `reg query`.
///
/// Wine peut écrire d'autres lignes avant (avertissements graphiques) : on ne
/// retient que celle qui commence par le nom de la valeur. Les noms de valeurs du
/// registre ne distinguent pas majuscules et minuscules. Une donnée vide donne
/// une chaîne vide.
fn extraire_valeur(texte: &str, valeur: &str) -> Option<String> {
    texte.lines().find_map(|ligne| {
        let ligne = ligne.trim_end_matches('\r');
        let reste = ligne.strip_prefix(SEPARATEUR_REG)?;
        let (nom, reste) = reste.split_at_checked(valeur.len())?;
        if !nom.eq_ignore_ascii_case(valeur) {
            return None;
        }
        let reste = reste.strip_prefix(SEPARATEUR_REG)?;
        let (type_reg, donnee) = match reste.split_once(SEPARATEUR_REG) {
            Some(paire) => paire,
            None => (reste, ""), // donnée vide : pas de dernière colonne
        };
        type_reg.starts_with("REG_").then(|| donnee.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Faux dossier de Wine sous `racine` : `bin/wine`, `bin/wineboot` et
    /// `bin/wineserver` existent (vides), assez pour construire les commandes.
    fn faux_wine(c: &mut Chemins, racine: &Path) {
        c.wine_dir = racine.join("faux-wine");
        std::fs::create_dir_all(c.wine_bin()).unwrap();
        for programme in ["wine", "wineboot", "wineserver"] {
            std::fs::write(c.wine_bin().join(programme), b"").unwrap();
        }
    }

    #[test]
    fn la_commande_pose_les_variables() {
        let d = tempfile::tempdir().unwrap();
        let mut chemins = Chemins::pour_test(d.path());
        faux_wine(&mut chemins, d.path());
        let cmd = Wine::nouveau(&chemins).commande("wine").unwrap();
        assert_eq!(cmd.get_program(), chemins.wine_bin().join("wine"));
        let env: std::collections::HashMap<_, _> = cmd
            .get_envs()
            .map(|(k, v)| (k.to_os_string(), v.map(|v| v.to_os_string())))
            .collect();
        let lire = |nom: &str| env.get(OsStr::new(nom)).cloned().flatten();
        assert_eq!(lire("WINEPREFIX"), Some(chemins.prefixe.clone().into()));
        assert_eq!(lire("WINEDEBUG"), Some("-all".into()));
        assert_eq!(lire("HOME"), Some(chemins.home.clone().into()));
        let path = lire("PATH").unwrap();
        let premier = std::env::split_paths(&path).next().unwrap();
        assert_eq!(premier, chemins.wine_bin());
    }

    /// Variables posées sur `cmd`, `None` pour une variable retirée.
    fn variables(cmd: &Command) -> std::collections::HashMap<String, Option<String>> {
        cmd.get_envs()
            .map(|(k, v)| {
                let v = v.map(|v| v.to_string_lossy().into_owned());
                (k.to_string_lossy().into_owned(), v)
            })
            .collect()
    }

    #[test]
    fn en_isolation_wine_n_ecrit_ni_menus_ni_hors_du_home() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(d.path());
        faux_wine(&mut c, d.path());
        let cmd = Wine::nouveau(&c)
            .commande_avec("wineboot", &[SURCHARGES_SANS_FENETRES])
            .unwrap();
        let env = variables(&cmd);
        let home = c.home.to_string_lossy().into_owned();
        assert_eq!(env["HOME"].as_deref(), Some(home.as_str()));
        for nom in ["XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME"] {
            let valeur = env[nom].clone().unwrap();
            assert!(Path::new(&valeur).starts_with(&c.home), "{nom} = {valeur}");
        }
        assert_eq!(
            env["WINEDLLOVERRIDES"].as_deref(),
            Some("mscoree,mshtml=;winemenubuilder.exe=d")
        );
        // Sans surcharge demandée, winemenubuilder reste désactivé.
        let simple = variables(&Wine::nouveau(&c).commande("wine").unwrap());
        assert_eq!(
            simple["WINEDLLOVERRIDES"].as_deref(),
            Some("winemenubuilder.exe=d")
        );
    }

    #[test]
    fn en_usage_reel_winemenubuilder_reste_actif() {
        let d = tempfile::tempdir().unwrap();
        let env = |nom: &str| (nom == "HOME").then(|| "/home/jeanne".into());
        let mut c = Chemins::depuis(env, |_| false).unwrap();
        faux_wine(&mut c, d.path());
        let cmd = Wine::nouveau(&c)
            .commande_avec("wineboot", &[SURCHARGES_SANS_FENETRES])
            .unwrap();
        let vars = variables(&cmd);
        assert_eq!(vars["WINEDLLOVERRIDES"].as_deref(), Some("mscoree,mshtml="));
        assert_eq!(vars["HOME"].as_deref(), Some("/home/jeanne"));
        // Les XDG_* de la personne sont respectés en usage réel.
        assert!(!vars.contains_key("XDG_DATA_HOME"));
        // Rien à surcharger : la variable n'est pas posée.
        let simple = Wine::nouveau(&c).commande("wine").unwrap();
        assert!(!variables(&simple).contains_key("WINEDLLOVERRIDES"));
    }

    #[test]
    fn wine_absent_est_une_erreur_claire_sans_repli_sur_le_path() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(d.path());
        c.wine_dir = d.path().join("pas-de-wine");
        // `sh` existe dans le PATH : il ne doit pas être pris à la place.
        match Wine::nouveau(&c).commande("sh") {
            Err(Erreur::Introuvable(texte)) => {
                assert!(texte.contains("pas-de-wine/bin/sh"), "{texte}");
                assert!(texte.contains("WIMAO_WINE_DIR"), "{texte}");
            }
            autre => panic!("attendu Introuvable, obtenu {autre:?}"),
        }
        assert!(matches!(
            Wine::nouveau(&c).arreter(),
            Err(Erreur::Introuvable(_))
        ));
    }

    #[test]
    fn l_erreur_de_commande_nomme_le_vrai_programme() {
        let sortie = Sortie {
            code: 3,
            texte: String::new(),
        };
        match erreur_commande("wineserver", &["-w"], sortie) {
            Erreur::Commande { programme, .. } => assert_eq!(programme, "wineserver -w"),
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn extraction_de_la_valeur() {
        let sortie = "libEGL warning: x\r\n\r\nHKEY_CURRENT_USER\\Software\\T\r\n    V    REG_SZ    a b\r\n\r\n";
        assert_eq!(extraire_valeur(sortie, "V"), Some("a b".to_string()));
        assert_eq!(extraire_valeur(sortie, "v"), Some("a b".to_string()));
        assert_eq!(extraire_valeur(sortie, "W"), None);
        assert_eq!(
            extraire_valeur("    Vide    REG_SZ\n", "Vide"),
            Some(String::new())
        );
        assert_eq!(
            extraire_valeur("    Nom long    REG_DWORD    0x1\n", "Nom long"),
            Some("0x1".to_string())
        );
        // Une valeur dont le nom n'est qu'un préfixe n'est pas confondue.
        assert_eq!(extraire_valeur("    VV    REG_SZ    x\n", "V"), None);
    }

    /// Environnement Wine neuf dans `racine`, sans fenêtres Mono/Gecko
    /// (`WINEDLLOVERRIDES`, voir la constante de l'initialisation dans NOTES.md).
    fn preparer(racine: &Path) -> Chemins {
        let chemins = Chemins::pour_test(racine);
        // Wine exige que le dossier de l'environnement existe déjà (« chdir to … »).
        std::fs::create_dir_all(&chemins.home).unwrap();
        std::fs::create_dir_all(&chemins.prefixe).unwrap();
        let wine = Wine::nouveau(&chemins);
        let mut cmd = wine
            .commande_avec("wineboot", &[SURCHARGES_SANS_FENETRES])
            .unwrap();
        cmd.arg("-u");
        let sortie = lancer(&mut cmd, Duration::from_secs(180)).unwrap();
        assert_eq!(sortie.code, 0, "{}", sortie.texte);
        chemins
    }

    #[test]
    #[ignore = "Wine requis"]
    fn registre_ajouter_lire_supprimer_et_arreter() {
        let d = tempfile::tempdir().unwrap();
        let chemins = preparer(d.path());
        let wine = Wine::nouveau(&chemins);
        let cle = r"HKCU\Software\WiMaoTest";
        assert_eq!(wine.reg_lire(cle, "Valeur").unwrap(), None); // clé absente
        wine.reg_ajouter(cle, "Valeur", "a b").unwrap();
        assert_eq!(
            wine.reg_lire(cle, "Valeur").unwrap(),
            Some("a b".to_string())
        );
        assert_eq!(wine.reg_lire(cle, "Autre").unwrap(), None); // valeur absente
        wine.reg_supprimer(cle, "Valeur").unwrap();
        assert_eq!(wine.reg_lire(cle, "Valeur").unwrap(), None);
        wine.arreter().unwrap();
        wine.arreter().unwrap(); // aucun serveur : pas une erreur
    }
}
