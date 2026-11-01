//! Point d'entrée de la ligne de commande `wimao-cli`.
//!
//! Rôle : régler la langue et le journal, analyser les arguments, lancer la sous-commande
//! et traduire son issue en code de sortie. Le détail est rangé dans :
//! - `cli` : description des options et sous-commandes (`clap`) ;
//! - `commandes/` : une fonction par sous-commande ;
//! - `progression_terminal`, `confirmation` : affichage et questions.
//!
//! Codes de sortie (voir `sortie`) : 0 succès, 1 erreur ou erreur d'usage (message traduit sur stderr, avec l'explication
//! d'une règle d'erreur si elle s'applique), 2 au moins un plugin ❌ (`verifier`,
//! `installer`). Utilise `wimao-core` et `wimao-recettes`. Voir `ARCHITECTURE.md`.

mod cli;
mod commandes;
mod confirmation;
mod progression_terminal;
mod sortie;

use std::process::ExitCode;

use wimao_core::i18n::init_langue;
use wimao_core::{journal, Chemins};

use cli::{langue_demandee, Commande};
use sortie::{afficher_erreur, code_de_sortie};

/// Nom du fichier journal, dans le dossier de données.
const FICHIER_JOURNAL: &str = "journal.log";

fn main() -> ExitCode {
    // La langue d'abord : `clap` construit ses textes d'aide avec `tr()` en analysant les arguments.
    let args: Vec<String> = std::env::args().collect();
    init_langue(langue_demandee(&args).as_deref());
    let cli = cli::analyser();

    let chemins = Chemins::depuis_env();
    if let Ok(c) = &chemins {
        preparer_journal(&cli.commande, c);
    }
    let resultat = commandes::executer(cli.commande, chemins);
    if let Err(erreur) = &resultat {
        afficher_erreur(erreur);
    }
    ExitCode::from(code_de_sortie(&resultat))
}

/// Active le journal des commandes lancées, dans le dossier de données.
///
/// Une erreur de journal ne fait jamais échouer la commande (`initialiser` ne renvoie
/// rien, et les erreurs d'écriture sont signalées une fois par le journal lui-même).
/// Exceptions : `verifier --prefixe` ne doit rien écrire dans les dossiers de WiMao, et
/// `relayer-lien`, lancé par le bureau à chaque lien reçu, doit rester instantané et sans
/// effet de bord : ni l'un ni l'autre n'a de journal.
fn preparer_journal(commande: &Commande, c: &Chemins) {
    if matches!(
        commande,
        Commande::Verifier { prefixe: Some(_) } | Commande::RelayerLien { .. }
    ) {
        return;
    }
    journal::initialiser(c.donnees.join(FICHIER_JOURNAL));
}
