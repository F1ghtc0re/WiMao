//! Outils communs aux correctifs qui remplacent des DLL de Wine (DXVK, msvcp140).
//!
//! Utilise [`Chemins`], [`Wine`] (registre) et [`empreinte`]. Utilisé par
//! `dxvk` et `msvcp` : ils posent des DLL de Microsoft ou de DXVK dans
//! `system32` (64 bits) et `syswow64` (32 bits) de l'environnement, sauvegardent
//! l'original de Wine dans `<donnees>/originaux/{system32,syswow64}/` et
//! déclarent l'ordre de chargement `native,builtin`, exactement comme le
//! faisaient `scripts/install-dxvk.sh` et `scripts/install-msvcp140.sh`.

use crate::chemins::Chemins;
use crate::erreur::Resultat;
use crate::registre_fichier::lire_valeur;
use crate::telechargement::empreinte;
use crate::wine::Wine;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::time::Duration;

/// Dossier des DLL 64 bits de l'environnement (sous `drive_c/windows`).
pub(super) const SYSTEM32: &str = "system32";
/// Dossier des DLL 32 bits de l'environnement (sous `drive_c/windows`) : Wine y
/// range celles des programmes 32 bits, d'où le nom trompeur « syswow64 ».
pub(super) const SYSWOW64: &str = "syswow64";
/// Clé du registre où Wine lit l'ordre de chargement des DLL.
const CLE_SURCHARGES: &str = r"HKCU\Software\Wine\DllOverrides";
/// La même clé, telle que la lit `registre_fichier` (relative à `HKEY_CURRENT_USER`).
const CLE_SURCHARGES_FICHIER: &str = r"Software\Wine\DllOverrides";
/// Ordre de chargement voulu : la DLL posée par nous d'abord, celle de Wine en
/// secours. Sans cette valeur Wine garderait sa version intégrée.
pub(super) const SURCHARGE_NATIVE: &str = "native,builtin";
/// Dossier, sous les données de WiMao, des DLL d'origine de Wine.
const DOSSIER_ORIGINAUX: &str = "originaux";

/// `<prefixe>/drive_c/windows/<sous>` (`sous` : [`SYSTEM32`] ou [`SYSWOW64`]).
pub(super) fn dossier_windows(c: &Chemins, sous: &str) -> PathBuf {
    c.drive_c().join("windows").join(sous)
}

/// `<donnees>/originaux/<sous>` : où sont gardées les DLL d'origine.
pub(super) fn dossier_originaux(c: &Chemins, sous: &str) -> PathBuf {
    c.donnees.join(DOSSIER_ORIGINAUX).join(sous)
}

/// Copie `source` vers `dest` en remplaçant `dest`, de façon atomique.
///
/// 1. un `<dest>.part` laissé par une copie interrompue est supprimé ;
/// 2. `source` est copié vers `<dest>.part`, puis `sync_all` le force sur le disque ;
/// 3. `<dest>.part` est renommé en `dest`.
///
/// Le renommage remplace `dest` d'un coup : si la copie échoue (disque plein, source
/// absente), `dest` garde son contenu au lieu de disparaître. Et s'il était un lien
/// symbolique, `rename` remplace le lien lui-même sans suivre sa cible : une copie
/// directe écrirait dans la cible (potentiellement les fichiers de Wine, partagés par
/// tous les environnements).
pub(super) fn remplacer_fichier(source: &Path, dest: &Path) -> Resultat<()> {
    let mut nom_part = dest.as_os_str().to_owned();
    nom_part.push(".part");
    let part = PathBuf::from(nom_part);
    match std::fs::remove_file(&part) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if let Err(e) = copier_et_synchroniser(source, &part) {
        // Ne laisse pas de fichier partiel derrière soi (l'erreur d'origine prime).
        let _ = std::fs::remove_file(&part);
        return Err(e.into());
    }
    if let Err(e) = std::fs::rename(&part, dest) {
        let _ = std::fs::remove_file(&part);
        return Err(e.into());
    }
    Ok(())
}

/// Copie `source` vers `cible` et force l'écriture sur le disque.
fn copier_et_synchroniser(source: &Path, cible: &Path) -> std::io::Result<()> {
    std::fs::copy(source, cible)?;
    std::fs::File::open(cible)?.sync_all()
}

