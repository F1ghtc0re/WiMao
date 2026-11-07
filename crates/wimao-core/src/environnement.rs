//! Création de l'environnement Wine (le « préfixe ») et application des correctifs.
//!
//! Utilise [`Wine`] pour `wineboot`, [`crate::correctifs`] pour ce qui s'installe
//! ensuite, [`Progression`] pour annoncer les étapes et [`restauration`] pour périmer les
//! points d'un environnement remplacé par un neuf. Utilisé par la CLI et
//! l'interface (premier lancement, « Réparer »), et par [`crate::diagnostic`] pour
//! détecter et réparer les dossiers personnels reliés ([`dossiers_relies`]).

use crate::chemins::Chemins;
use crate::correctifs::correctifs;
use crate::erreur::{Erreur, Resultat};
use crate::i18n::{tr, trf};
use crate::progression::Progression;
use crate::restauration;
use crate::wine::{Wine, SURCHARGES_SANS_FENETRES};
use std::path::PathBuf;
use std::time::Duration;

/// Délai de `wineboot -u` : première création d'un préfixe, quelques dizaines de
/// secondes d'ordinaire ; 5 minutes couvrent une machine très lente.
const DELAI_WINEBOOT: Duration = Duration::from_secs(5 * 60);
/// Fichier que `wineboot` écrit en dernier : sa présence prouve un préfixe complet.
const FICHIER_PREFIXE_PRET: &str = "system.reg";
/// Dossiers de chaque utilisateur Wine (`drive_c/users/<u>/`) que `wineboot` relie par
/// un lien symbolique aux dossiers Linux de la personne (`~/Documents`…), quand ils
/// existent (constaté avec Wine 11.20 : seul un dossier Linux existant donne un lien).
/// Les installeurs y déposeraient alors leurs bibliothèques, parfois des dizaines de Go,
/// hors de l'environnement (NOTES.md : « Les installeurs écrivaient dans ~/Documents »,
/// comme `scripts/install-yabridge.sh`). Les noms « My … » sont ceux des Wine antérieurs
/// à la version 5 : inoffensifs à vérifier, utiles si l'environnement vient d'un vieux Wine.
const DOSSIERS_PERSONNELS: [&str; 10] = [
    "Desktop",
    "Documents",
    "Downloads",
    "Music",
    "Pictures",
    "Videos",
    "My Documents",
    "My Music",
    "My Pictures",
    "My Videos",
];

/// Crée l'environnement Wine s'il n'existe pas, puis applique les correctifs manquants.
///
/// Idempotent : un environnement complet n'est pas touché, chaque étape déjà
/// faite est seulement annoncée (« Already done »). Reprend un essai interrompu : préfixe
/// inachevé (sans `system.reg`) recommencé dans le même dossier, liens personnels restants
/// remplacés, correctifs manquants posés. Quand le préfixe est à créer, les points
/// de restauration existants sont d'abord périmés
/// ([`restauration::perimer_tous_les_points`]) : ils décrivent un autre environnement.
///
/// Erreurs : [`Erreur::Io`] (création du dossier), [`Erreur::Introuvable`] (Wine de
/// WiMao absent), [`Erreur::Delai`], ou
/// [`Erreur::Commande`] si `wineboot` échoue ; celles des correctifs.
pub fn creer_environnement(c: &Chemins, p: &dyn Progression) -> Resultat<()> {
    if c.prefixe.join(FICHIER_PREFIXE_PRET).exists() {
        annoncer_deja_fait(p, &tr("Wine environment"));
        // Reprise d'un essai interrompu entre `wineboot` (qui écrit `system.reg` en dernier)
        // et le remplacement des dossiers personnels : le préfixe paraît complet, mais ses
        // liens vers `~/Documents`… seraient restés pour toujours. Sans effet (0 lien)
        // sur un environnement déjà propre ; aucun programme Windows ne tourne ici.
        remplacer_dossiers_relies(c)?;
    } else {
        p.etape(&tr("Creating the Wine environment…"));
        // Un préfixe neuf n'est pas celui que décrivent les points de restauration existants
        // (environnement rendu par l'annulation de l'import, supprimé à la main…) : ils sont
        // périmés **avant** la création, qui peut échouer à mi-chemin.
        restauration::perimer_tous_les_points(c, "création d'un environnement neuf");
        // `lancer_wineboot` arrête Wine avant de rendre la main : aucun programme Windows
        // n'écrit dans ces dossiers pendant le remplacement.
        lancer_wineboot(c)?;
        remplacer_dossiers_relies(c)?;
    }
    for correctif in correctifs() {
        if correctif.est_applique(c)? {
            annoncer_deja_fait(p, correctif.nom());
        } else {
            correctif.appliquer(c, p)?;
        }
    }
    Ok(())
}

