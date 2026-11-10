//! Le mode d'emploi intégré, partie pure (sans GTK) : les chapitres et le convertisseur Markdown.
//!
//! - [`CHAPITRES`] : les neuf chapitres, chacun avec son texte en `fr`, `en` et `es`
//!   (fichiers `aide/<langue>/NN-<nom>.md`, embarqués par `include_str!`) ; [`texte`] choisit
//!   la langue, avec repli sur le français ;
//! - [`convertir`] : transforme le petit Markdown des chapitres en [`Bloc`]s dont les textes
//!   sont **déjà en balisage Pango** (prêts pour `GtkLabel::use_markup`) ;
//! - [`lien`] : classe l'adresse d'un lien cliqué ([`Lien::Web`] ou [`Lien::Chapitre`]).
//!
//! Choix non évidents :
//! - le mode d'emploi ne passe **pas** par gettext (spec 9.2) : un fichier par langue ;
//! - le Markdown pris en charge est volontairement minuscule (titres `#`/`##`, listes, gras,
//!   code, liens) ; tout le reste est rendu en texte brut, échappé, plutôt que de planter ;
//! - l'échappement Pango est une petite fonction pure ([`echapper`]) plutôt que
//!   `glib::markup_escape_text`, pour que tout ce module se teste sans initialiser GTK ;
//! - les liens `aide:<clé>` ne passent jamais par le système : seuls `https://` et `aide:`
//!   sont reconnus, tout autre schéma (`file://`…) est refusé.
//!
//! Utilisé par `aide_fenetre.rs` (la fenêtre qui affiche ces blocs).

use wimao_core::i18n::tr_noop;

/// Un chapitre du mode d'emploi.
pub struct Chapitre {
    /// Identifiant stable, utilisé dans les liens `aide:<clé>`.
    pub cle: &'static str,
    /// Titre affiché dans la barre latérale : un msgid anglais, marqué par `tr_noop` (pour
    /// que `xgettext` l'extraie) et traduit à l'affichage avec `tr`.
    pub titre_msgid: &'static str,
    /// Le texte Markdown du chapitre, par langue : `(code de langue, contenu)`.
    pub textes: [(&'static str, &'static str); 3],
}

/// Langue de repli : le français est la langue de référence du mode d'emploi.
const LANGUE_DE_REPLI: &str = "fr";

/// Construit un [`Chapitre`] à partir du nom de ses trois fichiers (même nom dans les trois
/// dossiers de langue) : évite de répéter trois `include_str!` par chapitre.
macro_rules! chapitre {
    ($cle:expr, $titre:expr, $fichier:expr) => {
        Chapitre {
            cle: $cle,
            titre_msgid: $titre,
            textes: [
                ("fr", include_str!(concat!("../aide/fr/", $fichier, ".md"))),
                ("en", include_str!(concat!("../aide/en/", $fichier, ".md"))),
                ("es", include_str!(concat!("../aide/es/", $fichier, ".md"))),
            ],
        }
    };
}

/// Les chapitres, dans l'ordre de la barre latérale.
pub const CHAPITRES: [Chapitre; 9] = [
    chapitre!(
        "premiers-pas",
        tr_noop("Getting started"),
        "01-premiers-pas"
    ),
    chapitre!(
        "ajouter-un-plugin",
        tr_noop("Adding a plugin"),
        "02-ajouter-un-plugin"
    ),
    chapitre!(
        "gestionnaires",
        tr_noop("Publisher managers"),
        "03-gestionnaires"
    ),
    chapitre!(
        "logiciel",
        tr_noop("Using plugins in your music software"),
        "04-logiciel-de-musique"
    ),
    chapitre!("reparer", tr_noop("Repair"), "05-reparer"),
    chapitre!(
        "historique",
        tr_noop("History and rollback"),
        "06-historique"
    ),
    chapitre!("sauvegarde", tr_noop("Backup and restore"), "07-sauvegarde"),
    chapitre!(
        "import",
        tr_noop("Importing an existing environment"),
        "08-import"
    ),
    chapitre!(
        "problemes",
        tr_noop("Common problems"),
        "09-problemes-frequents"
    ),
];

