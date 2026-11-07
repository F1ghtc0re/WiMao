//! Traductions (français, anglais, espagnol) en Rust pur, sans locales glibc.
//!
//! Lit directement les fichiers `<dossier>/<langue>/LC_MESSAGES/wimao.mo` (compilés
//! depuis `po/*.po` par `build.rs`) avec la crate `gettext`. Pourquoi pas la libintl
//! de la glibc : elle ignore la langue imposée tant que la locale système correspondante
//! (par exemple `es_ES.UTF-8`) n'est pas générée, ou en locale `C` ; un utilisateur qui
//! choisit l'espagnol resterait en anglais. Ici la langue ne dépend que du fichier `.mo`.
//!
//! Utilisé par toutes les crates : toute chaîne visible par l'utilisateur passe par
//! [`tr`] ou [`trf`], avec un `msgid` en anglais. Pour ajouter une langue ou une
//! chaîne, voir `CONTRIBUTING.md`.

use crate::erreur::{Erreur, Resultat};
use gettext::Catalog;
use std::path::PathBuf;
use std::sync::RwLock;

/// Nom du domaine gettext, donc du fichier `wimao.mo`.
const DOMAINE: &str = "wimao";
/// Variable d'environnement qui impose le dossier des traductions (tests, paquets).
const VARIABLE_LOCALEDIR: &str = "WIMAO_LOCALEDIR";
/// Dossier système habituel des traductions installées.
const LOCALEDIR_SYSTEME: &str = "/usr/share/locale";
/// Dossier compilé par `build.rs` : dernier recours, permet `cargo run` sans installation.
///
/// Absent des builds de paquet (cfg `wimao_paquet`) : `OUT_DIR` est un chemin de la machine de
/// fabrication et ne doit pas se retrouver dans le binaire. Les tests, eux, en ont toujours
/// besoin pour retrouver les `.mo` compilés.
#[cfg(any(test, not(wimao_paquet)))]
const LOCALEDIR_COMPILE: &str = concat!(env!("OUT_DIR"), "/locale");
/// Dernier recours effectivement utilisé : le dossier compilé en développement, aucun en paquet.
#[cfg(not(wimao_paquet))]
const REPLI_DEVELOPPEMENT: Option<&str> = Some(LOCALEDIR_COMPILE);
#[cfg(wimao_paquet)]
const REPLI_DEVELOPPEMENT: Option<&str> = None;
/// Langue de référence connue pour tester si les traductions sont installées sur le système.
const LANGUE_SONDE: &str = "fr";
/// Langues pour lesquelles WiMao fournit un texte (l'anglais est le `msgid` lui-même).
const LANGUES_PRISES_EN_CHARGE: [&str; 3] = ["fr", "en", "es"];

/// Catalogue de la langue courante ; `None` = anglais (les `msgid` sont rendus tels quels).
static CATALOGUE: RwLock<Option<Catalog>> = RwLock::new(None);
/// Code de la langue courante (`"fr"`, `"es"`, `"en"`), tenu à jour avec [`CATALOGUE`] ;
/// vide tant que [`init_langue_verifiee`] n'a pas été appelée (vaut alors l'anglais).
static LANGUE: RwLock<String> = RwLock::new(String::new());
/// Langue des `msgid` : celle qui s'applique quand aucun catalogue n'est chargé.
const LANGUE_DES_MSGID: &str = "en";

/// Chemin du `.mo` d'une langue dans `dossier`.
fn chemin_mo(dossier: &std::path::Path, langue: &str) -> PathBuf {
    dossier
        .join(langue)
        .join("LC_MESSAGES")
        .join(format!("{DOMAINE}.mo"))
}

