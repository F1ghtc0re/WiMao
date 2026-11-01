//! Question « o/N » posée avant une action qui modifie ou met de côté l'environnement.
//!
//! Utilisé par `commandes::restauration` (`revenir`), `commandes::sauvegarde`
//! (`restaurer`), `commandes::import` et, via `commandes::accord_avant_arret`, par
//! `installer`, `gestionnaire ouvrir` et `sauvegarder`. La lecture est injectée (`BufRead`)
//! pour être testable sans terminal.

use std::io::{BufRead, Write};

use wimao_core::i18n::{tr, trf};

/// Réponses qui valent « oui » (insensible à la casse) : français, anglais, espagnol.
/// L'abréviation « s » est celle de « sí ».
const REPONSES_OUI: [&str; 6] = ["o", "oui", "y", "yes", "s", "si"];

/// Dit si `reponse` (une ligne saisie) veut dire « oui ». Toute autre saisie, y compris
/// une ligne vide (le choix par défaut, « N »), veut dire « non ».
pub fn est_oui(reponse: &str) -> bool {
    let reponse = reponse.trim().to_lowercase();
    REPONSES_OUI.contains(&reponse.as_str())
}

/// Demande confirmation. Si `deja_confirme` (option `--oui`), ne pose pas la question.
///
/// La question est écrite sur la sortie standard, suivie de l'invite traduite
/// « [o/N] ». Une entrée fermée ou illisible compte pour « non » : dans le doute on
/// ne modifie rien.
pub fn confirmer(
    question: &str,
    deja_confirme: bool,
    entree: &mut impl BufRead,
    sortie: &mut impl Write,
) -> bool {
    if deja_confirme {
        return true;
    }
    // Un échec d'écriture sur la sortie n'empêche pas de lire la réponse.
    let _ = write!(sortie, "{question} {} ", tr("[y/N]"));
    let _ = sortie.flush();
    let mut ligne = String::new();
    match entree.read_line(&mut ligne) {
        Ok(_) => est_oui(&ligne),
        Err(_) => false,
    }
}

/// Nomme les `programmes` ouverts que l'action fermerait (elle arrête Wine) et demande si on
/// continue. Texte générique : l'installation, l'ouverture d'un gestionnaire, la sauvegarde et
/// l'import s'en servent ; `reparer` garde le sien, qui parle de réparation.
pub fn confirmer_fermeture(
    programmes: &[String],
    oui: bool,
    entree: &mut impl BufRead,
    sortie: &mut impl Write,
) -> bool {
    let question = trf(
        "Wine will be stopped and these open programs will be closed: {programs}. Continue?",
        &[("programs", &programmes.join(", "))],
    );
    confirmer(&question, oui, entree, sortie)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seules_les_reponses_positives_sont_acceptees() {
        for oui in ["o", "O\n", "oui", " Yes ", "s", "SI"] {
            assert!(est_oui(oui), "{oui:?}");
        }
        for non in ["", "\n", "n", "non", "peut-être"] {
            assert!(!est_oui(non), "{non:?}");
        }
    }

    #[test]
    fn l_option_oui_ne_pose_pas_la_question() {
        let mut sortie = Vec::new();
        assert!(confirmer("Q ?", true, &mut "".as_bytes(), &mut sortie));
        assert!(sortie.is_empty());
    }

    #[test]
    fn la_question_de_fermeture_nomme_les_programmes() {
        let programmes = vec!["Serum.exe".to_string(), "Kontakt.exe".to_string()];
        let mut sortie = Vec::new();
        assert!(!confirmer_fermeture(
            &programmes,
            false,
            &mut "n\n".as_bytes(),
            &mut sortie
        ));
        let texte = String::from_utf8(sortie).unwrap();
        assert!(texte.contains("Serum.exe, Kontakt.exe"), "{texte}");
        assert!(confirmer_fermeture(
            &programmes,
            true,
            &mut "".as_bytes(),
            &mut Vec::new()
        ));
    }

    #[test]
    fn une_entree_vide_ou_fermee_vaut_non() {
        let mut sortie = Vec::new();
        assert!(!confirmer("Q ?", false, &mut "".as_bytes(), &mut sortie));
        assert!(!confirmer("Q ?", false, &mut "\n".as_bytes(), &mut sortie));
        assert!(confirmer("Q ?", false, &mut "o\n".as_bytes(), &mut sortie));
        assert!(String::from_utf8(sortie).unwrap().contains("Q ?"));
    }
}
