//! Installation sans Wine : extraction 7z en étapes (`extraire-7z`) et installeur Squirrel
//! (`squirrel`).
//!
//! Certains installeurs ne sont pas exécutables sous Wine mais se laissent ouvrir comme des
//! archives (NOTES.md, « Native Access » et « AIR / inMusic ») ; WiMao refait alors à la main,
//! de façon répétable, ce que l'utilisateur avait fait à la main :
//! - `installer_extraire_7z` : `7z x` de l'installeur, puis une archive interne par étape
//!   de la recette, décompressée à part puis copiée dans un dossier Windows ;
//! - `installer_squirrel` : `7z x` de l'installeur, du paquet `*-full.nupkg` qu'il contient,
//!   puis rangement sous `{appdata-local}\<id>` comme le ferait `Update.exe`.
//!
//! Utilise l'outil externe `7z` via `archive.rs` (`extraire`, `copier`), `chemins_windows.rs`
//! (variables, traduction vers Linux, motifs) et `wimao-core` (erreurs, chemins).
//! Utilisé par `installation.rs` selon la méthode de la recette. Voir `ARCHITECTURE.md`.
//!
//! # Sécurité
//! **Toutes** les extractions (l'installeur, chaque archive d'étape, le paquet Squirrel) se font
//! dans `<cache>/extraction-<pid>-<n>/`, supprimé à la fin même en cas d'erreur
//! (`DossierTemporaire`) ; `7z x` n'écrit jamais directement dans l'environnement Wine. Ce qui
//! doit y aller est ensuite **copié** par `archive::copier`, comme pour les archives de plugins
//! (`archive.rs`). Aucun lien symbolique de l'archive n'est suivi : ni recherché, ni copié, ni
//! traversé par un motif. Un lien vers `~/.ssh` ou `/etc` glissé dans une archive d'étape
//! n'atterrit donc jamais dans `Program Files`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use walkdir::WalkDir;
use wimao_core::i18n::{tr, trf};
use wimao_core::{journal, Chemins, Erreur, Progression, Resultat};

use crate::archive::{copier, extraire, OUTIL_7Z};
use crate::chemins_windows::{remplacer_variables, resoudre_motif_relatif, vers_linux};
use crate::recette::EtapeExtraction;

/// Préfixe des dossiers d'extraction, sous le cache de WiMao.
const PREFIXE_EXTRACTION: &str = "extraction";
/// Motif (fin de nom, en minuscules) du paquet complet d'un installeur Squirrel.
const FIN_NUPKG: &str = "-full.nupkg";
/// Nom (en minuscules) du programme de mise à jour que Squirrel installe à la racine.
const NOM_UPDATE: &str = "update.exe";
/// Nom (en minuscules) de la liste des paquets de Squirrel.
const NOM_RELEASES: &str = "releases";
/// Extension (en minuscules) de la description d'un paquet NuGet.
const EXTENSION_NUSPEC: &str = "nuspec";
/// Dossier du nupkg dont le contenu devient `app-<version>` (cible .NET Framework 4.5).
const DOSSIER_APP_DANS_NUPKG: &str = "lib/net45";
/// Dossier, sous `{appdata-local}\<id>`, où Squirrel garde les paquets reçus.
const DOSSIER_PAQUETS: &str = "packages";
/// Racine des programmes de Squirrel dans le profil de l'utilisateur.
const RACINE_SQUIRREL: &str = r"{appdata-local}";

/// Numéro qui distingue les extractions d'un même processus (tests en parallèle compris).
static COMPTEUR: AtomicUsize = AtomicUsize::new(0);

/// Dossier d'extraction supprimé quand il sort de portée, succès comme erreur
/// (`remove_dir_all` ne suit pas les liens symboliques).
struct DossierTemporaire(PathBuf);

