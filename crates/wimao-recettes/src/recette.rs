//! Format d'une recette WiMao et lecture validée d'un fichier de recette.
//!
//! Utilise `serde` et `toml` pour lire le fichier, `wimao-core` pour l'erreur
//! [`Erreur::Recette`] et les traductions. Utilisé par [`crate::catalogue`].
//! Toutes les structures refusent les clés inconnues : une faute de frappe est
//! signalée au lieu d'être ignorée en silence. Référence : `docs/recettes.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use wimao_core::i18n::{tr, trf};
use wimao_core::{Erreur, Resultat};

/// Langues obligatoires de chaque consigne.
const LANGUES_CONSIGNES: [&str; 3] = ["fr", "en", "es"];
/// Langue de secours d'une consigne : toute recette valide l'a (voir `LANGUES_CONSIGNES`).
const LANGUE_DE_SECOURS: &str = "en";
/// Nom de la consigne affichée à l'ouverture d'un gestionnaire (spec 4.2, étape 6), dans
/// l'interface comme dans `wimao-cli gestionnaire ouvrir`.
pub const CONSIGNE_CONNEXION: &str = "connexion";
/// Nom de la consigne affichée après une installation réussie (spec, section 6) : activer
/// une licence, entrer un numéro de série…
pub const CONSIGNE_ACTIVATION: &str = "activation";

/// Longueur d'une empreinte sha256 en caractères hexadécimaux.
const LONGUEUR_EMPREINTE: usize = 64;

/// Préfixe obligatoire d'une adresse de téléchargement (les installeurs sont des exécutables :
/// le HTTP en clair permettrait de les remplacer en route).
const PREFIXE_HTTPS: &str = "https://";

/// Ce que représente le produit décrit par la recette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TypeRecette {
    /// Application qui télécharge et installe elle-même les plugins (Native Access…).
    Gestionnaire,
    /// Installeur d'un seul produit (défaut).
    Installeur,
    /// Archive à décompresser.
    Archive,
}

/// Façon d'installer. Le défaut, [`Methode::Auto`], laisse WiMao reconnaître l'installeur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Methode {
    /// Extraire l'installeur avec 7z, puis appliquer les étapes `[[installation.extraire]]`.
    #[serde(rename = "extraire-7z")]
    Extraire7z,
    /// Application « Squirrel » (inMusic Software Center) : installation reprise de `NOTES.md`.
    Squirrel,
    /// Inno Setup.
    Inno,
    /// Paquet MSI.
    Msi,
    /// WiX / Burn.
    Wix,
    /// NSIS.
    Nsis,
    /// InstallAware.
    Installaware,
    /// Reconnaissance automatique (défaut).
    #[default]
    Auto,
}

/// Une étape `[[installation.extraire]]` de la méthode `extraire-7z`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct EtapeExtraction {
    /// Chemin de l'archive relatif au dossier décompressé (`*` admis, une seule
    /// correspondance exigée), par exemple `$PLUGINSDIR/app-64.7z`.
    pub archive: String,
    /// Dossier Windows où l'archive est décompressée.
    pub vers: String,
}

/// Section `[installation]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
pub struct Installation {
    /// Méthode d'installation.
    pub methode: Methode,
    /// Schéma de lien (`native-access` pour `native-access://`) à relayer vers Wine ;
    /// exige la section `[gestionnaire]`, dont le `programme` reçoit le lien.
    pub lien_connexion: Option<String>,
    /// Étapes d'extraction : obligatoires avec `extraire-7z`, refusées avec toute autre méthode.
    pub extraire: Vec<EtapeExtraction>,
}

/// Une entrée `[[dependances]]` : installeur à lancer après l'installation principale.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Dependance {
    /// Chemin Windows de l'installeur ; `*` admis dans le nom du fichier (pas dans les
    /// dossiers), un seul fichier doit correspondre.
    pub fichier: String,
    /// Méthode d'installation silencieuse ; ni `extraire-7z` ni `squirrel`.
    pub methode: Methode,
}

