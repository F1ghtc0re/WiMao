//! Obtention de l'installeur d'une recette : téléchargement vérifié dans le cache.
//!
//! Utilise `wimao_core::telecharger_verifie` (fichier `.part` puis renommage atomique,
//! vérification SHA-256) et le journal. Sert à l'installation des gestionnaires publics
//! (Native Access…) dont la recette donne l'adresse `telecharger` ; appelé par
//! [`crate::installation`] (`installer_recette` sans fichier).

use std::path::{Path, PathBuf};

use wimao_core::i18n::{tr, trf};
use wimao_core::{empreinte, journal, telecharger_verifie, Chemins, Erreur, Progression, Resultat};

use crate::recette::Recette;

/// Extensions d'installeur ou d'archive reconnues dans le nom d'une adresse.
const EXTENSIONS_CONNUES: [&str; 4] = [".exe", ".msi", ".zip", ".7z"];
/// Extension donnée au fichier quand l'adresse n'en fournit pas (`https://inmusic.to/ISCPC`) :
/// ce sont presque toujours des `.exe`, et la détection du type se fait sur le contenu.
const EXTENSION_PAR_DEFAUT: &str = ".exe";
/// Sous-dossier du cache qui reçoit les téléchargements.
const DOSSIER_TELECHARGEMENTS: &str = "telechargements";
/// Début obligatoire d'une adresse de téléchargement (jamais de HTTP en clair).
const PREFIXE_HTTPS: &str = "https://";

/// Télécharge l'installeur de la recette dans `<cache>/telechargements/` et renvoie son chemin.
///
/// Le téléchargement est refait à chaque appel (spec 3.6) : un fichier déjà présent est
/// supprimé d'abord, car l'éditeur publie souvent une nouvelle version sous la même adresse.
/// `telecharger_verifie` vérifie l'empreinte si la recette en donne une, puis le journal
/// garde l'adresse et l'empreinte.
///
/// Erreurs : [`Erreur::Introuvable`] si la recette n'a pas d'adresse ;
/// [`Erreur::Telechargement`] si l'adresse n'est pas en HTTPS (seconde garde : le
/// chargement d'une recette l'a déjà vérifié) ou si le téléchargement échoue ;
/// [`Erreur::Empreinte`] si le fichier reçu ne correspond pas ; [`Erreur::Io`] sinon.
pub(crate) fn obtenir_installeur(
    c: &Chemins,
    r: &Recette,
    p: &dyn Progression,
) -> Resultat<PathBuf> {
    let url = r.telecharger.as_deref().ok_or_else(|| {
        Erreur::Introuvable(tr(
            "no download address in the recipe: choose the installer file",
        ))
    })?;
    if !url.starts_with(PREFIXE_HTTPS) {
        return Err(Erreur::Telechargement(trf(
            "the address must start with https:// (got \"{address}\")",
            &[("address", url)],
        )));
    }
    let dest = c
        .cache
        .join(DOSSIER_TELECHARGEMENTS)
        .join(nom_du_telechargement(r, url));
    repartir_a_neuf(&dest)?;
    p.etape(&trf("Downloading {name}…", &[("name", &r.nom)]));
    let fichier = telecharger_verifie(&[url], r.empreinte.as_deref(), &dest)?;
    journal::noter_info(
        "telechargement",
        &detail_du_journal(url, &empreinte(&fichier)?),
    );
    Ok(fichier)
}

