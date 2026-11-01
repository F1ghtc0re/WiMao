//! Rapport de problème anonymisé, à joindre à une demande d'aide.
//!
//! [`rapport`] rassemble la version de WiMao et de Wine, la distribution, le
//! diagnostic de l'environnement et la fin du journal, puis [`anonymiser`] retire
//! tout ce qui identifie la personne : dossier personnel, nom d'utilisateur,
//! adresses e-mail et nom de la machine. L'utilisateur peut relire le texte avant de
//! l'envoyer ; rien n'est envoyé par WiMao.
//!
//! # Ordre des remplacements
//!
//! Un seul passage sur le texte d'origine : on repère toutes les occurrences, puis on
//! garde la plus à gauche, en cas d'égalité la plus prioritaire (dossier personnel,
//! e-mail, machine, utilisateur). Ainsi `/home/alice/x` devient `~/x` (et non
//! `/home/<user>/x`), `alice@a.fr` devient `<e-mail>` en entier, `alice-pc` donne
//! `<machine>` si la machine s'appelle ainsi, et un remplacement ne peut jamais
//! recouper le texte d'un autre (`<user>` contient « user »).
//!
//! Utilise `diagnostic`, `journal`, `wine` ; utilisé par la CLI et l'interface.

use crate::chemins::Chemins;
use crate::diagnostic::{diagnostiquer, message_probleme};
use crate::erreur::Resultat;
use crate::i18n::{tr, trf};
use crate::journal;
use crate::wine::Wine;
use regex::Regex;
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

/// Délai de `wine --version` (aucun serveur Wine n'est démarré : réponse immédiate).
const DELAI_VERSION_WINE: Duration = Duration::from_secs(20);
/// Lignes du journal jointes au rapport.
const LIGNES_JOURNAL: usize = 200;
/// Expression d'une adresse e-mail.
const MOTIF_EMAIL: &str = r"[\w.+-]+@[\w-]+\.[\w.]+";

/// Une occurrence à remplacer : position dans le texte d'origine et remplacement.
struct Occurrence {
    debut: usize,
    fin: usize,
    /// Plus petit = plus prioritaire à position égale.
    priorite: u8,
    remplacement: &'static str,
}

/// Retire du `texte` le dossier personnel (`~`), les adresses e-mail, le nom de la
/// machine et le nom d'utilisateur (mot entier, sans tenir compte de la casse : le
/// « alice » de `alicex` est conservé). Un nom vide est ignoré.
///
/// ```
/// use std::path::Path;
/// let t = wimao_core::rapport::anonymiser(
///     "/home/alice/x alice@a.fr ALICE alicex", "alice", Path::new("/home/alice"), "pc");
/// assert_eq!(t, "~/x <e-mail> <user> alicex");
/// ```
pub fn anonymiser(texte: &str, utilisateur: &str, home: &Path, machine: &str) -> String {
    let mut trouvees = Vec::new();
    ajouter_home(texte, home, &mut trouvees);
    ajouter_emails(texte, &mut trouvees);
    ajouter_mot(texte, machine, 2, "<machine>", &mut trouvees);
    ajouter_mot(texte, utilisateur, 3, "<user>", &mut trouvees);
    trouvees.sort_by_key(|o| (o.debut, o.priorite));
    let mut sortie = String::with_capacity(texte.len());
    let mut position = 0;
    for o in &trouvees {
        // Recoupe une occurrence déjà remplacée (plus à gauche ou plus prioritaire) : ignorée.
        if o.debut < position {
            continue;
        }
        sortie.push_str(&texte[position..o.debut]);
        sortie.push_str(o.remplacement);
        position = o.fin;
    }
    sortie.push_str(&texte[position..]);
    sortie
}

