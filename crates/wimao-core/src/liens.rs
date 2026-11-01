//! Liens de connexion (`native-access://`, `arturia://`…) déclarés au bureau Linux.
//!
//! Quand on se connecte dans le gestionnaire d'un éditeur (Native Access, Arturia
//! Software Center…), le site renvoie le navigateur vers un lien propre à
//! l'éditeur (`native-access://…`). Sous Linux, c'est le bureau qui choisit le
//! programme à lancer, d'après un fichier `.desktop` qui déclare le type MIME
//! `x-scheme-handler/<schéma>`. Ce module déclare un `.desktop` WiMao qui lance
//! `wimao-cli relayer-lien <lien>` (le relais, qui transmet le lien au gestionnaire
//! tournant dans Wine).
//!
//! - [`etat_lien`] : qui gère déjà ce schéma ? (lecture seule) ;
//! - [`declarer_lien`] : écrit le `.desktop` et l'associe au schéma. Elle **ne
//!   consulte pas l'état** : c'est l'appelant qui décide s'il remplace un gestionnaire
//!   existant (l'utilisateur doit donner son accord, il peut en avoir un qui marche) ;
//! - [`commande_relais`] : où est `wimao-cli`.
//!
//! Utilise `xdg-mime` et `update-desktop-database` (paquets `xdg-utils` et
//! `desktop-file-utils`), lancés avec l'environnement de
//! [`crate::wine::preparer_environnement`] : en isolation (tests) ils ne voient que
//! le faux `XDG_DATA_HOME`/`XDG_CONFIG_HOME` de test, jamais ceux de la personne.
//! Le `schema` est inséré tel quel dans un nom de fichier : l'appelant doit l'avoir
//! validé (`[a-z0-9+.-]`, voir `recette.rs`), sans quoi `../` en sortirait du dossier.
//!
//! Le dossier où le `.desktop` est écrit vient de [`Chemins::xdg_data_home`], la
//! même source que celle de `XDG_DATA_HOME`. Voir `ARCHITECTURE.md`.

use crate::chemins::Chemins;
use crate::erreur::{Erreur, Resultat};
use crate::i18n::tr;
use crate::outils::lancer;
use crate::wine::preparer_environnement;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Délai maximal d'un appel à `xdg-mime` ou `update-desktop-database` (de simples
/// lectures/écritures de petits fichiers : bien plus que nécessaire).
const DELAI_OUTILS: Duration = Duration::from_secs(30);
/// Nom de l'exécutable de relais, livré avec le paquet de WiMao.
const NOM_RELAIS: &str = "wimao-cli";
/// Préfixe du type MIME qui désigne le gestionnaire d'un schéma de lien.
const MIME_SCHEMA: &str = "x-scheme-handler/";

/// Qui gère actuellement un schéma de lien.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EtatLien {
    /// Aucun programme ne gère ce schéma.
    Absent,
    /// Le `.desktop` de WiMao (`wimao-lien-<schéma>.desktop`) le gère déjà.
    DejaWimao,
    /// Un autre programme le gère ; contient le nom de son `.desktop`
    /// (par exemple `wimao-native-access.desktop`, l'ancien script de l'utilisateur).
    Autre(String),
}

/// Faut-il demander l'accord de la personne avant de déclarer le lien ? Rend le nom du
/// gestionnaire actuel à citer dans la question quand c'est un **autre** programme
/// ([`EtatLien::Autre`]), sinon `None` : le lien est libre ([`EtatLien::Absent`]) ou déjà à
/// WiMao ([`EtatLien::DejaWimao`], le redéclarer ne remplace rien).
///
/// Décision commune à la ligne de commande et à l'interface : seule la question, elle,
/// est posée par chacune à sa façon.
pub fn lien_demande_confirmation(etat: &EtatLien) -> Option<String> {
    match etat {
        EtatLien::Autre(gestionnaire) => Some(gestionnaire.clone()),
        EtatLien::Absent | EtatLien::DejaWimao => None,
    }
}

