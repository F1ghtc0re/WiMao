//! Définition de la ligne de commande : options et sous-commandes (`clap`, version « derive »).
//!
//! Utilise `clap` et `wimao_core::i18n::tr`. Les structures décrivent la forme des
//! commandes ; les textes d'aide, eux, sont ajoutés par [`commande_traduite`] avec
//! `tr()`. Ils ne sont pas écrits dans les attributs `#[…]` parce que `xgettext`
//! (`scripts/verifier-traductions.sh`) prend les lignes qui commencent par `#` pour des
//! directives de préprocesseur et n'y extrairait aucune chaîne. La langue doit donc être
//! choisie **avant** l'analyse des arguments (voir [`langue_demandee`]).
//! Utilisé par `main.rs` (analyse) et `commandes/` (une fonction par sous-commande).
//! Les commentaires des champs sont des `//` (et non `///`) pour que `clap` n'en tire
//! pas un texte d'aide français : tout texte d'aide vient de [`commande_traduite`].

use std::path::PathBuf;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand};
use wimao_core::i18n::tr;

/// Langues acceptées par `--langue` (les mêmes que `wimao_core::i18n`).
pub const LANGUES: [&str; 3] = ["fr", "en", "es"];

/// Ligne de commande complète de `wimao-cli`.
#[derive(Debug, Parser)]
#[command(name = "wimao-cli", version)]
pub struct Cli {
    // Langue des messages ; par défaut celle du système.
    #[arg(long, global = true, value_parser = LANGUES, value_name = "LANG")]
    pub langue: Option<String>,

    // Sous-commande à exécuter.
    #[command(subcommand)]
    pub commande: Commande,
}

/// Les sous-commandes de `wimao-cli`.
#[derive(Debug, Subcommand)]
pub enum Commande {
    // Crée l'environnement Wine de WiMao.
    Init,
    // Installe un plugin depuis son installeur ou son archive.
    Installer(ArgsInstaller),
    // Reconnaît le type d'un installeur sans rien lancer.
    Detecter {
        // Fichier à examiner.
        #[arg(value_name = "FILE")]
        fichier: PathBuf,
    },
    // Teste l'ouverture de chaque plugin déjà ponté.
    Verifier {
        // Environnement Wine à examiner (par défaut celui de WiMao).
        #[arg(long, value_name = "DIR")]
        prefixe: Option<PathBuf>,
    },
    // Cherche les défauts de l'environnement.
    Diagnostiquer,
    // Répare les défauts trouvés par `diagnostiquer`.
    Reparer {
        // Ne demande pas confirmation avant de fermer les programmes ouverts.
        #[arg(long)]
        oui: bool,
    },
    // Points de restauration de l'environnement.
    #[command(subcommand)]
    Restauration(ActionRestauration),
    // Sauvegarde l'environnement sur un disque.
    Sauvegarder {
        // Dossier de destination (disque externe par exemple).
        #[arg(value_name = "DESTINATION")]
        destination: PathBuf,
        // Ne demande pas confirmation avant de fermer les programmes ouverts.
        #[arg(long)]
        oui: bool,
    },
    // Restaure l'environnement depuis une sauvegarde.
    Restaurer(ArgsRestaurer),
    // Imprime un rapport de problème anonymisé.
    Rapport,
    // Liste les recettes (gestionnaires d'éditeurs et plugins connus).
    Recettes,
    // Installe ou ouvre le gestionnaire d'un éditeur (Native Access…).
    #[command(subcommand)]
    Gestionnaire(ActionGestionnaire),
    // Affiche les dernières actions (installations, réparations, sauvegardes…).
    Historique {
        // Affiche toutes les actions, pas seulement les 20 dernières.
        #[arg(long)]
        tout: bool,
    },
    // Importe l'environnement Wine existant (`~/.wine-mao`) ou annule cet import.
    Importer {
        // Annule le dernier import : l'environnement revient dans `~/.wine-mao`.
        #[arg(long)]
        annuler: bool,
        // Ne demande pas confirmation.
        #[arg(long)]
        oui: bool,
    },
    // Transmet un lien de connexion (`native-access://…`) au gestionnaire dans Wine.
    RelayerLien {
        // Lien reçu du navigateur.
        #[arg(value_name = "URL")]
        url: String,
    },
}

