import { expect, type Browser, type Page } from '@playwright/test'

/** Mot de passe des comptes de test (jamais utilisé ailleurs). */
export const PASSWORD = 'e2e-mot-de-passe-solide'

const MAILPIT = process.env.MAILPIT_URL ?? 'http://localhost:8025'

/** Email unique par test et par exécution. */
export function uniqueEmail(prefix: string) {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}@e2e.test`
}

/** Crée un compte par le formulaire d'inscription ; la page arrive sur « Mes voyages ». */
export async function register(page: Page, name: string, email: string) {
  await page.goto('/register')
  await page.getByLabel('Prénom ou pseudo').fill(name)
  await page.getByLabel('Adresse email').fill(email)
  await page.getByLabel('Mot de passe').fill(PASSWORD)
  await page.getByRole('button', { name: 'Créer mon compte' }).click()
  await expect(page.getByRole('heading', { name: 'Mes voyages' })).toBeVisible()
}

/** Nouvelle session de navigateur (cookies séparés) : un autre membre du groupe. */
export async function newPerson(browser: Browser) {
  const context = await browser.newContext()
  return context.newPage()
}

/**
 * Appelle l'API depuis la page, comme le ferait l'application (cookie de session et en-tête
 * `Origin` posés par le navigateur). Sert à préparer des données sans service externe.
 */
export async function api<T = unknown>(page: Page, method: string, path: string, body?: unknown): Promise<T> {
  const result = await page.evaluate(
    async ({ method, path, body }) => {
      const res = await fetch(`/api/v1${path}`, {
        method,
        credentials: 'include',
        headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
        body: body === undefined ? undefined : JSON.stringify(body),
      })
      return { status: res.status, body: res.status === 204 ? null : await res.json() }
    },
    { method, path, body },
  )
  expect(result.status, `${method} ${path} : ${JSON.stringify(result.body)}`).toBeLessThan(300)
  return result.body as T
}

/** Identifiant du voyage affiché. */
export function tripIdOf(page: Page) {
  const match = page.url().match(/\/trips\/([0-9a-f-]{36})/)
  expect(match, page.url()).not.toBeNull()
  return match![1]
}

/** Lien du dernier email reçu par `to` dans Mailpit (attend son arrivée). */
export async function linkFromLastEmail(to: string, pattern: RegExp): Promise<string> {
  let link: string | undefined
  await expect
    .poll(
      async () => {
        const search = await fetch(`${MAILPIT}/api/v1/search?query=${encodeURIComponent(`to:"${to}"`)}`)
        const { messages } = (await search.json()) as { messages: { ID: string }[] }
        if (!messages?.length) return false
        const message = (await (await fetch(`${MAILPIT}/api/v1/message/${messages[0].ID}`)).json()) as { Text: string }
        link = message.Text.match(pattern)?.[0]
        return !!link
      },
      { message: `email pour ${to}`, timeout: 15_000 },
    )
    .toBe(true)
  return link!
}
