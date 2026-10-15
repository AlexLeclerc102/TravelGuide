import { defineConfig, devices } from '@playwright/test'

/**
 * Tests de bout en bout : navigateur réel contre la pile complète (frontend, API, PostgreSQL,
 * Valhalla, Mailpit). Lancement : `docker compose --profile e2e run --rm e2e`.
 * Aucun service externe : les lieux de test sont créés par l'API (pas de Nominatim public).
 */
export default defineConfig({
  testDir: './tests',
  // Chaque test crée ses propres comptes et voyages : ils peuvent tourner en parallèle.
  fullyParallel: true,
  workers: 2,
  retries: 0,
  timeout: 90_000,
  expect: { timeout: 10_000 },
  reporter: [['list'], ['html', { open: 'never', outputFolder: 'playwright-report' }]],
  use: {
    baseURL: process.env.E2E_BASE_URL ?? 'http://localhost:5173',
    locale: 'fr-FR',
    timezoneId: 'America/Costa_Rica',
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'], viewport: { width: 1440, height: 900 } } }],
})
