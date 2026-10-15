import { expect, test, type Locator, type Page } from '@playwright/test'
import { api, newPerson, register, tripIdOf, uniqueEmail } from '../helpers'

/** Lieux du voyage (coordonnées réelles, routées par Valhalla). */
const PLACES = {
  hotelSanJose: { name: 'Hotel Don Carlos', category: 'lodging', lat: 9.9355, lon: -84.084 },
  museum: {
    name: 'Museo Nacional',
    category: 'museum',
    lat: 9.9327,
    lon: -84.0716,
    opening_hours: 'Tu-Sa 08:30-16:30; Su 09:00-16:30',
    country_code: 'CR',
  },
  cafe: { name: 'Café Central', category: 'food', lat: 9.934, lon: -84.079 },
  hotelFortuna: { name: 'Hotel Arenal Lodge', category: 'lodging', lat: 10.471, lon: -84.645 },
  arenal: { name: 'Volcán Arenal', category: 'nature', lat: 10.4621, lon: -84.7034 },
  waterfall: {
    name: 'Catarata La Fortuna',
    category: 'nature',
    lat: 10.441,
    lon: -84.665,
    opening_hours: '07:00-17:00',
  },
} as const

const tab = (page: Page, name: string) => page.getByRole('tab', { name, exact: true }).click()
const day = (page: Page, label: string) => page.getByRole('region', { name: label })
/** Un lieu dans la liste (la carte affiche aussi un marqueur du même nom). */
const listedPlace = (page: Page, name: RegExp) => page.locator('.poi-list').getByRole('button', { name })
const stopNames = (page: Page, label: string) => day(page, label).locator('.stop-name')

/** Les étapes du jour contiennent ces lieux (dans l'ordre choisi par l'algorithme). */
async function expectStops(page: Page, label: string, names: string[]) {
  await expect
    .poll(() => stopNames(page, label).allTextContents(), { timeout: 15_000 })
    .toEqual(expect.arrayContaining(names))
}

/** Glisser-déposer à la souris, en plusieurs mouvements (le déplacement démarre après 6 px). */
async function drag(page: Page, handle: Locator, target: Locator) {
  // La barre latérale défile : la journée doit être visible pour que la souris l'atteigne.
  await target.scrollIntoViewIfNeeded()
  await handle.scrollIntoViewIfNeeded()
  const from = (await handle.boundingBox())!
  const to = (await target.boundingBox())!
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2)
  await page.mouse.down()
  await page.mouse.move(from.x + from.width / 2, from.y + from.height / 2 - 10, { steps: 5 })
  await page.mouse.move(to.x + 10, to.y + to.height / 2, { steps: 15 })
  await page.mouse.up()
}

