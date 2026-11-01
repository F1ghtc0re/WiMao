//! Sous-commandes `sauvegarder` et `restaurer`.
//!
//! Utilise `wimao_core::sauvegarde` et `confirmation`. Utilisé par `commandes::executer`.

use std::io::{stdin, stdout};
use std::path::Path;

use wimao_core::i18n::{tr, trf};
use wimao_core::sauvegarde::{restaurer as restaurer_archive, sauvegarder as sauvegarder_dans};
use wimao_core::{Chemins, Erreur, Resultat};

use super::{accord_avant_arret, Issue};
use crate::cli::ArgsRestaurer;
use crate::confirmation::confirmer;
use crate::progression_terminal::ProgressionTerminal;
use crate::sortie::afficher;

/// `sauvegarder` : écrit la sauvegarde et imprime le fichier `.sha256` à garder.
///
/// La sauvegarde arrête Wine : les programmes ouverts sont nommés avant, et un refus (sauf
/// `--oui`) termine la commande sans rien faire.
pub fn sauvegarder(c: &Chemins, destination: &Path, oui: bool) -> Resultat<Issue> {
    if !accord_avant_arret(c, oui)? {
        return Ok(Issue::Succes);
    }
    let fichier = sauvegarder_dans(c, destination, &ProgressionTerminal)?;
    afficher!(
        "{}",
        trf(
            "Backup written. To restore it, keep this file: {path}",
            &[("path", &fichier.display().to_string())]
        )
    );
    Ok(Issue::Succes)
}

/// `restaurer` : après confirmation, remplace l'environnement actuel par la sauvegarde.
///
/// Le fichier `.sha256` est vérifié **avant** de poser la question : inutile de demander
/// une confirmation pour une opération qui échouerait aussitôt. La question dit ce qui va
/// se passer : l'environnement actuel n'est pas supprimé, il est mis de côté dans
/// `<prefixe>.ancien-<date>`. Une annulation n'est pas une erreur (code 0).
pub fn restaurer(c: &Chemins, args: &ArgsRestaurer) -> Resultat<Issue> {
    if !args.archive.is_file() {
        return Err(Erreur::Introuvable(trf(
            "the backup file {path} does not exist",
            &[("path", &args.archive.display().to_string())],
        )));
    }
    afficher!(
        "{}",
        trf(
            "The current environment will be set aside as {path}.ancien-<date> (nothing is deleted), then replaced by the backup.",
            &[("path", &c.prefixe.display().to_string())]
        )
    );
    let question = tr("Continue?");
    if !confirmer(&question, args.oui, &mut stdin().lock(), &mut stdout()) {
        afficher!("{}", tr("Cancelled: nothing was changed"));
        return Ok(Issue::Succes);
    }
    restaurer_archive(c, &args.archive, &ProgressionTerminal)?;
    afficher!("{}", tr("Backup restored"));
    Ok(Issue::Succes)
}
