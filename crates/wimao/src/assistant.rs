//! Assistant de premier lancement : l'automate des pages, sans aucun widget.
//!
//! Le parcours est : `Bienvenue` -> `Environnement` (choix entre importer un environnement
//! existant et en créer un neuf) -> `Operation` (travail en arrière-plan) -> `PremierPas`, avec
//! `Echec` si l'opération échoue (on peut réessayer ou revenir au choix).
//!
//! Séparer l'automate de la fenêtre (`assistant_fenetre.rs`) permet de tester le parcours sans
//! affichage : la fenêtre ne fait que montrer la page courante et traduire clics et résultats
//! du travail en [`Evenement`]s.
//!
//! Utilisé par `assistant_fenetre.rs`.
//!
//! Choix non évidents :
//! - un événement qui n'a pas de sens à la page courante est **ignoré** (jamais d'erreur) : un
//!   double clic ou un résultat tardif ne peut pas mettre l'assistant dans un état incohérent ;
//! - pendant `Operation`, `Retour` est ignoré : l'import ou la création ne s'interrompt pas ;
//! - l'existence d'un environnement à importer n'est connue qu'après une détection qui parcourt
//!   tout le dossier (donc en arrière-plan) : l'assistant naît avec `existant = false` puis
//!   [`Assistant::definir_existant`] est appelé une fois la réponse arrivée.

/// Une page de l'assistant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageAssistant {
    /// Accueil : présentation et bouton « Commencer ».
    Bienvenue,
    /// Choix entre importer l'environnement existant et en créer un nouveau.
    Environnement,
    /// Import ou création en cours.
    Operation,
    /// L'opération a échoué.
    Echec,
    /// L'environnement est prêt : que faire en premier ?
    PremierPas,
}

/// Ce que la personne décide de faire de l'environnement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoixEnvironnement {
    /// Reprendre `~/.wine-mao` tel quel (déplacement + lien).
    Importer,
    /// Créer un environnement neuf.
    Creer,
}

/// Ce qui fait avancer l'assistant : un clic ou l'issue du travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evenement {
    /// « Commencer ».
    Commencer,
    /// Un choix de la page `Environnement`.
    Choisir(ChoixEnvironnement),
    /// L'opération s'est bien terminée.
    Reussite,
    /// L'opération a échoué.
    Echec,
    /// « Réessayer » après un échec.
    Reessayer,
    /// Bouton retour.
    Retour,
}

/// L'état de l'assistant : page courante, existence d'un environnement à importer, choix fait.
#[derive(Debug, Clone)]
pub struct Assistant {
    /// Page courante.
    page: PageAssistant,
    /// Vrai si un environnement existant peut être importé.
    existant: bool,
    /// Choix fait à la page `Environnement` (`None` tant qu'aucun).
    choix: Option<ChoixEnvironnement>,
}

impl Assistant {
    /// Un assistant à la page `Bienvenue`. `existant` : un environnement à importer a été trouvé.
    pub fn nouveau(existant: bool) -> Assistant {
        Assistant {
            page: PageAssistant::Bienvenue,
            existant,
            choix: None,
        }
    }

    /// La page courante.
    pub fn page(&self) -> PageAssistant {
        self.page
    }

    /// Les choix offerts à la page `Environnement`, dans l'ordre d'affichage : `[Importer, Creer]`
    /// si un environnement existe, sinon `[Creer]`.
    pub fn choix_proposes(&self) -> Vec<ChoixEnvironnement> {
        if self.existant {
            vec![ChoixEnvironnement::Importer, ChoixEnvironnement::Creer]
        } else {
            vec![ChoixEnvironnement::Creer]
        }
    }

    /// Le choix fait, s'il y en a un.
    pub fn choix(&self) -> Option<ChoixEnvironnement> {
        self.choix
    }

    /// Enregistre le résultat de la détection. Sans effet pendant `Operation` et au-delà : un
    /// choix déjà fait ne doit pas devenir invalide en cours de route.
    pub fn definir_existant(&mut self, existant: bool) {
        if matches!(
            self.page,
            PageAssistant::Bienvenue | PageAssistant::Environnement
        ) {
            self.existant = existant;
        }
    }

    /// Dit si `e` ferait passer l'assistant à la page `Operation` depuis une autre page. La
    /// fenêtre ne lance le travail (import, création) que dans ce cas : un double clic ou un
    /// « Réessayer » pressé deux fois ne démarre pas deux opérations.
    pub fn lance_operation(&self, e: Evenement) -> bool {
        let mut apres = self.clone();
        apres.suivant(e);
        self.page != PageAssistant::Operation && apres.page == PageAssistant::Operation
    }

