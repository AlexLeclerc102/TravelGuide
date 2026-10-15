import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Poi } from '../api/pois'
import type { RoutePlan, TripDay } from '../api/transport'
import { demoUser, mockApi, renderApp } from '../test/utils'

vi.mock('../components/MapView', () => ({
  MapView: ({ route }: { route?: [number, number][] | null }) => (
    <div data-testid="map">{route ? `tracé ${route.length} points` : 'carte'}</div>
  ),
}))

const health = () => ({ status: 200, body: { status: 'ok', database: 'ok', version: '0.1.0' } })
const trip = (my_role: string) => ({
  id: 'trip-1',
  name: 'Pura Vida',
  description: '',
  start_date: '2026-12-01',
  end_date: '2026-12-02',
  timezone: 'America/Costa_Rica',
  day_start: '09:00',
  day_end: '19:00',
  default_modes: ['car'],
  version: 1,
  my_role,
  member_count: 2,
  created_at: '2026-10-15T00:00:00Z',
})

function poi(id: string, name: string, category: Poi['category'] = 'sight'): Poi {
  return {
    id,
    trip_id: 'trip-1',
    name,
    category,
    lat: 9.93,
    lon: -84.07,
    address: '',
    duration_min: 60,
    opening_hours: null,
    opening_hours_status: 'none',
    closed_dates: [],
    country_code: 'CR',
    website: null,
    phone: null,
    email: null,
    wikidata: null,
    wikipedia: null,
    notes: '',
    osm: null,
    rating: { count: 0, average: null, min: null, max: null, disagreement: false },
    my_review: null,
    version: 1,
    created_by: null,
    created_by_name: null,
    created_at: '2026-10-15T00:00:00Z',
    updated_at: '2026-10-15T00:00:00Z',
  }
}

const pois = [poi('p1', 'Museo Nacional', 'museum'), poi('p2', 'Hotel Fortuna', 'lodging'), poi('p3', 'Café Central', 'food')]
const day = (date: string, extra: Partial<TripDay> = {}): TripDay => ({
  date,
  modes: ['car'],
  modes_inherited: true,
  base_poi_id: null,
  ...extra,
})

function routes(role: string, extra: Record<string, Parameters<typeof mockApi>[0][string]> = {}) {
  return {
    'GET /me': () => ({ status: 200, body: demoUser }),
    'GET /health': health,
    'GET /trips/trip-1': () => ({ status: 200, body: trip(role) }),
    'GET /trips/trip-1/pois': () => ({ status: 200, body: pois }),
    'GET /trips/trip-1/days': () => ({ status: 200, body: [day('2026-12-01'), day('2026-12-02')] }),
    ...extra,
  }
}

async function openTransport() {
  renderApp('/trips/trip-1')
  fireEvent.click(await screen.findByRole('tab', { name: 'Transports' }))
}

afterEach(() => vi.restoreAllMocks())