/// Nom du `.desktop` de WiMao pour un schéma : `wimao-lien-<schema>.desktop`.
pub(crate) fn fichier_desktop(schema: &str) -> String {
    format!("wimao-lien-{schema}.desktop")
}

/// Type MIME qui désigne le gestionnaire d'un schéma.
fn type_mime(schema: &str) -> String {
    format!("{MIME_SCHEMA}{schema}")
}

/// Dit qui gère `schema` (`xdg-mime query default x-scheme-handler/<schema>`).
/// Ne modifie rien.
///
/// Erreurs : [`Erreur::Introuvable`] si `xdg-mime` manque (paquet `xdg-utils`),
/// [`Erreur::Commande`] s'il échoue, [`Erreur::Delai`].
pub fn etat_lien(c: &Chemins, schema: &str) -> Resultat<EtatLien> {
    etat_lien_avec_path(c, schema, None)
}

/// Cœur de [`etat_lien`] : `path`, s'il est donné, remplace le `PATH` des commandes
/// (les tests y mettent seulement leurs faux scripts, sans toucher au `PATH` du
/// processus ni pouvoir atteindre les vrais outils).
fn etat_lien_avec_path(c: &Chemins, schema: &str, path: Option<&OsStr>) -> Resultat<EtatLien> {
    let mut cmd = commande("xdg-mime", c, path);
    cmd.args(["query", "default", &type_mime(schema)]);
    let sortie = lancer_ou_echec(cmd, "xdg-mime query")?;
    // `xdg-mime` affiche une ligne vide quand rien n'est associé.
    let gestionnaire = sortie.lines().map(str::trim).find(|l| !l.is_empty());
    Ok(match gestionnaire {
        None => EtatLien::Absent,
        Some(nom) if nom == fichier_desktop(schema) => EtatLien::DejaWimao,
        Some(nom) => EtatLien::Autre(nom.to_string()),
    })
}

/// Déclare WiMao comme gestionnaire de `schema` : écrit
/// `<XDG_DATA_HOME>/applications/wimao-lien-<schema>.desktop` (dont `Exec` lance
/// `relais relayer-lien %u`), l'associe au schéma avec `xdg-mime default`, puis
/// rafraîchit la base avec `update-desktop-database` (s'il est absent, ce n'est pas
/// une erreur : les bureaux modernes relisent le dossier tout seuls).
///
/// **Remplace** l'association existante sans la regarder : appeler d'abord
/// [`etat_lien`] et demander l'accord de la personne si [`EtatLien::Autre`].
///
/// Erreurs : [`Erreur::Io`] (écriture du `.desktop`), [`Erreur::Introuvable`] si
/// `xdg-mime` manque, [`Erreur::Commande`] s'il échoue, [`Erreur::Delai`].
pub fn declarer_lien(c: &Chemins, schema: &str, relais: &Path) -> Resultat<()> {
    declarer_lien_avec_path(c, schema, relais, None)
}

/// Cœur de [`declarer_lien`], avec un `PATH` injectable (voir [`etat_lien_avec_path`]).
fn declarer_lien_avec_path(
    c: &Chemins,
    schema: &str,
    relais: &Path,
    path: Option<&OsStr>,
) -> Resultat<()> {
    let dossier = c.xdg_data_home().join("applications");
    std::fs::create_dir_all(&dossier)?;
    let fichier = fichier_desktop(schema);
    std::fs::write(dossier.join(&fichier), contenu_desktop(schema, relais))?;

    let mut cmd = commande("xdg-mime", c, path);
    cmd.args(["default", &fichier, &type_mime(schema)]);
    lancer_ou_echec(cmd, "xdg-mime default")?;

    let mut cmd = commande("update-desktop-database", c, path);
    cmd.arg(&dossier);
    match lancer_ou_echec(cmd, "update-desktop-database") {
        // Outil facultatif (paquet `desktop-file-utils`) : son absence est permise.
        Err(Erreur::Introuvable(_)) => Ok(()),
        autre => autre.map(|_| ()),
    }
}

