//! « Restore a backup… » et « Cancel the import » dans la fenêtre.
//!
//! - [`resume_archive`] (fonction pure, testée) : ce qu'on montre avant de restaurer, lu dans
//!   le `.sha256` choisi : nom de l'archive, date (tirée du nom) et taille totale ;
//! - [`Fenetre::demander_restauration`] : sélecteur `*.sha256`, dialogue de confirmation
//!   (style destructif), question « arrêter Wine ? », restauration en arrière-plan, toast puis
//!   relevé ;
//! - [`Fenetre::demander_annulation_import`] : même parcours pour l'annulation de l'import, puis
//!   la fenêtre se ferme et l'application est réactivée ;
//! - [`Fenetre::actualiser_menu_import`] : remplit la section du menu principal (« Restore a
//!   backup… » toujours, « Cancel the import » seulement si l'annulation est possible).
//!
//! Utilise `dialogues.rs` (sélecteur), `operations.rs` (appels au noyau, dans le fil de travail)
//! et `fenetre/` (`lancer`, `avant_d_arreter_wine`, toasts). Utilisé par `fenetre/actions.rs`
//! (les actions `win.restaurer` et `win.annuler-import`) et `fenetre/mod.rs` (menu, relevé).
//!
//! Choix non évidents :
//! - le résumé s'appuie sur `sauvegarde::pieces_listees` du noyau (même lecture du `.sha256`,
//!   mêmes refus que la restauration) mais ne vérifie **pas** les empreintes : le noyau le fait
//!   avant de toucher à quoi que ce soit, ce qui peut durer ;
//! - le résumé tourne dans le fil de travail (disque externe possible, donc lent) ;
//! - après l'annulation de l'import, WiMao n'a plus d'environnement : on ferme la fenêtre et on
//!   rappelle `activate`, dont la logique de démarrage choisit la bonne fenêtre (tâche 12).

use std::path::Path;

use gtk4::{gio, prelude::*};
use libadwaita as adw;
use libadwaita::prelude::*;
use wimao_core::i18n::{tr, trf};
use wimao_core::import::{ancien_emplacement, annulation_possible};
use wimao_core::sauvegarde::pieces_listees;
use wimao_core::{Erreur, Resultat};

use crate::dialogues;
use crate::fenetre::Fenetre;
use crate::operations;

/// Action de la fenêtre qui lance la restauration.
pub const ACTION_RESTAURER: &str = "restaurer";
/// Action de la fenêtre qui annule l'import.
pub const ACTION_ANNULER_IMPORT: &str = "annuler-import";

/// Identifiant de la réponse « annuler » des dialogues (rien ne change).
const REPONSE_ANNULER: &str = "annuler";
/// Identifiant de la réponse « restaurer ».
const REPONSE_RESTAURER: &str = "restaurer";
/// Identifiant de la réponse « annuler l'import ».
const REPONSE_ANNULER_IMPORT: &str = "annuler-import";
/// Début du nom d'une archive faite par WiMao (`wimao-AAAA-MM-JJ-HHhMM.tar.zst`).
const PREFIXE_ARCHIVE: &str = "wimao-";

/// Ce que la confirmation montre d'une sauvegarde.
#[derive(Debug, PartialEq, Eq)]
pub struct ResumeArchive {
    /// Nom de l'archive (sans le suffixe `.partNNN` d'un morceau).
    pub nom: String,
    /// `AAAA-MM-JJ HH:MM`, tiré du nom `wimao-AAAA-MM-JJ-HHhMM…` ; `None` si le nom a une
    /// autre forme (archive renommée à la main, par exemple).
    pub date: Option<String>,
    /// Somme des tailles des morceaux listés dans le `.sha256`, en octets.
    pub taille: u64,
}