/// Garde la DLL d'origine `<dll>.dll` de `sous` dans `<donnees>/originaux/<sous>/`.
///
/// Ne remplace jamais une copie existante : lors d'une deuxième application, le
/// fichier du préfixe est déjà celui du correctif, et l'écraser perdrait
/// définitivement l'original. Sans fichier d'origine dans le préfixe, ne fait rien.
pub(super) fn sauvegarder_original(c: &Chemins, sous: &str, dll: &str) -> Resultat<()> {
    let nom = format!("{dll}.dll");
    let copie = dossier_originaux(c, sous).join(&nom);
    let actuel = dossier_windows(c, sous).join(&nom);
    if copie.exists() || !actuel.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dossier_originaux(c, sous))?;
    std::fs::copy(actuel, copie)?;
    Ok(())
}

/// Remet la DLL d'origine sauvegardée. Vrai si une copie existait, faux sinon
/// (le correctif n'a jamais été appliqué : le fichier du préfixe est laissé tel quel).
pub(super) fn restaurer_original(c: &Chemins, sous: &str, dll: &str) -> Resultat<bool> {
    let nom = format!("{dll}.dll");
    let copie = dossier_originaux(c, sous).join(&nom);
    if !copie.exists() {
        return Ok(false);
    }
    remplacer_fichier(&copie, &dossier_windows(c, sous).join(nom))?;
    Ok(true)
}

/// Vrai si les deux fichiers existent et ont la même empreinte SHA-256.
/// Un fichier absent n'est identique à rien (pas une erreur).
pub(super) fn fichiers_identiques(a: &Path, b: &Path) -> Resultat<bool> {
    if !a.exists() || !b.exists() {
        return Ok(false);
    }
    Ok(empreinte(a)? == empreinte(b)?)
}

/// Déclare `<dll>` en `native,builtin` dans le registre de l'environnement.
pub(super) fn poser_surcharge(wine: &Wine, dll: &str) -> Resultat<()> {
    wine.reg_ajouter(CLE_SURCHARGES, dll, SURCHARGE_NATIVE)
}

/// Vrai si `<dll>` est déclarée `native,builtin`.
///
/// Lit `user.reg` directement, sans lancer Wine (voir `registre_fichier`) : la valeur
/// n'y est qu'après l'arrêt du `wineserver`, que `appliquer` et `retirer` provoquent.
pub(super) fn surcharge_est_native(c: &Chemins, dll: &str) -> Resultat<bool> {
    let lue = lire_valeur(&c.prefixe.join("user.reg"), CLE_SURCHARGES_FICHIER, dll)?;
    Ok(lue.as_deref() == Some(SURCHARGE_NATIVE))
}

/// Supprime la déclaration de `<dll>` ; sans effet si elle n'existe pas
/// (`reg delete` échouerait, ce qui rendrait « Réparer » impossible à répéter).
pub(super) fn retirer_surcharge(wine: &Wine, dll: &str) -> Resultat<()> {
    if wine.reg_lire(CLE_SURCHARGES, dll)?.is_some() {
        wine.reg_supprimer(CLE_SURCHARGES, dll)?;
    }
    Ok(())
}