/// Commande `programme` avec l'environnement commun de WiMao
/// ([`preparer_environnement`]) et, si `path` est donné, ce `PATH`.
fn commande(programme: &str, c: &Chemins, path: Option<&OsStr>) -> Command {
    let mut cmd = Command::new(programme);
    preparer_environnement(&mut cmd, c, &[]);
    if let Some(path) = path {
        cmd.env("PATH", path);
    }
    cmd
}

/// Texte du `.desktop` (voir la spécification « Desktop Entry »).
/// `NoDisplay` : l'entrée sert de gestionnaire de lien, pas de lanceur dans le menu.
fn contenu_desktop(schema: &str, relais: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=WiMao ({schema})\n\
         Exec={} relayer-lien %u\nNoDisplay=true\nMimeType={};\n",
        echapper_exec(relais),
        type_mime(schema)
    )
}

/// Chemin entre guillemets, pour la clé `Exec`. Selon la spécification, dans un
/// argument entre guillemets `"`, `` ` ``, `$` et `\` se protègent par `\`, et cette
/// barre oblique est elle-même doublée dans la valeur du fichier ; `%` s'écrit `%%`
/// (sinon c'est un code de champ). Un chemin avec espaces fonctionne ainsi.
fn echapper_exec(chemin: &Path) -> String {
    let mut sortie = String::from("\"");
    for car in chemin.to_string_lossy().chars() {
        match car {
            // Une barre oblique littérale : `\\` au niveau Exec, doublé dans la
            // valeur du fichier, soit quatre caractères `\` en tout.
            '\\' => sortie.push_str(r"\\\\"),
            // Les autres : une barre de protection (`\`) doublée dans la valeur.
            '"' | '`' | '$' => {
                sortie.push_str(r"\\");
                sortie.push(car);
            }
            '%' => sortie.push_str("%%"),
            _ => sortie.push(car),
        }
    }
    sortie.push('"');
    sortie
}

/// Lance `cmd` ; un code non nul devient [`Erreur::Commande`] nommant `nom`.
/// Renvoie le texte affiché.
fn lancer_ou_echec(mut cmd: Command, nom: &str) -> Resultat<String> {
    let sortie = lancer(&mut cmd, DELAI_OUTILS)?;
    if sortie.code != 0 {
        return Err(Erreur::Commande {
            programme: nom.to_string(),
            code: Some(sortie.code),
            sortie: sortie.texte,
        });
    }
    Ok(sortie.texte)
}

/// Chemin de `wimao-cli`, le programme que le bureau lance avec le lien : à côté du
/// programme en cours (installation par le paquet, ou dossier `target/` en
/// développement), sinon le premier trouvé dans le `PATH`.
///
/// Erreurs : [`Erreur::Introuvable`] (message traduit) si aucun n'existe.
pub fn commande_relais() -> Resultat<PathBuf> {
    commande_relais_dans(std::env::var_os("PATH").as_deref())
}