/// Résume la sauvegarde décrite par `sha256` (un fichier au format de `sha256sum` : une ligne
/// « empreinte  nom » par morceau, les morceaux étant à côté du `.sha256`).
///
/// # Erreurs
/// Celles de [`pieces_listees`] (`.sha256` illisible ou invalide, morceau absent) et
/// [`wimao_core::Erreur::Io`] si la taille d'un morceau est illisible.
pub fn resume_archive(sha256: &Path) -> Resultat<ResumeArchive> {
    // Lecture, forme du `.sha256`, noms sûrs, morceaux sans trou, fichiers présents : tout est
    // fait par le noyau, comme avant une vraie restauration (sans calculer les empreintes).
    let pieces = pieces_listees(sha256)?;
    let mut taille = 0;
    for piece in &pieces {
        taille += std::fs::metadata(piece)?.len();
    }
    // Le noyau garantit au moins un fichier, au nom sans `/`.
    let premier = pieces
        .first()
        .and_then(|p| p.file_name())
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let nom = nom_sans_morceau(&premier).to_owned();
    Ok(ResumeArchive {
        date: date_du_nom(&nom),
        nom,
        taille,
    })
}

/// `archive.tar.zst.part003` -> `archive.tar.zst` ; un nom sans suffixe de morceau reste tel quel.
fn nom_sans_morceau(nom: &str) -> &str {
    match nom.rsplit_once(".part") {
        Some((debut, numero))
            if !numero.is_empty() && numero.bytes().all(|b| b.is_ascii_digit()) =>
        {
            debut
        }
        _ => nom,
    }
}

/// `wimao-2026-11-01-14h30.tar.zst` -> `Some("2026-11-01 14:30")` ; toute autre forme -> `None`.
fn date_du_nom(nom: &str) -> Option<String> {
    // `AAAA-MM-JJ-HHhMM` : 16 octets, chiffres et séparateurs à des places fixes.
    let motif = nom.strip_prefix(PREFIXE_ARCHIVE)?.as_bytes().get(..16)?;
    let attendu = b"dddd-dd-dd-ddhdd";
    let conforme = motif
        .iter()
        .zip(attendu)
        .all(|(&lu, &modele)| match modele {
            b'd' => lu.is_ascii_digit(),
            autre => lu == autre,
        });
    conforme.then(|| {
        // Les 16 octets sont de l'ASCII : la conversion ne peut pas échouer.
        let t = String::from_utf8_lossy(motif);
        format!("{} {}:{}", &t[..10], &t[11..13], &t[14..16])
    })
}

// --- Dans la fenêtre -------------------------------------------------------------------------

impl Fenetre {
    /// Remplit la section « sauvegarde et import » du menu principal : « Restore a backup… »
    /// toujours ; « Cancel the import » seulement si [`annulation_possible`]. Un élément de
    /// menu ne peut pas être caché seul : on reconstruit la section. À rappeler après tout ce
    /// qui change la trace de l'import (la lecture est de quelques octets, sans danger pour le
    /// fil de GTK).
    pub(crate) fn actualiser_menu_import(&self) {
        self.menu_import.remove_all();
        self.menu_import.append(
            Some(&tr("Restore a backup…")),
            Some(&format!("win.{ACTION_RESTAURER}")),
        );
        if annulation_possible(&self.chemins) {
            self.menu_import.append(
                Some(&tr("Cancel the import")),
                Some(&format!("win.{ACTION_ANNULER_IMPORT}")),
            );
        }
    }

    /// « Restore a backup… » : sélecteur `.sha256`, résumé de la sauvegarde (dans le fil de
    /// travail : le disque peut être externe et lent), puis confirmation.
    pub(crate) fn demander_restauration(&self) {
        let moi = self.clone();
        dialogues::choisir_sauvegarde(&self.fenetre, move |sha256| {
            let (suite, chemin) = (moi.clone(), sha256.clone());
            moi.lancer(
                &tr("Reading the backup…"),
                move |_| resume_archive(&sha256),
                move |resultat| match resultat {
                    Ok(resume) => suite.confirmer_restauration(chemin, &resume),
                    Err(erreur) => suite.erreur(&erreur, false),
                },
            );
        });
    }