/// Vrai si `c` peut faire partie d'un mot (lettre, chiffre ou `_`).
fn est_mot(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Vrai si l'occurrence `debut..fin` de `texte` est bordée de caractères qui ne font
/// pas partie d'un mot.
fn mot_entier(texte: &str, debut: usize, fin: usize) -> bool {
    let avant = texte[..debut]
        .chars()
        .next_back()
        .is_none_or(|c| !est_mot(c));
    let apres = texte[fin..].chars().next().is_none_or(|c| !est_mot(c));
    avant && apres
}

/// Le dossier personnel (casse respectée : un chemin Linux est sensible à la casse),
/// pas lorsqu'il est le préfixe d'un autre nom (`/home/alicex`). Ignoré s'il est vide ou `/`.
fn ajouter_home(texte: &str, home: &Path, sortie: &mut Vec<Occurrence>) {
    let home = home.to_string_lossy();
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return;
    }
    for (debut, trouve) in texte.match_indices(home) {
        let fin = debut + trouve.len();
        if texte[fin..].chars().next().is_none_or(|c| !est_mot(c)) {
            sortie.push(Occurrence {
                debut,
                fin,
                priorite: 0,
                remplacement: "~",
            });
        }
    }
}

/// Les adresses e-mail.
fn ajouter_emails(texte: &str, sortie: &mut Vec<Occurrence>) {
    // Le motif est une constante valide : l'échec n'arrive pas, mais sans `unwrap` on l'ignore.
    let Ok(re) = Regex::new(MOTIF_EMAIL) else {
        return;
    };
    for m in re.find_iter(texte) {
        sortie.push(Occurrence {
            debut: m.start(),
            fin: m.end(),
            priorite: 1,
            remplacement: "<e-mail>",
        });
    }
}

/// Les occurrences de `mot` (sans tenir compte de la casse) qui sont des mots entiers.
fn ajouter_mot(
    texte: &str,
    mot: &str,
    priorite: u8,
    remplacement: &'static str,
    sortie: &mut Vec<Occurrence>,
) {
    if mot.is_empty() {
        return;
    }
    let Ok(re) = Regex::new(&format!("(?i){}", regex::escape(mot))) else {
        return;
    };
    for m in re.find_iter(texte) {
        if mot_entier(texte, m.start(), m.end()) {
            sortie.push(Occurrence {
                debut: m.start(),
                fin: m.end(),
                priorite,
                remplacement,
            });
        }
    }
}

/// Construit le rapport anonymisé : versions, distribution, diagnostic et fin du journal.
///
/// Un élément qu'on ne peut pas lire (Wine absent, diagnostic en échec) devient une
/// ligne d'explication : un rapport partiel vaut mieux que pas de rapport. Seule une
/// erreur d'entrée/sortie sur le journal est renvoyée.
pub fn rapport(c: &Chemins) -> Resultat<String> {
    let journal = match journal::fichier_actif() {
        Some(f) => f.to_path_buf(),
        None => c.donnees.join("journal.log"),
    };
    let lignes = journal::dernieres_lignes(&journal, LIGNES_JOURNAL)?;
    let mut t = String::new();
    t.push_str(&trf(
        "WiMao version: {v}",
        &[("v", env!("CARGO_PKG_VERSION"))],
    ));
    t.push('\n');
    t.push_str(&trf("Wine version: {v}", &[("v", &version_wine(c))]));
    t.push('\n');
    t.push_str(&trf("Distribution: {v}", &[("v", &distribution())]));
    t.push_str("\n\n");
    t.push_str(&tr("Diagnostic:"));
    t.push('\n');
    t.push_str(&texte_diagnostic(c));
    t.push_str("\n\n");
    t.push_str(&tr("Last lines of the log:"));
    t.push('\n');
    t.push_str(&lignes.join("\n"));
    t.push('\n');
    Ok(anonymiser(&t, &utilisateur(c), &c.home, &machine()))
}

/// Sortie de `wine --version`, ou « inconnue » si Wine ne répond pas.
fn version_wine(c: &Chemins) -> String {
    match Wine::nouveau(c).executer(&[OsStr::new("--version")], DELAI_VERSION_WINE) {
        Ok(s) if s.code == 0 => s.texte.trim().to_string(),
        _ => tr("unknown"),
    }
}