/// Aide des tests avec Wine : environnement neuf (`wineboot -u` sans Mono ni Gecko)
/// sous `racine`, sans aucun correctif, pour comparer avec les DLL d'origine.
/// Le `wineserver` est arrêté avant de rendre la main (écriture des `.reg`).
#[cfg(test)]
pub(super) fn prefixe_nu_pour_test(racine: &Path) -> Chemins {
    let c = Chemins::pour_test(racine);
    std::fs::create_dir_all(&c.home).unwrap();
    std::fs::create_dir_all(&c.prefixe).unwrap();
    let mut cmd = Wine::nouveau(&c)
        .commande_avec("wineboot", &[crate::wine::SURCHARGES_SANS_FENETRES])
        .unwrap();
    cmd.arg("-u");
    let sortie = crate::outils::lancer(&mut cmd, Duration::from_secs(180)).unwrap();
    assert_eq!(sortie.code, 0, "{}", sortie.texte);
    Wine::nouveau(&c).arreter().unwrap();
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chemins::Chemins;

    fn preparer(racine: &std::path::Path) -> Chemins {
        let c = Chemins::pour_test(racine);
        for sous in [SYSTEM32, SYSWOW64] {
            std::fs::create_dir_all(dossier_windows(&c, sous)).unwrap();
        }
        c
    }

    #[test]
    fn la_sauvegarde_ne_remplace_pas_une_copie_existante() {
        let d = tempfile::tempdir().unwrap();
        let c = preparer(d.path());
        let dans_prefixe = dossier_windows(&c, SYSTEM32).join("x.dll");
        std::fs::write(&dans_prefixe, b"original").unwrap();
        sauvegarder_original(&c, SYSTEM32, "x").unwrap();
        // Le fichier du préfixe est remplacé (par un correctif), puis sauvegardé à nouveau.
        std::fs::write(&dans_prefixe, b"remplace").unwrap();
        sauvegarder_original(&c, SYSTEM32, "x").unwrap();
        let copie = dossier_originaux(&c, SYSTEM32).join("x.dll");
        assert_eq!(std::fs::read(copie).unwrap(), b"original");
    }

    #[test]
    fn la_restauration_remet_l_original() {
        let d = tempfile::tempdir().unwrap();
        let c = preparer(d.path());
        let dans_prefixe = dossier_windows(&c, SYSWOW64).join("x.dll");
        std::fs::write(&dans_prefixe, b"original").unwrap();
        sauvegarder_original(&c, SYSWOW64, "x").unwrap();
        std::fs::write(&dans_prefixe, b"remplace").unwrap();
        assert!(restaurer_original(&c, SYSWOW64, "x").unwrap());
        assert_eq!(std::fs::read(&dans_prefixe).unwrap(), b"original");
        // Sans copie : rien à restaurer, le fichier reste tel quel.
        assert!(!restaurer_original(&c, SYSWOW64, "y").unwrap());
    }

    #[test]
    fn comparaison_de_fichiers() {
        let d = tempfile::tempdir().unwrap();
        let (a, b, z) = (d.path().join("a"), d.path().join("b"), d.path().join("z"));
        std::fs::write(&a, b"1").unwrap();
        std::fs::write(&b, b"1").unwrap();
        assert!(fichiers_identiques(&a, &b).unwrap());
        std::fs::write(&b, b"2").unwrap();
        assert!(!fichiers_identiques(&a, &b).unwrap());
        assert!(!fichiers_identiques(&a, &z).unwrap()); // absent : différent
    }

    #[test]
    fn copie_par_dessus_un_lien_ne_modifie_pas_la_cible() {
        let d = tempfile::tempdir().unwrap();
        let cible = d.path().join("cible");
        let lien = d.path().join("lien");
        let source = d.path().join("source");
        std::fs::write(&cible, b"intacte").unwrap();
        std::fs::write(&source, b"neuve").unwrap();
        std::os::unix::fs::symlink(&cible, &lien).unwrap();
        remplacer_fichier(&source, &lien).unwrap();
        assert_eq!(std::fs::read(&cible).unwrap(), b"intacte");
        assert_eq!(std::fs::read(&lien).unwrap(), b"neuve");
    }

    #[test]
    fn remplacer_un_lien_ne_touche_pas_sa_cible() {
        let d = tempfile::tempdir().unwrap();
        let ailleurs = d.path().join("ailleurs");
        let dest = d.path().join("dest");
        let source = d.path().join("source");
        std::fs::write(&ailleurs, b"intact").unwrap();
        std::fs::write(&source, b"neuf").unwrap();
        std::os::unix::fs::symlink(&ailleurs, &dest).unwrap();
        remplacer_fichier(&source, &dest).unwrap();
        assert_eq!(std::fs::read(&ailleurs).unwrap(), b"intact");
        assert!(!std::fs::symlink_metadata(&dest)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(&dest).unwrap(), b"neuf");
    }

    #[test]
    fn part_residuel_est_supprime() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("x.dll");
        let part = d.path().join("x.dll.part");
        let source = d.path().join("source");
        std::fs::write(&source, b"neuf").unwrap();
        std::fs::write(
            &part,
            b"reste d'une copie interrompue, plus long que la source",
        )
        .unwrap();
        remplacer_fichier(&source, &dest).unwrap();
        assert!(!part.exists());
        assert_eq!(std::fs::read(&dest).unwrap(), b"neuf");
    }

    #[test]
    fn source_absente_laisse_la_cible_intacte() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("x.dll");
        std::fs::write(&dest, b"ancien").unwrap();
        assert!(remplacer_fichier(&d.path().join("absente"), &dest).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"ancien");
        assert!(!d.path().join("x.dll.part").exists());
    }
}
