# Architecture et décisions

## Décisions validées

| Sujet | Décision |
|-------|----------|
| Routage marche/vélo/voiture | Valhalla auto-hébergé (MIT), extrait OSM Costa Rica. Repli à vol d'oiseau derrière le trait `RoutingProvider`. |
| Transports en commun | Pas de GTFS exploitable au Costa Rica, Navitia ne le couvre pas : approximation (temps voiture × 1,8 + 15 min) clairement signalée « estimation ». |
| Recherche de lieux | Nominatim public via le backend (cache, 1 req/s, User-Agent identifié, pas d'autocomplétion). Auto-hébergement prévu avant production. |
| Détails POI | Tags OSM fournis par Nominatim (`extratags` : `opening_hours`, `website`, `phone`, `email`, `wikidata`, `wikipedia`), horaires interprétés avec la crate `opening-hours`. Overpass n'apporte rien de plus pour le MVP (révisé à l'incrément 3). |
| Présentation des lieux | Wikipédia (résumé), Wikidata (articles par langue, photo P18) et Wikimedia Commons (miniature, auteur, licence), via le backend avec cache 7 jours. Langues : fr, puis en, puis es. |
| Suggestions | Lieux importés de l'extrait OSM de Valhalla dans PostGIS (`osm_places`, au démarrage si le fichier a changé) : Overpass public mesuré à 25–40 s ou en erreur 502, inutilisable en interactif. Score déterministe et explicable (`planner::recommend`), sans apprentissage ni classement sponsorisé. |
| Fond de carte | OpenFreeMap (tuiles vectorielles, sans clé). |
| Authentification | Mot de passe (Argon2id) + lien magique (SMTP, Mailpit en dev). Sessions en base, cookie HttpOnly. |
| Invités | Le lien d'invitation demande prénom + email : compte sans mot de passe créé et connecté immédiatement. |
| Abonnement | Statut simulé (`users.plan`) : seul un compte `organizer` crée des voyages. |
| Suppression de compte | Voyages transférés au plus ancien éditeur, sinon supprimés ; avis supprimés ; POI conservés sans auteur. |
| Langue | Français, chaînes regroupées dans `frontend/src/i18n/`. |
| Départ du jour | Hébergement optionnel par jour (`trip_days.base_poi_id`), point de départ et d'arrivée. |
| Hébergement à choisir | Zone d'hébergement (`pois.placeholder`, catégorie `lodging`) : une ville sert de départ et d'arrivée approximatifs, puis est remplacée par l'hôtel retenu. Recherche Booking.com / Airbnb par simples liens, sans affiliation. |
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
- **Reporté** : `trip_days` (modes et hébergement par jour) arrive avec l'incrément transports (fait à l'incrément 5).

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
- **Coordonnées et liens** (migration 0009) : téléphone, email, identifiant Wikidata et article
  Wikipédia (`langue:Titre`) repris des tags OSM à l'ajout (première valeur si plusieurs, valeur
  invalide ignorée), modifiables via l'API ; téléphone et email aussi dans le formulaire. La fiche
  d'un lieu affiche à tous les membres un bloc « Liens » construit côté interface
  (`lib/links.ts`) : site officiel, appel, email, Wikipédia, fiche OpenStreetMap, recherche et
  itinéraire Google Maps (nom + adresse, sinon coordonnées). Aucun appel tant qu'on ne clique pas.