/// Le texte du chapitre `cle` dans `langue`, ou en français si cette langue est inconnue ou si
/// son fichier est vide. `None` seulement si le chapitre n'existe pas.
pub fn texte(cle: &str, langue: &str) -> Option<&'static str> {
    let chapitre = CHAPITRES.iter().find(|c| c.cle == cle)?;
    let dans = |code: &str| {
        chapitre
            .textes
            .iter()
            .find(|(l, t)| *l == code && !t.trim().is_empty())
            .map(|(_, t)| *t)
    };
    dans(langue).or_else(|| dans(LANGUE_DE_REPLI))
}

/// Un bloc du chapitre, prêt à afficher.
#[derive(Debug, PartialEq, Eq)]
pub enum Bloc {
    /// Titre de niveau 1 ou 2 (texte en balisage Pango).
    Titre(u8, String),
    /// Paragraphe (texte en balisage Pango).
    Paragraphe(String),
    /// Liste à puces ou numérotée (éléments en balisage Pango).
    Liste {
        /// `true` pour une liste numérotée (`1. …`).
        numerotee: bool,
        /// Un texte par élément.
        elements: Vec<String>,
    },
}

/// Où mène un lien cliqué dans le mode d'emploi.
#[derive(Debug, PartialEq, Eq)]
pub enum Lien {
    /// Une adresse web `https://…`, à ouvrir dans le navigateur.
    Web(String),
    /// Un autre chapitre, désigné par sa clé (`aide:<clé>`).
    Chapitre(String),
}

/// Classe l'adresse d'un lien : `https://…` → [`Lien::Web`], `aide:<clé>` → [`Lien::Chapitre`],
/// tout autre schéma (ou une clé vide) → `None` : le lien est ignoré.
pub fn lien(uri: &str) -> Option<Lien> {
    if uri.starts_with("https://") && uri.len() > "https://".len() {
        return Some(Lien::Web(uri.to_owned()));
    }
    uri.strip_prefix("aide:")
        .filter(|cle| !cle.is_empty())
        .map(|cle| Lien::Chapitre(cle.to_owned()))
}

/// Échappe `& < > " '` pour le balisage Pango (équivalent pur de `glib::markup_escape_text`).
pub fn echapper(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len());
    for c in texte.chars() {
        match c {
            '&' => sortie.push_str("&amp;"),
            '<' => sortie.push_str("&lt;"),
            '>' => sortie.push_str("&gt;"),
            '"' => sortie.push_str("&quot;"),
            '\'' => sortie.push_str("&apos;"),
            autre => sortie.push(autre),
        }
    }
    sortie
}

/// Si `ligne` est un élément numéroté (`12. texte`), rend son texte.
fn element_numerote(ligne: &str) -> Option<&str> {
    let (chiffres, reste) = ligne.split_once(". ")?;
    (!chiffres.is_empty() && chiffres.chars().all(|c| c.is_ascii_digit())).then_some(reste)
}

/// Convertit le texte d'une ligne ou d'un paragraphe : `**gras**`, `` `code` `` et
/// `[texte](adresse)` deviennent du balisage Pango ; **tout le reste est échappé**. Un
/// marqueur sans fermeture (`**a`, `[t](`) reste du texte brut, échappé.
fn en_ligne(texte: &str) -> String {
    let mut sortie = String::new();
    let mut reste = texte;
    while !reste.is_empty() {
        // Gras : `**x**` (x non vide).
        if let Some(apres) = reste.strip_prefix("**") {
            if let Some(fin) = apres.find("**").filter(|&f| f > 0) {
                sortie.push_str(&format!("<b>{}</b>", echapper(&apres[..fin])));
                reste = &apres[fin + 2..];
                continue;
            }
        }
        // Code : `x` (x non vide).
        if let Some(apres) = reste.strip_prefix('`') {
            if let Some(fin) = apres.find('`').filter(|&f| f > 0) {
                sortie.push_str(&format!("<tt>{}</tt>", echapper(&apres[..fin])));
                reste = &apres[fin + 1..];
                continue;
            }
        }
        // Lien : `[texte](adresse)`.
        if let Some(apres) = reste.strip_prefix('[') {
            if let Some((titre, suite)) = apres.split_once("](") {
                if let Some((adresse, suite)) = suite.split_once(')') {
                    if !titre.is_empty() && !adresse.is_empty() {
                        sortie.push_str(&format!(
                            "<a href=\"{}\">{}</a>",
                            echapper(adresse),
                            echapper(titre)
                        ));
                        reste = suite;
                        continue;
                    }
                }
            }
        }
        // Texte ordinaire : un caractère, échappé.
        let mut caracteres = reste.chars();
        if let Some(c) = caracteres.next() {
            sortie.push_str(&echapper(c.encode_utf8(&mut [0; 4])));
        }
        reste = caracteres.as_str();
    }
    sortie
}

