//! Téléchargements vérifiés par empreinte SHA-256.
//!
//! Utilise `ureq` (client HTTP bloquant, TLS par rustls : pas de dépendance à
//! OpenSSL) et `sha2`. Sert aux modules d'installation (Wine Mono, DXVK,
//! installeurs Microsoft…) : rien n'est gardé sur disque sans avoir été vérifié.

use crate::erreur::{Erreur, Resultat};
use crate::i18n::tr;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Délai maximal pour établir la connexion (DNS + TCP + TLS). Au-delà, on passe
/// à l'adresse suivante plutôt que de laisser l'utilisateur devant un blocage.
const DELAI_CONNEXION: Duration = Duration::from_secs(20);
/// Délai maximal pour recevoir les en-têtes de la réponse après la requête.
const DELAI_REPONSE: Duration = Duration::from_secs(30);
/// Durée totale maximale d'un téléchargement (le plus gros fichier prévu, DXVK
/// ou un installeur Microsoft, fait quelques dizaines de Mo : 30 min couvrent
/// une très mauvaise connexion sans jamais bloquer indéfiniment).
const DELAI_TOTAL: Duration = Duration::from_secs(30 * 60);
/// Taille du tampon de lecture, partagée par le téléchargement et le calcul d'empreinte.
const TAILLE_BLOC: usize = 64 * 1024;

/// Calcule l'empreinte SHA-256 d'un fichier, en hexadécimal minuscule.
///
/// Le fichier est lu par blocs : sa taille n'est pas limitée par la mémoire.
///
/// Erreurs : [`Erreur::Io`] si le fichier ne peut pas être lu.
pub fn empreinte(fichier: &Path) -> Resultat<String> {
    let mut lecteur = File::open(fichier)?;
    let mut hash = Sha256::new();
    let mut bloc = vec![0u8; TAILLE_BLOC];
    loop {
        let n = lecteur.read(&mut bloc)?;
        if n == 0 {
            break;
        }
        hash.update(&bloc[..n]);
    }
    Ok(en_hexadecimal(&hash.finalize()))
}

