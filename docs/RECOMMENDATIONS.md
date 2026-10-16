# Recommandations de lieux : feuille de route

Objectif : aider un groupe qui ne sait pas encore où aller dans le pays, pas seulement
compléter un voyage déjà ébauché.

## Constat (octobre 2026)

- Les suggestions ne cherchent qu'autour des hébergements et des lieux du voyage
  (`suggest.rs`, rayon de 1,5 à 40 km) : un voyage vide renvoie `no_anchor` et une liste vide,
  au moment où l'aide serait la plus utile.
- Les goûts du groupe ne comptent qu'à partir de 2 avis par catégorie : rien au départ.
- La notoriété (langues Wikipédia) est demandée à Wikidata à chaque calcul, pour les
  candidats proches seulement : impossible de classer tout le pays.
- Un seul extrait OSM (Costa Rica) ; le voyage n'a pas de pays de destination.

## Étapes

1. **Découverte du pays** — *fait*
   - Notoriété Wikidata stockée par élément (`wikidata_notability`), rafraîchie en tâche de
     fond après l'import OSM : un classement national devient une simple requête SQL.
   - Sans point de départ, les suggestions classent les lieux de tout l'extrait (notoriété,
     diversité, données connues) au lieu de renvoyer une liste vide (statut `discovery`).
   - Reporté : pays de destination du voyage, utile seulement avec plusieurs extraits.

2. **Régions à découvrir** — *fait*
   - Les lieux les plus notables sont regroupés en régions (« masse » de notoriété dans un
     rayon d'une journée), nommées d'après la localité où loger.
   - Fiche par région : points forts, thèmes dominants, nuits conseillées, lieux du voyage
     déjà dedans. « Ajouter ces lieux » ajoute les points forts au voyage ; les suggestions de
     proximité et « Où dormir ? » prennent ensuite le relais.
   - Accès vérifié avec Valhalla : une région hors du réseau routier principal (île Cocos) est
     présentée comme une expédition en bateau ou en avion, pas déclassée.
   - Limites constatées sur le Costa Rica : le centre d'une relation est celui de son emprise (parc marin : en mer), d'où des bases
     parfois éloignées (La Amistad → Cahuita) ; Monteverde manque (ses réserves n'ont pas de tag
     `wikidata` dans OSM). Pistes : point représentatif des relations, accessibilité, lier les
     lieux sans tag à Wikidata par nom et position, signaux de l'étape 5.

3. **Questionnaire d'envies** — *fait*
   - À la création du voyage (et dans ses paramètres) : centres d'intérêt (nature, plages,
     sites, musées, activités, gastronomie, shopping), rythme (tranquille, équilibré, intense)
     et hébergement (petit budget, confort, sans préférence).
   - Envies : bonus et raison « Correspond à vos envies » dans les suggestions (plus fort pour
     les lieux notables), qui s'efface à mesure que le groupe note ce genre de lieux ; poids
     doublé dans les régions ; pour « plages », toutes les plages nommées comptent (rarement
     dans Wikipédia), et les côtes qui en sont pleines deviennent des régions.
   - Rythme : nuits conseillées par région, nuits minimum par étape (« Où dormir ? », circuits).
   - Budget : type d'hébergement suggéré autour d'une zone (auberges, maisons d'hôtes,
     campings ou hôtels) — aucune donnée de prix dans OSM.

4. **Circuits types** — *fait*
   - Aéroports à code IATA importés de l'OSM (internationaux d'abord) pour l'arrivée et le
     départ.
   - `planner::circuits` (et non `planner::stays`, qui ne choisit pas *quelles* régions
     garder) : insertion gloutonne des régions selon leur intérêt (3 meilleurs points forts,
     plus la moitié de ce qui répond aux envies) moins le détour de route, sans étapes à moins
     de 45 min l'une de l'autre ni trajet de plus de 7 h ; 2-opt ; nuits restantes réparties ;
     variantes en dévaluant les régions déjà proposées.
   - « Appliquer ce circuit » : une zone d'hébergement par étape (comme « Où dormir ? ») et les
     points forts ajoutés au voyage. Les régions en bateau ou avion sont signalées à part.
   - Limites : thèmes limités aux catégories (les plages restent « nature ») ; les variantes
     sont parfois maigres (deux étapes voisines) ; la journée de route n'est pas déduite des
     nuits d'une étape.

5. **Meilleurs signaux d'intérêt** : guide Wikivoyage, patrimoine (UNESCO, Wikidata P1435),
   superficie des parcs nationaux, consultations Wikipédia ; affichés comme raisons.

6. **Carte d'exploration** : régions colorées par thème avant toute planification.