/// Met l'environnement à niveau avec le Wine de WiMao : `wineboot -u` (même délai que la
/// création), puis remplacement des dossiers personnels que Wine a pu relier de nouveau.
///
/// Appelée avant un installeur : si Wine a été mis à jour depuis la création du préfixe,
/// `wineboot -u` le met à niveau maintenant, plutôt que de laisser l'installeur le
/// déclencher au milieu de son travail (et lui faire écrire dans `~/Documents`).
/// `lancer_wineboot` arrête Wine avant de rendre la main : aucun programme Windows
/// n'écrit dans ces dossiers pendant le remplacement.
///
/// Erreurs : celles de `lancer_wineboot` ([`Erreur::Introuvable`], [`Erreur::Delai`],
/// [`Erreur::Commande`]) et [`Erreur::Io`] du remplacement des dossiers.
/// `_p` n'annonce rien : l'étape est annoncée par l'appelant, qui connaît le contexte.
pub fn mettre_a_jour(c: &Chemins, _p: &dyn Progression) -> Resultat<()> {
    lancer_wineboot(c)?;
    remplacer_dossiers_relies(c)?;
    Ok(())
}

/// Message « Already done: <nom> ».
fn annoncer_deja_fait(p: &dyn Progression, nom: &str) {
    p.etape(&trf("Already done: {name}", &[("name", nom)]));
}

/// Lance `wineboot -u` sur un dossier de préfixe fraîchement créé.
fn lancer_wineboot(c: &Chemins) -> Resultat<()> {
    // `wineboot` refuse un préfixe dont le dossier n'existe pas (« chdir to … »).
    std::fs::create_dir_all(&c.prefixe)?;
    // Sans fenêtres Mono/Gecko (voir la constante), plus l'isolation éventuelle.
    let mut cmd = Wine::nouveau(c).commande_avec("wineboot", &[SURCHARGES_SANS_FENETRES])?;
    cmd.arg("-u");
    let sortie = crate::outils::lancer(&mut cmd, DELAI_WINEBOOT)?;
    if sortie.code != 0 {
        return Err(Erreur::Commande {
            programme: "wineboot -u".to_string(),
            code: Some(sortie.code),
            sortie: sortie.texte,
        });
    }
    // Le `wineserver` n'écrit les fichiers `.reg` (dont `system.reg`) sur disque qu'à
    // son arrêt : sans cela le préfixe paraîtrait encore absent à l'appel suivant.
    Wine::nouveau(c).arreter()
}