/// Convertit un chapitre Markdown en blocs.
///
/// - `# ` et `## ` : titres ; `- ` : liste à puces ; `N. ` : liste numérotée (des lignes de
///   même sorte se regroupent en une seule liste) ;
/// - les autres lignes consécutives forment un paragraphe (jointes par une espace), qu'une
///   ligne vide, un titre ou une liste termine.
pub fn convertir(markdown: &str) -> Vec<Bloc> {
    let mut blocs = Vec::new();
    // Paragraphe en cours de lecture (lignes déjà converties).
    let mut paragraphe: Vec<String> = Vec::new();

    // Termine le paragraphe en cours, s'il y en a un.
    fn vider(paragraphe: &mut Vec<String>, blocs: &mut Vec<Bloc>) {
        if !paragraphe.is_empty() {
            blocs.push(Bloc::Paragraphe(paragraphe.join(" ")));
            paragraphe.clear();
        }
    }
    // Ajoute un élément : à la liste qui vient d'être lue si `continuer` et qu'elle est de même
    // sorte, sinon dans une nouvelle liste.
    fn element(blocs: &mut Vec<Bloc>, continuer: bool, numerotee: bool, texte: String) {
        if continuer {
            if let Some(Bloc::Liste {
                numerotee: n,
                elements,
            }) = blocs.last_mut()
            {
                if *n == numerotee {
                    elements.push(texte);
                    return;
                }
            }
        }
        blocs.push(Bloc::Liste {
            numerotee,
            elements: vec![texte],
        });
    }

    // `liste_ouverte` : la ligne précédente était un élément. Une ligne vide, un titre ou un
    // paragraphe la ferme : deux listes qu'ils séparent restent donc distinctes.
    let mut liste_ouverte = false;
    for ligne in markdown.lines() {
        let ligne = ligne.trim_end();
        let mut est_element = false;
        if ligne.trim().is_empty() {
            vider(&mut paragraphe, &mut blocs);
        } else if let Some(titre) = ligne.strip_prefix("## ") {
            vider(&mut paragraphe, &mut blocs);
            blocs.push(Bloc::Titre(2, en_ligne(titre.trim())));
        } else if let Some(titre) = ligne.strip_prefix("# ") {
            vider(&mut paragraphe, &mut blocs);
            blocs.push(Bloc::Titre(1, en_ligne(titre.trim())));
        } else if let Some((numerotee, texte)) = ligne
            .strip_prefix("- ")
            .map(|t| (false, t))
            .or_else(|| element_numerote(ligne).map(|t| (true, t)))
        {
            vider(&mut paragraphe, &mut blocs);
            element(&mut blocs, liste_ouverte, numerotee, en_ligne(texte.trim()));
            est_element = true;
        } else {
            paragraphe.push(en_ligne(ligne.trim()));
        }
        liste_ouverte = est_element;
    }
    vider(&mut paragraphe, &mut blocs);
    blocs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titres_listes_et_paragraphes() {
        let md = "# A\n\nPremière ligne\nseconde ligne\n\n- x\n- y\n1. z\n";
        assert_eq!(
            convertir(md),
            vec![
                Bloc::Titre(1, "A".into()),
                Bloc::Paragraphe("Première ligne seconde ligne".into()),
                Bloc::Liste {
                    numerotee: false,
                    elements: vec!["x".into(), "y".into()]
                },
                Bloc::Liste {
                    numerotee: true,
                    elements: vec!["z".into()]
                },
            ]
        );
    }

    #[test]
    fn titre_de_niveau_deux_et_listes_separees() {
        let md = "## B\n- a\n\n- b\n";
        assert_eq!(
            convertir(md),
            vec![
                Bloc::Titre(2, "B".into()),
                Bloc::Liste {
                    numerotee: false,
                    elements: vec!["a".into()]
                },
                Bloc::Liste {
                    numerotee: false,
                    elements: vec!["b".into()]
                },
            ]
        );
    }

    #[test]
    fn gras_code_et_liens() {
        assert_eq!(
            convertir("**a** `b` [t](aide:import)"),
            vec![Bloc::Paragraphe(
                "<b>a</b> <tt>b</tt> <a href=\"aide:import\">t</a>".into()
            )]
        );
    }

    #[test]
    fn texte_echappe() {
        assert_eq!(
            convertir("a < b & c"),
            vec![Bloc::Paragraphe("a &lt; b &amp; c".into())]
        );
        // Les guillemets de l'adresse d'un lien sont échappés aussi.
        assert_eq!(en_ligne("[t](a\"b)"), "<a href=\"a&quot;b\">t</a>");
        // Marqueurs non fermés : texte brut.
        assert_eq!(en_ligne("**a `b [c]("), "**a `b [c](");
    }

    #[test]
    fn element_inconnu_en_texte_brut() {
        assert_eq!(
            convertir("> citation\n\n| a | b |"),
            vec![
                Bloc::Paragraphe("&gt; citation".into()),
                Bloc::Paragraphe("| a | b |".into()),
            ]
        );
    }

    #[test]
    fn lien_web_chapitre_et_inconnu() {
        assert_eq!(lien("https://x"), Some(Lien::Web("https://x".into())));
        assert_eq!(lien("aide:import"), Some(Lien::Chapitre("import".into())));
        assert_eq!(lien("file:///etc"), None);
        assert_eq!(lien("aide:"), None);
    }

    #[test]
    fn repli_sur_le_francais() {
        assert_eq!(texte("import", "de"), texte("import", "fr"));
        assert!(texte("import", "en").is_some());
        assert_eq!(texte("inconnu", "fr"), None);
    }

    #[test]
    fn chaque_chapitre_existe_dans_les_trois_langues() {
        for chapitre in &CHAPITRES {
            let langues: Vec<_> = chapitre.textes.iter().map(|(l, _)| *l).collect();
            assert_eq!(langues, ["fr", "en", "es"], "{}", chapitre.cle);
            for (langue, contenu) in chapitre.textes {
                assert!(
                    contenu.starts_with("# "),
                    "{} ({langue}) doit commencer par un titre",
                    chapitre.cle
                );
            }
        }
    }

    /// Les adresses `aide:<clé>` d'un texte Markdown, dans l'ordre (même lecture des liens
    /// `[texte](adresse)` que [`en_ligne`], réduite aux adresses).
    fn liens_internes(markdown: &str) -> Vec<&str> {
        markdown
            .split("](aide:")
            .skip(1)
            .filter_map(|suite| suite.split_once(')').map(|(cle, _)| cle))
            .collect()
    }

    #[test]
    fn liens_internes_valides() {
        // Tout lien `aide:<clé>` des 27 textes mène à un chapitre existant : un lien vers une
        // clé inconnue serait un clic sans effet dans la fenêtre d'aide.
        let cles: Vec<_> = CHAPITRES.iter().map(|c| c.cle).collect();
        let mut nombre = 0;
        for chapitre in &CHAPITRES {
            for (langue, contenu) in chapitre.textes {
                for cle in liens_internes(contenu) {
                    assert!(
                        cles.contains(&cle),
                        "{} ({langue}) : lien vers la clé inconnue « {cle} »",
                        chapitre.cle
                    );
                    nombre += 1;
                }
            }
        }
        // Garde-fou : le test ne doit pas passer « à vide » (textes sans aucun lien interne).
        assert!(nombre > 0, "aucun lien aide:<clé> dans le mode d'emploi");
    }

    #[test]
    fn longueur_des_chapitres_en_francais() {
        // Le plan demande de 150 à 500 mots par chapitre en français (titres et marques
        // Markdown compris : l'écart est de quelques mots, sans effet sur la limite). Le chapitre
        // « gestionnaires » et « logiciel » ont gagné une section (sous-projet 9) : plafond
        // relevé à 700 mots.
        for chapitre in &CHAPITRES {
            let (_, fr) = chapitre.textes[0];
            let mots = fr.split_whitespace().count();
            assert!(
                (150..=700).contains(&mots),
                "{} : {mots} mots en français (attendu : de 150 à 700)",
                chapitre.cle
            );
        }
    }

    #[test]
    fn titre_anglais_du_fichier_egal_au_msgid() {
        // Le titre `# …` du fichier anglais est le msgid de la barre latérale : les trois
        // langues restent ainsi alignées sur les titres de la spec.
        for chapitre in &CHAPITRES {
            let (_, en) = chapitre.textes[1];
            assert_eq!(
                en.lines().next(),
                Some(format!("# {}", chapitre.titre_msgid).as_str()),
                "{}",
                chapitre.cle
            );
        }
    }
}