- **Présentation Wikipédia** (`GET /trips/{id}/pois/{poi}/wiki`, module `wiki`) : pour un lieu
  qui a un tag `wikidata` ou `wikipedia`. L'élément Wikidata donne les articles (fr, puis en, puis
  es, sinon l'article du tag) et la photo P18 ; sans Wikidata, l'élément est retrouvé via
  l'article du tag. Résumé par l'API REST de Wikipédia (pages d'homonymie ignorées), photo par
  Commons avec auteur et licence affichés (attribution obligatoire), lien Wikivoyage s'il existe.
  Seules les références d'un lieu visible par l'utilisateur sont résolues (pas de relais ouvert) ;
  liens limités aux domaines Wikimedia en https. Cache mémoire 7 jours (absences comprises,
  erreurs non), 60 requêtes / min par utilisateur, **503** si Wikimedia ne répond pas (l'interface
  l'indique sans bloquer la fiche). Une photo indisponible n'empêche pas d'afficher le texte.
  La miniature est chargée par le navigateur depuis les serveurs Wikimedia (sans `Referer`),
  comme les tuiles de la carte le sont depuis OpenFreeMap.
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

## Transports et routage (incrément 5)

- **Modes par jour** (`trip_days`) : une ligne n'existe que si le jour s'écarte du voyage. `modes`
  NULL = modes par défaut du voyage (`modes_inherited`), sinon liste propre au jour ; `base_poi_id`
  = hébergement du soir (n'importe quel lieu du voyage), remis à NULL si le lieu est supprimé.
  Révisé à l'incrément 6 : la journée part de l'hébergement de la veille (sinon de celui du jour)
  et se termine à celui du jour, ce qui gère les changements d'hôtel. Réglages des jours sortis des dates supprimés quand le voyage raccourcit.
- **Droits** : lecture pour tous les membres ; jours modifiables par les éditeurs (dernière
  écriture gagnante, `PUT /trips/{t}/days/{date}` remplace tout) ; modes par défaut du voyage
  réservés à l'organisateur (paramètres). Calcul de trajets ouvert aux lecteurs (lecture seule).
- **Routage** (`api::routing`) : trait `RoutingProvider`, implémenté par Valhalla
  (`sources_to_targets` pour les matrices, `route` pour les trajets avec tracé polyline6).
  `RoutingService` ne renvoie jamais d'erreur : panne, région non reliée, lieu loin d'une route ou
  distance hors limites → **estimation à vol d'oiseau** (distance × 1,3 à pied/vélo, × 1,4 en
  voiture ; 4,5 / 14 / 40 km/h). Une panne coupe le moteur pour le reste de la requête (pas de
  cumul de délais). Matrices découpées par groupes de lignes (2 500 paires par requête), un
  groupe sans chemin est réessayé ligne par ligne ; itinéraires découpés par fenêtres de 20 points,
  une fenêtre sans chemin est reprise paire par paire. Cache mémoire 7 jours (200 000 trajets,
  coordonnées au mètre) des seuls résultats du moteur. Sans `VALHALLA_URL` : vol d'oiseau partout.
- **Transports en commun** : toujours estimés, `temps voiture × 1,8 + 15 min`, tracé de la
  voiture ; signalés dans l'API (`source = transit_estimate`) et l'interface.
- **Choix du mode d'une étape** (`planner::travel::choose_mode`, réutilisé par l'itinéraire) :
  marche si ≤ 20 min, sinon vélo si ≤ 45 min, sinon l'option la plus rapide ; égalité départagée
  dans l'ordre marche, vélo, voiture, transports en commun.
- **API** : `GET /trips/{t}/days`, `PUT /trips/{t}/days/{date}`, `POST /trips/{t}/routes`
  (2 à 25 lieux dans l'ordre, modes explicites ou ceux d'un jour ou du voyage ; renvoie chaque
  option, l'option retenue, son tracé GeoJSON et `has_estimates`). 60 calculs / min par utilisateur.
- **Interface** : onglet « Transports » (modes et hébergement de chaque jour, comparaison d'un
  trajet entre deux lieux, tracé retenu affiché sur la carte, estimations marquées « ≈ »).

## Séjours et zones d'hébergement

- **Séjour** : suite de nuits consécutives au même hébergement (l'hébergement d'un jour est celui
  de sa nuit). `PUT /trips/{id}/stays` règle les nuits `first_night`…`last_night` sans toucher aux
  modes ; `POST /trips/{id}/stays/replace` remplace un hébergement par un autre sur toutes ses
  nuits et supprime la zone remplacée si demandé (jamais un lieu qui n'est pas une zone).
- **Zone d'hébergement** (migration 0010) : lieu `lodging` marqué `placeholder`, créé depuis une
  ville trouvée par la recherche de lieux. Contrainte en base : seule une catégorie `lodging`
  peut être une zone ; changer de catégorie retire le statut.
- **Proposition d'étapes** (« Où dormir ? », `POST /trips/{id}/stays/proposal`, calcul seul ;
  `…/proposal/apply` crée une zone par étape et la règle sur ses nuits) : `planner::stays`
  groupe les lieux à visiter (k-médoïdes pondérés par la note du groupe) sur une matrice de temps
  **Valhalla** (les temps à vol d'oiseau trompent au Costa Rica : Monteverde–La Fortuna ≈ 3 h de
  route pour 25 km), choisit le nombre d'étapes (au moins `min_nights` nuits chacune) en
  minimisant allers-retours quotidiens, transferts, 45 min par changement d'hôtel et journées
  surchargées, répartit les nuits au prorata du temps de visite et ordonne les étapes (toutes les
  permutations jusqu'à 7) depuis l'arrivée jusqu'au départ (lieux du voyage, ex. aéroport).
  Chaque étape prend le nom de la localité où loger (`place=city|town|village` importés de l'OSM) :
  la plus proche rapportée au nombre d'hébergements connus à moins de 2 km, les villes passant
  devant leurs quartiers. Exemple réel (8 nuits) : San José → La Fortuna → Santa Elena → Quepos.
- **Interface** (onglet Transports, section « Séjours ») : liste des séjours (zones en pointillés),
  liens de recherche Booking.com et Airbnb avec les dates d'arrivée et de départ et le nombre de
  membres, sans affiliation ni suivi ; « Remplacer par… » parmi les hébergements du voyage.

## Suggestions

- **Import** (`osm_import`, migration 0011) : `OSM_EXTRACT` (l'extrait de Valhalla, monté en lecture
  seule) est lu au démarrage en tâche de fond (crate `osmpbf`, trois passes : relations, chemins,
  nœuds ; ~11 000 lieux en 1,5 s pour le Costa Rica) si sa taille ou sa date a changé. Lieux nommés
  à visiter (`tourism=attraction|viewpoint|museum…`, `waterway=waterfall`, `natural=beach|volcano…`,
  parcs nationaux, sites historiques), restaurants et cafés, hébergements. Position : nœud, moyenne
  des nœuds d'un chemin, centre de l'emprise d'une relation.
- **Candidats** (`suggest`) : autour des points de départ de chaque jour (hébergement de la veille et
  du jour, zones comprises ; à défaut les lieux du voyage), rayon selon le mode le plus rapide
  (3 km à pied, 10 km à vélo, 40 km en voiture), hors lieux déjà ajoutés et suggestions écartées.
  Sans filtre, ni restaurants (proches de tout, ils évinceraient les sites : filtre « Où manger »)
  ni hébergements (proposés autour d'une zone, `?lodging_near=`).
- **Score** (`planner::recommend`, pur et testé) : proximité (trajet estimé à vol d'oiseau majoré,
  écarté au-delà de 2 h 30), notoriété (langues Wikipédia de l'élément Wikidata, en cache 7 jours,
  0 si Wikidata ne répond pas), goûts du groupe (moyenne par catégorie, au moins 2 avis), données
  connues (horaires, contact, Wikipédia), temps libre du jour d'après l'itinéraire, pénalités de
  saturation (catégorie déjà très présente, refus du groupe). Fermé à chaque date : écarté. Puis
  diversification (même catégorie à moins de 5 km pénalisée) et doublons homonymes supprimés.
  Chaque critère qui compte donne une raison affichée (« ≈ 15 min de La Fortuna (mer. 2 déc.) »).
- **Refus** : `trip_dismissed_suggestions`, partagé par le groupe (éditeurs), réinitialisable ;
  sujet temps réel `suggestions`. Les suggestions sont rechargées à tout changement du voyage.
- **Interface** : onglet « Suggestions » (filtres, raisons, liens, Ajouter / Pas intéressé),
  marqueurs en pointillés sur la carte ; « Hébergements dans la zone » sous une zone d'hébergement,
  « Choisir » ajoute l'hôtel et remplace la zone en une fois.

## Itinéraire (incrément 6)

- **Algorithme** (`planner::itinerary`, pur et déterministe, testé unitairement) en deux temps :
  1. *Répartition par jour* (vol d'oiseau, sans moteur de routage) : centres fixés par les
     hébergements du soir, les autres par point le plus éloigné puis k-moyennes (20 itérations
     max.). Les lieux sont placés par **note du groupe décroissante** (note neutre 3 sans avis),
     puis durée croissante, dans le jour ouvert le plus proche qui a encore le temps (visite +
     trajet estimé depuis le centre × 1,2). À distance égale (même hôtel plusieurs nuits), le
     jour le plus libre l'emporte. Le budget d'un jour de changement d'hôtel déduit le transfert
     estimé. Les jours sans hébergement sont ensuite réordonnés pour suivre la géographie.
  2. *Journée* (vrais temps de trajet, mode choisi par paire avec la règle de l'incrément 5) :
     insertion des lieux les mieux notés d'abord, à la position qui termine la journée le plus
     tôt en respectant les **horaires d'ouverture** (attente si on arrive avant l'ouverture, la
     visite doit finir avant la fermeture) et la **plage de la journée** (retour à l'hôtel
     compris), puis amélioration **2-opt** du temps de trajet total.
- **Lieux exclus** : hébergements (catégorie ou hébergement d'un jour). Horaires absents ou
  invalides : lieu considéré ouvert. Raisons d'exclusion : `closed_all_days`, `opening_hours`,
  `no_time` (les mieux notés passent d'abord), `added_after` (calculée à la lecture).
- **Stockage** : `itineraries` (version, auteur, date), `itinerary_days` (modes, hébergements,
  plage horaire, nombre d'étapes et instant du calcul, trajet de retour), `itinerary_stops`
  (ordre, horaires en secondes, trajet d'arrivée, `locked` pour l'incrément 7),
  `itinerary_unscheduled`. Régénérer remplace tout (verrou consultatif sur le voyage).
- **Itinéraire périmé** (calculé à la lecture, rien n'est modifié en base) : un jour est
  `stale` si ses modes, ses hébergements ou la plage horaire ont changé, si une étape a disparu
  (lieu supprimé) ou si un lieu de la journée a été modifié depuis le calcul ; jours ajoutés
  au voyage vides et périmés.
- **API** : `GET /trips/{t}/itinerary` (membres, `null` si jamais généré), `POST
  /trips/{t}/itinerary` (éditeurs, 10 / min par utilisateur). L'auteur (`generated_by`) passe
  à NULL à la suppression du compte.
- **Interface** : onglet « Itinéraire » (frise de chaque jour avec trajets, attentes et
  estimations, lieux hors itinéraire avec la raison, tracé du jour choisi sur la carte, avertissement
  si l'itinéraire est périmé, confirmation avant de régénérer).
- **Limites connues** : pas de pause repas ; un lieu écarté d'un jour faute de temps n'est pas
  retenté sur un autre jour (à placer à la main, voir l'incrément 7) ; les lieux hors réseau routier
  (sommet d'un volcan) sont rejoints au point routier le plus proche.

## Ajustements manuels (incrément 7)

- **Opérations** (éditeurs, toutes avec la `version` de l'itinéraire, **409** si elle a changé) :
  `POST /trips/{t}/itinerary/moves` (`poi_id`, `date`, `position`) pour réordonner, changer de jour,
  ajouter un lieu hors itinéraire ou le retirer (`date: null`) ; `PATCH
  /trips/{t}/itinerary/stops/{poi}` (`locked`) ; `POST /trips/{t}/itinerary/days/{date}/recompute`.
  Chaque opération incrémente la version et renvoie l'itinéraire complet.
- **Recalcul sans casser le reste** : seuls les jours touchés (départ et arrivée d'un déplacement,
  ou le jour demandé) sont recalculés, **dans l'ordre choisi** (`planner::itinerary::time_day`),
  avec les lieux et réglages actuels (ce qui efface leur signalement « périmé »). Rien n'est
  retiré : une étape qui ne respecte pas une contrainte est signalée (`issue` :
  `outside_opening_hours` — visite à l'arrivée —, `after_day_end`), la journée qui déborde aussi
  (`overtime`, horaires possibles au-delà de 24:00).
- **Verrou** : une étape verrouillée garde son jour et l'ordre relatif des étapes verrouillées
  quand on régénère (`Candidate::pinned` à la répartition, `DayProblem::fixed` pour la journée :
  les autres lieux s'insèrent autour, le 2-opt n'inverse jamais deux étapes verrouillées). Si les
  étapes verrouillées ne tiennent pas, elles sont gardées et signalées ; les autres lieux
  s'insèrent encore s'ils n'ajoutent aucun problème (ni nouvelle étape signalée, ni débordement).
- **Retrait manuel** : raison `removed`, conservée quand on régénère (le lieu reste hors
  itinéraire jusqu'à ce qu'on le replace à la main).
- **Concurrence** : générations et ajustements d'un même voyage s'enchaînent grâce à un verrou
  consultatif de transaction (`pg_advisory_xact_lock`) ; la version de l'itinéraire détecte les
  modifications concurrentes. Les trajets sont calculés dans la transaction (modes en parallèle,
  cache) sans bloquer les autres écritures du voyage, qui rendent au besoin l'itinéraire périmé.
- **Interface** : glisser-déposer `@dnd-kit` (souris, tactile et clavier : Espace, flèches,
  Espace ; annonces lecteur d'écran en français) dans une journée ou vers une autre (toute la
  carte d'une journée accepte un dépôt, y compris vide) ; boutons ↑/↓, verrou, « Déplacer… »
  (autre jour ou retrait), « Ajouter au… » pour les lieux hors itinéraire, « Recalculer ce
  jour » sur un jour périmé. Affichage optimiste des déplacements (annulé en cas d'erreur, et
  itinéraire rechargé sur conflit).

## Collaboration en temps réel (incrément 8)

- **Canal** : `GET /api/v1/trips/{t}/ws` (WebSocket), cookie de session et appartenance au voyage
  vérifiés à l'ouverture. L'en-tête **`Origin` doit être autorisé** (sinon 403) : un site tiers
  ne peut pas ouvrir le canal avec le cookie de la victime (détournement de WebSocket intersite).
  30 ouvertures / min par utilisateur, 200 connexions par voyage, messages entrants ≤ 1 Ko
  (le client n'envoie rien d'utile).
- **Notifications, pas de données** : le serveur envoie `changed` (sujets : `trip`, `members`,
  `invitations`, `pois`, `reviews`, `days`, `itinerary`, auteur, onglet d'origine), `presence`
  (membres en ligne, une entrée par personne), `removed` (accès perdu : retiré du voyage, voyage
  ou compte supprimé, session terminée — la connexion se ferme) et `resync` (notifications
  perdues par un client trop lent). Chaque client recharge ensuite par l'API REST avec ses
  propres droits : rien ne fuit par le canal (les emails restent réservés à l'organisateur).
- **Émission** : chaque route qui modifie un voyage publie après succès (lieux, avis, jours,
  voyage, membres, invitations, itinéraire, changement de nom ou suppression de compte).
  L'en-tête `X-Client-Id` (identifiant aléatoire de l'onglet) est renvoyé dans `changed` :
  l'onglet auteur ignore sa propre notification, ses autres onglets se mettent à jour.
- **Revérification** toutes les 30 s (avec un ping) : session toujours valide (sans la prolonger :
  un onglet resté ouvert n'est pas une activité) et toujours membre, sinon `removed`. Retrait d'un membre et suppression d'un voyage : immédiat.
- **Hub en mémoire** (`realtime::Hub`, un canal `broadcast` par voyage ouvert) : une seule
  instance de l'API. Pour en déployer plusieurs, relayer les notifications entre instances
  (PostgreSQL `LISTEN/NOTIFY`, par exemple).
- **Frontend** (`useTripRealtime`) : invalide les requêtes TanStack Query des sujets reçus,
  affiche l'état de la connexion, les autres membres en ligne et la dernière modification
  (« Bruno a modifié les avis (22:27) »). Reconnexion automatique (0,5 s, doublé jusqu'à 30 s)
  puis rechargement complet du voyage pour rattraper les notifications manquées. Accès perdu :
  message dédié, plus de reconnexion. Une reconnexion refusée (le navigateur ne donne pas le statut
  HTTP) déclenche une lecture du voyage : 403/404 → accès perdu, 401 → retour à la connexion. Les modifications concurrentes restent arbitrées par les
  versions (409) déjà en place.
- **Tests** : hub (présence, isolation des voyages), intégration par un vrai WebSocket
  (`tokio-tungstenite`) : notifications et présence, refus hors membres / mauvaise origine /
  sans session, déconnexion d'un membre retiré et d'un voyage supprimé ; frontend avec un
  WebSocket factice (invalidations, onglet auteur ignoré, reconnexion, accès perdu).

## Tests de bout en bout

- **Playwright** (`e2e/`, version 1.63.0, image officielle `mcr.microsoft.com/playwright`),
  service `e2e` de docker-compose (profil `e2e`), réseau de l'hôte : le navigateur utilise
  l'origine `http://localhost:5173` autorisée par l'API (CSRF, `Origin` du WebSocket).
- **Parcours** : voir le README. Deux contextes de navigateur (deux personnes) ; la carte
  MapLibre s'affiche réellement (WebGL logiciel de Chromium) ; Valhalla calcule les trajets.
- **Sans service externe** : lieux créés par l'API depuis la page (même cookie et même
  `Origin` que l'application, ce qui vérifie au passage la mise à jour en direct de la liste) ;
  emails lus par l'API de Mailpit.
- **Pas en CI pour l'instant** : la pile complète demande la construction des tuiles Valhalla
  (téléchargement de l'extrait OSM, plusieurs minutes). À ajouter en déclenchement manuel ou
  nocturne avec un cache des tuiles.