/// Supprime le fichier `dest` s'il existe, pour que le téléchargement reparte à neuf.
///
/// Un fichier absent n'est pas une erreur. Erreurs : [`Erreur::Io`] pour toute autre cause
/// (droits refusés, `dest` est un dossier…).
fn repartir_a_neuf(dest: &Path) -> Resultat<()> {
    match std::fs::remove_file(dest) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Détail écrit au journal : l'adresse et l'empreinte réellement calculée du fichier reçu
/// (même quand la recette n'en fixe pas), pour retrouver ce qui a été installé (spec 3.6).
fn detail_du_journal(url: &str, empreinte_calculee: &str) -> String {
    format!("{url} sha256={empreinte_calculee}")
}

/// Nom du fichier téléchargé : le dernier segment de l'adresse s'il finit par `.exe`,
/// `.msi`, `.zip` ou `.7z` ; sinon le nom de la recette assaini, suivi de `.exe`.
///
/// Assainir = minuscules, tout ce qui n'est pas `[a-z0-9]` devient `-`. Cela garantit un
/// nom sans `/` ni `..`. Exemple : `https://inmusic.to/ISCPC` avec la recette
/// « inMusic Software Center » donne `inmusic-software-center.exe`.
pub(crate) fn nom_du_telechargement(r: &Recette, url: &str) -> String {
    // On ignore `?requête` et `#ancre`, qui ne font pas partie du nom de fichier.
    let chemin = url.split(['?', '#']).next().unwrap_or(url);
    let dernier = chemin.rsplit('/').next().unwrap_or("");
    let minuscule = dernier.to_lowercase();
    if EXTENSIONS_CONNUES.iter().any(|e| minuscule.ends_with(e)) && !dernier.starts_with('.') {
        return dernier.to_owned();
    }
    let assaini: String = r
        .nom
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{assaini}{EXTENSION_PAR_DEFAUT}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recette::charger;
    use wimao_core::ProgressionMuette;

    /// Charge une recette valide minimale nommée `nom`, avec ou sans `telecharger`.
    fn recette(nom: &str, telecharger: Option<&str>) -> Recette {
        let dossier = tempfile::tempdir().unwrap();
        let f = dossier.path().join("a.toml");
        let adresse = telecharger
            .map(|u| format!("telecharger = \"{u}\"\n"))
            .unwrap_or_default();
        std::fs::write(
            &f,
            format!("nom = \"{nom}\"\nreconnaitre = [\"a*\"]\n{adresse}"),
        )
        .unwrap();
        charger(&f).unwrap()
    }

    #[test]
    fn nom_depuis_l_url() {
        let r = recette("Native Access", None);
        let n = nom_du_telechargement(&r, "https://x.org/dl/Native-Access_2.exe");
        assert_eq!(n, "Native-Access_2.exe");
    }

    #[test]
    fn nom_depuis_la_recette() {
        let r = recette("inMusic Software Center", None);
        assert_eq!(
            nom_du_telechargement(&r, "https://inmusic.to/ISCPC"),
            "inmusic-software-center.exe"
        );
    }

    #[test]
    fn sans_adresse_introuvable() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let r = recette("A", None);
        let e = obtenir_installeur(&c, &r, &ProgressionMuette);
        assert!(matches!(e, Err(Erreur::Introuvable(_))), "{e:?}");
    }

    #[test]
    fn adresse_http_refusee() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let mut r = recette("A", Some("https://x.org/a.exe"));
        // Une recette chargée est déjà en HTTPS : on force l'adresse pour tester la seconde garde.
        r.telecharger = Some("http://x.org/a.exe".to_owned());
        let e = obtenir_installeur(&c, &r, &ProgressionMuette);
        assert!(matches!(e, Err(Erreur::Telechargement(_))), "{e:?}");
        assert!(!c.cache.join(DOSSIER_TELECHARGEMENTS).exists());
    }

    #[test]
    fn detail_du_journal_donne_l_adresse_et_l_empreinte() {
        assert_eq!(
            detail_du_journal("https://x.org/a.exe", "ab12"),
            "https://x.org/a.exe sha256=ab12"
        );
    }

    #[test]
    fn repartir_a_neuf_supprime_le_fichier() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("a.exe");
        std::fs::write(&f, b"x").unwrap();
        repartir_a_neuf(&f).unwrap();
        assert!(!f.exists());
    }

    #[test]
    fn repartir_a_neuf_tolere_l_absence() {
        let t = tempfile::tempdir().unwrap();
        assert!(repartir_a_neuf(&t.path().join("absent.exe")).is_ok());
    }

    #[test]
    fn repartir_a_neuf_propage_les_autres_erreurs() {
        let t = tempfile::tempdir().unwrap();
        // Un dossier à la place du fichier : remove_file échoue (ni NotFound).
        let e = repartir_a_neuf(t.path());
        assert!(matches!(e, Err(Erreur::Io(_))), "{e:?}");
    }
}