    /// Dialogue de confirmation : nom, date, taille et ce qui arrive à l'environnement actuel.
    /// « Cancel » (et Échap) ne change rien ; « Restore » est destructif par apparence, mais
    /// l'environnement actuel est gardé en copie.
    fn confirmer_restauration(&self, sha256: std::path::PathBuf, resume: &ResumeArchive) {
        let mut lignes = vec![trf("Backup: {name}", &[("name", &resume.nom)])];
        if let Some(date) = &resume.date {
            lignes.push(trf("Date: {date}", &[("date", date)]));
        }
        let taille = gtk4::glib::format_size(resume.taille);
        lignes.push(trf("Size: {size}", &[("size", &taille)]));
        lignes.push(tr("Your current environment will be kept as a copy."));
        let dialogue =
            adw::AlertDialog::new(Some(&tr("Restore this backup?")), Some(&lignes.join("\n")));
        dialogue.add_response(REPONSE_ANNULER, &tr("Cancel"));
        dialogue.add_response(REPONSE_RESTAURER, &tr("Restore the backup"));
        dialogue.set_response_appearance(REPONSE_RESTAURER, adw::ResponseAppearance::Destructive);
        // Par défaut, l'issue sans danger : Entrée ne remplace rien par mégarde.
        dialogue.set_default_response(Some(REPONSE_ANNULER));
        dialogue.set_close_response(REPONSE_ANNULER);
        let moi = self.clone();
        dialogue.choose(
            Some(&self.fenetre),
            None::<&gio::Cancellable>,
            move |reponse| {
                if reponse != REPONSE_RESTAURER {
                    return;
                }
                // Restaurer arrête Wine : même question que pour un installeur.
                let suite = moi.clone();
                moi.avant_d_arreter_wine(move || suite.restaurer(sha256));
            },
        );
    }

