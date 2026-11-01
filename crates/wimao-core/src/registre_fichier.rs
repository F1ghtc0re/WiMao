//! Lecteur des fichiers de registre de Wine (`user.reg`, `system.reg`), sans lancer Wine.
//!
//! Sert aux vérifications « le correctif est-il en place ? » (`Correctif::est_applique`)
//! et donc au diagnostic, qui doit rester en lecture seule : interroger `wine reg query`
//! démarre un `wineserver`, ce qui n'est pas anodin (il peut réécrire les fichiers à son
//! arrêt et se confondre avec un Wine « bloqué »). Utilisé par `correctifs::{mono, dll}`.
//! L'écriture, elle, passe toujours par Wine ([`crate::wine::Wine::reg_ajouter`]).
//!
//! # Limite à connaître
//!
//! Le `wineserver` ne réécrit ces fichiers **qu'à son arrêt** (NOTES.md). Une valeur
//! écrite par `reg add` n'y apparaît donc qu'après [`crate::wine::Wine::arreter`] : les
//! correctifs l'appellent à la fin de `appliquer` et de `retirer`, pour que
//! `est_applique` relise un fichier à jour.
//!
//! # Format (extrait réel d'un `user.reg`, Wine 11.20, après `reg add … /v d3d11`)
//!
//! ```text
//! [Software\\Wine\\DllOverrides] 1793458275
//! #time=1dd6947478b7aa0
//! "d3d11"="native,builtin"
//! "we\"ird\\x"="a\"b"
//!
//! [Software\\Wine\\Fonts] 1793458272
//! ```
//!
//! - une section = `[chemin de clé]` puis un horodatage (secondes Unix) sur la même
//!   ligne, une ligne `#time=` (FILETIME en hexadécimal), les valeurs, une ligne vide ;
//! - le chemin écrit chaque `\` de la clé `\\` ; il est relatif à la racine du fichier
//!   (`HKEY_CURRENT_USER` pour `user.reg`, `HKEY_LOCAL_MACHINE` pour `system.reg`) ;
//! - une valeur est `"nom"=donnée` (`@=` pour la valeur par défaut) ; une chaîne est
//!   entre guillemets, avec `\\` pour `\` et `\"` pour `"` ; les autres types sont
//!   `dword:0000000a` ou `hex:aa,bb,\` (les octets se poursuivent sur les lignes suivantes,
//!   chacune terminée par `\` sauf la dernière).
//!
//! Les noms de clés et de valeurs sont comparés sans tenir compte de la casse, comme
//! le registre Windows.

use crate::erreur::Resultat;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Lit la valeur `valeur` de la clé `cle` dans le fichier de registre `fichier`.
///
/// `cle` s'écrit avec des `\` simples et sans racine : `Software\Wine\DllOverrides`.
/// Pour une chaîne, rend le texte sans guillemets et déséchappé (`\\` et `\"`) ; pour
/// un autre type, rend la donnée brute (`dword:0000000a`). Les séquences `\xHHHH`
/// (caractères non ASCII) sont laissées telles quelles.
///
/// Rend `Ok(None)` si le **fichier**, la **clé** ou la **valeur** n'existe pas : c'est
/// un « absent » normal. Toute autre erreur d'entrée/sortie ([`crate::Erreur::Io`]) remonte.
pub fn lire_valeur(fichier: &Path, cle: &str, valeur: &str) -> Resultat<Option<String>> {
    let ouvert = match std::fs::File::open(fichier) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let section = cle.replace('\\', "\\\\").to_lowercase();
    let mut lignes = BufReader::new(ouvert).split(b'\n');
    let mut dans_la_section = false;
    while let Some(ligne) = lignes.next() {
        let ligne = String::from_utf8_lossy(&ligne?)
            .trim_end_matches('\r')
            .to_string();
        if let Some(entete) = ligne.strip_prefix('[') {
            dans_la_section = nom_de_section(entete).to_lowercase() == section;
        } else if dans_la_section {
            if let Some((nom, donnee)) = decouper_valeur(&ligne) {
                if nom.to_lowercase() == valeur.to_lowercase() {
                    return Ok(Some(donnee_lisible(donnee.to_string(), &mut lignes)?));
                }
            }
        }
    }
    Ok(None)
}

/// Chemin de clé d'une ligne d'en-tête (`Software\\Wine\\Fonts] 1793458272`, sans le `[`).
fn nom_de_section(entete: &str) -> &str {
    entete.rfind(']').map_or(entete, |fin| &entete[..fin])
}

/// Sépare `"nom"=donnée` en (nom déséchappé, donnée brute) ; `None` pour une ligne qui
/// n'est pas une valeur (`#time=…`, ligne vide, suite d'octets).
fn decouper_valeur(ligne: &str) -> Option<(String, &str)> {
    if let Some(donnee) = ligne.strip_prefix("@=") {
        return Some(("@".to_string(), donnee));
    }
    let reste = ligne.strip_prefix('"')?;
    let mut nom = String::new();
    let mut caracteres = reste.char_indices();
    while let Some((i, c)) = caracteres.next() {
        match c {
            '\\' => match caracteres.next() {
                Some((_, suivant @ ('\\' | '"'))) => nom.push(suivant),
                Some((_, autre)) => {
                    nom.push('\\');
                    nom.push(autre);
                }
                None => return None,
            },
            '"' => return reste[i + 1..].strip_prefix('=').map(|d| (nom, d)),
            _ => nom.push(c),
        }
    }
    None
}