/// Cœur de [`commande_relais`], avec le `PATH` injecté.
fn commande_relais_dans(path: Option<&OsStr>) -> Resultat<PathBuf> {
    let voisin = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(NOM_RELAIS)))
        .filter(|p| p.is_file());
    let dans_path = || {
        path.and_then(|path| {
            std::env::split_paths(path)
                .map(|d| d.join(NOM_RELAIS))
                .find(|p| p.is_file())
        })
    };
    voisin
        .or_else(dans_path)
        .ok_or_else(|| Erreur::Introuvable(tr("the wimao-cli program (is WiMao installed?)")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn seul_un_autre_gestionnaire_demande_confirmation() {
        assert_eq!(lien_demande_confirmation(&EtatLien::Absent), None);
        assert_eq!(lien_demande_confirmation(&EtatLien::DejaWimao), None);
        assert_eq!(
            lien_demande_confirmation(&EtatLien::Autre("ancien.desktop".into())),
            Some("ancien.desktop".to_string())
        );
    }

    /// Environnement de test : racine temporaire, `Chemins` isolés et faux
    /// `xdg-mime` / `update-desktop-database`. Le `PATH` du processus n'est jamais
    /// modifié : les commandes reçoivent [`Banc::path`], réduit au dossier des faux.
    struct Banc {
        racine: tempfile::TempDir,
        chemins: Chemins,
    }

    impl Banc {
        /// `reponse` : ce que le faux `xdg-mime query` affiche ; `avec_update` :
        /// fournit ou non un faux `update-desktop-database`.
        fn nouveau(reponse: &str, avec_update: bool) -> Banc {
            let racine = tempfile::tempdir().unwrap();
            let chemins = Chemins::pour_test(racine.path());
            let bin = racine.path().join("faux-bin");
            fs::create_dir_all(&bin).unwrap();
            fs::write(racine.path().join("reponse"), reponse).unwrap();
            // Le faux xdg-mime note ses arguments ; `query` répond depuis un fichier.
            let script = format!(
                "#!/bin/sh\necho \"$@\" >> '{r}/appels-xdg-mime'\n\
                 echo \"$XDG_CONFIG_HOME\" > '{r}/config-vue'\n\
                 if [ \"$1\" = query ]; then while read l; do echo \"$l\"; done < '{r}/reponse'; fi\nexit 0\n",
                r = racine.path().display()
            );
            installer_faux(&bin.join("xdg-mime"), &script);
            if avec_update {
                let script = format!(
                    "#!/bin/sh\necho \"$@\" >> '{}/appels-update'\n",
                    racine.path().display()
                );
                installer_faux(&bin.join("update-desktop-database"), &script);
            }
            Banc { racine, chemins }
        }
        /// `PATH` des commandes : le seul dossier des faux (le script `#!/bin/sh` n'a
        /// besoin que de `echo` et `read`, intégrés à `sh`). Les vrais `xdg-mime` et
        /// `update-desktop-database` de la machine sont donc inatteignables.
        fn path(&self) -> std::ffi::OsString {
            self.racine.path().join("faux-bin").into()
        }
        fn applications(&self) -> std::path::PathBuf {
            self.chemins.xdg_data_home().join("applications")
        }
        fn lire(&self, nom: &str) -> String {
            fs::read_to_string(self.racine.path().join(nom)).unwrap_or_default()
        }
    }

    fn installer_faux(chemin: &Path, script: &str) {
        crate::test_utils::ecrire_executable(chemin, script);
    }

    #[test]
    fn declarer_ecrit_le_desktop_et_appelle_xdg_mime() {
        let b = Banc::nouveau("", true);
        declarer_lien_avec_path(
            &b.chemins,
            "test",
            Path::new("/x/wimao-cli"),
            Some(&b.path()),
        )
        .unwrap();
        let desktop = b.applications().join("wimao-lien-test.desktop");
        assert!(
            desktop.starts_with(b.racine.path()),
            "doit rester dans le test"
        );
        assert_eq!(
            fs::read_to_string(desktop).unwrap(),
            "[Desktop Entry]\nType=Application\nName=WiMao (test)\n\
             Exec=\"/x/wimao-cli\" relayer-lien %u\nNoDisplay=true\n\
             MimeType=x-scheme-handler/test;\n"
        );
        assert_eq!(
            b.lire("appels-xdg-mime").trim(),
            "default wimao-lien-test.desktop x-scheme-handler/test"
        );
        // Le faux xdg-mime a reçu l'XDG_CONFIG_HOME du test, pas celui de la personne.
        assert_eq!(
            b.lire("config-vue").trim(),
            b.chemins.home.join(".config").to_str().unwrap()
        );
        assert_eq!(
            b.lire("appels-update").trim(),
            b.applications().to_str().unwrap()
        );
    }

    #[test]
    fn update_desktop_database_absent_n_est_pas_une_erreur() {
        let b = Banc::nouveau("", false);
        declarer_lien_avec_path(
            &b.chemins,
            "test",
            Path::new("/x/wimao-cli"),
            Some(&b.path()),
        )
        .unwrap();
        assert!(b.applications().join("wimao-lien-test.desktop").exists());
    }

    #[test]
    fn xdg_mime_en_echec_est_une_erreur_commande() {
        let b = Banc::nouveau("", false);
        installer_faux(
            &b.racine.path().join("faux-bin/xdg-mime"),
            "#!/bin/sh\necho boum\nexit 3\n",
        );
        let e = declarer_lien_avec_path(
            &b.chemins,
            "test",
            Path::new("/x/wimao-cli"),
            Some(&b.path()),
        )
        .unwrap_err();
        assert!(matches!(e, Erreur::Commande { code: Some(3), .. }), "{e:?}");
    }

    #[test]
    fn etat_absent() {
        let b = Banc::nouveau("\n", false);
        assert_eq!(
            etat_lien_avec_path(&b.chemins, "test", Some(&b.path())).unwrap(),
            EtatLien::Absent
        );
        assert_eq!(
            b.lire("appels-xdg-mime").trim(),
            "query default x-scheme-handler/test"
        );
    }

    #[test]
    fn etat_deja_wimao() {
        let b = Banc::nouveau("wimao-lien-test.desktop\n", false);
        assert_eq!(
            etat_lien_avec_path(&b.chemins, "test", Some(&b.path())).unwrap(),
            EtatLien::DejaWimao
        );
    }

    #[test]
    fn lien_gere_par_un_autre_programme_est_signale_sans_etre_remplace() {
        let b = Banc::nouveau("wimao-native-access.desktop\n", true);
        assert_eq!(
            etat_lien_avec_path(&b.chemins, "native-access", Some(&b.path())).unwrap(),
            EtatLien::Autre("wimao-native-access.desktop".to_string())
        );
        // Seul `query` a été appelé : rien écrit, aucun `default`.
        assert!(!b.applications().exists());
        assert_eq!(
            b.lire("appels-xdg-mime").trim(),
            "query default x-scheme-handler/native-access"
        );
        assert_eq!(b.lire("appels-update"), "");
    }

    #[test]
    fn fichier_desktop_porte_le_schema() {
        assert_eq!(
            fichier_desktop("native-access"),
            "wimao-lien-native-access.desktop"
        );
    }

    #[test]
    fn exec_echappe_les_caracteres_speciaux() {
        assert_eq!(echapper_exec(Path::new("/a b/c")), "\"/a b/c\"");
        assert_eq!(echapper_exec(Path::new("/a\"b$c")), "\"/a\\\\\"b\\\\$c\"");
        assert_eq!(echapper_exec(Path::new("/100%")), "\"/100%%\"");
        // Une barre oblique : `\\` au niveau Exec, doublé dans la valeur = quatre.
        assert_eq!(echapper_exec(Path::new("/a\\b")), "\"/a\\\\\\\\b\"");
    }

    #[test]
    fn commande_relais_cherche_dans_le_path() {
        let b = Banc::nouveau("", false);
        // Ni à côté du binaire de test, ni dans le PATH : introuvable.
        assert!(matches!(
            commande_relais_dans(Some(&b.path())),
            Err(Erreur::Introuvable(_))
        ));
        installer_faux(&b.racine.path().join("faux-bin/wimao-cli"), "#!/bin/sh\n");
        assert_eq!(
            commande_relais_dans(Some(&b.path())).unwrap(),
            b.racine.path().join("faux-bin/wimao-cli")
        );
    }
}
