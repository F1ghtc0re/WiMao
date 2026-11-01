//! Correctif Wine Mono : l'implémentation .NET libre utilisée par Wine.
//!
//! Utilise [`Wine`] (registre, `msiexec`) et [`telecharger_verifie`]. Utilisé par
//! `correctifs::correctifs`.
//!
//! Pourquoi installer Mono : les installeurs AIR (Mini Grand, Velvet) contiennent
//! dans leur MSI une action personnalisée .NET (`VelvetFixContentLocation`) ; sans
//! Mono, l'installation échoue (0x80070643) et le contenu manque (NOTES.md,
//! « Mini Grand » et « Velvet »). Mono est donc posé dans tout environnement neuf,
//! alors que `wineboot` est lancé sans lui (voir `environnement`).

use super::Correctif;
use crate::chemins::Chemins;
use crate::erreur::{Erreur, Resultat};
use crate::i18n::tr;
use crate::progression::Progression;
use crate::registre_fichier::lire_valeur;
use crate::telechargement::telecharger_verifie;
use crate::wine::Wine;
use std::ffi::OsStr;
use std::time::Duration;

/// Nom du fichier MSI (Wine Mono 11.3.0, version attendue par Wine 11.20 selon NOTES.md), dans le cache de WiMao.
const FICHIER_MSI: &str = "wine-mono-11.3.0-x86.msi";
/// Adresse officielle du MSI (WineHQ).
const URL_MSI: &str = "https://dl.winehq.org/wine/wine-mono/11.3.0/wine-mono-11.3.0-x86.msi";
/// Empreinte SHA-256 du MSI, relevée dans NOTES.md.
const SHA256_MSI: &str = "df2dfc1665c2511882e7cabd56eafd0c0a3d94e5a7e86f969277f6c189d418d3";
/// Clé du registre où .NET 4 déclare sa version ; Wine Mono la renseigne.
/// (Écrite sans la racine `HKLM` : on la lit dans `system.reg`, voir `registre_fichier`.)
const CLE_NET4: &str = r"Software\Microsoft\NET Framework Setup\NDP\v4\Full";
/// Valeur de [`CLE_NET4`] qui prouve qu'un .NET 4 est installé.
const VALEUR_VERSION: &str = "Version";
/// Dossier de Mono dans `drive_c` : second indice d'une installation réussie.
const DOSSIER_MONO: &str = "windows/mono/mono-2.0";
/// Délai de `msiexec` : l'installation copie plusieurs centaines de fichiers et
/// compile des images natives, ce qui reste sous quelques minutes même lentement.
const DELAI_INSTALLATION: Duration = Duration::from_secs(10 * 60);

/// Wine Mono 11.3.0.
#[derive(Debug, Clone, Copy)]
pub struct Mono;

impl Correctif for Mono {
    fn nom(&self) -> &'static str {
        "Wine Mono"
    }

    /// Appliqué si le dossier de Mono existe, ou si `system.reg` déclare un .NET 4.
    /// Le registre est lu dans le fichier, sans lancer Wine (`registre_fichier`).
    fn est_applique(&self, c: &Chemins) -> Resultat<bool> {
        if c.drive_c().join(DOSSIER_MONO).exists() {
            return Ok(true);
        }
        let lue = lire_valeur(&c.prefixe.join("system.reg"), CLE_NET4, VALEUR_VERSION)?;
        Ok(lue.is_some())
    }

    /// Télécharge le MSI (vérifié) dans le cache puis l'installe en silencieux.
    ///
    /// Erreurs : téléchargement ([`Erreur::Telechargement`], [`Erreur::Empreinte`]),
    /// [`Erreur::Delai`], ou [`Erreur::Commande`] si `msiexec` échoue.
    fn appliquer(&self, c: &Chemins, p: &dyn Progression) -> Resultat<()> {
        p.etape(&tr("Installing Wine Mono…"));
        let msi = c.cache.join(FICHIER_MSI);
        telecharger_verifie(&[URL_MSI], Some(SHA256_MSI), &msi)?;
        // `/qn` : aucune interface, sinon l'installateur attend un clic ; `/norestart` :
        // jamais de redémarrage demandé (options MSI des contraintes du projet, comme
        // pour les installeurs de plugins).
        let args: [&OsStr; 5] = [
            OsStr::new("msiexec"),
            OsStr::new("/i"),
            msi.as_os_str(),
            OsStr::new("/qn"),
            OsStr::new("/norestart"),
        ];
        let wine = Wine::nouveau(c);
        let sortie = wine.executer(&args, DELAI_INSTALLATION)?;
        if sortie.code != 0 {
            return Err(Erreur::Commande {
                programme: format!("wine msiexec /i {} /qn /norestart", msi.display()),
                code: Some(sortie.code),
                sortie: sortie.texte,
            });
        }
        // Le registre n'est écrit sur disque qu'à l'arrêt du serveur : `est_applique` le lit.
        wine.arreter()
    }
}