/// Les liens symboliques des dossiers personnels (`Desktop`, `Documents`, `Downloads`,
/// `Music`, `Pictures`, `Videos`, et les « My … » des vieux Wine) dans l'environnement,
/// pour tous les utilisateurs de `drive_c/users`. Lecture seule ; ne suit jamais les liens
/// (`symlink_metadata`). Un environnement sans `drive_c/users` n'en a aucun.
///
/// Erreurs : [`Erreur::Io`] si `drive_c/users` ou un dossier d'utilisateur est illisible.
pub fn dossiers_relies(c: &Chemins) -> Resultat<Vec<PathBuf>> {
    let utilisateurs = c.drive_c().join("users");
    let entrees = match std::fs::read_dir(&utilisateurs) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut liens = Vec::new();
    for entree in entrees {
        let entree = entree?;
        // Seuls les vrais dossiers d'utilisateur Wine sont examinés. `file_type()` ne
        // suit pas les liens : si `users/<u>` était lui-même un lien vers un dossier de
        // la personne (disposition que Wine ne crée pas), on ne le traverse pas, pour ne
        // jamais remplacer un lien qui lui appartient. Un fichier égaré dans `users`
        // est ignoré au lieu de faire échouer tout le diagnostic.
        if !entree.file_type()?.is_dir() {
            continue;
        }
        let dossier = entree.path();
        for nom in DOSSIERS_PERSONNELS {
            let chemin = dossier.join(nom);
            // `symlink_metadata` : renseigne sur le lien lui-même, sans le suivre.
            match std::fs::symlink_metadata(&chemin) {
                Ok(m) if m.file_type().is_symlink() => liens.push(chemin),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    liens.sort();
    Ok(liens)
}

/// Remplace chaque lien de [`dossiers_relies`] par un vrai dossier vide de
/// l'environnement. Renvoie le nombre de liens remplacés.
///
/// Seul le **lien** est supprimé (`remove_file` sur un lien ne touche jamais sa
/// cible) : le contenu des dossiers Linux de la personne reste intact, y compris ce
/// qu'un installeur y aurait déjà déposé. Appelé après `wineboot` et par la
/// réparation du diagnostic (`wineboot -u`, lancé par Wine après une mise à jour,
/// recrée ces liens). Erreurs : [`Erreur::Io`].
pub fn remplacer_dossiers_relies(c: &Chemins) -> Resultat<usize> {
    let liens = dossiers_relies(c)?;
    for lien in &liens {
        std::fs::remove_file(lien)?;
        std::fs::create_dir(lien)?;
    }
    Ok(liens.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::correctifs::dxvk::Dxvk;
    use crate::correctifs::mono::Mono;
    use crate::correctifs::msvcp::Msvcp140;
    use crate::correctifs::Correctif;
    use std::os::unix::fs::symlink;
    use std::sync::Mutex;

    /// Progression qui garde les messages reçus.
    #[derive(Default)]
    struct Enregistreuse(Mutex<Vec<String>>);

    impl Progression for Enregistreuse {
        fn etape(&self, message: &str) {
            if let Ok(mut v) = self.0.lock() {
                v.push(message.to_string());
            }
        }
    }

    /// Arrête le `wineserver` à la fin du test, même si une assertion échoue.
    struct ArretWine<'a>(&'a Chemins);

    impl Drop for ArretWine<'_> {
        fn drop(&mut self) {
            let _ = Wine::nouveau(self.0).arreter();
        }
    }

    #[test]
    fn les_liens_deviennent_de_vrais_dossiers_et_la_cible_reste_intacte() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        // Dossier « Linux » de la personne, avec un fichier qui ne doit pas bouger.
        let cible = d.path().join("vrai-home/Documents");
        std::fs::create_dir_all(&cible).unwrap();
        std::fs::write(cible.join("garde.txt"), "à garder").unwrap();
        let u = c.drive_c().join("users/jeanne");
        std::fs::create_dir_all(u.join("Music")).unwrap(); // déjà un vrai dossier
        symlink(&cible, u.join("Documents")).unwrap();
        symlink(&cible, u.join("Desktop")).unwrap();
        symlink(d.path().join("absent"), u.join("Videos")).unwrap(); // lien cassé

        let attendus = vec![u.join("Desktop"), u.join("Documents"), u.join("Videos")];
        assert_eq!(dossiers_relies(&c).unwrap(), attendus);
        assert_eq!(remplacer_dossiers_relies(&c).unwrap(), 3);

        for dossier in &attendus {
            let m = std::fs::symlink_metadata(dossier).unwrap();
            assert!(m.is_dir() && !m.file_type().is_symlink(), "{dossier:?}");
            assert_eq!(std::fs::read_dir(dossier).unwrap().count(), 0);
        }
        assert_eq!(
            std::fs::read_to_string(cible.join("garde.txt")).unwrap(),
            "à garder"
        );
        assert!(dossiers_relies(&c).unwrap().is_empty());
        assert_eq!(remplacer_dossiers_relies(&c).unwrap(), 0); // idempotent
    }

    #[test]
    fn un_utilisateur_relie_ou_un_fichier_dans_users_n_est_pas_traverse() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        // Dossier personnel de la personne, avec SON lien Documents vers un autre disque :
        // ce lien-là ne doit jamais être remplacé.
        let home_personne = d.path().join("vrai-home");
        let autre_disque = d.path().join("autre-disque");
        std::fs::create_dir_all(&home_personne).unwrap();
        std::fs::create_dir_all(&autre_disque).unwrap();
        symlink(&autre_disque, home_personne.join("Documents")).unwrap();
        let users = c.drive_c().join("users");
        std::fs::create_dir_all(&users).unwrap();
        symlink(&home_personne, users.join("relie")).unwrap();
        std::fs::write(users.join("egare.txt"), "fichier").unwrap();

        assert!(dossiers_relies(&c).unwrap().is_empty());
        assert_eq!(remplacer_dossiers_relies(&c).unwrap(), 0);
        let lien = std::fs::symlink_metadata(home_personne.join("Documents")).unwrap();
        assert!(lien.file_type().is_symlink());
    }

    /// Relecture finale I2 : un environnement neuf (après l'annulation d'un import, ou un
    /// préfixe supprimé à la main) ne doit pas garder des points qui décrivent l'ancien : y
    /// revenir remettrait le registre de l'ancien sur le neuf et supprimerait ses fichiers.
    #[test]
    fn un_environnement_neuf_perime_les_points_existants() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(d.path());
        // Faux Wine : seulement `wineserver` (pour `restauration::creer`), jamais le vrai Wine.
        c.wine_dir = d.path().join("faux-wine");
        crate::test_utils::ecrire_executable(
            &c.wine_bin().join("wineserver"),
            "#!/bin/sh\nexit 0\n",
        );
        std::fs::create_dir_all(c.drive_c()).unwrap();
        for nom in ["system.reg", "user.reg", "userdef.reg"] {
            std::fs::write(c.prefixe.join(nom), nom).unwrap();
        }
        let point = crate::restauration::creer(&c, "avant").unwrap();
        // L'environnement décrit par le point s'en va.
        std::fs::remove_dir_all(&c.prefixe).unwrap();
        // Sans `wineboot` factice, la création échoue : les points doivent être périmés avant.
        creer_environnement(&c, &Enregistreuse::default()).unwrap_err();
        let liste = crate::restauration::lister(&c).unwrap();
        assert!(liste.points.is_empty(), "{liste:?}");
        assert_eq!(liste.perimes.len(), 1, "{liste:?}");
        assert_eq!(liste.perimes[0].id, point.id);
    }

    /// Faux Wine pour les tests de reprise : `wineboot` note chaque appel dans `appels`
    /// puis écrit `system.reg` (comme le vrai, en dernier) ; `wineserver` ne fait rien.
    /// Aucun correctif ne peut s'appliquer ensuite (pas de `wine`) : `creer_environnement`
    /// échoue au premier, ce qui laisse observer l'état laissé par le début de la création.
    fn installer_faux_wine(c: &mut Chemins, racine: &std::path::Path) -> PathBuf {
        c.wine_dir = racine.join("faux-wine");
        let appels = racine.join("appels-wineboot");
        crate::test_utils::ecrire_executable(
            &c.wine_bin().join("wineserver"),
            "#!/bin/sh\nexit 0\n",
        );
        crate::test_utils::ecrire_executable(
            &c.wine_bin().join("wineboot"),
            &format!(
                "#!/bin/sh\necho appel >> '{}'\nmkdir -p \"$WINEPREFIX/drive_c\"\n\
                 echo reg > \"$WINEPREFIX/system.reg\"\n",
                appels.display()
            ),
        );
        appels
    }

    /// Nombre de lancements de `wineboot` relevés par le faux Wine.
    fn nombre_d_appels(appels: &std::path::Path) -> usize {
        std::fs::read_to_string(appels).map_or(0, |t| t.lines().count())
    }

    /// Cas du défaut reporté : un essai précédent a échoué avant que `wineboot` n'ait fini
    /// (dossier du préfixe présent, `drive_c` entamé, pas de `system.reg`). « Créer » doit
    /// reprendre dans CE dossier : un seul `wineboot`, pas de second préfixe, et les points de
    /// restauration (qui ne décrivent pas ce préfixe inachevé) sont périmés comme pour tout
    /// préfixe neuf.
    #[test]
    fn creer_reprend_un_prefixe_interrompu_avant_la_fin_de_wineboot() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(d.path());
        let appels = installer_faux_wine(&mut c, d.path());
        std::fs::create_dir_all(c.drive_c().join("windows")).unwrap();
        // Un point pris sur un ancien environnement complet, puis l'essai raté qui a laissé
        // un préfixe inachevé à sa place (sans `system.reg`, que `wineboot` écrit en dernier).
        for nom in ["system.reg", "user.reg", "userdef.reg"] {
            std::fs::write(c.prefixe.join(nom), nom).unwrap();
        }
        let point = crate::restauration::creer(&c, "avant").unwrap();
        std::fs::remove_file(c.prefixe.join(FICHIER_PREFIXE_PRET)).unwrap();

        // Échoue ensuite au premier correctif (pas de `wine`), mais APRÈS wineboot.
        creer_environnement(&c, &Enregistreuse::default()).unwrap_err();
        assert_eq!(nombre_d_appels(&appels), 1);
        assert!(c.prefixe.join(FICHIER_PREFIXE_PRET).exists());
        // Un seul préfixe : rien de voisin dans le dossier des données.
        let voisins: Vec<_> = std::fs::read_dir(&c.donnees)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert!(voisins.contains(&"prefixe".into()), "{voisins:?}");
        assert_eq!(
            voisins
                .iter()
                .filter(|n| n.to_string_lossy().contains("prefixe"))
                .count(),
            1,
            "{voisins:?}"
        );
        let liste = crate::restauration::lister(&c).unwrap();
        assert_eq!(liste.perimes.len(), 1, "{liste:?}");
        assert_eq!(liste.perimes[0].id, point.id);
    }

    /// Cas du défaut reporté, second visage : `wineboot` a fini (`system.reg` présent) mais
    /// l'essai précédent s'est arrêté avant le remplacement des dossiers personnels ou les
    /// correctifs. « Créer » ne relance pas `wineboot`, finit le remplacement des liens
    /// (sinon ils resteraient pour toujours, le préfixe paraissant complet), et ne périme pas
    /// les points : l'environnement est bien celui qu'ils décrivent.
    #[test]
    fn creer_reprend_un_prefixe_dont_wineboot_est_fini() {
        let d = tempfile::tempdir().unwrap();
        let mut c = Chemins::pour_test(d.path());
        let appels = installer_faux_wine(&mut c, d.path());
        for nom in ["system.reg", "user.reg", "userdef.reg"] {
            std::fs::create_dir_all(c.drive_c()).unwrap();
            std::fs::write(c.prefixe.join(nom), nom).unwrap();
        }
        let cible = d.path().join("vrai-home/Documents");
        std::fs::create_dir_all(&cible).unwrap();
        let lien = c.drive_c().join("users/jeanne/Documents");
        std::fs::create_dir_all(lien.parent().unwrap()).unwrap();
        symlink(&cible, &lien).unwrap();
        let point = crate::restauration::creer(&c, "avant").unwrap();

        creer_environnement(&c, &Enregistreuse::default()).unwrap_err(); // correctifs
        assert_eq!(
            nombre_d_appels(&appels),
            0,
            "wineboot ne doit pas être relancé"
        );
        assert!(
            dossiers_relies(&c).unwrap().is_empty(),
            "liens non remplacés"
        );
        let liste = crate::restauration::lister(&c).unwrap();
        assert!(liste.perimes.is_empty(), "{liste:?}");
        assert_eq!(liste.points.len(), 1);
        assert_eq!(liste.points[0].id, point.id);
    }

    #[test]
    fn sans_dossier_users_aucun_lien() {
        let d = tempfile::tempdir().unwrap();
        let c = Chemins::pour_test(d.path());
        assert!(dossiers_relies(&c).unwrap().is_empty());
    }

    #[test]
    #[ignore = "Wine requis"]
    fn creation_idempotente() {
        let d = tempfile::tempdir().unwrap();
        let chemins = Chemins::pour_test(d.path());
        std::fs::create_dir_all(&chemins.home).unwrap();
        let _arret = ArretWine(&chemins);

        // Un `~/Documents` dans le HOME de test : Wine 11 y relie le préfixe.
        std::fs::create_dir_all(chemins.home.join("Documents")).unwrap();
        creer_environnement(&chemins, &Enregistreuse::default()).unwrap();
        assert!(chemins.prefixe.join("system.reg").exists());
        assert!(dossiers_relies(&chemins).unwrap().is_empty());
        assert!(Mono.est_applique(&chemins).unwrap());
        assert!(Dxvk.est_applique(&chemins).unwrap());
        assert!(Msvcp140.est_applique(&chemins).unwrap());

        // Second appel : ni wineboot ni installeur, seulement des « déjà fait »
        // (l'environnement, puis Mono, DXVK et msvcp140).
        let deuxieme = Enregistreuse::default();
        creer_environnement(&chemins, &deuxieme).unwrap();
        let messages = deuxieme.0.lock().unwrap();
        assert_eq!(messages.len(), 4, "{messages:?}");
        assert!(
            messages.iter().all(|m| m.starts_with("Already done")),
            "{messages:?}"
        );
    }

    #[test]
    #[ignore = "Wine requis"]
    fn mettre_a_jour_retire_les_dossiers_relies() {
        let d = tempfile::tempdir().unwrap();
        let chemins = Chemins::pour_test(d.path());
        std::fs::create_dir_all(&chemins.home).unwrap();
        let _arret = ArretWine(&chemins);
        creer_environnement(&chemins, &Enregistreuse::default()).unwrap();

        // Un dossier du HOME de test, vers lequel on relie `Documents` comme le ferait
        // un `wineboot -u` lancé par une mise à jour de Wine.
        let cible = chemins.home.join("Documents");
        std::fs::create_dir_all(&cible).unwrap();
        let users = chemins.drive_c().join("users");
        let utilisateur = std::fs::read_dir(&users)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.join("Desktop").exists() || p.join("Music").exists())
            .expect("un dossier d'utilisateur Wine");
        let lien = utilisateur.join("Documents");
        let _ = std::fs::remove_dir_all(&lien);
        symlink(&cible, &lien).unwrap();
        assert!(dossiers_relies(&chemins).unwrap().contains(&lien));

        mettre_a_jour(&chemins, &Enregistreuse::default()).unwrap();
        assert!(dossiers_relies(&chemins).unwrap().is_empty());
    }
}