/// Télécharge un fichier en vérifiant son empreinte SHA-256.
///
/// - Si `dest` existe déjà avec la bonne empreinte, rien n'est téléchargé.
///   (Sans `sha256`, un `dest` existant est considéré valide : on ne peut rien
///   vérifier, et re-télécharger à chaque appel serait inutile.)
/// - Sinon chaque adresse de `urls` est essayée dans l'ordre (miroirs) ; le
///   contenu est écrit dans `dest.part`, vérifié, puis renommé en `dest`. Le
///   renommage final rend l'apparition de `dest` atomique : un fichier présent
///   est toujours complet.
/// - `sha256: None` : aucune vérification (fichiers Microsoft dont l'empreinte
///   change ; leur contenu utile est vérifié après extraction).
///
/// Les dossiers parents de `dest` sont créés si besoin. `dest.part` est supprimé
/// quel que soit l'échec.
///
/// Erreurs : [`Erreur::Empreinte`] si le contenu reçu ne correspond pas (arrêt
/// immédiat : un autre miroir donnerait le même fichier piégé ou une
/// empreinte de recette fausse, ce n'est pas un problème réseau) ;
/// [`Erreur::Telechargement`] si aucune adresse n'a répondu (le message liste
/// la cause de chacune) ; [`Erreur::Io`] pour un problème de disque.
///
/// ```no_run
/// use std::path::Path;
/// use wimao_core::telechargement::telecharger_verifie;
/// let chemin = telecharger_verifie(&["https://exemple.org/f.tar.gz"], None, Path::new("/tmp/f.tar.gz"))?;
/// # Ok::<(), wimao_core::Erreur>(())
/// ```
pub fn telecharger_verifie(urls: &[&str], sha256: Option<&str>, dest: &Path) -> Resultat<PathBuf> {
    if dest.exists() && deja_valide(dest, sha256)? {
        return Ok(dest.to_path_buf());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = chemin_part(dest);
    let resultat = essayer_les_adresses(urls, sha256, &part);
    match resultat {
        Ok(()) => {
            if let Err(e) = std::fs::rename(&part, dest) {
                let _ = std::fs::remove_file(&part);
                return Err(e.into());
            }
            Ok(dest.to_path_buf())
        }
        Err(e) => {
            // Quel que soit l'échec, ne laisser aucun fichier partiel.
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Fichier de travail : `dest` suivi de `.part` (ajouté au nom entier, sans
/// remplacer l'extension).
fn chemin_part(dest: &Path) -> PathBuf {
    let mut nom = dest.as_os_str().to_os_string();
    nom.push(".part");
    PathBuf::from(nom)
}

/// Vrai si `dest` n'a pas besoin d'être re-téléchargé.
fn deja_valide(dest: &Path, sha256: Option<&str>) -> Resultat<bool> {
    match sha256 {
        None => Ok(true),
        Some(attendue) => Ok(empreinte(dest)?.eq_ignore_ascii_case(attendue)),
    }
}

/// Essaie chaque adresse jusqu'à ce que `part` contienne un fichier vérifié.
fn essayer_les_adresses(urls: &[&str], sha256: Option<&str>, part: &Path) -> Resultat<()> {
    let mut echecs = Vec::new();
    for url in urls {
        match telecharger_une_adresse(url, part).and_then(|obtenue| controler(sha256, obtenue)) {
            Ok(()) => return Ok(()),
            // Empreinte fausse : pas de miroir suivant (voir la doc de la fonction publique).
            Err(e @ Erreur::Empreinte { .. }) => return Err(e),
            Err(e) => echecs.push(format!("{url} : {e}")),
        }
    }
    if echecs.is_empty() {
        echecs.push(tr("no address given"));
    }
    Err(Erreur::Telechargement(echecs.join("\n")))
}

/// Compare l'empreinte reçue à celle attendue (si elle est connue).
fn controler(attendue: Option<&str>, obtenue: String) -> Resultat<()> {
    match attendue {
        Some(a) if !a.eq_ignore_ascii_case(&obtenue) => Err(Erreur::Empreinte {
            attendue: a.to_ascii_lowercase(),
            obtenue,
        }),
        _ => Ok(()),
    }
}

/// Télécharge `url` en flux vers `part` et renvoie l'empreinte SHA-256 calculée
/// au fil de l'eau (un seul passage sur les données).
fn telecharger_une_adresse(url: &str, part: &Path) -> Resultat<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(DELAI_CONNEXION))
        .timeout_recv_response(Some(DELAI_REPONSE))
        .timeout_global(Some(DELAI_TOTAL))
        .build()
        .into();
    // Par défaut ureq 3 transforme un code HTTP 4xx/5xx en erreur : c'est voulu.
    let reponse = agent
        .get(url)
        .call()
        .map_err(|e| Erreur::Telechargement(e.to_string()))?;
    let mut corps = reponse.into_body().into_reader();
    let mut sortie = File::create(part)?;
    let mut hash = Sha256::new();
    let mut bloc = vec![0u8; TAILLE_BLOC];
    loop {
        let n = corps
            .read(&mut bloc)
            .map_err(|e| Erreur::Telechargement(e.to_string()))?;
        if n == 0 {
            break;
        }
        hash.update(&bloc[..n]);
        sortie.write_all(&bloc[..n])?;
    }
    sortie.flush()?;
    Ok(en_hexadecimal(&hash.finalize()))
}

/// Représentation hexadécimale minuscule d'une suite d'octets.
pub(crate) fn en_hexadecimal(octets: &[u8]) -> String {
    octets.iter().map(|o| format!("{o:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tiny_http::{Response, Server};

    /// Contenu servi par les serveurs de test.
    const CONTENU: &[u8] = b"contenu de test WiMao";

    fn sha_du_contenu() -> String {
        let dossier = tempfile::tempdir().unwrap();
        let f = dossier.path().join("c");
        std::fs::write(&f, CONTENU).unwrap();
        empreinte(&f).unwrap()
    }

    /// Serveur local : `/absent` répond 404, `/ok` sert [`CONTENU`].
    fn serveur() -> (String, Arc<Server>) {
        let s = Arc::new(Server::http("127.0.0.1:0").unwrap());
        let base = format!("http://{}", s.server_addr().to_ip().unwrap());
        let s2 = Arc::clone(&s);
        std::thread::spawn(move || {
            for requete in s2.incoming_requests() {
                let _ = if requete.url() == "/ok" {
                    requete.respond(Response::from_data(CONTENU))
                } else {
                    requete.respond(Response::from_string("non").with_status_code(404))
                };
            }
        });
        (base, s)
    }

    #[test]
    fn empreinte_connue() {
        // sha256("abc") : vecteur de test standard (FIPS 180-2).
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("abc");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(
            empreinte(&f).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn deuxieme_adresse_si_la_premiere_est_en_404() {
        let (base, _s) = serveur();
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("sous/dossier/fichier");
        let (a, b) = (format!("{base}/absent"), format!("{base}/ok"));
        let sha = sha_du_contenu();
        let r = telecharger_verifie(&[&a, &b], Some(&sha), &dest).unwrap();
        assert_eq!(r, dest);
        assert_eq!(std::fs::read(&dest).unwrap(), CONTENU);
        assert!(!chemin_part(&dest).exists());
    }

    #[test]
    fn mauvaise_empreinte_ne_laisse_rien() {
        let (base, _s) = serveur();
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("fichier.bin");
        let url = format!("{base}/ok");
        let r = telecharger_verifie(&[&url], Some(&"0".repeat(64)), &dest);
        assert!(matches!(r, Err(Erreur::Empreinte { .. })), "{r:?}");
        assert!(!dest.exists());
        assert!(!chemin_part(&dest).exists());
    }

    #[test]
    fn fichier_deja_present_et_correct_pas_de_reseau() {
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("fichier");
        std::fs::write(&dest, CONTENU).unwrap();
        // Adresse injoignable : le test échoue si le réseau est touché.
        let r = telecharger_verifie(&["http://127.0.0.1:1/x"], Some(&sha_du_contenu()), &dest);
        assert_eq!(r.unwrap(), dest);
    }

    #[test]
    fn fichier_present_mais_corrompu_est_retelecharge() {
        let (base, _s) = serveur();
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("fichier");
        std::fs::write(&dest, b"abime").unwrap();
        let url = format!("{base}/ok");
        telecharger_verifie(&[&url], Some(&sha_du_contenu()), &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), CONTENU);
    }

    #[test]
    fn sans_empreinte_aucune_verification() {
        let (base, _s) = serveur();
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("f");
        telecharger_verifie(&[&format!("{base}/ok")], None, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), CONTENU);
    }

    #[test]
    fn toutes_les_adresses_en_echec() {
        let (base, _s) = serveur();
        let d = tempfile::tempdir().unwrap();
        let dest = d.path().join("f");
        let r = telecharger_verifie(&[&format!("{base}/absent")], None, &dest);
        assert!(matches!(r, Err(Erreur::Telechargement(_))), "{r:?}");
        assert!(!dest.exists() && !chemin_part(&dest).exists());
    }

    #[test]
    fn aucune_adresse() {
        let d = tempfile::tempdir().unwrap();
        let r = telecharger_verifie(&[], None, &d.path().join("f"));
        assert!(matches!(r, Err(Erreur::Telechargement(_))));
    }
}