/// Une valeur de registre à écrire dans l'environnement Wine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct EntreeRegistre {
    /// Clé, par exemple `HKCU\Software\Éditeur\Produit`.
    pub cle: String,
    /// Nom de la valeur.
    pub valeur: String,
    /// Donnée ; accepte la variable `{telechargements}` (voir `crate::chemins_windows::remplacer_variables`).
    pub donnee: String,
}

/// Section `[correctifs]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
pub struct Correctifs {
    /// Valeurs de registre à écrire.
    pub registre: Vec<EntreeRegistre>,
    /// Services Windows à démarrer à chaque ouverture du gestionnaire.
    pub services: Vec<String>,
    /// Dossiers Windows à créer s'ils manquent (installation et chaque ouverture).
    pub dossiers: Vec<String>,
}

/// Section `[gestionnaire]` : comment ouvrir l'application de l'éditeur.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Gestionnaire {
    /// Exécutable Windows à lancer ; `*` admis dans un dossier (`app-*` de Squirrel),
    /// la version la plus haute est retenue.
    pub programme: String,
    /// Arguments ajoutés au lancement, par exemple `["--no-sandbox"]`.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// Dossiers Windows où le gestionnaire dépose les installeurs qu'il lance ; non vide,
    /// le pilote est actif.
    #[serde(default)]
    pub pilote: Vec<String>,
    /// Noms d'exécutables à arrêter à la fermeture de la session (défaut : celui de `programme`).
    #[serde(default)]
    pub fermer: Vec<String>,
}

impl Gestionnaire {
    /// Noms des processus à arrêter : `fermer` s'il est renseigné, sinon le nom de fichier
    /// de `programme` (le chemin est Windows : on coupe sur `\` et `/`).
    pub fn noms_a_fermer(&self) -> Vec<String> {
        if !self.fermer.is_empty() {
            return self.fermer.clone();
        }
        let nom = self
            .programme
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&self.programme);
        vec![nom.to_string()]
    }
}

/// Une recette complète, telle qu'écrite dans le fichier TOML.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Recette {
    /// Nom du produit (obligatoire) ; sert aussi d'identité dans le catalogue.
    pub nom: String,
    /// Éditeur (facultatif, vide par défaut).
    #[serde(default)]
    pub editeur: String,
    /// Adresse de téléchargement de l'installeur.
    #[serde(default)]
    pub telecharger: Option<String>,
    /// Empreinte sha256 attendue de l'installeur téléchargé (64 caractères hexadécimaux,
    /// stockée en minuscules).
    #[serde(default)]
    pub empreinte: Option<String>,
    /// Type de produit ; `installeur` par défaut. Le mot-clé `type` est réservé en Rust.
    #[serde(rename = "type", default = "type_par_defaut")]
    pub type_: TypeRecette,
    /// Motifs de noms de fichiers (`*`, `?`, sans tenir compte des majuscules).
    /// Obligatoire : au moins un motif.
    #[serde(default)]
    pub reconnaitre: Vec<String>,
    /// Section `[installation]`.
    #[serde(default)]
    pub installation: Installation,
    /// Section `[correctifs]`.
    #[serde(default)]
    pub correctifs: Correctifs,
    /// Sections `[[dependances]]` : installeurs à lancer après l'installation principale.
    #[serde(default)]
    pub dependances: Vec<Dependance>,
    /// Section `[gestionnaire]` (obligatoire si `type = "gestionnaire"`, refusée sinon).
    #[serde(default)]
    pub gestionnaire: Option<Gestionnaire>,
    /// Textes affichés à l'utilisateur : nom de la consigne, puis langue, puis texte.
    #[serde(default)]
    pub consignes: BTreeMap<String, BTreeMap<String, String>>,
    /// Fichier d'où la recette a été lue (rempli par [`charger`], absent du TOML) ; sert à
    /// retrouver une recette par le nom de son fichier (`gestionnaire installer native-access`).
    #[serde(skip)]
    pub fichier: Option<PathBuf>,
}