    /// Restaure dans le fil de travail. Après un échec aussi, on relit : le noyau a pu changer
    /// des fichiers avant d'échouer, et l'historique a une entrée de plus.
    fn restaurer(&self, sha256: std::path::PathBuf) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        self.lancer(
            &tr("Restoring the backup…"),
            move |p| operations::restaurer(&c, &sha256, p),
            move |resultat| {
                match resultat {
                    Ok(()) => moi.toast(&tr("Backup restored")),
                    Err(erreur) => moi.erreur(&erreur, true),
                }
                moi.rafraichir();
            },
        );
    }

    /// « Cancel the import » : confirmation (le dossier retourne à `~/.wine-mao`), puis
    /// annulation.
    pub(crate) fn demander_annulation_import(&self) {
        let ancien = ancien_emplacement(&self.chemins)
            .map_or_else(|| "~/.wine-mao".to_owned(), |p| p.display().to_string());
        let dialogue = adw::AlertDialog::new(
            Some(&tr("Cancel the import?")),
            Some(&trf(
                "The import will be cancelled: the environment goes back to {path} and the link is removed.",
                &[("path", &ancien)],
            )),
        );
        dialogue.add_response(REPONSE_ANNULER, &tr("Cancel"));
        dialogue.add_response(REPONSE_ANNULER_IMPORT, &tr("Cancel the import"));
        dialogue
            .set_response_appearance(REPONSE_ANNULER_IMPORT, adw::ResponseAppearance::Destructive);
        dialogue.set_default_response(Some(REPONSE_ANNULER));
        dialogue.set_close_response(REPONSE_ANNULER);
        let moi = self.clone();
        dialogue.choose(
            Some(&self.fenetre),
            None::<&gio::Cancellable>,
            move |reponse| {
                if reponse != REPONSE_ANNULER_IMPORT {
                    return;
                }
                let suite = moi.clone();
                moi.avant_d_arreter_wine(move || suite.annuler_import());
            },
        );
    }

    /// Annule l'import dans le fil de travail. Réussite : plus d'environnement, la fenêtre se
    /// ferme et `activate` choisit la suite. Échec : dialogue, et le menu est relu (la trace
    /// a pu changer).
    fn annuler_import(&self) {
        let (moi, c) = (self.clone(), (*self.chemins).clone());
        self.lancer(
            &tr("Cancelling the import…"),
            move |p| operations::annuler_import(&c, p),
            move |resultat| match resultat {
                Ok(()) => moi.apres_annulation_import(),
                Err(erreur) => {
                    moi.erreur(&erreur, false);
                    moi.actualiser_menu_import();
                    moi.rafraichir();
                }
            },
        );
    }

    /// Ferme la fenêtre puis rappelle `activate`. Le « hold » empêche l'application de se
    /// terminer pendant l'instant où elle n'a plus de fenêtre.
    fn apres_annulation_import(&self) {
        let Some(app) = self.fenetre.application() else {
            // Cas théorique (la fenêtre est toujours créée par l'application) : on le dit
            // plutôt que de laisser une fenêtre sur un environnement qui n'existe plus.
            self.erreur(
                &Erreur::Sauvegarde(tr("The import was cancelled; restart WiMao.")),
                false,
            );
            return;
        };
        let _garde = app.hold();
        self.fenetre.close();
        app.activate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une empreinte quelconque, de la bonne longueur.
    const H: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// Écrit un `.sha256` listant `fichiers` (créés avec les contenus donnés) dans `dossier` ;
    /// rend son chemin.
    fn sauvegarde_factice(dossier: &Path, fichiers: &[(&str, &[u8])]) -> std::path::PathBuf {
        let mut listing = String::new();
        for (nom, contenu) in fichiers {
            std::fs::write(dossier.join(nom), contenu).unwrap();
            listing.push_str(&format!("{H}  {nom}\n"));
        }
        let sha256 = dossier.join("wimao.sha256");
        std::fs::write(&sha256, listing).unwrap();
        sha256
    }

    #[test]
    fn resume_d_une_archive_en_un_morceau() {
        let d = tempfile::tempdir().unwrap();
        let sha = sauvegarde_factice(d.path(), &[("wimao-2026-11-01-14h30.tar.zst", &[0; 1234])]);
        assert_eq!(
            resume_archive(&sha).unwrap(),
            ResumeArchive {
                nom: "wimao-2026-11-01-14h30.tar.zst".to_owned(),
                date: Some("2026-11-01 14:30".to_owned()),
                taille: 1234,
            }
        );
    }

    #[test]
    fn resume_d_une_archive_en_morceaux() {
        let d = tempfile::tempdir().unwrap();
        let sha = sauvegarde_factice(
            d.path(),
            &[
                ("wimao-2026-11-01-14h30.tar.zst.part000", &[0; 1000]),
                ("wimao-2026-11-01-14h30.tar.zst.part001", &[0; 234]),
            ],
        );
        let resume = resume_archive(&sha).unwrap();
        assert_eq!(resume.taille, 1234, "somme des deux morceaux");
        assert_eq!(resume.nom, "wimao-2026-11-01-14h30.tar.zst");
        assert_eq!(resume.date.as_deref(), Some("2026-11-01 14:30"));
    }

    #[test]
    fn nom_inattendu_donne_une_date_absente() {
        let d = tempfile::tempdir().unwrap();
        for nom in ["ma-sauvegarde.tar.zst", "wimao-2026-11-01.tar.zst"] {
            let sha = sauvegarde_factice(d.path(), &[(nom, &[0; 10])]);
            let resume = resume_archive(&sha).unwrap();
            assert_eq!(resume.date, None, "{nom}");
            assert_eq!(resume.nom, nom);
            assert_eq!(resume.taille, 10);
        }
    }

    #[test]
    fn morceau_absent_ligne_invalide_ou_liste_vide_sont_refuses() {
        let d = tempfile::tempdir().unwrap();
        let sha = d.path().join("x.sha256");
        std::fs::write(&sha, format!("{H}  absent.tar.zst\n")).unwrap();
        assert!(resume_archive(&sha).is_err(), "morceau absent");
        std::fs::write(&sha, "pas une empreinte\n").unwrap();
        assert!(resume_archive(&sha).is_err(), "ligne invalide");
        std::fs::write(&sha, format!("{H}  ../dehors\n")).unwrap();
        assert!(resume_archive(&sha).is_err(), "nom hors du dossier");
        std::fs::write(&sha, "\n").unwrap();
        assert!(resume_archive(&sha).is_err(), "liste vide");
        assert!(resume_archive(&d.path().join("nexiste-pas")).is_err());
    }
}
