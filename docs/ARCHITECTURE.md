# Architecture et décisions

## Décisions validées

| Sujet | Décision |
|-------|----------|
| Routage marche/vélo/voiture | Valhalla auto-hébergé (MIT), extrait OSM Costa Rica. Repli à vol d'oiseau derrière le trait `RoutingProvider`. |
| Transports en commun | Pas de GTFS exploitable au Costa Rica, Navitia ne le couvre pas : approximation (temps voiture × 1,8 + 15 min) clairement signalée « estimation ». |
| Recherche de lieux | Nominatim public via le backend (cache, 1 req/s, User-Agent identifié, pas d'autocomplétion). Auto-hébergement prévu avant production. |
| Détails POI | Overpass (tags `opening_hours`, `tourism`, `amenity`), interprétation avec la crate `opening-hours`. |
| Fond de carte | OpenFreeMap (tuiles vectorielles, sans clé). |
| Authentification | Mot de passe (Argon2id) + lien magique (SMTP, Mailpit en dev). Sessions en base, cookie HttpOnly. |
| Invités | Le lien d'invitation demande prénom + email : compte sans mot de passe créé et connecté immédiatement. |
| Abonnement | Statut simulé (`users.plan`) : seul un compte `organizer` crée des voyages. |
| Suppression de compte | Voyages transférés au plus ancien éditeur, sinon supprimés ; avis supprimés ; POI conservés sans auteur. |
| Langue | Français, chaînes regroupées dans `frontend/src/i18n/`. |
| Départ du jour | Hébergement optionnel par jour (`trip_days.base_poi_id`), point de départ et d'arrivée. |
| Fuseau | `America/Costa_Rica` par défaut (UTC−6, sans heure d'été). |
| Requêtes SQL | `sqlx::query_as` (sans macros vérifiées à la compilation) : pas besoin de base pour compiler ni de cache `.sqlx` ; couverture par les tests d'intégration. |

## Incréments

0. Socle (ce dépôt) · 1. Authentification et compte · 2. Voyages et membres · 3. POI ·
4. Avis · 5. Transports et routage · 6. Itinéraire · 7. Ajustements manuels · 8. Temps réel