/// Un problème par ligne, ou « aucun problème ».
fn texte_diagnostic(c: &Chemins) -> String {
    match diagnostiquer(c) {
        Ok(p) if p.is_empty() => tr("No problem found"),
        Ok(p) => p
            .iter()
            .map(|p| format!("- {}", message_probleme(p)))
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => trf(
            "The diagnostic failed: {error}",
            &[("error", &e.to_string())],
        ),
    }
}

/// `PRETTY_NAME` de `/etc/os-release`, ou « inconnue ».
fn distribution() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| pretty_name(&t))
        .unwrap_or_else(|| tr("unknown"))
}

/// Valeur de `PRETTY_NAME` dans le contenu d'un fichier `os-release` (guillemets retirés).
fn pretty_name(contenu: &str) -> Option<String> {
    contenu
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
}

/// Nom d'utilisateur : `USER`, sinon `LOGNAME`, sinon le dernier élément du dossier personnel.
fn utilisateur(c: &Chemins) -> String {
    ["USER", "LOGNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .or_else(|| c.home.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default()
}

/// Nom de la machine (noyau Linux : `/proc/sys/kernel/hostname`), vide si illisible.
fn machine() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anon(t: &str) -> String {
        anonymiser(t, "alice", Path::new("/home/alice"), "pc")
    }

    #[test]
    fn exemple_du_plan() {
        assert_eq!(
            anon("/home/alice/x alice@a.fr ALICE alicex"),
            "~/x <e-mail> <user> alicex"
        );
    }

    #[test]
    fn home_avant_utilisateur_et_prefixe_de_nom_conserve() {
        assert_eq!(anon("/home/alice"), "~");
        assert_eq!(anon("/home/alicex/y"), "/home/alicex/y");
        assert_eq!(anon("user=Alice."), "user=<user>.");
    }

    #[test]
    fn machine_en_mot_entier_et_prioritaire_sur_l_utilisateur() {
        assert_eq!(anon("host pc, pcx, PC"), "host <machine>, pcx, <machine>");
        assert_eq!(
            anonymiser("alice-pc", "alice", Path::new("/h"), "alice-pc"),
            "<machine>"
        );
    }

    #[test]
    fn les_marques_ne_sont_pas_re_remplacees() {
        // Une machine « mail » ne doit pas casser « <e-mail> » (remplacement sur le texte d'origine).
        assert_eq!(
            anonymiser("a@b.fr mail", "zzz", Path::new("/h"), "mail"),
            "<e-mail> <machine>"
        );
    }

    #[test]
    fn noms_vides_et_home_racine_ignores() {
        assert_eq!(anonymiser("a b /x", "", Path::new("/"), ""), "a b /x");
    }

    #[test]
    fn nom_avec_caracteres_speciaux() {
        assert_eq!(
            anonymiser("c++ et C++.", "c++", Path::new("/h"), ""),
            "<user> et <user>."
        );
    }

    #[test]
    fn lecture_de_pretty_name() {
        assert_eq!(
            pretty_name("NAME=X\nPRETTY_NAME=\"Ubuntu 26.04 LTS\"\n").as_deref(),
            Some("Ubuntu 26.04 LTS")
        );
        assert_eq!(pretty_name("NAME=X"), None);
    }

    #[test]
    fn le_rapport_est_anonymise_et_contient_le_journal() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(d.path());
        c.home = std::path::PathBuf::from("/home/jeanne-secrete");
        std::fs::create_dir_all(&c.donnees).unwrap();
        std::fs::write(
            c.donnees.join("journal.log"),
            "[x] $ ls /home/jeanne-secrete/a\n  code: 0\n",
        )
        .unwrap();
        let r = rapport(&c).unwrap();
        assert!(r.contains("~/a"), "{r}");
        assert!(!r.contains("jeanne-secrete"), "{r}");
        assert!(r.contains(env!("CARGO_PKG_VERSION")));
    }
}