/// Actions de `gestionnaire`.
#[derive(Debug, Subcommand)]
pub enum ActionGestionnaire {
    // Installe le gestionnaire d'une recette.
    Installer {
        // Nom de la recette (voir `recettes`) ou de son fichier sans `.toml`.
        #[arg(value_name = "NAME")]
        nom: String,
        // Installeur déjà téléchargé (sinon la recette le télécharge).
        #[arg(long, value_name = "FILE")]
        fichier: Option<PathBuf>,
        // Ne demande pas confirmation avant de remplacer le gestionnaire d'un lien.
        #[arg(long)]
        oui: bool,
    },
    // Ouvre le gestionnaire et surveille ses installations jusqu'à sa fermeture.
    Ouvrir {
        // Nom de la recette (voir `recettes`) ou de son fichier sans `.toml`.
        #[arg(value_name = "NAME")]
        nom: String,
        // Ne teste pas l'ouverture des plugins installés pendant la session.
        #[arg(long)]
        sans_test: bool,
        // Ne demande pas confirmation avant de fermer les programmes ouverts.
        #[arg(long)]
        oui: bool,
    },
}

/// Arguments de `installer`.
#[derive(Debug, Args)]
pub struct ArgsInstaller {
    // Installeur ou archive à installer.
    #[arg(value_name = "FILE")]
    pub fichier: PathBuf,
    // Ouvre l'installeur normalement au lieu de le lancer en silence.
    #[arg(long)]
    pub interactif: bool,
    // Ne teste pas l'ouverture des plugins installés.
    #[arg(long)]
    pub sans_test: bool,
    // Ne demande pas confirmation avant de fermer les programmes ouverts.
    #[arg(long)]
    pub oui: bool,
}

/// Arguments de `restaurer`.
#[derive(Debug, Args)]
pub struct ArgsRestaurer {
    // Fichier `.sha256` rendu par `sauvegarder`.
    #[arg(value_name = "FILE.sha256")]
    pub archive: PathBuf,
    // Ne demande pas confirmation.
    #[arg(long)]
    pub oui: bool,
}

/// Actions de `restauration`.
#[derive(Debug, Subcommand)]
pub enum ActionRestauration {
    // Liste les points de restauration.
    Lister,
    // Remet l'environnement dans l'état d'un point.
    Revenir {
        // Identifiant du point (voir `restauration lister`).
        #[arg(value_name = "ID")]
        id: String,
        // Ne demande pas confirmation.
        #[arg(long)]
        oui: bool,
    },
}

