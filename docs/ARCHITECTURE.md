# Architecture et décisions

## Décisions validées

| Sujet | Décision |
|-------|----------|
| Routage marche/vélo/voiture | Valhalla auto-hébergé (MIT), extrait OSM Costa Rica. Repli à vol d'oiseau derrière le trait `RoutingProvider`. |
| Transports en commun | Pas de GTFS exploitable au Costa Rica, Navitia ne le couvre pas : approximation (temps voiture × 1,8 + 15 min) clairement signalée « estimation ». |
| Recherche de lieux | Nominatim public via le backend (cache, 1 req/s, User-Agent identifié, pas d'autocomplétion). Auto-hébergement prévu avant production. |
| Détails POI | Tags OSM fournis par Nominatim (`extratags` : `opening_hours`, `website`), interprétés avec la crate `opening-hours`. Overpass n'apporte rien de plus pour le MVP (révisé à l'incrément 3). |
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

## Authentification (incrément 1)

- **Mots de passe** : Argon2id (paramètres par défaut), 10 à 128 caractères, calcul hors du runtime
  async. Connexion à durée constante : une empreinte factice est vérifiée si le compte n'existe pas.
- **Sessions** : jeton aléatoire de 256 bits dans le cookie `tg_session` (`HttpOnly`, `SameSite=Lax`,
  `Secure` sauf en dev HTTP). Seule l'empreinte SHA-256 est stockée. Durée 30 jours, prolongée à
  l'usage (au plus une écriture par heure). Changer de mot de passe ferme les autres sessions.
- **Liens magiques** : réservés aux comptes existants, 15 minutes, usage unique, empreinte stockée.
  Réponse identique que le compte existe ou non (pas d'énumération), envoi en tâche de fond.
  Le lien ouvre une page qui échange le jeton par POST (les scanners de liens ne le consomment pas).
- **CSRF** : toute requête d'écriture doit porter un en-tête `Origin` autorisé (`PUBLIC_ORIGIN`
  et `EXTRA_ALLOWED_ORIGINS`), en plus de `SameSite=Lax`.
- **Limitation de débit** (en mémoire, une instance) : connexion 10 / 15 min par email et 50 / 15 min
  par IP ; lien magique 5 / 15 min par email ; inscription 20 / h par IP.