/// Choisit le dossier des traductions : `WIMAO_LOCALEDIR`, sinon le système si WiMao
/// y est installé, sinon le dossier de repli de développement (`repli`), sinon le système
/// quand il n'y a pas de repli (paquet : `init_langue_verifiee` signale alors l'absence
/// des `.mo`, au lieu d'aller chercher un chemin de la machine de fabrication).
///
/// Fonction pure : l'environnement, la présence des traductions système et le repli sont
/// fournis par l'appelant, ce qui permet de tester les deux comportements (développement
/// et paquet) dans un build ordinaire.
fn choisir_dossier(
    variable: Option<std::ffi::OsString>,
    systeme_installe: bool,
    repli: Option<&std::path::Path>,
) -> PathBuf {
    if let Some(dossier) = variable {
        return PathBuf::from(dossier);
    }
    match repli {
        Some(repli) if !systeme_installe => repli.to_path_buf(),
        _ => PathBuf::from(LOCALEDIR_SYSTEME),
    }
}

/// Dossier des traductions pour cette exécution (voir [`choisir_dossier`]).
fn dossier_traductions() -> PathBuf {
    choisir_dossier(
        std::env::var_os(VARIABLE_LOCALEDIR),
        chemin_mo(std::path::Path::new(LOCALEDIR_SYSTEME), LANGUE_SONDE).exists(),
        REPLI_DEVELOPPEMENT.map(std::path::Path::new),
    )
}

/// Vrai si une valeur de locale désigne la locale neutre (`C`, `POSIX`, avec ou sans
/// suffixe comme `.UTF-8`) : pas de langue à choisir.
fn locale_neutre(valeur: &str) -> bool {
    let nom = valeur.split(['.', '@']).next().unwrap_or("");
    nom == "C" || nom == "POSIX"
}

/// Déduit la langue du système en suivant la convention gettext.
///
/// 1. La première variable définie et non vide parmi `LC_ALL`, `LC_MESSAGES`, `LANG`
///    fait foi pour la locale. Si elle vaut `C` ou `POSIX` (avec ou sans `.UTF-8`),
///    le résultat est l'anglais, sans consulter les suivantes ni `LANGUAGE` : la glibc
///    ignore `LANGUAGE` en locale neutre, WiMao fait de même.
/// 2. Sinon `LANGUAGE` (liste séparée par `:`) est prioritaire : première langue prise
///    en charge de la liste (aussi quand aucune variable de locale n'est définie).
/// 3. Sinon la langue de la locale elle-même (`fr_FR.UTF-8` donne `fr`).
///
/// Seules les deux premières lettres comptent. `lire` donne la valeur d'une variable ;
/// paramètre pour tester sans toucher à l'environnement.
fn langue_du_systeme(lire: impl Fn(&str) -> Option<String>) -> Option<&'static str> {
    let non_vide = |nom: &str| lire(nom).filter(|v| !v.is_empty());
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|nom| non_vide(nom));
    if locale.as_deref().is_some_and(locale_neutre) {
        return None;
    }
    let liste = non_vide("LANGUAGE").into_iter().chain(locale);
    liste
        .flat_map(|valeur| valeur.split(':').map(str::to_owned).collect::<Vec<_>>())
        .filter_map(|code| code.get(..2).map(str::to_lowercase))
        .find_map(|deux| LANGUES_PRISES_EN_CHARGE.iter().find(|l| **l == deux))
        .copied()
}

/// Charge le catalogue d'une langue ; erreur explicite si le `.mo` manque ou est invalide.
fn charger_catalogue(langue: &str) -> Resultat<Catalog> {
    let chemin = chemin_mo(&dossier_traductions(), langue);
    let fichier = std::fs::File::open(&chemin).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Erreur::Introuvable(trf(
                "translation file {path} (install the gettext package, then rebuild)",
                &[("path", &chemin.display().to_string())],
            ))
        } else {
            Erreur::Io(e)
        }
    })?;
    Catalog::parse(fichier).map_err(|e| {
        Erreur::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            trf(
                "invalid translation file {path}: {error}",
                &[
                    ("path", &chemin.display().to_string()),
                    ("error", &e.to_string()),
                ],
            ),
        ))
    })
}

/// Remplace le catalogue courant (`None` = anglais) et retient le code de sa langue.
fn definir_catalogue(catalogue: Option<Catalog>, langue: &str) {
    // Un verrou empoisonné ne peut venir que d'une panique ailleurs ; on récupère l'état.
    let mut garde = CATALOGUE.write().unwrap_or_else(|e| e.into_inner());
    *garde = catalogue;
    let mut code = LANGUE.write().unwrap_or_else(|e| e.into_inner());
    *code = langue.to_owned();
}