test('un groupe prépare un voyage ensemble, en direct', async ({ page: ana, browser }) => {
  const anaEmail = uniqueEmail('ana')
  let tripId = ''
  const ids: Record<string, string> = {}

  await test.step('Ana crée son compte et un voyage de deux jours', async () => {
    await register(ana, 'Ana', anaEmail)
    await ana.getByRole('button', { name: 'Nouveau voyage' }).click()
    await ana.getByLabel('Nom du voyage').fill('Costa Rica en groupe')
    await ana.getByLabel('Du', { exact: true }).fill('2026-12-01')
    await ana.getByLabel('Au', { exact: true }).fill('2026-12-02')
    // Voiture (par défaut) et marche pour les courts trajets en ville.
    await ana.getByRole('checkbox', { name: 'À pied' }).check()
    await ana.getByRole('button', { name: 'Créer le voyage' }).click()
    await expect(ana.getByRole('heading', { name: 'Costa Rica en groupe' })).toBeVisible()
    await expect(ana.getByText('En direct')).toBeVisible()
    tripId = tripIdOf(ana)
  })

  await test.step('les lieux ajoutés apparaissent sans recharger la page', async () => {
    for (const [key, place] of Object.entries(PLACES)) {
      ids[key] = (await api<{ id: string }>(ana, 'POST', `/trips/${tripId}/pois`, place)).id
    }
    // Ajoutés hors de l'interface : c'est la notification temps réel qui met la liste à jour.
    await expect(ana.getByText('Lieux du voyage (6)')).toBeVisible()
  })

  const bruno = await newPerson(browser)
  await test.step('Bruno rejoint le voyage par le lien d’invitation, sans mot de passe', async () => {
    await tab(ana, 'Membres')
    await ana.getByRole('button', { name: 'Créer un lien d’invitation' }).click()
    const link = await ana.getByLabel('Lien à partager').inputValue()
    expect(link).toContain('/invite/')

    await bruno.goto(new URL(link).pathname)
    await bruno.getByLabel('Prénom ou pseudo').fill('Bruno')
    await bruno.getByLabel('Adresse email').fill(uniqueEmail('bruno'))
    await bruno.getByRole('button', { name: 'Rejoindre ce voyage' }).click()
    await expect(bruno.getByRole('heading', { name: 'Costa Rica en groupe' })).toBeVisible()
    await expect(bruno.getByText('Éditeur', { exact: true })).toBeVisible()
  })

  await test.step('chacun voit l’autre en ligne', async () => {
    await expect(ana.getByText('Aussi en ligne : Bruno')).toBeVisible()
    await expect(bruno.getByText('Aussi en ligne : Ana')).toBeVisible()
    await expect(ana.getByRole('listitem').filter({ hasText: 'Bruno' })).toBeVisible()
  })

  await test.step('Bruno note un lieu ; Ana voit la note arriver', async () => {
    await listedPlace(bruno, /Volcán Arenal/).click()
    const review = bruno.getByRole('form', { name: 'Mon avis' })
    await review.getByRole('radio', { name: 'Noter 5 sur 5' }).click()
    await review.getByLabel('Commentaire').fill('Incontournable au lever du soleil')
    await review.getByRole('button', { name: 'Enregistrer mon avis' }).click()
    await expect(bruno.getByRole('status')).toHaveText('Avis enregistré.')
    await expect(bruno.getByRole('region', { name: 'Avis du groupe' })).toContainText('Incontournable au lever du soleil')
    await expect(review.getByRole('button', { name: 'Retirer mon avis' })).toBeVisible()

    await tab(ana, 'Lieux')
    await expect(listedPlace(ana, /Volcán Arenal/)).toContainText('1 avis')
    await expect(ana.getByText(/Bruno a modifié les avis/)).toBeVisible()
  })

  await test.step('Ana choisit les hébergements de chaque soir', async () => {
    await tab(ana, 'Transports')
    await ana.getByRole('combobox', { name: /Hébergement du mar\. 1 déc\./ }).selectOption({ label: PLACES.hotelSanJose.name })
    await ana.getByRole('combobox', { name: /Hébergement du mer\. 2 déc\./ }).selectOption({ label: PLACES.hotelFortuna.name })
    await expect(ana.getByRole('combobox', { name: /Hébergement du mer\. 2 déc\./ })).toHaveValue(ids.hotelFortuna)
  })

  await test.step('l’itinéraire regroupe les lieux par région et respecte les horaires', async () => {
    await tab(ana, 'Itinéraire')
    await ana.getByRole('button', { name: 'Générer l’itinéraire' }).click()
    await expect(day(ana, 'mar. 1 déc.')).toBeVisible({ timeout: 30_000 })
    await expectStops(ana, 'mar. 1 déc.', [PLACES.museum.name, PLACES.cafe.name])
    await expectStops(ana, 'mer. 2 déc.', [PLACES.arenal.name, PLACES.waterfall.name])
    // Départ de San José, arrivée au lodge : le transfert est dans la journée.
    await expect(day(ana, 'mer. 2 déc.')).toContainText(`Départ : ${PLACES.hotelSanJose.name}`)
    await expect(day(ana, 'mer. 2 déc.')).toContainText(`Arrivée : ${PLACES.hotelFortuna.name}`)
    // Les hébergements ne sont pas des visites.
    await expect(ana.getByText(/Hors itinéraire/)).toHaveCount(0)

    // Bruno, sur l'onglet Itinéraire, voit le même résultat.
    await tab(bruno, 'Itinéraire')
    await expectStops(bruno, 'mer. 2 déc.', [PLACES.arenal.name, PLACES.waterfall.name])
  })

  await test.step('Ana déplace et verrouille des étapes ; Bruno suit en direct', async () => {
    await day(ana, 'mar. 1 déc.')
      .getByRole('combobox', { name: `Changer le jour de ${PLACES.cafe.name}` })
      .selectOption({ label: 'Vers le mer. 2 déc.' })
    await expect(stopNames(ana, 'mer. 2 déc.')).toContainText([PLACES.cafe.name])
    await expect(stopNames(bruno, 'mer. 2 déc.')).toContainText([PLACES.cafe.name])
    await expect(stopNames(bruno, 'mar. 1 déc.')).not.toContainText([PLACES.cafe.name])

    await ana.getByRole('button', { name: `Verrouiller ${PLACES.museum.name}` }).click()
    await expect(ana.getByRole('button', { name: `Déverrouiller ${PLACES.museum.name}` })).toBeVisible()
    // Bruno est éditeur : il voit le verrou actif (et peut l'ôter).
    await expect(bruno.getByRole('button', { name: `Déverrouiller ${PLACES.museum.name}` })).toHaveAttribute(
      'aria-pressed',
      'true',
    )
  })

  await test.step('les étapes se réordonnent au glisser-déposer', async () => {
    const before = await stopNames(ana, 'mer. 2 déc.').allTextContents()
    const last = before.at(-1)!
    const first = before[0]
    await drag(
      ana,
      day(ana, 'mer. 2 déc.').getByRole('button', { name: `Déplacer ${last}` }),
      day(ana, 'mer. 2 déc.').getByRole('button', { name: first, exact: true }),
    )
    await expect(stopNames(ana, 'mer. 2 déc.').first()).toHaveText(last)
    await expect(stopNames(bruno, 'mer. 2 déc.').first()).toHaveText(last)
  })

  await test.step('Bruno, retiré du voyage, perd l’accès immédiatement', async () => {
    await tab(ana, 'Membres')
    ana.once('dialog', (d) => d.accept())
    await ana.getByRole('listitem').filter({ hasText: 'Bruno' }).getByRole('button', { name: 'Retirer' }).click()
    await expect(bruno.getByRole('alert')).toContainText('Vous n’avez plus accès à ce voyage')
    await expect(ana.getByText('Personne d’autre en ligne')).toBeVisible()
  })

  await bruno.context().close()
})