impl DossierTemporaire {
    /// Crée `<cache>/extraction-<pid>-<n>/`.
    fn creer(c: &Chemins) -> Resultat<Self> {
        let n = COMPTEUR.fetch_add(1, Ordering::Relaxed);
        let dossier = c
            .cache
            .join(format!("{PREFIXE_EXTRACTION}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dossier)?;
        Ok(Self(dossier))
    }

    /// Sous-dossier neuf nommé `nom`, pour une extraction de plus.
    fn sous_dossier(&self, nom: &str) -> Resultat<PathBuf> {
        let dossier = self.0.join(nom);
        fs::create_dir_all(&dossier)?;
        Ok(dossier)
    }
}

impl Drop for DossierTemporaire {
    fn drop(&mut self) {
        // Rien à faire d'une erreur ici : le dossier est dans le cache, jetable.
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Méthode `extraire-7z` : décompresse l'installeur, puis chaque étape de la recette.
///
/// Pour chaque étape, l'archive désignée par `archive` (relative au dossier décompressé, `*`
/// admis, une seule correspondance) est décompressée dans un sous-dossier temporaire, puis
/// son contenu est copié dans `vers` (chemin Windows, variables admises, dossier créé) sans
/// suivre aucun lien symbolique (voir « Sécurité » dans la documentation du module).
///
/// # Erreurs
/// `Erreur::Introuvable` (`7z` absent ou archive d'étape sans correspondance),
/// `Erreur::MotifAmbigu`, `Erreur::Commande` si `7z` échoue, `Erreur::NonPrisEnCharge` pour
/// un chemin qu'on ne sait pas traduire, `Erreur::Io`.
pub(crate) fn installer_extraire_7z(
    c: &Chemins,
    fichier: &Path,
    etapes: &[EtapeExtraction],
    utilisateur: &str,
) -> Resultat<()> {
    let temporaire = DossierTemporaire::creer(c)?;
    let decompresse = temporaire.sous_dossier("installeur")?;
    extraire_en_silence(fichier, &decompresse)?;
    for (numero, etape) in etapes.iter().enumerate() {
        let archive = resoudre_motif_relatif(&decompresse, &etape.archive)?;
        let vers = remplacer_variables(&etape.vers, utilisateur);
        let destination = vers_linux(&c.drive_c(), &vers)?;
        // D'abord à part (une archive n'est pas fiable), puis copie sans suivre de lien.
        let etape_decompressee = temporaire.sous_dossier(&format!("etape-{numero}"))?;
        extraire_en_silence(&archive, &etape_decompressee)?;
        fs::create_dir_all(&destination)?;
        copier(&etape_decompressee, &destination)?;
    }
    Ok(())
}

/// Méthode `squirrel` : installe un installeur Squirrel sans l'exécuter.
///
/// Reprend l'installation manuelle de NOTES.md (inMusic Software Center) : le nupkg du
/// `Setup.exe` est décompressé, son `lib/net45` devient `app-<version>/`, et `Update.exe`,
/// `RELEASES` et le nupkg sont rangés à côté, sous `{appdata-local}\<id>`. Rend le dossier
/// `app-<version>`.
///
/// # Erreurs
/// `Erreur::Introuvable` (nupkg, `Update.exe`, `RELEASES`, nuspec ou `lib/net45` absent, ou
/// nuspec sans `<id>`/`<version>`), `Erreur::MotifAmbigu` si plusieurs nupkg,
/// `Erreur::Commande`, `Erreur::NonPrisEnCharge`, `Erreur::Io`.
pub(crate) fn installer_squirrel(
    c: &Chemins,
    fichier: &Path,
    utilisateur: &str,
) -> Resultat<PathBuf> {
    let temporaire = DossierTemporaire::creer(c)?;
    let decompresse = temporaire.sous_dossier("installeur")?;
    extraire_en_silence(fichier, &decompresse)?;
    let nupkg = chercher_unique(&decompresse, |nom| nom.ends_with(FIN_NUPKG), "*-full.nupkg")?;
    let update = chercher_unique(&decompresse, |nom| nom == NOM_UPDATE, "Update.exe")?;
    let releases = chercher_unique(&decompresse, |nom| nom == NOM_RELEASES, "RELEASES")?;

    let paquet = temporaire.sous_dossier("paquet")?;
    extraire_en_silence(&nupkg, &paquet)?;
    let (id, version) = lire_identite_paquet(&paquet)?;

    // Même rangement que celui d'`Update.exe` de Squirrel, relevé à la main (NOTES.md,
    // « AIR / inMusic ») : app-<version>/ + Update.exe + RELEASES + packages/.
    let racine_windows = remplacer_variables(&format!(r"{RACINE_SQUIRREL}\{id}"), utilisateur);
    let racine = vers_linux(&c.drive_c(), &racine_windows)?;
    let app = racine.join(format!("app-{version}"));
    let sources = resoudre_motif_relatif(&paquet, DOSSIER_APP_DANS_NUPKG)?;
    copier(&sources, &app)?;
    copier(&update, &racine.join("Update.exe"))?;
    copier(&releases, &racine.join("RELEASES"))?;
    let paquets = racine.join(DOSSIER_PAQUETS);
    fs::create_dir_all(&paquets)?;
    copier(&nupkg, &paquets.join(nom_de(&nupkg)))?;
    Ok(app)
}

/// Lit l'unique `*.nuspec` à la racine du nupkg décompressé et en tire `(id, version)`.
///
/// L'`id` sert de nom de dossier : il est refusé s'il pourrait sortir de `AppData\Local`.
fn lire_identite_paquet(paquet: &Path) -> Resultat<(String, String)> {
    let nuspec = lister_racine(paquet)?
        .into_iter()
        .filter(|f| extension_minuscule(f).as_deref() == Some(EXTENSION_NUSPEC))
        .collect::<Vec<_>>();
    let [nuspec] = nuspec.as_slice() else {
        return Err(Erreur::Introuvable(tr("the .nuspec file of the package")));
    };
    let texte = fs::read_to_string(nuspec)?;
    let (id, version) = lire_nuspec(&texte).ok_or_else(|| {
        Erreur::Introuvable(tr("<id> and <version> in the .nuspec file of the package"))
    })?;
    for valeur in [&id, &version] {
        if !nom_sans_danger(valeur) {
            return Err(Erreur::NonPrisEnCharge(trf(
                "package name {name} (it must not contain a path)",
                &[("name", valeur)],
            )));
        }
    }
    Ok((id, version))
}

/// Fichiers (non liens) placés directement à la racine de `dossier`.
fn lister_racine(dossier: &Path) -> Resultat<Vec<PathBuf>> {
    Ok(fs::read_dir(dossier)?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect())
}

/// Vrai si `nom` peut servir de nom de dossier unique (ni séparateur, ni `..`, non vide).
fn nom_sans_danger(nom: &str) -> bool {
    !nom.is_empty() && nom != "." && nom != ".." && !nom.contains(['/', '\\', '\0'])
}

/// Extension en minuscules.
fn extension_minuscule(chemin: &Path) -> Option<String> {
    chemin
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
}

/// Dernier composant d'un chemin.
fn nom_de(chemin: &Path) -> std::ffi::OsString {
    chemin.file_name().unwrap_or_default().to_owned()
}

/// `7z x` sans interface de progression. Un avertissement de `7z` (code 1 : des fichiers
/// n'ont peut-être pas été extraits) n'est pas perdu : [`JournalAvertissements`] l'écrit au
/// journal de WiMao.
fn extraire_en_silence(archive: &Path, destination: &Path) -> Resultat<()> {
    extraire(
        Path::new(OUTIL_7Z),
        archive,
        destination,
        &JournalAvertissements,
    )
}

/// Progression qui n'affiche rien mais note chaque message (avertissement de `7z`) au journal.
struct JournalAvertissements;

impl Progression for JournalAvertissements {
    fn etape(&self, message: &str) {
        journal::noter(&evenement_avertissement(), message);
    }
}

/// Titre de l'événement de journal pour un avertissement d'extraction.
fn evenement_avertissement() -> String {
    tr("7z extraction warning")
}

/// Cherche récursivement, sans tenir compte de la casse et sans suivre de lien, le fichier
/// dont le nom en minuscules vérifie `convient`. Il doit y en avoir exactement un.
///
/// `description` nomme la recherche dans les erreurs (`*-full.nupkg`, `Update.exe`…).
fn chercher_unique(
    racine: &Path,
    convient: impl Fn(&str) -> bool,
    description: &str,
) -> Resultat<PathBuf> {
    let mut trouves = Vec::new();
    for entree in WalkDir::new(racine).follow_links(false) {
        let entree = entree.map_err(|e| std::io::Error::other(e.to_string()))?;
        let nom = entree.file_name().to_string_lossy().to_lowercase();
        if entree.file_type().is_file() && convient(&nom) {
            trouves.push(entree.into_path());
        }
    }
    match trouves.len() {
        0 => Err(Erreur::Introuvable(trf(
            "{name} in the installer",
            &[("name", description)],
        ))),
        1 => Ok(trouves.remove(0)),
        nombre => Err(Erreur::MotifAmbigu {
            motif: description.to_string(),
            nombre,
        }),
    }
}

/// Lit `(id, version)` dans le texte d'un fichier `.nuspec` : contenu des balises `<id>` et
/// `<version>`, sans espaces autour. `None` si l'une manque ou est vide.
pub(crate) fn lire_nuspec(texte: &str) -> Option<(String, String)> {
    Some((
        contenu_balise(texte, "id")?,
        contenu_balise(texte, "version")?,
    ))
}

/// Contenu (sans espaces autour) de la première balise `<nom>…</nom>` non vide de `texte`.
/// Une balise à attributs (`<dependency id="x">`) n'est pas confondue avec `<id>`.
fn contenu_balise(texte: &str, nom: &str) -> Option<String> {
    let ouverture = format!("<{nom}>");
    let fermeture = format!("</{nom}>");
    let debut = texte.find(&ouverture)? + ouverture.len();
    let fin = debut + texte[debut..].find(&fermeture)?;
    let contenu = texte[debut..fin].trim();
    (!contenu.is_empty()).then(|| contenu.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Écrit un fichier (dossiers parents créés).
    fn ecrire(chemin: &Path, contenu: &str) {
        fs::create_dir_all(chemin.parent().unwrap()).unwrap();
        fs::write(chemin, contenu).unwrap();
    }

    /// `7z a -t<format> <archive> .` depuis `source`.
    fn archiver(format: &str, source: &Path, archive: &Path) {
        let sortie = Command::new("7z")
            .current_dir(source)
            .args(["a", &format!("-t{format}"), "-snl"])
            .arg(archive)
            .arg(".")
            .output()
            .unwrap();
        assert!(sortie.status.success(), "7z a a échoué");
    }

    #[test]
    fn extraire_7z_en_deux_etapes() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        // app-64.7z contient Faux.exe ; setup.exe (7z) contient $PLUGINSDIR/app-64.7z.
        let interne = t.path().join("interne");
        ecrire(&interne.join("Faux.exe"), "MZ");
        let externe = t.path().join("externe");
        fs::create_dir_all(externe.join("$PLUGINSDIR")).unwrap();
        archiver("7z", &interne, &externe.join("$PLUGINSDIR/app-64.7z"));
        let setup = t.path().join("setup.exe");
        archiver("7z", &externe, &setup);

        let etapes = [EtapeExtraction {
            archive: "$PLUGINSDIR/app-64.7z".into(),
            vers: r"C:\Program Files\Faux".into(),
        }];
        installer_extraire_7z(&c, &setup, &etapes, "u").unwrap();

        assert!(c.drive_c().join("Program Files/Faux/Faux.exe").is_file());
        let restes: Vec<_> = fs::read_dir(&c.cache)
            .map(|d| d.flatten().collect())
            .unwrap_or_default();
        assert!(restes.is_empty(), "restes : {restes:?}");
    }

    /// Vrai si l'outil `7z` est installé. Sans lui, les tests qui fabriquent des archives
    /// malveillantes s'arrêtent proprement (message sur la sortie d'erreur) au lieu d'échouer.
    fn sept_z_present() -> bool {
        Command::new("7z").output().is_ok()
    }

    #[test]
    fn extraire_7z_ne_copie_ni_lien_ni_chemin_qui_sort() {
        if !sept_z_present() {
            eprintln!("test ignoré : l'outil 7z n'est pas installé");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let secret = t.path().join("secret.txt");
        ecrire(&secret, "secret");
        // app.7z : Faux.exe et un lien symbolique vers le secret (`-snl` le stocke tel quel).
        let interne = t.path().join("interne");
        ecrire(&interne.join("Faux.exe"), "MZ");
        std::os::unix::fs::symlink(&secret, interne.join("lien")).unwrap();
        let externe = t.path().join("externe");
        fs::create_dir_all(&externe).unwrap();
        archiver("7z", &interne, &externe.join("app.7z"));
        // evil.zip : une entrée `../evil.vst3`, qui voudrait sortir de sa destination.
        crate::test_utils::zip_malveillant(&externe.join("evil.zip"));
        let setup = t.path().join("setup.exe");
        archiver("7z", &externe, &setup);

        let etapes = [
            EtapeExtraction {
                archive: "app.7z".into(),
                vers: r"C:\Program Files\Faux".into(),
            },
            EtapeExtraction {
                archive: "evil.zip".into(),
                vers: r"C:\Program Files\Faux\sous".into(),
            },
        ];
        // Selon la version de 7-Zip, le lien et le `../` sont neutralisés (Ok, le contenu normal
        // est alors copié) ou l'archive entière est refusée (code 2, rien n'est copié : c'est le
        // cas de la version d'Ubuntu en CI). Les deux sont sûrs ; l'essentiel est vérifié ensuite.
        let faux = c.drive_c().join("Program Files/Faux");
        match installer_extraire_7z(&c, &setup, &etapes, "u") {
            Ok(()) => assert!(
                faux.join("Faux.exe").is_file(),
                "le contenu normal est copié"
            ),
            Err(e) => assert!(matches!(e, Erreur::Commande { .. }), "{e:?}"),
        }
        assert!(
            fs::symlink_metadata(faux.join("lien")).is_err(),
            "aucun lien symbolique copié dans l'environnement"
        );
        assert!(
            !faux.join("evil.vst3").exists(),
            "rien hors de la destination"
        );
        assert!(!c.drive_c().join("Program Files/evil.vst3").exists());
        assert_eq!(fs::read_to_string(&secret).unwrap(), "secret");
        cache_propre(&c);
    }

    #[test]
    fn extraire_7z_etape_absente() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let source = t.path().join("source");
        ecrire(&source.join("autre.txt"), "x");
        let setup = t.path().join("setup.exe");
        archiver("7z", &source, &setup);
        let etapes = [EtapeExtraction {
            archive: "$PLUGINSDIR/app-64.7z".into(),
            vers: r"C:\Faux".into(),
        }];
        let r = installer_extraire_7z(&c, &setup, &etapes, "u");
        assert!(matches!(r, Err(Erreur::Introuvable(_))), "{r:?}");
        assert!(!c.drive_c().join("Faux").exists());
        cache_propre(&c);
    }

    /// Fabrique `Setup.exe` (zip) avec `RELEASES`, `Update.exe` et, si `avec_nupkg`, un
    /// nupkg qui contient `Faux.nuspec` et `lib/net45/Faux.exe`.
    fn setup_squirrel(racine: &Path, avec_nupkg: bool) -> PathBuf {
        let contenu = racine.join("contenu");
        ecrire(&contenu.join("RELEASES"), "0 Faux-1.2.3-full.nupkg 1");
        ecrire(&contenu.join("Update.exe"), "MZ");
        if avec_nupkg {
            let paquet = racine.join("paquet");
            ecrire(
                &paquet.join("Faux.nuspec"),
                "<package><metadata><id>faux_app</id><version>1.2.3</version></metadata></package>",
            );
            ecrire(&paquet.join("lib/net45/Faux.exe"), "MZ");
            archiver("zip", &paquet, &contenu.join("Faux-1.2.3-full.nupkg"));
        }
        let setup = racine.join("Setup.exe");
        archiver("zip", &contenu, &setup);
        setup
    }

    #[test]
    fn squirrel_installe_comme_a_la_main() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let setup = setup_squirrel(t.path(), true);

        let app = installer_squirrel(&c, &setup, "u").unwrap();

        let racine = c.drive_c().join("users/u/AppData/Local/faux_app");
        assert_eq!(app, racine.join("app-1.2.3"));
        assert!(racine.join("app-1.2.3/Faux.exe").is_file());
        assert!(racine.join("Update.exe").is_file());
        assert!(racine.join("RELEASES").is_file());
        assert!(racine.join("packages/Faux-1.2.3-full.nupkg").is_file());
        let restes: Vec<_> = fs::read_dir(&c.cache)
            .map(|d| d.flatten().collect())
            .unwrap_or_default();
        assert!(restes.is_empty(), "restes : {restes:?}");
    }

    #[test]
    fn squirrel_sans_nupkg() {
        let t = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(t.path());
        let setup = setup_squirrel(t.path(), false);
        let r = installer_squirrel(&c, &setup, "u");
        assert!(matches!(r, Err(Erreur::Introuvable(_))), "{r:?}");
        cache_propre(&c);
    }

    /// `base/<nom>` est un lien symbolique vers `cible`.
    fn base_avec_lien(t: &Path, nom: &str, cible: &Path) -> PathBuf {
        let base = t.join("base");
        fs::create_dir_all(&base).unwrap();
        std::os::unix::fs::symlink(cible, base.join(nom)).unwrap();
        base
    }

    #[test]
    fn lien_en_dossier_avec_motif_n_est_pas_suivi() {
        let t = tempfile::tempdir().unwrap();
        let reel = t.path().join("reel");
        ecrire(&reel.join("secret.txt"), "x");
        let base = base_avec_lien(t.path(), "piege", &reel);
        for motif in ["piege/*", "piege/secret.txt", "pie*/secret.txt"] {
            let r = resoudre_motif_relatif(&base, motif);
            assert!(
                matches!(r, Err(Erreur::NonPrisEnCharge(_))),
                "{motif} : {r:?}"
            );
        }
    }

    #[test]
    fn lien_en_dernier_composant_ou_vers_l_inexistant_est_refuse() {
        let t = tempfile::tempdir().unwrap();
        let base = base_avec_lien(t.path(), "lien", &t.path().join("n_existe_pas"));
        for motif in ["lien", "li*"] {
            let r = resoudre_motif_relatif(&base, motif);
            assert!(
                matches!(r, Err(Erreur::NonPrisEnCharge(_))),
                "{motif} : {r:?}"
            );
        }
    }

    #[test]
    fn avertissement_7z_porte_un_titre_pour_le_journal() {
        assert!(!evenement_avertissement().is_empty());
    }

    /// Le cache ne contient plus aucun dossier d'extraction.
    fn cache_propre(c: &Chemins) {
        let restes: Vec<_> = fs::read_dir(&c.cache)
            .map(|d| d.flatten().collect())
            .unwrap_or_default();
        assert!(restes.is_empty(), "restes : {restes:?}");
    }

    #[test]
    fn lire_nuspec_simple() {
        let texte = "<package><metadata>\n <id> faux_app </id>\n<version>1.2.3</version>\n</metadata></package>";
        assert_eq!(
            lire_nuspec(texte),
            Some(("faux_app".to_string(), "1.2.3".to_string()))
        );
    }

    #[test]
    fn lire_nuspec_sans_version() {
        assert_eq!(lire_nuspec("<metadata><id>faux_app</id></metadata>"), None);
    }
}
