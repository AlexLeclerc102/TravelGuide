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
docs/               architecture et décisions
```

Voir [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