/// La description `clap` de [`Cli`] avec tous les textes d'aide traduits (langue courante).
///
/// `mut_subcommand` et `mut_arg` échouent (panique) si un nom n'existe pas : le test
/// `tous_les_textes_d_aide_sont_presents` attrape donc un nom oublié ou renommé.
pub fn commande_traduite() -> clap::Command {
    Cli::command()
        .about(tr(
            "Run Windows audio plugins on Linux with Wine and yabridge",
        ))
        .mut_arg("langue", |a| {
            a.help(tr("Language of the messages (default: system language)"))
        })
        .mut_subcommand("init", |s| s.about(tr("Create the Wine environment")))
        .mut_subcommand("installer", |s| {
            s.about(tr("Install a plugin from its installer or archive"))
                .mut_arg("fichier", |a| a.help(tr("Installer or archive to install")))
                .mut_arg("interactif", |a| {
                    a.help(tr(
                        "Open the installer normally instead of running it silently",
                    ))
                })
                .mut_arg("sans_test", |a| {
                    a.help(tr("Do not test that the installed plugins open"))
                })
                .mut_arg("oui", |a| {
                    a.help(tr("Do not ask before closing the open programs"))
                })
        })
        .mut_subcommand("detecter", |s| {
            s.about(tr("Recognise the type of an installer without running it"))
                .mut_arg("fichier", |a| a.help(tr("File to examine")))
        })
        .mut_subcommand("verifier", |s| {
            s.about(tr("Test that each already bridged plugin opens"))
                .mut_arg("prefixe", |a| {
                    a.help(tr("Wine environment to examine (default: WiMao's own)"))
                })
        })
        .mut_subcommand("diagnostiquer", |s| {
            s.about(tr("Look for problems in the Wine environment"))
        })
        .mut_subcommand("reparer", |s| {
            s.about(tr("Repair the problems found by the diagnosis"))
                .mut_arg("oui", |a| {
                    a.help(tr("Do not ask before closing the open programs"))
                })
        })
        .mut_subcommand("restauration", |s| {
            s.about(tr("Restore points of the Wine environment"))
                .mut_subcommand("lister", |s| s.about(tr("List the restore points")))
                .mut_subcommand("revenir", |s| {
                    s.about(tr(
                        "Put the environment back in the state of a restore point",
                    ))
                    .mut_arg("id", |a| {
                        a.help(tr("Restore point identifier (see the \"lister\" command)"))
                    })
                    .mut_arg("oui", |a| a.help(tr("Do not ask for confirmation")))
                })
        })
        .mut_subcommand("sauvegarder", |s| {
            s.about(tr("Back up the Wine environment to a disk"))
                .mut_arg("destination", |a| {
                    a.help(tr("Destination folder (an external disk, for example)"))
                })
                .mut_arg("oui", |a| {
                    a.help(tr("Do not ask before closing the open programs"))
                })
        })
        .mut_subcommand("restaurer", |s| {
            s.about(tr("Restore the Wine environment from a backup"))
                .mut_arg("archive", |a| {
                    a.help(tr("The .sha256 file written by the backup"))
                })
                .mut_arg("oui", |a| a.help(tr("Do not ask for confirmation")))
        })
        .mut_subcommand("rapport", |s| {
            s.about(tr("Print an anonymised problem report"))
        })
        .mut_subcommand("recettes", |s| {
            s.about(tr(
                "List the recipes (publisher managers and known plugins)",
            ))
        })
        .mut_subcommand("gestionnaire", |s| {
            s.about(tr("Install or open a publisher's manager (Native Access…)"))
                .mut_subcommand("installer", |s| {
                    s.about(tr("Install the manager of a recipe"))
                        .mut_arg("nom", |a| {
                            a.help(tr(
                                "Recipe name (see \"recettes\") or its file name without .toml",
                            ))
                        })
                        .mut_arg("fichier", |a| {
                            a.help(tr(
                                "Installer already downloaded (otherwise the recipe downloads it)",
                            ))
                        })
                        .mut_arg("oui", |a| {
                            a.help(tr(
                                "Do not ask before replacing the program that handles the link",
                            ))
                        })
                })
                .mut_subcommand("ouvrir", |s| {
                    s.about(tr(
                        "Open the manager and watch its installations until it is closed",
                    ))
                    .mut_arg("nom", |a| {
                        a.help(tr(
                            "Recipe name (see \"recettes\") or its file name without .toml",
                        ))
                    })
                    .mut_arg("sans_test", |a| {
                        a.help(tr(
                            "Do not test that the plugins installed in the session open",
                        ))
                    })
                    .mut_arg("oui", |a| {
                        a.help(tr("Do not ask before closing the open programs"))
                    })
                })
        })
        .mut_subcommand("historique", |s| {
            s.about(tr(
                "Show the latest actions (installations, repairs, backups…)",
            ))
            .mut_arg("tout", |a| {
                a.help(tr("Show every action, not only the latest 20"))
            })
        })
        .mut_subcommand("importer", |s| {
            s.about(tr(
                "Import the existing Wine environment (~/.wine-mao) into WiMao",
            ))
            .mut_arg("annuler", |a| {
                a.help(tr(
                    "Cancel the last import: the environment goes back to ~/.wine-mao",
                ))
            })
            .mut_arg("oui", |a| a.help(tr("Do not ask for confirmation")))
        })
        .mut_subcommand("relayer-lien", |s| {
            s.about(tr(
                "Pass a sign-in link (native-access://…) to the manager in Wine",
            ))
            .mut_arg("url", |a| a.help(tr("Link received from the browser")))
        })
}

