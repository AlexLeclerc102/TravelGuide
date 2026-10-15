import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { dayPoiIds, type Itinerary, type ItineraryDay } from '../api/itinerary'
import type { Poi } from '../api/pois'
import type { RoutePlan } from '../api/transport'
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

const pois = [
  poi('hotel', 'Hotel San José', 'lodging'),
  poi('museum', 'Museo Nacional', 'museum'),
  poi('cafe', 'Café Central', 'food'),
  poi('closed', 'Toujours fermé'),
]

const car = (minutes: number) => ({ mode: 'car' as const, duration_s: minutes * 60, distance_m: minutes * 500, source: 'router' as const })

const day1: ItineraryDay = {
  date: '2026-12-01',
  modes: ['car'],
  start_poi_id: 'hotel',
  end_poi_id: 'hotel',
  day_start: '09:00',
  day_end: '19:00',
  stops: [
    { poi_id: 'museum', arrival: '09:10', start: '09:30', end: '11:00', wait_s: 1200, travel: car(10), locked: false },
    {
      poi_id: 'cafe',
      arrival: '11:05',
      start: '11:05',
      end: '12:20',
      wait_s: 0,
      travel: { mode: 'walk', duration_s: 300, distance_m: 400, source: 'straight_line' },
      locked: false,
    },
  ],
  back: car(8),
  total_travel_s: 23 * 60,
  total_visit_s: 165 * 60,
  finish: '12:28',
  stale: false,
}
const day2: ItineraryDay = {
  ...day1,
  date: '2026-12-02',
  stops: [],
  back: null,
  total_travel_s: 0,
  total_visit_s: 0,
  finish: '09:00',
}

const itinerary = (extra: Partial<Itinerary> = {}): Itinerary => ({
  version: 1,
  generated_at: '2026-10-15T10:00:00Z',
  generated_by_name: 'Ana',
  days: [day1, day2],
  unscheduled: [{ poi_id: 'closed', reason: 'closed_all_days' }],
  stale: false,
  has_estimates: true,
  ...extra,
})

const routePlan: RoutePlan = {
  modes: ['car'],
  total_duration_s: 1380,
  has_estimates: false,
  legs: [
    ['hotel', 'museum'],
    ['museum', 'cafe'],
    ['cafe', 'hotel'],
  ].map(([from, to]) => ({
    from_poi_id: from,
    to_poi_id: to,
    chosen: car(5),
    options: [car(5)],
    geometry: [
      [-84.07, 9.93],
      [-84.08, 9.94],
    ] as [number, number][],
  })),
}

function routes(role: string, extra: Record<string, Parameters<typeof mockApi>[0][string]> = {}) {
  return {
    'GET /me': () => ({ status: 200, body: demoUser }),
    'GET /health': health,
    'GET /trips/trip-1': () => ({ status: 200, body: trip(role) }),
    'GET /trips/trip-1/pois': () => ({ status: 200, body: pois }),
    'POST /trips/trip-1/routes': () => ({ status: 200, body: routePlan }),
    ...extra,
  }
}

async function openItinerary() {
  renderApp('/trips/trip-1')
  fireEvent.click(await screen.findByRole('tab', { name: 'Itinéraire' }))
}

afterEach(() => vi.restoreAllMocks())

describe('itinéraire', () => {
  it('un éditeur génère l’itinéraire et voit chaque journée', async () => {
    let generated = false
    mockApi(
      routes('editor', {
        'GET /trips/trip-1/itinerary': () => ({ status: 200, body: generated ? itinerary() : null }),
        'POST /trips/trip-1/itinerary': () => {
          generated = true
          return { status: 200, body: itinerary() }
        },
      }),
    )
    await openItinerary()
    fireEvent.click(await screen.findByRole('button', { name: 'Générer l’itinéraire' }))

    const first = await screen.findByRole('region', { name: 'mar. 1 déc.' })
    expect(within(first).getByText(/2 étapes · visites 2 h 45 · trajets 23 min · fin 12:28/)).toBeInTheDocument()
    expect(within(first).getByText('🛏 Départ : Hotel San José')).toBeInTheDocument()
    expect(within(first).getByText('09:30–11:00')).toBeInTheDocument()
    expect(within(first).getByText('attente 20 min')).toBeInTheDocument()
    // Trajet estimé : signalé.
    expect(within(first).getByText(/🚶 À pied · ≈ 5 min · 400 m · estimation à vol d’oiseau/)).toBeInTheDocument()
    expect(within(first).getByText('🛏 Arrivée : Hotel San José')).toBeInTheDocument()

    expect(screen.getByRole('region', { name: 'mer. 2 déc.' })).toHaveTextContent('Journée libre')
    expect(screen.getByText('fermé à toutes les dates du voyage')).toBeInTheDocument()
    expect(screen.getByText(/Certains trajets sont des estimations/)).toBeInTheDocument()
    // Le premier jour avec des étapes est tracé sur la carte.
    await waitFor(() => expect(screen.getByTestId('map')).toHaveTextContent('tracé 6 points'))

    // Cliquer sur une étape ouvre la fiche du lieu.
    fireEvent.click(within(first).getByRole('button', { name: 'Café Central' }))
    expect(await screen.findByRole('heading', { name: '🍽️ Café Central' })).toBeInTheDocument()
    expect(screen.getByTestId('map')).toHaveTextContent('carte')
  })

  it('un lecteur sans itinéraire ne peut pas le générer', async () => {
    mockApi(routes('viewer', { 'GET /trips/trip-1/itinerary': () => ({ status: 200, body: null }) }))
    await openItinerary()
    expect(await screen.findByText('Un éditeur du voyage peut le générer.')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Générer l’itinéraire' })).not.toBeInTheDocument()
  })

  it('signale un itinéraire périmé et demande confirmation avant de régénérer', async () => {
    const post = vi.fn(() => ({ status: 200, body: itinerary({ version: 2 }) }))
    mockApi(
      routes('editor', {
        'GET /trips/trip-1/itinerary': () => ({
          status: 200,
          body: itinerary({ stale: true, days: [{ ...day1, stale: true }, day2] }),
        }),
        'POST /trips/trip-1/itinerary': post,
      }),
    )
    const confirm = vi.spyOn(window, 'confirm').mockReturnValueOnce(false).mockReturnValueOnce(true)
    await openItinerary()
    expect(await screen.findByText(/ont changé depuis/)).toBeInTheDocument()
    expect(within(screen.getByRole('region', { name: 'mar. 1 déc.' })).getByText('à recalculer')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Régénérer' }))
    expect(post).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Régénérer' }))
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1))
    expect(confirm).toHaveBeenCalledTimes(2)
    await waitFor(() => expect(screen.queryByText(/ont changé depuis/)).not.toBeInTheDocument())
  })
})

describe('dayPoiIds', () => {
  it('relie les hébergements et les étapes sans doublon consécutif', () => {
    expect(dayPoiIds(day1)).toEqual(['hotel', 'museum', 'cafe', 'hotel'])
    expect(dayPoiIds(day2)).toEqual(['hotel'])
    expect(dayPoiIds({ ...day1, start_poi_id: null, end_poi_id: 'b' })).toEqual(['museum', 'cafe', 'b'])
  })
})
