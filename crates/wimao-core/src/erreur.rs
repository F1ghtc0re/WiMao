//! Erreur unique de WiMao et alias `Resultat`.
//!
//! Utilisé par toutes les crates : toute fonction qui peut échouer renvoie
//! [`Resultat`]. Un seul type d'erreur permet à l'interface d'afficher un message
//! cohérent, sans panique (`unwrap`/`expect` sont interdits hors des tests).

use crate::i18n::{tr, trf, trn};
use std::fmt;
use std::path::PathBuf;

/// Tout ce qui peut mal tourner dans WiMao, avec assez de contexte pour corriger.
#[derive(Debug)]
pub enum Erreur {
    /// Une commande externe a échoué (code de sortie non nul ou tuée par un signal).
    Commande {
        /// Nom du programme lancé.
        programme: String,
        /// Code de sortie, `None` si le processus a été tué par un signal.
        code: Option<i32>,
        /// Sortie standard et d'erreur mélangées, pour diagnostiquer.
        sortie: String,
    },
    /// Une commande externe a dépassé son délai et a été tuée.
    Delai {
        /// Nom du programme lancé.
        programme: String,
    },
    /// Un téléchargement a échoué ; le texte donne la cause.
    Telechargement(String),
    /// L'empreinte SHA-256 d'un fichier ne correspond pas à celle attendue.
    Empreinte {
        /// Empreinte attendue (hexadécimal).
        attendue: String,
        /// Empreinte calculée (hexadécimal).
        obtenue: String,
    },
    /// Une recette est invalide ; indique le fichier et, si connue, la ligne.
    Recette {
        /// Fichier de recette concerné.
        fichier: PathBuf,
        /// Numéro de ligne (à partir de 1) si on sait le déterminer.
        ligne: Option<usize>,
        /// Ce qui ne va pas, formulé pour que l'auteur de la recette le corrige.
        message: String,
    },
    /// Un élément cherché (fichier, outil, plugin…) n'existe pas.
    Introuvable(String),
    /// Un motif de recette (avec `*`) désigne plusieurs fichiers alors qu'un seul est
    /// attendu : la recette doit être précisée.
    MotifAmbigu {
        /// Motif de la recette (chemin Windows avec `*`).
        motif: String,
        /// Nombre de fichiers qui correspondent.
        nombre: usize,
    },
    /// Erreur d'entrée/sortie du système.
    Io(std::io::Error),
    /// Un plugin tourne dans un logiciel de musique (processus `yabridge-host` sur
    /// notre environnement Wine) : arrêter le `wineserver` le ferait planter.
    /// Renvoyée avant toute opération qui arrête Wine (points de restauration).
    /// Le message est traduit ; l'interface peut aussi tester cette variante pour
    /// proposer « fermez votre logiciel de musique ».
    PluginsEnCours,
    /// Un retour en arrière (points de restauration) n'a pas pu supprimer toutes les
    /// entrées demandées ; les autres l'ont été et le registre est restauré.
    RetourIncomplet {
        /// Nombre d'entrées supprimées avec succès.
        supprimes: usize,
        /// Entrées qu'on n'a pas pu supprimer.
        echecs: Vec<PathBuf>,
    },
    /// Le point de restauration (identifiant donné) est périmé : il ne correspond plus à
    /// l'environnement. Depuis sa création, une sauvegarde a été restaurée, un point plus ancien
    /// a été utilisé (`revenir`), ou l'environnement a été remplacé (import, annulation de
    /// l'import, préfixe neuf : `restauration::perimer_tous_les_points`). Renvoyée par `revenir`
    /// (et par la CLI, qui la connaît d'après la liste) avant toute modification.
    PointPerime(String),
    /// Une installation de plugin est déjà en cours (fichier verrou
    /// `<données>/installation.lock` tenu par un processus vivant) : en lancer une
    /// seconde dans le même environnement Wine les ferait se gêner.
    InstallationEnCours,
    /// La réparation a rencontré des échecs ; les autres problèmes ont quand même été
    /// traités. Chaque entrée dit quel problème et pourquoi il n'a pas pu être réparé.
    ReparationIncomplete {
        /// Une ligne « problème : cause » par échec.
        echecs: Vec<String>,
    },
    /// Une sauvegarde ou une restauration de l'environnement a échoué (destination
    /// pleine ou débranchée, archive déjà existante, place insuffisante, archive
    /// vide…). Le texte est déjà traduit et dit quoi faire.
    Sauvegarde(String),
    /// Fonction demandée mais non gérée (système ou format non pris en charge).
    NonPrisEnCharge(String),
    /// Un numéro de processus n'est pas valide (0 ou trop grand) : l'envoi d'un signal
    /// viserait alors tout un groupe de processus, on le refuse.
    PidInvalide(u32),
    /// Import de `~/.wine-mao` refusé : l'environnement Wine de WiMao (le chemin donné) existe
    /// déjà, et l'import l'écraserait. L'assistant ne propose l'import que s'il est absent.
    DejaPresent(PathBuf),
    /// Import de `~/.wine-mao` refusé : l'ancien dossier (`source`) et le dossier qui doit le
    /// recevoir (`destination`) sont sur deux disques différents. WiMao ne fait qu'un
    /// déplacement instantané sur un même disque, jamais une copie (plus de 100 Go) ; rien n'a
    /// été modifié.
    AutreDisque {
        /// Ancien emplacement de l'environnement (`~/.wine-mao`).
        source: PathBuf,
        /// Dossier qui aurait reçu l'environnement (parent de `Chemins::prefixe`).
        destination: PathBuf,
    },
    /// Annulation de l'import refusée : le chemin donné (`~/.wine-mao`) n'est plus le lien créé
    /// par l'import vers l'environnement de WiMao (remplacé par un dossier, un autre lien…).
    /// Le déplacer écraserait ce qui s'y trouve.
    PasNotreLien(PathBuf),
    /// Un import ou une annulation d'import a été interrompu (coupure de courant…) et l'état
    /// trouvé ne permet pas de savoir sans risque comment le terminer : rien n'a été modifié,
    /// les deux emplacements sont à vérifier à la main.
    ImportInterrompu {
        /// Ancien emplacement (`~/.wine-mao`).
        ancien: PathBuf,
        /// Environnement de WiMao (`Chemins::prefixe`).
        prefixe: PathBuf,
    },
}