/// Consigne `nom` de la recette (section `[consignes]`) dans `langue` (code `fr`, `es`…), ou en
/// anglais si cette langue manque ; `None` si la recette n'a pas cette consigne.
///
/// Les noms que WiMao affiche lui-même sont [`CONSIGNE_CONNEXION`] et [`CONSIGNE_ACTIVATION`]
/// (voir `docs/recettes.md`, section `[consignes]`).
pub fn consigne(recette: &Recette, nom: &str, langue: &str) -> Option<String> {
    let textes = recette.consignes.get(nom)?;
    textes
        .get(langue)
        .or_else(|| textes.get(LANGUE_DE_SECOURS))
        .cloned()
}

/// Valeur par défaut de `type` (serde exige une fonction).
fn type_par_defaut() -> TypeRecette {
    TypeRecette::Installeur
}

/// Lit et valide un fichier de recette.
///
/// Erreur : [`Erreur::Recette`] avec la ligne en cause quand le TOML est invalide, une clé
/// est inconnue ou une valeur est incorrecte ; sans ligne pour les contrôles de fond
/// (`reconnaitre` vide, consigne sans `fr`/`en`/`es`, motif illisible).
pub fn charger(fichier: &Path) -> Resultat<Recette> {
    let texte = std::fs::read_to_string(fichier).map_err(|e| {
        erreur(
            fichier,
            None,
            trf(
                "Cannot read the recipe: {error}",
                &[("error", &e.to_string())],
            ),
        )
    })?;
    let mut recette: Recette = toml::from_str(&texte).map_err(|e| {
        let ligne = e.span().map(|s| ligne_du_decalage(&texte, s.start));
        erreur(fichier, ligne, e.message().to_string())
    })?;
    valider(&recette).map_err(|message| erreur(fichier, None, message))?;
    // Après validation : l'empreinte est stockée en minuscules, comme `sha256sum` l'écrit.
    recette.empreinte = recette.empreinte.map(|e| e.to_ascii_lowercase());
    recette.fichier = Some(fichier.to_path_buf());
    Ok(recette)
}

/// Numéro de ligne (à partir de 1) d'un décalage en octets : on compte les `\n` avant lui.
fn ligne_du_decalage(texte: &str, decalage: usize) -> usize {
    let debut = decalage.min(texte.len());
    texte.as_bytes()[..debut]
        .iter()
        .filter(|&&o| o == b'\n')
        .count()
        + 1
}

/// Construit l'erreur de recette.
fn erreur(fichier: &Path, ligne: Option<usize>, message: String) -> Erreur {
    Erreur::Recette {
        fichier: fichier.to_path_buf(),
        ligne,
        message,
    }
}

/// Contrôles que le format TOML seul ne sait pas exprimer ; renvoie le message d'échec.
fn valider(recette: &Recette) -> Result<(), String> {
    if recette.reconnaitre.is_empty() {
        return Err(tr(
            "The recipe must list at least one pattern in `reconnaitre`",
        ));
    }
    for motif in &recette.reconnaitre {
        if let Err(e) = glob::Pattern::new(motif) {
            return Err(trf(
                "Invalid file name pattern \"{pattern}\": {error}",
                &[("pattern", motif), ("error", &e.to_string())],
            ));
        }
    }
    for (nom, textes) in &recette.consignes {
        for langue in LANGUES_CONSIGNES {
            if !textes.contains_key(langue) {
                return Err(trf(
                    "The instruction \"{name}\" has no text in the language \"{language}\"",
                    &[("name", nom), ("language", langue)],
                ));
            }
        }
    }
    valider_installation(recette)?;
    valider_gestionnaire(recette)?;
    valider_telechargement(recette)
}

/// Règles 1 et 4 : étapes `extraire` cohérentes avec la méthode, dépendances sans
/// `extraire-7z` ni `squirrel` (elles n'ont pas d'archive à ouvrir).
fn valider_installation(recette: &Recette) -> Result<(), String> {
    let installation = &recette.installation;
    let extraction = installation.methode == Methode::Extraire7z;
    if extraction && installation.extraire.is_empty() {
        return Err(tr(
            "`methode = \"extraire-7z\"` needs at least one [[installation.extraire]] step",
        ));
    }
    if !extraction && !installation.extraire.is_empty() {
        return Err(tr(
            "[[installation.extraire]] steps are only allowed with `methode = \"extraire-7z\"`",
        ));
    }
    for dependance in &recette.dependances {
        let nom = match dependance.methode {
            Methode::Extraire7z => "extraire-7z",
            Methode::Squirrel => "squirrel",
            _ => continue,
        };
        return Err(trf(
            "A [[dependances]] entry cannot use `methode = \"{method}\"`",
            &[("method", nom)],
        ));
    }
    Ok(())
}

