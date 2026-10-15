import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { sortPois, type GroupRating, type Poi } from '../api/pois'
import { demoUser, mockApi, renderApp } from '../test/utils'

vi.mock('../components/MapView', () => ({ MapView: () => <div>carte</div> }))

const health = () => ({ status: 200, body: { status: 'ok', database: 'ok', version: '0.1.0' } })
const trip = (my_role: string) => ({
  id: 'trip-1',
  name: 'Pura Vida',
  description: '',
  start_date: '2026-12-01',
  end_date: '2026-12-08',
  timezone: 'America/Costa_Rica',
  day_start: '09:00',
  day_end: '19:00',
  default_modes: ['car'],
  version: 1,
  my_role,
  member_count: 3,
  created_at: '2026-10-15T00:00:00Z',
})

const noRating: GroupRating = { count: 0, average: null, min: null, max: null, disagreement: false }

function poi(id: string, name: string, rating: GroupRating = noRating, extra: Partial<Poi> = {}): Poi {
  return {
    id,
    trip_id: 'trip-1',
    name,
    category: 'nature',
    lat: 10.46,
    lon: -84.7,
    address: '',
    duration_min: 150,
    opening_hours: null,
    opening_hours_status: 'none',
    closed_dates: [],
    country_code: 'CR',
    website: null,
    notes: '',
    osm: null,
    rating,
    my_review: null,
    version: 1,
    created_by: null,
    created_by_name: null,
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
    ...extra,
  }
}

const split: GroupRating = { count: 2, average: 3, min: 1, max: 5, disagreement: true }
const loved: GroupRating = { count: 2, average: 4.5, min: 4, max: 5, disagreement: false }

function routes(role: string, pois: Poi[], extra: Record<string, Parameters<typeof mockApi>[0][string]> = {}) {
  return {
    'GET /me': () => ({ status: 200, body: demoUser }),
    'GET /health': health,
    'GET /trips/trip-1': () => ({ status: 200, body: trip(role) }),
    'GET /trips/trip-1/pois': () => ({ status: 200, body: pois }),
    ...extra,
  }
}

afterEach(() => vi.restoreAllMocks())

describe('avis', () => {
  it('un membre (même lecteur) note un lieu et la liste se met à jour', async () => {
    let body: Record<string, unknown> = {}
    mockApi(
      routes('viewer', [poi('p1', 'Volcán Arenal')], {
        'GET /trips/trip-1/pois/p1/reviews': () => ({ status: 200, body: [] }),
        'PUT /trips/trip-1/pois/p1/reviews/me': (init) => {
          body = JSON.parse(String(init?.body))
          return {
            status: 200,
            body: poi('p1', 'Volcán Arenal', { count: 1, average: 4, min: 4, max: 4, disagreement: false }, { my_review: { rating: 4, comment: 'Superbe' } }),
          }
        },
        'DELETE /trips/trip-1/pois/p1/reviews/me': () => ({ status: 200, body: poi('p1', 'Volcán Arenal') }),
      }),
    )
    renderApp('/trips/trip-1')
    expect(await screen.findByText('À noter')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Volcán Arenal/ }))
    const form = await screen.findByRole('form', { name: 'Mon avis' })
    const save = within(form).getByRole('button', { name: 'Enregistrer mon avis' })
    expect(save).toBeDisabled() // pas de note choisie
    fireEvent.click(within(form).getByRole('radio', { name: 'Noter 4 sur 5' }))
    fireEvent.change(within(form).getByLabelText('Commentaire (facultatif)'), { target: { value: 'Superbe' } })
    fireEvent.click(save)

    await waitFor(() => expect(screen.getByText(/★ 4 · 1 avis/)).toBeInTheDocument())
    expect(body).toEqual({ rating: 4, comment: 'Superbe' })
    expect(screen.queryByText('À noter')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Retirer mon avis' })).toBeInTheDocument()
    // Confirmation, qui disparaît dès que l'on modifie de nouveau son avis.
    expect(screen.getByRole('status')).toHaveTextContent('Avis enregistré.')
    const edited = screen.getByRole('form', { name: 'Mon avis' })
    fireEvent.click(within(edited).getByRole('radio', { name: 'Noter 5 sur 5' }))
    expect(screen.queryByText('Avis enregistré.')).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Retirer mon avis' }))
    expect(await screen.findByRole('status')).toHaveTextContent('Avis retiré.')
    expect(screen.queryByRole('button', { name: 'Retirer mon avis' })).not.toBeInTheDocument()
  })

  it('affiche la moyenne, l’avis de chacun et signale le désaccord', async () => {
    mockApi(
      routes('editor', [poi('p1', 'Playa Conchal', split)], {
        'GET /trips/trip-1/pois/p1/reviews': () => ({
          status: 200,
          body: [
            { user_id: 'u2', display_name: 'Lucía', rating: 5, comment: 'Eau turquoise', updated_at: '' },
            { user_id: 'u3', display_name: 'Marc', rating: 1, comment: 'Trop loin', updated_at: '' },
          ],
        }),
      }),
    )
    renderApp('/trips/trip-1')
    expect(await screen.findByText(/Désaccord/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Playa Conchal/ }))
    const section = await screen.findByRole('region', { name: 'Avis du groupe' })
    expect(within(section).getByText('3 / 5')).toBeInTheDocument()
    expect(within(section).getByText('Les notes vont de 1 à 5 : à discuter ensemble.')).toBeInTheDocument()
    expect(await within(section).findByText('Eau turquoise')).toBeInTheDocument()
    expect(within(section).getByText('Trop loin')).toBeInTheDocument()
    // Les deux avis extrêmes sont mis en évidence.
    expect(section.querySelectorAll('.review-list li.extreme')).toHaveLength(2)
  })

  it('trie la liste par note ou par désaccord', async () => {
    mockApi(routes('editor', [poi('p1', 'Sans avis'), poi('p2', 'Partagé', split), poi('p3', 'Adoré', loved)]))
    renderApp('/trips/trip-1')
    await screen.findByText('Adoré')
    const names = () => [...document.querySelectorAll('.poi-list strong')].map((e) => e.textContent)
    expect(names()).toEqual(['Sans avis', 'Partagé', 'Adoré'])
    fireEvent.change(screen.getByLabelText(/Trier/), { target: { value: 'rating' } })
    expect(names()).toEqual(['Adoré', 'Partagé', 'Sans avis'])
    fireEvent.change(screen.getByLabelText(/Trier/), { target: { value: 'disagreement' } })
    expect(names()[0]).toBe('Partagé')
  })
})

describe('sortPois', () => {
  it('ne modifie pas la liste d’origine', () => {
    const list = [poi('a', 'A'), poi('b', 'B', loved)]
    expect(sortPois(list, 'rating').map((p) => p.id)).toEqual(['b', 'a'])
    expect(list.map((p) => p.id)).toEqual(['a', 'b'])
    expect(sortPois(list, 'added')).toBe(list)
  })
})