/// Code de la langue réellement chargée (`"fr"`, `"es"` ou `"en"`), celui que rend
/// [`init_langue_verifiee`]. Sert aux textes qui ne passent pas par gettext mais existent en
/// plusieurs langues, comme les consignes des recettes (`[consignes]`, une entrée par code).
///
/// ```
/// # use wimao_core::i18n::{init_langue, langue_courante};
/// init_langue(Some("es"));
/// assert_eq!(langue_courante(), "es");
/// ```
pub fn langue_courante() -> String {
    let code = LANGUE.read().unwrap_or_else(|e| e.into_inner());
    if code.is_empty() {
        LANGUE_DES_MSGID.to_owned()
    } else {
        code.clone()
    }
}

/// Choisit la langue de l'interface et dit laquelle est réellement chargée.
///
/// - `None` : langue du système (voir `langue_du_systeme`), anglais si aucune n'est prise en charge ;
/// - `Some("fr")`, `Some("es")` : langue imposée ; `Some("en")` : anglais ;
/// - tout autre code (« xx ») : anglais, **sans erreur** (langue non prise en charge).
///
/// Renvoie le code effectivement chargé (`"fr"`, `"es"` ou `"en"`).
///
/// # Erreurs
/// [`Erreur::Introuvable`] ou [`Erreur::Io`] si la langue demandée est prise en charge
/// mais que son `.mo` est absent, illisible ou corrompu. L'anglais est alors actif.
///
/// Peut être rappelée pour changer de langue en cours d'exécution (thread-safe).
pub fn init_langue_verifiee(langue: Option<&str>) -> Resultat<String> {
    let code = match langue {
        Some(code) => Some(code),
        None => langue_du_systeme(|v| std::env::var(v).ok()),
    };
    let Some(code) =
        code.filter(|c| LANGUES_PRISES_EN_CHARGE.contains(c) && *c != LANGUE_DES_MSGID)
    else {
        definir_catalogue(None, LANGUE_DES_MSGID);
        return Ok(LANGUE_DES_MSGID.to_owned());
    };
    match charger_catalogue(code) {
        Ok(catalogue) => {
            definir_catalogue(Some(catalogue), code);
            Ok(code.to_owned())
        }
        Err(erreur) => {
            definir_catalogue(None, LANGUE_DES_MSGID);
            Err(erreur)
        }
    }
}

/// Comme [`init_langue_verifiee`], mais ne renvoie rien : en cas d'erreur, écrit une
/// ligne sur la sortie d'erreur et se replie sur l'anglais. Pratique au démarrage d'un
/// programme qui ne peut rien faire de plus utile de l'erreur.
pub fn init_langue(langue: Option<&str>) {
    if let Err(erreur) = init_langue_verifiee(langue) {
        // En anglais et sans `tr` : c'est justement le catalogue qui manque.
        eprintln!("WiMao: translations unavailable, using English ({erreur})");
    }
}

/// Traduit `msgid` (texte anglais) dans la langue courante ; renvoie `msgid` si aucune
/// traduction n'existe.
///
/// ```
/// # use wimao_core::i18n::tr;
/// # use wimao_core::i18n::init_langue;
/// init_langue(Some("fr"));
/// assert_eq!(tr("Ready"), "Prêt");
/// ```
pub fn tr(msgid: &str) -> String {
    let garde = CATALOGUE.read().unwrap_or_else(|e| e.into_inner());
    match garde.as_ref() {
        Some(catalogue) => catalogue.gettext(msgid).to_owned(),
        None => msgid.to_owned(),
    }
}

