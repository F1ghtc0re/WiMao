//! Chemins Windows des recettes : variables, traduction vers Linux et motifs `*`.
//!
//! Les recettes parlent en chemins Windows (`C:\Program Files\…`, `{appdata-local}\…`) ; WiMao
//! travaille sur le disque Linux de l'environnement Wine (`<prefixe>/drive_c`). Ce fichier fait
//! le pont :
//! - [`utilisateur_windows`] : le nom du profil Windows (`C:\users\<nom>`) ;
//! - `remplacer_variables` : `{utilisateur}`, `{telechargements}`, `{appdata-local}` ;
//! - `vers_linux` : `C:\…` vers un chemin réel, sans tenir compte des majuscules ;
//! - `resoudre_motif` : idem avec des `*` (nom de fichier unique, ou dossier de version) ;
//! - `comparer_versions` : ordre des numéros de version (`1.10.0 > 1.9.2`).
//!
//! Utilise `wimao-core` (chemins, erreurs, traductions). Utilisé par l'extraction
//! (`extraction.rs`), les dépendances et les réglages (`installation.rs` : fichiers, dossiers,
//! valeurs de registre), le pilote (`pilote.rs`) et la session des gestionnaires
//! (`gestionnaire.rs`). Voir `ARCHITECTURE.md` et `docs/recettes.md`.

use std::cmp::Ordering;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use wimao_core::i18n::trf;
use wimao_core::{Chemins, Erreur, Resultat};

/// Nom de l'utilisateur Windows quand ni `USER` ni le dossier personnel n'en donnent un.
const UTILISATEUR_PAR_DEFAUT: &str = "user";
/// Variable d'environnement qui contient le nom d'utilisateur Linux.
const VAR_UTILISATEUR: &str = "USER";
/// Dossier du profil Windows de Wine, `<user>` à remplacer.
const MODELE_PROFIL: &str = r"C:\users\<user>";
/// Séparateurs de segments d'un numéro de version.
const SEPARATEURS_VERSION: [char; 3] = ['.', '-', '_'];
/// Joker des motifs de recette.
const JOKER: char = '*';

/// Nom du profil Windows de Wine : celui de l'utilisateur Linux (variable `USER`) ; à défaut,
/// le nom du dossier personnel, puis `user`.
pub fn utilisateur_windows(c: &Chemins) -> String {
    utilisateur_depuis(c, std::env::var_os(VAR_UTILISATEUR))
}