/// Règles 2 et 3 : `[gestionnaire]` présente si et seulement si `type = "gestionnaire"`,
/// et `lien-connexion` (schéma `[a-z0-9+.-]`) seulement avec cette section.
fn valider_gestionnaire(recette: &Recette) -> Result<(), String> {
    let est_gestionnaire = recette.type_ == TypeRecette::Gestionnaire;
    if est_gestionnaire && recette.gestionnaire.is_none() {
        return Err(tr(
            "A recipe with `type = \"gestionnaire\"` needs a [gestionnaire] section",
        ));
    }
    if !est_gestionnaire && recette.gestionnaire.is_some() {
        return Err(tr(
            "The [gestionnaire] section is only allowed with `type = \"gestionnaire\"`",
        ));
    }
    if let Some(schema) = &recette.installation.lien_connexion {
        if recette.gestionnaire.is_none() {
            return Err(tr(
                "`lien-connexion` needs a [gestionnaire] section: its `programme` receives the link",
            ));
        }
        let valide = !schema.is_empty()
            && schema
                .chars()
                .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '+' | '.' | '-'));
        if !valide {
            return Err(trf(
                "Invalid `lien-connexion` \"{scheme}\": only a-z, 0-9, +, . and - are allowed",
                &[("scheme", schema)],
            ));
        }
    }
    Ok(())
}