/// Code de sortie d'une erreur d'usage : 1, comme toute erreur. `clap` sortirait en 2, code
/// que WiMao réserve à « au moins un plugin ❌ » (voir `sortie::code_de_sortie`).
const CODE_ERREUR_USAGE: i32 = 1;

/// Analyse les arguments du processus avec les textes traduits.
///
/// `--help` et `--version` écrivent leur texte sur stdout et terminent avec le code 0 ; une
/// erreur d'usage (option inconnue, valeur refusée…) écrit le message de `clap` sur stderr
/// et termine avec le code 1.
pub fn analyser() -> Cli {
    let resultat = commande_traduite()
        .try_get_matches()
        .and_then(|m| Cli::from_arg_matches(&m));
    match resultat {
        Ok(cli) => cli,
        Err(erreur) => {
            let code = if erreur.use_stderr() {
                CODE_ERREUR_USAGE
            } else {
                0
            };
            // Si l'écriture échoue (sortie fermée), on termine quand même avec le bon code.
            let _ = erreur.print();
            std::process::exit(code)
        }
    }
}

/// Cherche `--langue` dans les arguments bruts, avant l'analyse par `clap`.
///
/// Nécessaire parce que les textes d'aide appellent `tr()` : la langue doit déjà être
/// réglée quand on les construit. Accepte `--langue fr` et `--langue=fr` ; une valeur
/// invalide sera refusée ensuite par `clap`.
pub fn langue_demandee(args: &[String]) -> Option<String> {
    let mut suivants = args.iter().skip(1);
    while let Some(arg) = suivants.next() {
        if let Some(valeur) = arg.strip_prefix("--langue=") {
            return Some(valeur.to_owned());
        }
        if arg == "--langue" {
            return suivants.next().cloned();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(liste: &[&str]) -> Vec<String> {
        liste.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn la_langue_est_lue_avant_clap() {
        let lue = |l: &[&str]| langue_demandee(&args(l));
        assert_eq!(lue(&["w", "--langue", "es", "rapport"]), Some("es".into()));
        assert_eq!(lue(&["w", "rapport", "--langue=fr"]), Some("fr".into()));
        assert_eq!(lue(&["w", "rapport"]), None);
        assert_eq!(lue(&["w", "--langue"]), None);
    }

    /// Parcourt récursivement les sous-commandes et leurs arguments.
    fn verifier_aide(cmd: &clap::Command, chemin: &str) {
        for sous in cmd.get_subcommands().filter(|s| s.get_name() != "help") {
            let nom = format!("{chemin} {}", sous.get_name());
            assert!(sous.get_about().is_some(), "pas d'aide pour « {nom} »");
            verifier_aide(sous, &nom);
        }
        for arg in cmd.get_arguments() {
            let id = arg.get_id().as_str();
            if !matches!(id, "help" | "version") {
                assert!(arg.get_help().is_some(), "pas d'aide pour {chemin} --{id}");
            }
        }
    }

    #[test]
    fn tous_les_textes_d_aide_sont_presents() {
        let mut cmd = commande_traduite();
        cmd.build();
        assert!(cmd.get_about().is_some());
        verifier_aide(&cmd, "wimao-cli");
        cmd.debug_assert();
    }
}
