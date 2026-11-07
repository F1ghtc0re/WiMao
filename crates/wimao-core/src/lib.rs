//! Noyau de WiMao : types et utilitaires partagés par toutes les autres crates.
//!
//! - [`erreur`] : l'erreur unique du projet ;
//! - [`chemins`] : emplacements sur disque (données, Wine, yabridge) ;
//! - [`progression`] : comment une longue opération annonce ses étapes ;
//! - [`outils`] : lancement de commandes externes avec délai maximal ;
//! - [`wine`] : commandes Wine et registre ;
//! - [`telechargement`] : téléchargements vérifiés par SHA-256 ;
//! - [`correctifs`] : modifications de l'environnement Wine (Wine Mono, DXVK, msvcp140) ;
//! - [`environnement`] : création de l'environnement Wine ;
//! - [`i18n`] : traductions (`tr`, `trf`, `init_langue`) ;
//! - [`inventaire`] : relevé des plugins Windows d'un préfixe ;
//! - [`pontage`] : pontage yabridge isolé (doublons VST2, plugins déjà pontés) ;
//! - [`processus`] : processus d'un environnement Wine (lecture de `/proc`, arrêt) ;
//! - [`regles`] : règles d'erreurs (texte d'erreur brut vers message compréhensible) ;
//! - [`registre_fichier`] : lecture des `.reg` de Wine sans lancer Wine ;
//! - [`diagnostic`] : diagnostic, réparation et verrou d'installation ;
//! - [`restauration`] : points de restauration de l'environnement Wine (registre + liste de fichiers) ;
//! - [`journal`] : journal des commandes lancées (`journal.log`, 5 Mo) ;
//! - [`historique`] : journal des actions de l'utilisateur (`historique.jsonl`) ;
//! - [`import`] : import de l'environnement Wine existant `~/.wine-mao` (déplacement + lien) et annulation ;
//! - [`liens`] : liens de connexion (`native-access://`) déclarés au bureau Linux ;
//! - [`rapport`] : rapport de problème anonymisé ;
//! - [`sauvegarde`] : sauvegarde et restauration de l'environnement Wine (`tar` + `zstd`) ;
//! - [`tableau`] : verdict ✅/⚠️/❌ et ligne d'un plugin, communs à la CLI et à l'interface ;
//! - [`test_plugin`] : test d'ouverture d'un plugin sur écran virtuel (Xvfb + Carla) ;
//!
//! Voir `ARCHITECTURE.md` à la racine du dépôt pour la vue d'ensemble.
#![warn(missing_docs)]

pub mod chemins;
pub mod correctifs;
pub mod diagnostic;
pub mod environnement;
pub mod erreur;
pub mod historique;
pub mod i18n;
pub mod import;
pub mod inventaire;
pub mod journal;
pub mod liens;
pub mod outils;
pub mod pontage;
pub mod processus;
pub mod progression;
pub mod rapport;
pub mod registre_fichier;
pub mod regles;
pub mod restauration;
pub mod sauvegarde;
pub mod tableau;
pub mod telechargement;
pub mod test_plugin;
#[cfg(test)]
mod test_utils;
pub mod wine;

pub use chemins::Chemins;
pub use diagnostic::{
    commande_lien_yabridge, diagnostiquer, installation_en_cours, message_probleme,
    poser_verrou_installation, programmes_a_nommer_avant_reparer, reparer, Probleme,
    VerrouInstallation,
};
pub use environnement::{creer_environnement, mettre_a_jour};
pub use erreur::{Erreur, Resultat};
pub use inventaire::{Format, Inventaire, PluginWindows};
pub use outils::{lancer, Sortie};
pub use progression::{Progression, ProgressionMuette};
pub use regles::expliquer;
pub use telechargement::{empreinte, telecharger_verifie};
pub use test_plugin::{tester_plugin, ResultatTest};
pub use wine::Wine;