describe('transports', () => {
  it('un éditeur ajoute un mode et choisit l’hébergement d’un jour', async () => {
    const bodies: Record<string, unknown>[] = []
    mockApi(
      routes('editor', {
        'PUT /trips/trip-1/days/2026-12-01': (init) => {
          const body = JSON.parse(String(init?.body))
          bodies.push(body)
          return {
            status: 200,
            body: day('2026-12-01', {
              modes: body.modes ?? ['car'],
              modes_inherited: body.modes === null,
              base_poi_id: body.base_poi_id,
            }),
          }
        },
      }),
    )
    await openTransport()
    const modes = await screen.findByRole('group', { name: /Moyens de transport du mar\. 1 déc\./ })
    expect(within(modes).getByLabelText('Voiture')).toBeChecked()

    fireEvent.click(within(modes).getByLabelText('À pied'))
    await waitFor(() => expect(bodies).toHaveLength(1))
    // Les modes sont envoyés dans l'ordre canonique.
    expect(bodies[0]).toEqual({ modes: ['walk', 'car'], base_poi_id: null })
    expect(await screen.findByText('personnalisé')).toBeInTheDocument()

    const lodging = screen.getByRole('combobox', { name: /Hébergement du mar\. 1 déc\./ })
    // Les hébergements sont proposés en premier.
    expect(within(lodging).getAllByRole('option').map((o) => o.textContent)).toEqual([
      'Aucun',
      'Hotel Fortuna',
      'Museo Nacional',
      'Café Central',
    ])
    fireEvent.change(lodging, { target: { value: 'p2' } })
    await waitFor(() => expect(bodies).toHaveLength(2))
    expect(bodies[1]).toEqual({ modes: ['walk', 'car'], base_poi_id: 'p2' })

    // Décocher pour revenir aux modes du voyage : le jour les hérite de nouveau.
    fireEvent.click(within(modes).getByLabelText('À pied'))
    await waitFor(() => expect(bodies).toHaveLength(3))
    expect(bodies[2]).toEqual({ modes: null, base_poi_id: 'p2' })
  })

  it('le dernier mode ne peut pas être retiré', async () => {
    const put = vi.fn(() => ({ status: 200, body: day('2026-12-01') }))
    mockApi(routes('editor', { 'PUT /trips/trip-1/days/2026-12-01': put }))
    await openTransport()
    const modes = await screen.findByRole('group', { name: /Moyens de transport du mar\. 1 déc\./ })
    fireEvent.click(within(modes).getByLabelText('Voiture'))
    expect(put).not.toHaveBeenCalled()
  })

  it('un lecteur consulte sans pouvoir modifier', async () => {
    mockApi(routes('viewer'))
    await openTransport()
    const modes = await screen.findByRole('group', { name: /Moyens de transport du mar\. 1 déc\./ })
    expect(modes).toBeDisabled()
    expect(screen.getByRole('combobox', { name: /Hébergement du mar\. 1 déc\./ })).toBeDisabled()
  })

  it('compare les trajets entre deux lieux et signale les estimations', async () => {
    let request: Record<string, unknown> = {}
    const plan: RoutePlan = {
      modes: ['walk', 'car', 'transit'],
      total_duration_s: 840,
      has_estimates: false,
      legs: [
        {
          from_poi_id: 'p1',
          to_poi_id: 'p3',
          chosen: { mode: 'walk', duration_s: 840, distance_m: 1060, source: 'router' },
          options: [
            { mode: 'walk', duration_s: 840, distance_m: 1060, source: 'router' },
            { mode: 'car', duration_s: 3900, distance_m: 1200, source: 'straight_line' },
            { mode: 'transit', duration_s: 1300, distance_m: 1200, source: 'transit_estimate' },
          ],
          geometry: [
            [-84.07, 9.93],
            [-84.075, 9.932],
            [-84.079, 9.934],
          ],
        },
      ],
    }
    mockApi(
      routes('viewer', {
        'POST /trips/trip-1/routes': (init) => {
          request = JSON.parse(String(init?.body))
          return { status: 200, body: plan }
        },
      }),
    )
    await openTransport()
    fireEvent.change(await screen.findByRole('combobox', { name: 'Départ' }), { target: { value: 'p1' } })
    fireEvent.change(screen.getByRole('combobox', { name: 'Arrivée' }), { target: { value: 'p3' } })
    // Attendre le chargement des jours avant de choisir celui dont on compare les modes.
    await screen.findByRole('option', { name: 'Modes du mer. 2 déc.' })
    fireEvent.change(screen.getByRole('combobox', { name: 'Modes à comparer' }), { target: { value: '2026-12-02' } })

    const options = await screen.findByRole('list', { name: 'Comparer un trajet' })
    await waitFor(() => expect(request).toEqual({ poi_ids: ['p1', 'p3'], date: '2026-12-02' }))
    const items = within(options).getAllByRole('listitem')
    expect(items[0]).toHaveTextContent('À pied14 min1,1 kmRetenu')
    expect(items[1]).toHaveTextContent('≈ 1 h 05')
    expect(items[1]).toHaveTextContent('estimation à vol d’oiseau')
    expect(items[2]).toHaveTextContent('estimation')
    expect(screen.getByText(/estimés à vol d’oiseau/)).toBeInTheDocument()
    // Le tracé retenu est transmis à la carte.
    expect(screen.getByTestId('map')).toHaveTextContent('tracé 3 points')

    // Revenir à l'onglet Lieux efface le tracé.
    fireEvent.click(screen.getByRole('tab', { name: 'Lieux' }))
    expect(screen.getByTestId('map')).toHaveTextContent('carte')
  })
})
