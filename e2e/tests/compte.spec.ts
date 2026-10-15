import { readFile } from 'node:fs/promises'
import { expect, test } from '@playwright/test'
import { linkFromLastEmail, PASSWORD, register, uniqueEmail } from '../helpers'

test('compte : connexion, lien magique, export et suppression (RGPD)', async ({ page }) => {
  const email = uniqueEmail('compte')

  await test.step('inscription puis déconnexion', async () => {
    await register(page, 'Camille', email)
    await page.getByRole('button', { name: 'Se déconnecter' }).click()
    await expect(page.getByRole('heading', { name: 'Connexion' })).toBeVisible()
  })

  await test.step('un mauvais mot de passe est refusé, le bon accepté', async () => {
    await page.getByLabel('Adresse email').fill(email)
    await page.getByLabel('Mot de passe').fill('pas-le-bon-mot-de-passe')
    await page.getByRole('button', { name: 'Se connecter' }).click()
    await expect(page.getByText('Email ou mot de passe incorrect.')).toBeVisible()
    await page.getByLabel('Mot de passe').fill(PASSWORD)
    await page.getByRole('button', { name: 'Se connecter' }).click()
    await expect(page.getByRole('heading', { name: 'Mes voyages' })).toBeVisible()
    await page.getByRole('button', { name: 'Se déconnecter' }).click()
  })

  await test.step('connexion par lien magique reçu par email', async () => {
    await page.getByRole('tab', { name: 'Par lien magique' }).click()
    await page.getByLabel('Adresse email').fill(email)
    await page.getByRole('button', { name: 'Recevoir un lien de connexion' }).click()
    await expect(page.getByText(/un lien de connexion vient de vous être envoyé/)).toBeVisible()

    const link = await linkFromLastEmail(email, /https?:\/\/\S+\/auth\/magic\?token=[\w-]+/)
    await page.goto(new URL(link).pathname + new URL(link).search)
    await expect(page.getByRole('heading', { name: 'Mes voyages' })).toBeVisible()
  })

  await test.step('export de mes données (JSON, sans secret)', async () => {
    await page.getByRole('link', { name: 'Camille' }).click()
    const download = page.waitForEvent('download')
    await page.getByRole('button', { name: 'Exporter mes données (JSON)' }).click()
    const data = JSON.parse(await readFile(await (await download).path(), 'utf8'))
    expect(data.user.email).toBe(email)
    expect(JSON.stringify(data)).not.toMatch(/password_hash|token_hash|\$argon2/)
  })

  await test.step('suppression définitive du compte', async () => {
    const form = page.locator('form').filter({ has: page.getByRole('button', { name: 'Supprimer définitivement' }) })
    await form.getByLabel('Tapez votre email pour confirmer').fill(email)
    await form.getByLabel('Mot de passe').fill(PASSWORD)
    await page.getByRole('button', { name: 'Supprimer définitivement' }).click()
    await expect(page.getByRole('heading', { name: 'Connexion' })).toBeVisible()

    // Le compte n'existe plus.
    await page.getByRole('tab', { name: 'Avec mot de passe' }).click()
    await page.getByLabel('Adresse email').fill(email)
    await page.getByLabel('Mot de passe').fill(PASSWORD)
    await page.getByRole('button', { name: 'Se connecter' }).click()
    await expect(page.getByText('Email ou mot de passe incorrect.')).toBeVisible()
  })
})