/// Règles 5 et 6 : `telecharger` en HTTPS, `empreinte` en sha256 hexadécimal.
fn valider_telechargement(recette: &Recette) -> Result<(), String> {
    if let Some(adresse) = &recette.telecharger {
        if !adresse.starts_with(PREFIXE_HTTPS) {
            return Err(trf(
                "`telecharger` must start with https:// (got \"{address}\")",
                &[("address", adresse)],
            ));
        }
    }
    if let Some(empreinte) = &recette.empreinte {
        let valide = empreinte.len() == LONGUEUR_EMPREINTE
            && empreinte.chars().all(|c| c.is_ascii_hexdigit());
        if !valide {
            return Err(tr(
                "`empreinte` must be a sha256 hash: 64 hexadecimal characters",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fichier_de_test(nom: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/recettes")
            .join(nom)
    }

    /// Écrit `texte` dans un fichier temporaire et le charge (pour les tests de validation).
    fn charger_texte(texte: &str) -> Resultat<Recette> {
        let dossier = tempfile::tempdir().unwrap();
        let f = dossier.path().join("a.toml");
        std::fs::write(&f, texte).unwrap();
        charger(&f)
    }

    /// Recette minimale valide, suivie de `suite` (clés ou sections à tester).
    fn avec(suite: &str) -> String {
        format!("nom = \"A\"\nreconnaitre = [\"a*\"]\n{suite}")
    }

    /// Vérifie que `texte` est refusée par `Erreur::Recette` et que le message nomme `cle`.
    fn assert_refus(texte: &str, cle: &str) {
        match charger_texte(texte) {
            Err(Erreur::Recette { message, .. }) => {
                assert!(message.contains(cle), "« {cle} » absent de : {message}");
            }
            autre => panic!("attendu Erreur::Recette, obtenu {autre:?}"),
        }
    }

    #[test]
    fn consigne_dans_la_langue_demandee_ou_en_anglais() {
        let r = charger_texte(&avec(
            "[consignes]\nactivation.fr = \"Activez.\"\nactivation.en = \"Activate.\"\n\
             activation.es = \"Active.\"\n",
        ))
        .unwrap();
        assert_eq!(
            consigne(&r, CONSIGNE_ACTIVATION, "fr").as_deref(),
            Some("Activez.")
        );
        // Langue absente : l'anglais.
        assert_eq!(
            consigne(&r, CONSIGNE_ACTIVATION, "de").as_deref(),
            Some("Activate.")
        );
        // Consigne absente : rien.
        assert_eq!(consigne(&r, CONSIGNE_CONNEXION, "fr"), None);
    }

    #[test]
    fn recette_complete_du_sous_projet_2() {
        let r = charger(&fichier_de_test("valide.toml")).unwrap();
        assert_eq!(r.nom, "Native Access 2");
        assert_eq!(r.type_, TypeRecette::Gestionnaire);
        assert_eq!(r.reconnaitre, vec!["Native Access*.exe"]);
        assert_eq!(
            r.empreinte.as_deref(),
            Some("0f885b509a685d2bbfa652fed26b5fb31d88fbdab0a978c641d1c7b8aa460aa9")
        );
        assert_eq!(r.installation.methode, Methode::Extraire7z);
        assert_eq!(
            r.installation.lien_connexion.as_deref(),
            Some("native-access")
        );
        assert_eq!(
            r.installation.extraire,
            vec![EtapeExtraction {
                archive: "$PLUGINSDIR/app-64.7z".into(),
                vers: r"C:\Program Files\Native Instruments\Native Access".into(),
            }]
        );
        assert_eq!(r.dependances.len(), 1);
        assert_eq!(r.dependances[0].methode, Methode::Installaware);
        assert!(r.dependances[0]
            .fichier
            .contains("NTKDaemon * Setup PC.exe"));
        assert_eq!(r.correctifs.registre[0].valeur, "DownloadLocation");
        assert_eq!(r.correctifs.dossiers, vec![r"{telechargements}\NI"]);
        assert_eq!(r.correctifs.services, vec!["NTKDaemonService"]);
        let g = r.gestionnaire.as_ref().unwrap();
        assert!(g.programme.ends_with("Native Access.exe"));
        assert_eq!(g.arguments, vec!["--no-sandbox", "--disable-gpu"]);
        assert_eq!(g.pilote, vec![r"{telechargements}\NI"]);
        assert!(g.fermer.is_empty());
        assert_eq!(g.noms_a_fermer(), vec!["Native Access.exe"]);
        assert_eq!(
            r.consignes["connexion"]["es"],
            "Inicia sesión en tu cuenta de Native Instruments."
        );
    }

    #[test]
    fn fermer_explicite_remplace_le_nom_du_programme() {
        let g = Gestionnaire {
            programme: r"C:\A\app-*\App.exe".into(),
            arguments: vec![],
            pilote: vec![],
            fermer: vec!["Autre.exe".into()],
        };
        assert_eq!(g.noms_a_fermer(), vec!["Autre.exe"]);
    }

    #[test]
    fn refus_extraire_7z_sans_etape() {
        assert_refus(
            &avec("[installation]\nmethode = \"extraire-7z\"\n"),
            "extraire",
        );
    }

    #[test]
    fn refus_etapes_sans_extraire_7z() {
        let t = avec(
            "[installation]\nmethode = \"inno\"\n[[installation.extraire]]\narchive = \"a.7z\"\nvers = 'C:\\x'\n",
        );
        assert_refus(&t, "extraire-7z");
    }

    #[test]
    fn refus_gestionnaire_sans_section() {
        assert_refus(&avec("type = \"gestionnaire\"\n"), "[gestionnaire]");
    }

    #[test]
    fn refus_section_gestionnaire_hors_gestionnaire() {
        assert_refus(
            &avec("[gestionnaire]\nprogramme = 'C:\\a.exe'\n"),
            "[gestionnaire]",
        );
    }

    #[test]
    fn refus_lien_sans_gestionnaire() {
        assert_refus(
            &avec("[installation]\nlien-connexion = \"native-access\"\n"),
            "lien-connexion",
        );
    }

    #[test]
    fn refus_lien_au_schema_invalide() {
        let t = avec(
            "type = \"gestionnaire\"\n[installation]\nlien-connexion = \"Native_Access\"\n[gestionnaire]\nprogramme = 'C:\\a.exe'\n",
        );
        assert_refus(&t, "lien-connexion");
    }

    #[test]
    fn refus_dependance_squirrel() {
        assert_refus(
            &avec("[[dependances]]\nfichier = 'C:\\a.exe'\nmethode = \"squirrel\"\n"),
            "squirrel",
        );
    }

    #[test]
    fn refus_dependance_extraire_7z() {
        assert_refus(
            &avec("[[dependances]]\nfichier = 'C:\\a.exe'\nmethode = \"extraire-7z\"\n"),
            "extraire-7z",
        );
    }

    #[test]
    fn refus_telechargement_http() {
        assert_refus(
            &avec("telecharger = \"http://exemple.org/a.exe\"\n"),
            "telecharger",
        );
    }

    #[test]
    fn refus_empreinte_invalide() {
        assert_refus(&avec("empreinte = \"abc123\"\n"), "empreinte");
        let non_hexa = "z".repeat(64);
        assert_refus(&avec(&format!("empreinte = \"{non_hexa}\"\n")), "empreinte");
    }

    #[test]
    fn empreinte_majuscules_stockee_en_minuscules() {
        let r = charger_texte(&avec(&format!("empreinte = \"{}\"\n", "AB".repeat(32)))).unwrap();
        assert_eq!(r.empreinte, Some("ab".repeat(32)));
    }

    #[test]
    fn ancienne_cle_lancement_refusee() {
        assert_refus(&avec("[correctifs]\nlancement = []\n"), "lancement");
    }

    #[test]
    fn ancienne_section_apres_installation_refusee() {
        assert_refus(
            &avec("[apres-installation]\npilote = true\n"),
            "apres-installation",
        );
    }

    #[test]
    fn cle_inconnue_donne_la_ligne_et_le_nom() {
        match charger(&fichier_de_test("cle-inconnue.toml")) {
            Err(Erreur::Recette { ligne, message, .. }) => {
                assert_eq!(ligne, Some(3));
                assert!(message.contains("telechargerr"), "{message}");
            }
            autre => panic!("attendu Erreur::Recette, obtenu {autre:?}"),
        }
    }

    #[test]
    fn methode_invalide_refusee() {
        match charger(&fichier_de_test("methode-invalide.toml")) {
            Err(Erreur::Recette { ligne, message, .. }) => {
                assert!(ligne.is_some());
                assert!(message.contains("magie"), "{message}");
            }
            autre => panic!("attendu Erreur::Recette, obtenu {autre:?}"),
        }
    }

    #[test]
    fn consigne_sans_espagnol_refusee() {
        match charger(&fichier_de_test("consigne-sans-es.toml")) {
            Err(Erreur::Recette { message, .. }) => {
                assert!(
                    message.contains("connexion") && message.contains("es"),
                    "{message}"
                );
            }
            autre => panic!("attendu Erreur::Recette, obtenu {autre:?}"),
        }
    }

    #[test]
    fn reconnaitre_vide_refuse() {
        let dossier = tempfile::tempdir().unwrap();
        let f = dossier.path().join("a.toml");
        std::fs::write(&f, "nom = \"A\"\n").unwrap();
        assert!(matches!(
            charger(&f),
            Err(Erreur::Recette { ligne: None, .. })
        ));
    }

    #[test]
    fn motif_illisible_refuse() {
        let dossier = tempfile::tempdir().unwrap();
        let f = dossier.path().join("a.toml");
        std::fs::write(&f, "nom = \"A\"\nreconnaitre = [\"[abc\"]\n").unwrap();
        assert!(matches!(charger(&f), Err(Erreur::Recette { .. })));
    }

    #[test]
    fn fichier_absent_donne_erreur_recette() {
        let r = charger(Path::new("/inexistant/a.toml"));
        assert!(matches!(r, Err(Erreur::Recette { ligne: None, .. })));
    }

    #[test]
    fn ligne_comptee_depuis_le_decalage() {
        assert_eq!(ligne_du_decalage("a\nb\nc", 0), 1);
        assert_eq!(ligne_du_decalage("a\nb\nc", 4), 3);
    }
}