/// Comme [`tr`], mais dans un **contexte** (`pgettext`) : permet à un même mot anglais d'avoir
/// plusieurs traductions selon son rôle. Exemple : `Repair` est un verbe sur un bouton
/// (« Réparer », sans contexte) et un nom dans l'historique (`trc("history", "Repair")` donne
/// « Réparation »).
///
/// Le `.mo` stocke la clé `contexte\u{4}msgid` ; la crate `gettext` s'en charge. Renvoie `msgid`
/// si le contexte ou la traduction n'existe pas. `xgettext` extrait les deux arguments grâce à
/// `--keyword=trc:1c,2` (voir `scripts/verifier-traductions.sh`).
///
/// ```
/// # use wimao_core::i18n::{init_langue, trc};
/// init_langue(Some("fr"));
/// assert_eq!(trc("history", "Repair"), "Réparation");
/// ```
pub fn trc(contexte: &str, msgid: &str) -> String {
    let garde = CATALOGUE.read().unwrap_or_else(|e| e.into_inner());
    match garde.as_ref() {
        Some(catalogue) => catalogue.pgettext(contexte, msgid).to_owned(),
        None => msgid.to_owned(),
    }
}

/// Marque `msgid` comme chaîne à traduire **sans la traduire sur place**, et le rend tel quel.
///
/// Sert quand le texte est stocké dans une constante ou un tableau (ici les messages des
/// règles d'erreurs, `regles.rs`) et traduit plus tard avec [`tr`] : `xgettext`
/// (`--keyword=tr_noop`, voir `scripts/verifier-traductions.sh`) l'extrait alors dans
/// `po/wimao.pot`. C'est une fonction et non une macro car `xgettext` ne reconnaît un mot-clé
/// que suivi d'une parenthèse (`tr_noop!(` ne serait pas extrait).
///
/// ```
/// # use wimao_core::i18n::tr_noop;
/// const MESSAGE: &str = tr_noop("Ready");
/// assert_eq!(MESSAGE, "Ready");
/// ```
pub const fn tr_noop(msgid: &'static str) -> &'static str {
    msgid
}

/// Comme [`tr`], puis remplace chaque `{nom}` par la valeur associée dans `args`.
///
/// ```
/// # use wimao_core::i18n::trf;
/// # use wimao_core::i18n::init_langue;
/// init_langue(Some("fr"));
/// assert_eq!(trf("{n} plugins found", &[("n", "3")]), "3 plugins trouvés");
/// ```
pub fn trf(msgid: &str, args: &[(&str, &str)]) -> String {
    let mut texte = tr(msgid);
    for (nom, valeur) in args {
        texte = texte.replace(&format!("{{{nom}}}"), valeur);
    }
    texte
}

/// Traduit en choisissant la forme du **singulier** ou du **pluriel** selon `n` (ngettext) :
/// les règles de chaque langue viennent du `.mo` (`Plural-Forms`), sans « (s) ».
///
/// `{n}` est remplacé par `n` ; `args` ajoute d'autres variables, comme [`trf`]. Sans catalogue
/// (anglais), `singulier` sert pour `n == 1`, `pluriel` sinon. Les deux `msgid` sont extraits
/// par `xgettext --keyword=trn:1,2` (voir `scripts/verifier-traductions.sh`).
///
/// ```
/// # use wimao_core::i18n::{init_langue, trn};
/// init_langue(Some("fr"));
/// assert_eq!(trn("{n} ready", "{n} ready", 0, &[]), "0 prêt");
/// assert_eq!(trn("{n} ready", "{n} ready", 3, &[]), "3 prêts");
/// ```
pub fn trn(singulier: &str, pluriel: &str, n: u64, args: &[(&str, &str)]) -> String {
    let mut texte = {
        let garde = CATALOGUE.read().unwrap_or_else(|e| e.into_inner());
        match garde.as_ref() {
            Some(catalogue) => catalogue.ngettext(singulier, pluriel, n).to_owned(),
            None if n == 1 => singulier.to_owned(),
            None => pluriel.to_owned(),
        }
    };
    texte = texte.replace("{n}", &n.to_string());
    for (nom, valeur) in args {
        texte = texte.replace(&format!("{{{nom}}}"), valeur);
    }
    texte
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn trc_traduit_dans_son_contexte() {
        init_langue(Some("fr"));
        // Textes construits à l'exécution : xgettext n'extrait que les littéraux, donc ces
        // essais n'ajoutent aucune entrée (ni emplacement) à `po/wimao.pot`.
        let (contexte, msgid) = (String::from("history"), String::from("Repair"));
        // Même msgid que le bouton « Réparer », mais le nom de l'historique est « Réparation ».
        assert_eq!(trc(&contexte, &msgid), "Réparation");
        assert_eq!(tr(&msgid), "Réparer");
    }

    #[test]
    #[serial]
    fn trc_repli_sur_msgid_si_absent() {
        init_langue(Some("fr"));
        let msgid = String::from("Repair");
        // Contexte inconnu : le msgid revient tel quel, comme pour `tr`.
        assert_eq!(trc(&String::from("inconnu"), &msgid), msgid);
        // Sans catalogue (anglais), le msgid est rendu tel quel.
        init_langue(Some("en"));
        assert_eq!(trc(&String::from("history"), &msgid), msgid);
    }

    // `serial` : init_langue modifie l'état global (catalogue) et parfois l'environnement.
    #[test]
    #[serial]
    fn francais() {
        init_langue(Some("fr"));
        assert_eq!(tr("Ready"), "Prêt");
    }

    #[test]
    #[serial]
    fn espagnol() {
        init_langue(Some("es"));
        assert_eq!(tr("Ready"), "Listo");
    }

    #[test]
    #[serial]
    fn anglais() {
        init_langue(Some("en"));
        assert_eq!(tr("Ready"), "Ready");
    }

    #[test]
    #[serial]
    fn remplacement_des_variables() {
        init_langue(Some("fr"));
        assert_eq!(trf("{n} plugins found", &[("n", "3")]), "3 plugins trouvés");
    }

    fn env_simule<'a>(paires: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |nom| {
            paires
                .iter()
                .find(|(n, _)| *n == nom)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn langue_systeme_depuis_language() {
        let env = env_simule(&[("LANGUAGE", "es:fr"), ("LANG", "fr_FR.UTF-8")]);
        assert_eq!(langue_du_systeme(env), Some("es"));
    }

    #[test]
    fn langue_systeme_ignore_les_langues_non_prises_en_charge() {
        let env = env_simule(&[("LANGUAGE", "de:es")]);
        assert_eq!(langue_du_systeme(env), Some("es"));
    }

    #[test]
    fn langue_systeme_depuis_lang_seul() {
        assert_eq!(
            langue_du_systeme(env_simule(&[("LANG", "fr_FR.UTF-8")])),
            Some("fr")
        );
    }

    #[test]
    fn langue_systeme_non_geree_donne_anglais() {
        assert_eq!(
            langue_du_systeme(env_simule(&[("LANG", "de_DE.UTF-8")])),
            None
        );
        assert_eq!(langue_du_systeme(env_simule(&[("LANG", "C")])), None);
    }

    #[test]
    #[serial]
    fn init_langue_none_lit_l_environnement() {
        // Garde : retire les variables même si l'assertion échoue (sinon elles fuiraient
        // vers les autres tests).
        struct Nettoyage;
        impl Drop for Nettoyage {
            fn drop(&mut self) {
                for nom in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
                    std::env::remove_var(nom);
                }
            }
        }
        let _garde = Nettoyage;
        for nom in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            std::env::remove_var(nom);
        }
        std::env::set_var("LANGUAGE", "es:fr");
        init_langue(None);
        assert_eq!(tr("Ready"), "Listo");
    }

    #[test]
    #[serial]
    fn chaine_inconnue_renvoyee_telle_quelle() {
        init_langue(Some("fr"));
        // Texte construit à l'exécution : xgettext n'extrait que les littéraux, donc
        // cette chaîne ne doit pas apparaître dans `po/wimao.pot`.
        let inconnu = String::from("Unknown string xyz");
        assert_eq!(tr(&inconnu), inconnu);
    }

    #[test]
    fn locale_c_l_emporte_sur_la_suite() {
        let env = env_simule(&[("LC_ALL", "C"), ("LANG", "fr_FR.UTF-8")]);
        assert_eq!(langue_du_systeme(env), None);
        let env = env_simule(&[("LC_ALL", "C.UTF-8"), ("LANGUAGE", "es")]);
        assert_eq!(langue_du_systeme(env), None);
        let env = env_simule(&[("LANGUAGE", "es"), ("LC_ALL", "C")]);
        assert_eq!(langue_du_systeme(env), None);
    }

    #[test]
    fn locale_vide_est_ignoree() {
        let env = env_simule(&[("LC_ALL", ""), ("LANG", "fr_FR.UTF-8")]);
        assert_eq!(langue_du_systeme(env), Some("fr"));
    }

    #[test]
    #[serial]
    fn code_inconnu_donne_anglais_sans_erreur() {
        assert_eq!(init_langue_verifiee(Some("xx")).ok().as_deref(), Some("en"));
        assert_eq!(tr("Ready"), "Ready");
    }

    #[test]
    #[serial]
    fn langue_chargee_est_rendue() {
        assert_eq!(init_langue_verifiee(Some("es")).ok().as_deref(), Some("es"));
        assert_eq!(langue_courante(), "es");
        // Une langue non prise en charge, ou un `.mo` manquant, ramène à l'anglais.
        assert_eq!(init_langue_verifiee(Some("xx")).ok().as_deref(), Some("en"));
        assert_eq!(langue_courante(), "en");
    }

    /// Prépare un dossier de traductions contenant `contenu` comme `es/LC_MESSAGES/wimao.mo`.
    fn dossier_avec_mo(contenu: &[u8]) -> tempfile::TempDir {
        let dossier = tempfile::tempdir().expect("dossier temporaire");
        let sous = dossier.path().join("es").join("LC_MESSAGES");
        std::fs::create_dir_all(&sous).expect("création");
        std::fs::write(sous.join("wimao.mo"), contenu).expect("écriture");
        dossier
    }

    #[test]
    #[serial]
    fn mo_corrompu_donne_erreur_et_anglais() {
        let dossier = dossier_avec_mo(b"ceci n'est pas un fichier mo");
        std::env::set_var(VARIABLE_LOCALEDIR, dossier.path());
        let resultat = init_langue_verifiee(Some("es"));
        std::env::remove_var(VARIABLE_LOCALEDIR);
        assert!(resultat.is_err());
        assert_eq!(tr("Ready"), "Ready");
    }

    #[test]
    fn choix_du_dossier_des_traductions() {
        use std::path::Path;
        let repli = Some(Path::new("/repli"));
        // La variable d'environnement l'emporte toujours.
        assert_eq!(
            choisir_dossier(Some("/env".into()), true, repli),
            PathBuf::from("/env")
        );
        // Développement : système absent, on prend le repli.
        assert_eq!(choisir_dossier(None, false, repli), PathBuf::from("/repli"));
        // Traductions installées : le système, même s'il y a un repli.
        assert_eq!(
            choisir_dossier(None, true, repli),
            PathBuf::from(LOCALEDIR_SYSTEME)
        );
        // Paquet (pas de repli) : toujours le système, jamais un chemin de fabrication.
        for installe in [true, false] {
            assert_eq!(
                choisir_dossier(None, installe, None),
                PathBuf::from(LOCALEDIR_SYSTEME)
            );
        }
    }

    #[test]
    #[serial]
    fn mo_absent_donne_introuvable() {
        let dossier = tempfile::tempdir().expect("dossier temporaire");
        std::env::set_var(VARIABLE_LOCALEDIR, dossier.path());
        let resultat = init_langue_verifiee(Some("fr"));
        std::env::remove_var(VARIABLE_LOCALEDIR);
        assert!(matches!(resultat, Err(Erreur::Introuvable(_))));
    }

    #[test]
    #[serial]
    fn localedir_valide_est_prioritaire() {
        // Copie le vrai .mo espagnol compilé par build.rs dans un dossier à part.
        let source = chemin_mo(std::path::Path::new(LOCALEDIR_COMPILE), "es");
        let octets = std::fs::read(source).expect("mo compilé");
        let dossier = dossier_avec_mo(&octets);
        std::env::set_var(VARIABLE_LOCALEDIR, dossier.path());
        let resultat = init_langue_verifiee(Some("es"));
        let texte = tr("Ready");
        std::env::remove_var(VARIABLE_LOCALEDIR);
        assert_eq!(resultat.ok().as_deref(), Some("es"));
        assert_eq!(texte, "Listo");
    }
}