/// Le texte affiché est **traduit** (`msgid` anglais dans `po/`) : la ligne de commande et
/// l'interface l'affichent tel quel. Écrit à la main et non avec `#[error(...)]` de
/// `thiserror`, parce que `xgettext` ne lirait pas les chaînes de ces attributs
/// (`scripts/verifier-traductions.sh`). Les parties variables (sortie d'une commande,
/// message du système) restent telles quelles.
impl fmt::Display for Erreur {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Erreur::Commande {
                programme,
                code,
                sortie,
            } => f.write_str(&trf(
                "the command \"{program}\" failed (code {code}):\n{output}",
                &[
                    ("program", programme),
                    (
                        "code",
                        &code.map_or_else(|| "?".to_owned(), |c| c.to_string()),
                    ),
                    ("output", sortie),
                ],
            )),
            Erreur::Delai { programme } => f.write_str(&trf(
                "the command \"{program}\" took too long and was stopped",
                &[("program", programme)],
            )),
            Erreur::Telechargement(cause) => {
                f.write_str(&trf("download failed: {cause}", &[("cause", cause)]))
            }
            Erreur::Empreinte { attendue, obtenue } => f.write_str(&trf(
                "wrong checksum: expected {expected}, got {actual}",
                &[("expected", attendue), ("actual", obtenue)],
            )),
            Erreur::Recette {
                fichier,
                ligne,
                message,
            } => {
                let lieu = ligne.map_or_else(String::new, |l| {
                    trf(" (line {line})", &[("line", &l.to_string())])
                });
                f.write_str(&trf(
                    "invalid recipe {file}{place}: {message}",
                    &[
                        ("file", &fichier.display().to_string()),
                        ("place", &lieu),
                        ("message", message),
                    ],
                ))
            }
            Erreur::Introuvable(quoi) => f.write_str(&trf("not found: {what}", &[("what", quoi)])),
            Erreur::MotifAmbigu { motif, nombre } => f.write_str(&trf(
                "several files match {pattern} ({count}): the recipe must be corrected",
                &[("pattern", motif), ("count", &nombre.to_string())],
            )),
            Erreur::Io(e) => f.write_str(&trf(
                "input/output error: {error}",
                &[("error", &e.to_string())],
            )),
            Erreur::PluginsEnCours => f.write_str(&tr(
                "Close your music software before continuing: a plugin is running",
            )),
            Erreur::RetourIncomplet { supprimes, echecs } => f.write_str(&trf(
                "rollback incomplete: {removed} entries removed, {failed} failed:\n{list}",
                &[
                    ("removed", &supprimes.to_string()),
                    ("failed", &echecs.len().to_string()),
                    ("list", &noms_des_chemins(echecs)),
                ],
            )),
            Erreur::PointPerime(_) => {
                f.write_str(&tr(
                    "this restore point no longer matches the environment (a backup was restored, an older point was used, or the environment was replaced since then)",
                ))
            }
            Erreur::InstallationEnCours => f.write_str(&tr(
                "Another installation is already running: wait for it to finish, then try again",
            )),
            Erreur::ReparationIncomplete { echecs } => f.write_str(&trn(
                "repair incomplete, {n} problem not repaired:\n{list}",
                "repair incomplete, {n} problems not repaired:\n{list}",
                echecs.len() as u64,
                &[("list", &echecs.join("\n"))],
            )),
            Erreur::Sauvegarde(texte) => f.write_str(texte),
            Erreur::NonPrisEnCharge(quoi) => {
                f.write_str(&trf("not supported: {what}", &[("what", quoi)]))
            }
            Erreur::PidInvalide(pid) => f.write_str(&trf(
                "invalid process number: {pid}",
                &[("pid", &pid.to_string())],
            )),
            Erreur::DejaPresent(chemin) => f.write_str(&trf(
                "The WiMao Wine environment already exists ({path}): importing would overwrite it",
                &[("path", &chemin.display().to_string())],
            )),
            Erreur::AutreDisque {
                source,
                destination,
            } => f.write_str(&trf(
                "{source} and {destination} are on different disks. WiMao only moves the environment within the same disk and never copies it (it is too large). Nothing was changed: you can move the folder yourself or keep using it where it is",
                &[
                    ("source", &source.display().to_string()),
                    ("destination", &destination.display().to_string()),
                ],
            )),
            Erreur::PasNotreLien(chemin) => f.write_str(&trf(
                "{path} is no longer the link created by the import: cancelling the import is refused so that nothing is overwritten",
                &[("path", &chemin.display().to_string())],
            )),
            Erreur::ImportInterrompu { ancien, prefixe } => f.write_str(&trf(
                "An import of the Wine environment was interrupted and cannot be finished safely: nothing was changed. Check {old} and {new} by hand",
                &[
                    ("old", &ancien.display().to_string()),
                    ("new", &prefixe.display().to_string()),
                ],
            )),
        }
    }
}

impl std::error::Error for Erreur {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Erreur::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Erreur {
    fn from(e: std::io::Error) -> Self {
        Erreur::Io(e)
    }
}

/// Un chemin par ligne, pour les messages d'erreur.
fn noms_des_chemins(chemins: &[PathBuf]) -> String {
    chemins
        .iter()
        .map(|c| c.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Résultat standard de WiMao.
pub type Resultat<T> = Result<T, Erreur>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_de_recette_avec_ligne() {
        let e = Erreur::Recette {
            fichier: PathBuf::from("a.toml"),
            ligne: Some(4),
            message: "clé inconnue".into(),
        };
        let texte = e.to_string();
        assert!(texte.contains("4") && texte.contains("clé inconnue"));
    }

    #[test]
    fn message_de_recette_sans_ligne() {
        let e = Erreur::Recette {
            fichier: PathBuf::from("a.toml"),
            ligne: None,
            message: "x".into(),
        };
        assert!(!e.to_string().contains("(l")); // ni « (ligne » ni « (line »
    }
}