    /// Applique `e` à la page courante ; un événement inattendu ne change rien.
    pub fn suivant(&mut self, e: Evenement) {
        use Evenement::{Choisir, Commencer, Echec, Reessayer, Retour, Reussite};
        match (self.page, e) {
            (PageAssistant::Bienvenue, Commencer) => self.page = PageAssistant::Environnement,
            (PageAssistant::Environnement, Choisir(x)) if self.choix_proposes().contains(&x) => {
                self.choix = Some(x);
                self.page = PageAssistant::Operation;
            }
            (PageAssistant::Environnement, Retour) => self.page = PageAssistant::Bienvenue,
            (PageAssistant::Operation, Reussite) => self.page = PageAssistant::PremierPas,
            (PageAssistant::Operation, Echec) => self.page = PageAssistant::Echec,
            // Le choix est gardé : « Réessayer » refait la même opération.
            (PageAssistant::Echec, Reessayer) => self.page = PageAssistant::Operation,
            (PageAssistant::Echec, Retour) => self.page = PageAssistant::Environnement,
            // `Retour` pendant `Operation`, et tout le reste : ignoré (voir l'en-tête).
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ChoixEnvironnement::{Creer, Importer};
    use super::Evenement::*;
    use super::PageAssistant::{Bienvenue, Environnement, Operation, PremierPas};
    use super::*;

    #[test]
    fn sans_environnement_existant_seul_creer_est_propose() {
        let a = Assistant::nouveau(false);
        assert_eq!(a.page(), Bienvenue);
        assert_eq!(a.choix_proposes(), [Creer]);
        assert_eq!(Assistant::nouveau(true).choix_proposes(), [Importer, Creer]);
    }

    #[test]
    fn parcours_import_reussi() {
        let mut a = Assistant::nouveau(true);
        a.suivant(Commencer);
        assert_eq!(a.page(), Environnement);
        a.suivant(Choisir(Importer));
        assert_eq!(a.page(), Operation);
        a.suivant(Reussite);
        assert_eq!(a.page(), PremierPas);
        assert_eq!(a.choix(), Some(Importer));
    }

    #[test]
    fn echec_puis_reessayer_garde_le_choix() {
        let mut a = Assistant::nouveau(true);
        a.suivant(Commencer);
        a.suivant(Choisir(Creer));
        a.suivant(Echec);
        assert_eq!(a.page(), PageAssistant::Echec);
        a.suivant(Reessayer);
        assert_eq!(a.page(), Operation);
        assert_eq!(a.choix(), Some(Creer));
        // Echec puis Retour : retour au choix, qu'on peut refaire.
        a.suivant(Echec);
        a.suivant(Retour);
        assert_eq!(a.page(), Environnement);
        a.suivant(Choisir(Importer));
        assert_eq!(a.choix(), Some(Importer));
    }

    #[test]
    fn retour_ignore_pendant_l_operation() {
        let mut a = Assistant::nouveau(false);
        a.suivant(Commencer);
        a.suivant(Choisir(Creer));
        a.suivant(Retour);
        assert_eq!(a.page(), Operation);
    }

    #[test]
    fn choix_non_propose_est_ignore() {
        let mut a = Assistant::nouveau(false);
        a.suivant(Commencer);
        a.suivant(Choisir(Importer));
        assert_eq!(a.page(), Environnement);
        assert_eq!(a.choix(), None);
    }

    #[test]
    fn retour_de_l_environnement_ramene_a_l_accueil() {
        let mut a = Assistant::nouveau(true);
        a.suivant(Commencer);
        a.suivant(Retour);
        assert_eq!(a.page(), Bienvenue);
    }

    #[test]
    fn evenements_inattendus_ne_changent_rien() {
        let mut a = Assistant::nouveau(true);
        for e in [Choisir(Creer), Reussite, Echec, Reessayer, Retour] {
            a.suivant(e);
            assert_eq!(a.page(), Bienvenue, "{e:?} à l'accueil");
        }
        a.suivant(Commencer);
        a.suivant(Choisir(Importer));
        for e in [Commencer, Choisir(Creer), Reessayer] {
            a.suivant(e);
            assert_eq!(a.page(), Operation, "{e:?} pendant l'opération");
        }
        a.suivant(Reussite);
        for e in [Commencer, Echec, Retour, Reessayer, Choisir(Creer)] {
            a.suivant(e);
            assert_eq!(a.page(), PremierPas, "{e:?} à la fin");
        }
    }

    #[test]
    fn seule_une_entree_dans_l_operation_la_lance() {
        let mut a = Assistant::nouveau(true);
        assert!(!a.lance_operation(Choisir(Importer)), "à l'accueil");
        a.suivant(Commencer);
        assert!(a.lance_operation(Choisir(Importer)));
        assert!(!a.lance_operation(Retour));
        a.suivant(Choisir(Importer));
        // Deuxième clic pendant l'opération : refusé, rien ne se relance.
        assert!(!a.lance_operation(Choisir(Importer)));
        assert!(!a.lance_operation(Reessayer));
        a.suivant(Echec);
        assert!(a.lance_operation(Reessayer));
        a.suivant(Reessayer);
        assert!(!a.lance_operation(Reessayer), "second « Réessayer »");
    }

    #[test]
    fn la_detection_tardive_ajoute_l_import_aux_choix() {
        let mut a = Assistant::nouveau(false);
        a.suivant(Commencer);
        a.definir_existant(true);
        assert_eq!(a.choix_proposes(), [Importer, Creer]);
        a.suivant(Choisir(Importer));
        assert_eq!(a.page(), Operation);
        // Une fois l'opération lancée, la détection ne change plus rien.
        a.definir_existant(false);
        assert_eq!(a.choix_proposes(), [Importer, Creer]);
        assert_eq!(a.choix(), Some(Importer));
    }
}
