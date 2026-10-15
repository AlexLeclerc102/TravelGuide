# TravelGuide

Application collaborative de planification de voyage : un groupe ajoute des points d'intérêt,
chacun donne sa note et son avis, puis l'application génère un itinéraire (algorithmes
déterministes) selon les moyens de transport choisis. Tout reste modifiable à la main.

Principes : neutralité (aucun classement sponsorisé), vie privée (RGPD, aucune revente),
l'organisateur paie, les invités rejoignent gratuitement.

## Démarrage rapide

```bash
cp .env.example .env          # puis changer les mots de passe et NOMINATIM_CONTACT
docker compose up --build
```

| Service   | URL                              |
|-----------|----------------------------------|
| Frontend  | http://localhost:5173            |
| API docs  | http://localhost:8080/api/docs   |
| OpenAPI   | http://localhost:8080/api/openapi.json |
| Emails    | http://localhost:8025 (Mailpit)  |
| Valhalla  | http://localhost:8002            |

Si un PostgreSQL local occupe déjà le port 5432, changez `DB_PORT` dans `.env`.

Au premier démarrage, Valhalla télécharge l'extrait OSM du Costa Rica et construit ses tuiles
(quelques minutes). L'API fonctionne sans lui (repli à vol d'oiseau, signalé dans l'interface).

## Développement sans Docker pour le code

```bash
docker compose up -d db mailpit valhalla
# Backend
cd backend && DATABASE_URL=postgres://travel:<mdp>@localhost:5432/travel cargo run
# Frontend (proxy /api -> localhost:8080)
cd frontend && npm install && npm run dev
```

## Tests et qualité

```bash
cd backend  && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd frontend && npm run lint && npm run typecheck && npm test
```

### Tests de bout en bout (Playwright)

Navigateurs réels contre la pile complète, déjà lancée (`docker compose up`) :

```bash
docker compose --profile e2e run --rm e2e     # rapport : e2e/playwright-report/
```

Deux parcours : un groupe qui prépare un voyage à deux navigateurs (invitation, présence et
modifications en direct, avis, hébergements, génération de l'itinéraire, déplacement, verrou
et glisser-déposer d'étapes, retrait d'un membre) et le compte (connexion, lien magique lu dans
Mailpit, export et suppression RGPD). Aucun service externe : les lieux de test sont créés par
l'API, sans Nominatim. Chaque exécution crée 3 comptes ; l'inscription étant limitée à 20 par
heure et par IP, redémarrez le backend (`docker compose restart backend`) si une série
d'exécutions renvoie « trop de tentatives ».

Les tests d'intégration de l'API (`#[sqlx::test]`) créent une base temporaire par test :
ils nécessitent `DATABASE_URL` pointant vers un PostgreSQL avec PostGIS, et un rôle autorisé
à créer des bases.

## Structure

```
backend/
  crates/planner/   algorithme d'itinéraire pur (sans I/O), tests unitaires
  crates/api/       Axum + SQLx + OpenAPI (utoipa) + WebSocket (temps réel par voyage)
  migrations/       migrations SQL (appliquées au démarrage)
frontend/           React + TypeScript (Vite), TanStack Query, MapLibre GL
e2e/               tests de bout en bout (Playwright)
docs/               architecture et décisions
```

Voir [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