- **RGPD** : `GET /me/export` (profil + sessions, sans aucun secret), `DELETE /me` (confirmation par
  email + mot de passe s'il existe ; suppression en cascade).
- **Écart connu** : l'inscription renvoie 409 si l'email existe déjà, ce qui révèle l'existence d'un
  compte (compromis d'ergonomie usuel, atténué par la limitation de débit).

## Voyages, membres et invitations (incrément 2)

- **Rôles** (`viewer` < `editor` < `organizer`) : un seul organisateur par voyage (index unique
  partiel). Toutes les routes d'un voyage passent par `trips::require_role` : un non-membre reçoit
  **404** (l'existence du voyage n'est pas révélée), un rôle insuffisant **403**.
- **Droits** : lecture pour tous les membres ; paramètres, suppression, membres et invitations
  réservés à l'organisateur ; un membre peut quitter le voyage ; l'organisateur peut transférer
  l'organisation (il devient éditeur). Les emails des membres ne sont visibles que par l'organisateur.
- **Abonnement simulé** : création de voyage réservée au plan `organizer` (`403 plan_required`),
  accordé via `ORGANIZER_EMAILS` à l'inscription ou à la connexion. Les invités sont toujours `free`.
- **Verrouillage optimiste** : `trips.version` ; une modification avec une version obsolète
  renvoie **409** et le frontend recharge le voyage.
- **Invitations** : jeton 256 bits (empreinte stockée), rôle éditeur ou lecteur, validité 1–30 jours,
  nombre d'utilisations optionnel, révocables. Un membre existant qui réutilise un lien garde son
  rôle s'il est supérieur et ne consomme pas d'utilisation.
- **Invité sans friction** : prénom + email suffisent ; un compte sans mot de passe est créé et
  connecté. Si l'email correspond déjà à un compte : **409 `account_exists`**, et l'interface propose
  un lien magique qui ramène à l'invitation (`next`). Comme n'importe qui peut créer un compte invité
  avec l'email d'autrui, la première connexion par lien magique (preuve de possession de l'email)
  **ferme toutes les sessions existantes** de ce compte.
- **Suppression de compte** : transfert de chaque voyage organisé à l'éditeur le plus ancien, sinon
  suppression du voyage (les lecteurs ne reçoivent pas l'organisation).
- **Reporté** : `trip_days` (modes et hébergement par jour) arrive avec l'incrément transports.

## Points d'intérêt (incrément 3)

- **Table `pois`** : coordonnées en `geography(Point, 4326)` (index GiST), catégorie parmi
  `sight, nature, museum, food, activity, lodging, shopping, other`, durée de visite 0–720 min
  (durée type par catégorie, 0 pour un hébergement), horaires OSM bruts, code pays, site web
  (http(s) uniquement), notes, référence OSM. Un même objet OSM n'est ajouté qu'une fois par
  voyage (index unique partiel, **409**). Au plus 300 POI par voyage (borne l'algorithme).
- **Droits** : lecture pour tous les membres, ajout / modification / suppression pour éditeur et
  organisateur (y compris les POI ajoutés par d'autres). Verrouillage optimiste (`pois.version`).
  Un POI n'est accessible que via son voyage (`/trips/{trip}/pois/{poi}` : 404 sinon).
- **Recherche de lieux** (`GET /places/search`, `GET /places/reverse`) : réservée aux comptes
  connectés, relayée vers Nominatim derrière le trait `PlaceProvider`. Politique d'usage
  respectée : User-Agent avec contact (`NOMINATIM_CONTACT`), **une requête par seconde au plus**
  pour toute l'instance (créneaux réservés ; refus **503** au-delà de 5 s d'attente), pas
  d'autocomplétion (recherche à la validation), attribution ODbL affichée. Cache mémoire 24 h
  (5 000 entrées, zone de recherche arrondie au dixième de degré). Limite : 30 requêtes / min
  par utilisateur (**429**). La recherche privilégie la zone affichée (`viewbox`) sans l'imposer.
- **Catégories** : déduites de la clé/valeur OSM principale (`tourism=museum` → musée,
  `natural=*` → nature, etc.), modifiables par les membres.
- **Horaires** : la syntaxe OSM est conservée telle quelle, même invalide (`opening_hours_status`
  = `invalid`, signalé dans l'interface et ignoré par le futur planificateur). Interprétation en
  heure locale sur chaque date du voyage, avec les **jours fériés du pays** du lieu (base embarquée
  de la crate `opening-hours` ; ex. 1er et 25 décembre au Costa Rica). La liste renvoie les dates
  de fermeture complète (`closed_dates`) ; `POST /trips/{id}/opening-hours/preview` détaille les
  plages de chaque jour (vérification à la saisie, fiche d'un lieu). Les règles `unknown`
  (« sur rendez-vous ») sont marquées incertaines.
- **Ajout par clic** : géocodage inverse au point cliqué ; on garde les coordonnées du clic. Si
  OSM y connaît un lieu nommé (hors rue / commune), nom, catégorie et horaires sont préremplis ;
  sinon seule l'adresse l'est.
- **RGPD** : l'export inclut les lieux ajoutés ; à la suppression du compte, ils restent dans les
  voyages sans auteur.
- **Carte** : le worker MapLibre est fourni explicitement (`setWorkerUrl` + import `?url`), sinon
  il est introuvable sous Vite (fond de carte vide). Sans WebGL, un message remplace la carte et
  la liste reste utilisable.

## Avis (incrément 4)

- **Table `reviews`** : un avis par membre et par POI (clé `(poi_id, user_id)`), note 1–5 et
  commentaire (2 000 caractères max). Supprimés avec le POI, avec le compte (RGPD) et **quand le
  membre quitte le voyage ou en est retiré** (les notes du groupe ne reflètent que les membres
  actuels). L'export RGPD inclut ses avis.
- **Droits** : tous les membres, **lecteurs compris**, donnent leur avis (opinion personnelle, pas
  une modification du voyage) et voient celui des autres (nom affiché, jamais l'email). Chacun ne
  modifie que le sien (`PUT/DELETE /trips/{t}/pois/{p}/reviews/me`, dernière écriture gagnante).
- **Synthèse** (`planner::ratings::summarize`, partagée avec le futur algorithme d'itinéraire) :
  nombre d'avis, moyenne, min, max et **désaccord = au moins deux avis et un écart max − min ≥ 2**
  (constante `DISAGREEMENT_SPREAD`). Chaque POI renvoie `rating` et `my_review` ; enregistrer ou
  retirer son avis renvoie le POI recalculé.
- **Neutralité** : seules les notes des membres comptent ; aucune note externe ni sponsorisée.
- **Interface** : fiche du lieu avec les avis en premier (moyenne, désaccord expliqué, avis
  extrêmes mis en évidence), la modification du lieu étant repliée ; liste avec note du groupe,
  badge « Désaccord », rappel « À noter », et tri par ordre d'ajout, note ou désaccord.