/// Cœur de [`utilisateur_windows`], avec la valeur de `USER` donnée (testable sans toucher
/// à l'environnement du processus).
fn utilisateur_depuis(c: &Chemins, utilisateur: Option<OsString>) -> String {
    let nom = utilisateur
        .filter(|u| !u.is_empty())
        .or_else(|| c.home.file_name().map(OsString::from));
    nom.map_or_else(
        || UTILISATEUR_PAR_DEFAUT.to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Remplace les variables de `texte` : `{utilisateur}`, `{telechargements}` et
/// `{appdata-local}` (voir `docs/recettes.md`).
///
/// `utilisateur` est le nom du profil Windows (voir [`utilisateur_windows`]) : Wine nomme le
/// profil d'après l'utilisateur Linux (`C:\users\<nom>`).
pub(crate) fn remplacer_variables(texte: &str, utilisateur: &str) -> String {
    let profil = MODELE_PROFIL.replace("<user>", utilisateur);
    texte
        .replace("{telechargements}", &format!(r"{profil}\Downloads"))
        .replace("{appdata-local}", &format!(r"{profil}\AppData\Local"))
        .replace("{utilisateur}", utilisateur)
}

/// Traduit un chemin Windows en chemin Linux sous `drive_c`.
///
/// Accepte `C:\…` ou `c:/…`. Chaque composant existant est retrouvé sans tenir compte des
/// majuscules (Wine y est insensible) ; la suite, qui n'existe pas encore, est gardée telle
/// quelle (utile pour désigner un fichier à créer).
///
/// # Erreurs
/// `Erreur::NonPrisEnCharge` si le chemin n'est pas sur `C:` ou contient `..`.
pub(crate) fn vers_linux(drive_c: &Path, chemin_windows: &str) -> Resultat<PathBuf> {
    let mut chemin = drive_c.to_path_buf();
    let mut existe = true;
    for composant in composants(chemin_windows)? {
        if existe {
            match trouver_sans_casse(&chemin, composant) {
                Some(nom) => {
                    chemin.push(nom);
                    continue;
                }
                None => existe = false,
            }
        }
        chemin.push(composant);
    }
    Ok(chemin)
}

/// Résout un chemin Windows qui contient des `*` en un chemin Linux existant.
///
/// - `*` dans le **dernier** composant : il faut exactement un fichier correspondant ;
/// - `*` dans un **dossier** : on retient la version la plus haute (voir
///   `comparer_versions`, appliquée à la partie couverte par `*`).
///
/// Sans `*`, le chemin doit simplement exister.
///
/// # Erreurs
/// `Erreur::Introuvable` (avec le motif) si rien ne correspond, `Erreur::MotifAmbigu` si
/// plusieurs fichiers correspondent au dernier composant, `Erreur::NonPrisEnCharge` comme
/// `vers_linux`.
pub(crate) fn resoudre_motif(drive_c: &Path, chemin_windows: &str) -> Resultat<PathBuf> {
    let parties = composants(chemin_windows)?;
    resoudre(drive_c, &parties, chemin_windows, false)
}

/// Résout un motif **relatif** à `base` (séparateurs `/` ou `\\`, `*` admis) : mêmes règles que
/// `resoudre_motif` (une seule correspondance dans le dernier composant, version la plus
/// haute dans un dossier), sans le préfixe `C:`. Sert aux archives décompressées
/// (`$PLUGINSDIR/app-64.7z`), pour ne pas écrire une seconde logique de correspondance.
///
/// # Erreurs
/// `Erreur::Introuvable`, `Erreur::MotifAmbigu`, ou `Erreur::NonPrisEnCharge` si le motif
/// est vide, absolu ou contient `..` (il ne doit jamais sortir de `base`).
pub(crate) fn resoudre_motif_relatif(base: &Path, motif: &str) -> Resultat<PathBuf> {
    let parties: Vec<&str> = motif.split(['\\', '/']).filter(|p| !p.is_empty()).collect();
    let sort_de_base =
        parties.is_empty() || parties.contains(&"..") || motif.starts_with(['\\', '/']);
    if sort_de_base {
        return Err(Erreur::NonPrisEnCharge(trf(
            "archive path {path} (it must be relative and without \"..\")",
            &[("path", motif)],
        )));
    }
    resoudre(base, &parties, motif, true)
}

/// Descend récursivement dans `base` en consommant les `composants` restants.
///
/// Avec `refuser_liens` (archives décompressées, non fiables), tout lien symbolique rencontré
/// est refusé **avant** d'y descendre ou d'en lister le contenu, y compris en dernier
/// composant ou s'il pointe vers un chemin inexistant.
fn resoudre(
    base: &Path,
    composants: &[&str],
    motif: &str,
    refuser_liens: bool,
) -> Resultat<PathBuf> {
    if refuser_liens && est_lien(base) {
        return Err(Erreur::NonPrisEnCharge(trf(
            "archive path {path} (symbolic links are not followed)",
            &[("path", motif)],
        )));
    }
    let Some((premier, reste)) = composants.split_first() else {
        return Ok(base.to_path_buf());
    };
    let introuvable = || Erreur::Introuvable(motif.to_string());
    if !premier.contains(JOKER) {
        let nom = trouver_sans_casse(base, premier).ok_or_else(introuvable)?;
        return resoudre(&base.join(nom), reste, motif, refuser_liens);
    }
    let candidats = correspondances(base, premier);
    if reste.is_empty() {
        return match candidats.len() {
            0 => Err(introuvable()),
            // On repasse par `resoudre` pour que le candidat unique soit lui aussi contrôlé.
            1 => resoudre(&base.join(&candidats[0]), &[], motif, refuser_liens),
            nombre => Err(Erreur::MotifAmbigu {
                motif: motif.to_string(),
                nombre,
            }),
        };
    }
    // Dossier à joker : on retient la version la plus haute (spec 3.5) puis on résout la
    // suite ; toute erreur remonte telle quelle (pas de repli silencieux sur une ancienne
    // version, qui masquerait un `MotifAmbigu` ou un fichier manquant).
    let plus_haute = candidats
        .into_iter()
        .filter(|nom| base.join(nom).is_dir())
        .max_by(|a, b| {
            comparer_versions(&partie_couverte(a, premier), &partie_couverte(b, premier))
        })
        .ok_or_else(introuvable)?;
    resoudre(&base.join(plus_haute), reste, motif, refuser_liens)
}

/// Vrai si `chemin` est un lien symbolique (sans le suivre).
fn est_lien(chemin: &Path) -> bool {
    chemin
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
}

/// Découpe un chemin Windows en composants (après `C:`), en refusant ce qu'on ne sait pas
/// traduire sûrement : autre disque, chemin relatif, `..`.
fn composants(chemin_windows: &str) -> Resultat<Vec<&str>> {
    let refus = || {
        Erreur::NonPrisEnCharge(trf(
            "Windows path {path} (only C:\\ paths without \"..\" are supported)",
            &[("path", chemin_windows)],
        ))
    };
    let (lettre, suite) = chemin_windows.split_at_checked(2).ok_or_else(refus)?;
    let suite_ok = suite.starts_with(['\\', '/']);
    if !lettre.eq_ignore_ascii_case("c:") || !suite_ok {
        return Err(refus());
    }
    let parties: Vec<&str> = suite.split(['\\', '/']).filter(|p| !p.is_empty()).collect();
    if parties.contains(&"..") {
        return Err(refus());
    }
    Ok(parties)
}

/// Cherche dans `dossier` l'entrée qui porte `nom` aux majuscules près ; l'orthographe exacte
/// l'emporte si deux entrées ne diffèrent que par la casse.
fn trouver_sans_casse(dossier: &Path, nom: &str) -> Option<OsString> {
    let exact = dossier.join(nom);
    if exact.symlink_metadata().is_ok() {
        return Some(OsString::from(nom));
    }
    std::fs::read_dir(dossier)
        .ok()?
        .flatten()
        .map(|e| e.file_name())
        .find(|f| f.to_string_lossy().eq_ignore_ascii_case(nom))
}

/// Noms de `dossier` qui correspondent au motif `motif_nom` (avec `*`), sans tenir compte
/// de la casse. Le motif est échappé : seuls les `*` sont des jokers.
fn correspondances(dossier: &Path, motif_nom: &str) -> Vec<OsString> {
    let echappe: Vec<String> = motif_nom.split(JOKER).map(glob::Pattern::escape).collect();
    let Ok(motif) = glob::Pattern::new(&echappe.join("*")) else {
        return Vec::new();
    };
    let options = glob::MatchOptions {
        case_sensitive: false,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };
    let Ok(entrees) = std::fs::read_dir(dossier) else {
        return Vec::new();
    };
    entrees
        .flatten()
        .map(|e| e.file_name())
        .filter(|f| motif.matches_with(&f.to_string_lossy(), options))
        .collect()
}

/// Partie de `nom` couverte par les `*` de `motif_nom` : on retire ce qui précède le premier
/// `*` et ce qui suit le dernier (`app-1.9.2` pour `app-*` donne `1.9.2`).
fn partie_couverte(nom: &OsString, motif_nom: &str) -> String {
    let nom = nom.to_string_lossy();
    let debut = motif_nom.find(JOKER).unwrap_or(0);
    let fin = motif_nom.len() - motif_nom.rfind(JOKER).map_or(0, |i| i + 1);
    nom.get(debut..nom.len().saturating_sub(fin))
        .unwrap_or(&nom)
        .to_string()
}

/// Compare deux numéros de version segment par segment (séparateurs `.`, `-`, `_`).
///
/// Deux segments numériques se comparent comme des nombres, sinon comme du texte ; un
/// numéro plus court est plus petit s'il est le début de l'autre : `1.10.0 > 1.9.2`.
pub(crate) fn comparer_versions(a: &str, b: &str) -> Ordering {
    let mut segments_a = a.split(SEPARATEURS_VERSION);
    let mut segments_b = b.split(SEPARATEURS_VERSION);
    loop {
        match (segments_a.next(), segments_b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ordre = comparer_segments(x, y);
                if ordre != Ordering::Equal {
                    return ordre;
                }
            }
        }
    }
}

/// Compare deux segments : en nombres s'ils sont tous deux numériques, sinon en texte.
fn comparer_segments(x: &str, y: &str) -> Ordering {
    match (x.parse::<u64>(), y.parse::<u64>()) {
        (Ok(nx), Ok(ny)) => nx.cmp(&ny),
        _ => x.cmp(y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Crée `drive_c` dans un dossier temporaire avec les fichiers/dossiers donnés
    /// (un nom qui finit par `/` est un dossier).
    fn disque(entrees: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let drive_c = t.path().join("drive_c");
        for e in entrees {
            if e.ends_with('/') {
                std::fs::create_dir_all(drive_c.join(e)).unwrap();
            } else {
                let f = drive_c.join(e);
                std::fs::create_dir_all(f.parent().unwrap()).unwrap();
                std::fs::write(f, "x").unwrap();
            }
        }
        std::fs::create_dir_all(&drive_c).unwrap();
        (t, drive_c)
    }

    #[test]
    fn variables_remplacees() {
        assert_eq!(
            remplacer_variables("{appdata-local}\\x", "u"),
            r"C:\users\u\AppData\Local\x"
        );
        assert_eq!(
            remplacer_variables("{telechargements}\\NI", "u"),
            r"C:\users\u\Downloads\NI"
        );
        assert_eq!(remplacer_variables("{utilisateur}", "u"), "u");
        assert_eq!(remplacer_variables("sans variable", "u"), "sans variable");
    }

    #[test]
    fn utilisateur_windows_vient_de_user_puis_du_dossier_personnel() {
        let c = Chemins::pour_test(Path::new("/tmp/x"));
        assert_eq!(utilisateur_depuis(&c, Some("alice".into())), "alice");
        assert_eq!(utilisateur_depuis(&c, Some("".into())), "home");
        assert_eq!(utilisateur_depuis(&c, None), "home");
    }

    #[test]
    fn vers_linux_ignore_la_casse() {
        let (_t, d) = disque(&["Program Files/Foo/"]);
        assert_eq!(
            vers_linux(&d, r"C:\PROGRAM FILES\foo\bar.exe").unwrap(),
            d.join("Program Files/Foo/bar.exe")
        );
        assert_eq!(
            vers_linux(&d, "c:/program files").unwrap(),
            d.join("Program Files")
        );
    }

    #[test]
    fn vers_linux_refuse_hors_c_et_parent() {
        let (_t, d) = disque(&[]);
        for mauvais in [r"D:\x", r"C:\a\..\b", "relatif", "C:x", ""] {
            assert!(
                matches!(vers_linux(&d, mauvais), Err(Erreur::NonPrisEnCharge(_))),
                "{mauvais}"
            );
        }
    }

    #[test]
    fn motif_dans_le_nom_un_seul() {
        let (_t, d) = disque(&["x/NTKDaemon 1.2 Setup PC.exe", "x/Autre.exe"]);
        assert_eq!(
            resoudre_motif(&d, r"C:\x\NTKDaemon * Setup PC.exe").unwrap(),
            d.join("x/NTKDaemon 1.2 Setup PC.exe")
        );
    }

    #[test]
    fn motif_dans_le_nom_ambigu() {
        let (_t, d) = disque(&["x/NTKDaemon 1 Setup PC.exe", "x/NTKDaemon 2 Setup PC.exe"]);
        let r = resoudre_motif(&d, r"C:\x\NTKDaemon * Setup PC.exe");
        assert!(
            matches!(r, Err(Erreur::MotifAmbigu { nombre: 2, .. })),
            "{r:?}"
        );
    }

    #[test]
    fn motif_sans_correspondance_est_introuvable() {
        let (_t, d) = disque(&["x/a.exe"]);
        let motif = r"C:\x\NTK * Setup.exe";
        match resoudre_motif(&d, motif) {
            Err(Erreur::Introuvable(texte)) => assert!(texte.contains(motif)),
            autre => panic!("attendu Introuvable, obtenu {autre:?}"),
        }
    }

    #[test]
    fn motif_de_dossier_prend_la_version_la_plus_haute() {
        let (_t, d) = disque(&["L/app-1.9.2/x.exe", "L/app-1.40.0/x.exe"]);
        assert_eq!(
            resoudre_motif(&d, r"C:\L\app-*\x.exe").unwrap(),
            d.join("L/app-1.40.0/x.exe")
        );
    }

    #[test]
    fn version_la_plus_haute_sans_le_fichier_est_introuvable() {
        let (_t, d) = disque(&["L/app-2.0/vide/", "L/app-1.0/x.exe"]);
        let r = resoudre_motif(&d, r"C:\L\app-*\x.exe");
        assert!(matches!(r, Err(Erreur::Introuvable(_))), "{r:?}");
    }

    #[test]
    fn comparer_versions_numerique() {
        assert_eq!(comparer_versions("1.10.0", "1.9.2"), Ordering::Greater);
        assert_eq!(comparer_versions("1.2", "1.2"), Ordering::Equal);
        assert_eq!(comparer_versions("1.2", "1.2.1"), Ordering::Less);
        assert_eq!(comparer_versions("2-1", "2_1"), Ordering::Equal);
        assert_eq!(comparer_versions("1.a", "1.b"), Ordering::Less);
    }
}