/// Donnée telle que la voit l'appelant : chaîne déséchappée, ou texte brut pour les
/// autres types (avec leurs lignes de continuation `hex:…,\` regroupées).
fn donnee_lisible(
    donnee: String,
    suite: &mut impl Iterator<Item = std::io::Result<Vec<u8>>>,
) -> Resultat<String> {
    if let Some(chaine) = donnee.strip_prefix('"') {
        let interieur = chaine.strip_suffix('"').unwrap_or(chaine);
        return Ok(deseschapper(interieur));
    }
    let mut complet = donnee;
    while complet.ends_with('\\') {
        complet.pop();
        match suite.next() {
            Some(ligne) => complet.push_str(String::from_utf8_lossy(&ligne?).trim()),
            None => break,
        }
    }
    Ok(complet)
}

/// Remplace `\\` par `\` et `\"` par `"` ; toute autre séquence est conservée.
fn deseschapper(texte: &str) -> String {
    let mut sortie = String::with_capacity(texte.len());
    let mut caracteres = texte.chars().peekable();
    while let Some(c) = caracteres.next() {
        match (c, caracteres.peek()) {
            ('\\', Some(&suivant @ ('\\' | '"'))) => {
                sortie.push(suivant);
                caracteres.next();
            }
            _ => sortie.push(c),
        }
    }
    sortie
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extrait réel (Wine 11.20) d'un `user.reg` d'un préfixe de test, plus une valeur
    /// hexadécimale en continuation et un `dword`.
    const EXTRAIT: &str = "WINE REGISTRY Version 2\n\
;; All keys relative to REGISTRY\\\\User\\\\S-1-5-21-0-0-0-1000\n\
\n\
#arch=win64\n\
\n\
[Software\\\\Wine\\\\DllOverrides] 1793458275\n\
#time=1dd6947478b7aa0\n\
\"d3d11\"=\"native,builtin\"\n\
\"dxgi\"=\"builtin\"\n\
\"we\\\"ird\\\\x\"=\"a\\\"b\"\n\
\"binaire\"=hex:01,02,\\\n\
  03,04\n\
\"nombre\"=dword:0000000a\n\
@=\"par defaut\"\n\
\n\
[Software\\\\Wine\\\\Fonts] 1793458272\n\
#time=1dd6947459de64c\n\
\"d3d11\"=\"autre cle\"\n";

    fn lire(cle: &str, valeur: &str) -> Option<String> {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("user.reg");
        std::fs::write(&f, EXTRAIT).unwrap();
        lire_valeur(&f, cle, valeur).unwrap()
    }

    const CLE: &str = r"Software\Wine\DllOverrides";

    #[test]
    fn lit_une_chaine_dans_la_bonne_section() {
        assert_eq!(lire(CLE, "d3d11").as_deref(), Some("native,builtin"));
        assert_eq!(lire(CLE, "dxgi").as_deref(), Some("builtin"));
        assert_eq!(
            lire(r"Software\Wine\Fonts", "d3d11").as_deref(),
            Some("autre cle")
        );
    }

    #[test]
    fn la_casse_est_ignoree() {
        assert_eq!(
            lire(r"SOFTWARE\wine\dlloverrides", "D3D11").as_deref(),
            Some("native,builtin")
        );
    }

    #[test]
    fn echappements_dans_le_nom_et_la_donnee() {
        assert_eq!(lire(CLE, "we\"ird\\x").as_deref(), Some("a\"b"));
    }

    #[test]
    fn valeurs_non_chaine_et_par_defaut() {
        assert_eq!(lire(CLE, "nombre").as_deref(), Some("dword:0000000a"));
        assert_eq!(lire(CLE, "binaire").as_deref(), Some("hex:01,02,03,04"));
        assert_eq!(lire(CLE, "@").as_deref(), Some("par defaut"));
    }

    #[test]
    fn absent_donne_none() {
        assert_eq!(lire(CLE, "d3d9"), None);
        assert_eq!(lire(r"Software\Wine\Inconnue", "d3d11"), None);
        // `#time=` ressemble à une valeur mais n'en est pas une.
        assert_eq!(lire(CLE, "#time"), None);
    }

    #[test]
    fn fichier_absent_donne_none_mais_autre_erreur_remonte() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            lire_valeur(&d.path().join("absent.reg"), CLE, "x").unwrap(),
            None
        );
        // Un dossier à la place du fichier : lecture impossible, ce n'est pas un « absent ».
        assert!(lire_valeur(d.path(), CLE, "x").is_err());
    }
}
