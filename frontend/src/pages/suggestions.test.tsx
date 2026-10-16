import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Place, Poi } from '../api/pois'
import type { Regions, Suggestion, Suggestions } from '../api/suggestions'
import { demoUser, mockApi, renderApp } from '../test/utils'

vi.mock('../components/MapView', () => ({
  MapView: ({ markers = [] }: { markers?: { suggestion?: boolean }[] }) => (
    <div data-testid="map">{markers.filter((m) => m.suggestion).length} suggestions sur la carte</div>
  ),
}))

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

const fortuna: Poi = {
  id: 'area',
  trip_id: 'trip-1',
  name: 'La Fortuna',
  category: 'lodging',
  placeholder: true,
  lat: 10.47,
  lon: -84.64,
  address: '',
  duration_min: 0,
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

function place(id: number, name: string, extra: Partial<Place> = {}): Place {
  return {
    name,
    display_name: name,
    lat: 10.46,
    lon: -84.7,
    category: 'nature',
    osm_tag: 'natural=volcano',
    osm: { type: 'node', id },
    opening_hours: null,
    website: null,
    phone: null,
    email: null,
    wikidata: null,
    wikipedia: null,
    country_code: 'CR',
    ...extra,
  }
}

const arenal: Suggestion = {
  place: place(1, 'Volcán Arenal', { wikipedia: 'en:Arenal Volcano', website: 'https://arenal.example' }),
  score: 60.5,
  date: '2026-12-02',
  travel_min: 15,
  reasons: [
    { kind: 'notable', languages: 41 },
    { kind: 'world_heritage' },
    { kind: 'protected_area', km2: 1204, national_park: true },
    { kind: 'popular', monthly_views: 5249 },
    { kind: 'wikivoyage' },
    { kind: 'near', date: '2026-12-02', minutes: 15 },
    { kind: 'taste', category: 'nature', average: 4.5 },
    { kind: 'free_time', date: '2026-12-02', minutes: 150 },
    { kind: 'interest', interest: 'beach' },
  ],
}
const soda: Suggestion = {
  place: place(3, 'Soda La Parada', { category: 'food', osm_tag: 'amenity=restaurant' }),
  score: 40,
  date: '2026-12-01',
  travel_min: 1,
  reasons: [{ kind: 'near', date: '2026-12-01', minutes: 1 }],
}

function routes(role: string, suggestions: () => Suggestions, extra: Parameters<typeof mockApi>[0] = {}) {
  return {
    'GET /me': () => ({ status: 200, body: demoUser }),
    'GET /health': () => ({ status: 200, body: { status: 'ok', database: 'ok', version: '0.1.0' } }),
    'GET /trips/trip-1': () => ({ status: 200, body: trip(role) }),
    'GET /trips/trip-1/pois': () => ({ status: 200, body: [fortuna] }),
    'GET /trips/trip-1/days': () => ({
      status: 200,
      body: [
        { date: '2026-12-01', modes: ['car'], modes_inherited: true, base_poi_id: 'area' },
        { date: '2026-12-02', modes: ['car'], modes_inherited: true, base_poi_id: 'area' },
      ],
    }),
    'GET /trips/trip-1/suggestions': () => ({ status: 200, body: suggestions() }),
    'GET /trips/trip-1/regions': () => ({ status: 200, body: { status: 'ready', regions: [] } satisfies Regions }),
    ...extra,
  }
}

async function openSuggestions() {
  renderApp('/trips/trip-1')
  fireEvent.click(await screen.findByRole('tab', { name: 'Suggestions' }))
}

afterEach(() => vi.restoreAllMocks())

describe('suggestions', () => {
  it('explique chaque suggestion et l’affiche sur la carte', async () => {
    const api = mockApi(routes('viewer', () => ({ status: 'ready', suggestions: [arenal, soda] })))
    await openSuggestions()
    const card = (await screen.findByText('Volcán Arenal')).closest('li')!
    expect(within(card).getByText('Article Wikipédia en 41 langues')).toBeInTheDocument()
    expect(within(card).getByText('Patrimoine mondial de l’UNESCO')).toBeInTheDocument()
    expect(within(card).getByText(/^Parc national de 1\s204 km²$/)).toBeInTheDocument()
    expect(within(card).getByText(/^Très consulté : ≈ 5\s200 lectures Wikipédia par mois$/)).toBeInTheDocument()
    expect(within(card).getByText('Décrit dans un guide Wikivoyage')).toBeInTheDocument()
    expect(within(card).getByText('≈ 15 min de La Fortuna (mer. 2 déc.)')).toBeInTheDocument()
    expect(within(card).getByText('Le groupe aime : nature (4,5 ★)')).toBeInTheDocument()
    expect(within(card).getByText('2 h 30 libres le mer. 2 déc.')).toBeInTheDocument()
    expect(within(card).getByText('Correspond à vos envies : plages')).toBeInTheDocument()
    expect(within(card).getByRole('link', { name: 'Wikipédia' })).toHaveAttribute('href', 'https://en.wikipedia.org/wiki/Arenal_Volcano')
    expect(screen.getByTestId('map')).toHaveTextContent('2 suggestions sur la carte')
    // Lecteur : pas d'ajout ni de refus.
    expect(within(card).queryByRole('button', { name: /Ajouter/ })).not.toBeInTheDocument()

    // Filtre « Où manger » : la catégorie part dans la requête.
    fireEvent.click(screen.getByRole('radio', { name: 'Où manger' }))
    await waitFor(() => expect(api.mock.calls.some(([url]) => String(url).endsWith('/suggestions?category=food'))).toBe(true))
  })

  it('un éditeur ajoute une suggestion ou l’écarte', async () => {
    const bodies: Record<string, Record<string, unknown>> = {}
    let list = [arenal, soda]
    mockApi(
      routes('editor', () => ({ status: 'ready', suggestions: list }), {
        'POST /trips/trip-1/pois': (init) => {
          bodies.poi = JSON.parse(String(init?.body))
          list = [soda]
          return { status: 201, body: { ...fortuna, id: 'p1', name: 'Volcán Arenal', category: 'nature', placeholder: false, osm: { type: 'node', id: 1 } } }
        },
        'POST /trips/trip-1/suggestions/dismissals': (init) => {
          bodies.dismissal = JSON.parse(String(init?.body))
          list = []
          return { status: 204 }
        },
      }),
    )
    await openSuggestions()
    fireEvent.click(await screen.findByRole('button', { name: 'Ajouter Volcán Arenal' }))
    await waitFor(() => expect(screen.queryByText('Volcán Arenal')).not.toBeInTheDocument())
    expect(bodies.poi).toMatchObject({ name: 'Volcán Arenal', category: 'nature', osm: { type: 'node', id: 1 }, wikipedia: 'en:Arenal Volcano' })

    fireEvent.click(screen.getByRole('button', { name: 'Écarter Soda La Parada' }))
    expect(await screen.findByText('Aucune suggestion pour l’instant autour de vos journées.')).toBeInTheDocument()
    expect(bodies.dismissal).toEqual({ osm: { type: 'node', id: 3 }, category: 'food' })
  })

  it('voyage vide : découverte du pays, puis import en cours', async () => {
    let status: Suggestions['status'] = 'discovery'
    mockApi(routes('editor', () => ({ status, suggestions: status === 'discovery' ? [arenal] : [] })))
    await openSuggestions()
    expect(await screen.findByText(/les lieux les plus réputés de tout le pays/)).toBeInTheDocument()
    expect(screen.getByText('Volcán Arenal')).toBeInTheDocument()
    // Les régions sont ouvertes d'emblée.
    expect(screen.getByText('Régions à découvrir').closest('details')).toHaveAttribute('open')
    status = 'not_imported'
    fireEvent.click(screen.getByRole('radio', { name: 'Nature' }))
    expect(await screen.findByText(/en cours d’import/)).toBeInTheDocument()
  })

  it('propose des régions et en ajoute les points forts', async () => {
    const added: string[] = []
    const regions: Regions = {
      status: 'ready',
      regions: [
        {
          name: 'La Fortuna',
          lat: 10.47,
          lon: -84.64,
          osm: { type: 'node', id: 7 },
          themes: ['nature', 'activity'],
          nights: 2,
          access: 'road',
          place_count: 4,
          highlights: [
            place(1, 'Volcán Arenal', { wikipedia: 'en:Arenal Volcano' }),
            place(2, 'Catarata Río Fortuna', { osm_tag: 'waterway=waterfall' }),
          ],
          in_trip: 1,
          world_heritage: [],
          wikivoyage: true,
        },
        {
          name: 'Base Wafer',
          lat: 5.54,
          lon: -87.06,
          osm: { type: 'node', id: 8 },
          themes: ['nature'],
          nights: 1,
          access: 'boat_or_plane',
          place_count: 1,
          highlights: [place(9, 'Isla del Coco')],
          in_trip: 0,
          world_heritage: ['Isla del Coco'],
          wikivoyage: false,
        },
      ],
    }
    let pois: Poi[] = [fortuna]
    mockApi(
      routes('editor', () => ({ status: 'discovery', suggestions: [] }), {
        'GET /trips/trip-1/pois': () => ({ status: 200, body: pois }),
        'GET /trips/trip-1/regions': () => ({ status: 200, body: regions }),
        'POST /trips/trip-1/pois': (init) => {
          const body = JSON.parse(String(init?.body))
          added.push(body.name)
          const poi: Poi = { ...fortuna, id: `p${added.length}`, name: body.name, category: 'nature', placeholder: false, osm: body.osm }
          pois = [...pois, poi]
          return { status: 201, body: poi }
        },
      }),
    )
    await openSuggestions()
    const card = (await screen.findByText('La Fortuna', { selector: 'strong' })).closest('li')!
    expect(within(card).getByText('2 nuits conseillées · Nature, Activité')).toBeInTheDocument()
    expect(within(card).getByText('1 lieu du voyage')).toBeInTheDocument()
    expect(within(card).getByText('et 2 autres lieux notables')).toBeInTheDocument()
    expect(within(card).getByRole('link', { name: 'Volcán Arenal' })).toHaveAttribute('href', 'https://en.wikipedia.org/wiki/Arenal_Volcano')

    expect(within(card).getByText('Guide Wikivoyage')).toBeInTheDocument()
    expect(within(card).queryByText(/UNESCO/)).not.toBeInTheDocument()
    expect(within(card).queryByText(/bateau/)).not.toBeInTheDocument()
    // Une île sans route : dit clairement, sans nuits conseillées trompeuses.
    const island = screen.getByText('Base Wafer', { selector: 'strong' }).closest('li')!
    expect(within(island).getByText(/Accessible uniquement en bateau ou en avion/)).toBeInTheDocument()
    expect(within(island).getByText(/Expédition de plusieurs jours/)).toBeInTheDocument()
    expect(within(island).getByText('🏛 Patrimoine mondial de l’UNESCO : Isla del Coco')).toBeInTheDocument()
    expect(within(island).queryByText('Guide Wikivoyage')).not.toBeInTheDocument()
    expect(within(island).queryByText(/nuit conseillée/)).not.toBeInTheDocument()

    fireEvent.click(within(card).getByRole('button', { name: 'Ajouter les points forts de La Fortuna' }))
    await waitFor(() => expect(within(card).getByRole('button', { name: /La Fortuna/ })).toHaveTextContent('Points forts ajoutés'))
    expect(added).toEqual(['Volcán Arenal', 'Catarata Río Fortuna'])
  })

  it('propose des circuits types entre deux aéroports et en applique un', async () => {
    const calls: { url: string; body: unknown }[] = []
    const sjo = { type: 'way' as const, id: 23 }
    const stop = (name: string, first: string, last: string, nights: number, highlight: Place) => ({
      name,
      lat: 10.47,
      lon: -84.64,
      osm: { type: 'node' as const, id: nights },
      first_night: first,
      last_night: last,
      nights,
      transfer_min: 150,
      themes: ['nature' as const],
      highlights: [highlight],
    })
    mockApi(
      routes('editor', () => ({ status: 'discovery', suggestions: [] }), {
        'GET /trips/trip-1/airports': () => ({
          status: 200,
          body: [{ name: 'Juan Santamaría (SJO)', lat: 10, lon: -84.2, osm: sjo }],
        }),
        'POST /trips/trip-1/circuits': (init) => {
          calls.push({ url: 'circuits', body: JSON.parse(String(init?.body)) })
          return {
            status: 200,
            body: {
              status: 'ready',
              nights: 2,
              circuits: [
                {
                  themes: ['nature'],
                  stops: [
                    stop('La Fortuna', '2026-12-01', '2026-12-01', 1, place(1, 'Volcán Arenal')),
                    stop('Santa Elena', '2026-12-02', '2026-12-02', 1, place(2, 'Reserva Monteverde')),
                  ],
                  drive_min: 400,
                  return_min: 150,
                },
                { themes: ['nature'], stops: [stop('Quepos', '2026-12-01', '2026-12-02', 2, place(3, 'Manuel Antonio'))], drive_min: 300, return_min: 150 },
              ],
              boat_or_plane: ['Base Wafer'],
            },
          }
        },
        'POST /trips/trip-1/stays/proposal/apply': (init) => {
          calls.push({ url: 'apply', body: JSON.parse(String(init?.body)) })
          return { status: 200, body: [] }
        },
        'POST /trips/trip-1/pois': (init) => {
          const body = JSON.parse(String(init?.body))
          calls.push({ url: 'poi', body: body.name })
          return { status: 201, body: { ...fortuna, id: `p${calls.length}`, name: body.name, osm: body.osm } }
        },
      }),
    )
    await openSuggestions()
    const summary = await screen.findByText('Circuits types pour vos 2 nuits')
    const details = summary.closest('details')!
    details.open = true
    fireEvent(details, new Event('toggle'))
    const arrival = await screen.findByLabelText('Aéroport d’arrivée')
    await within(arrival).findByRole('option', { name: 'Juan Santamaría (SJO)' })
    fireEvent.change(arrival, { target: { value: 'way/23' } })
    fireEvent.click(screen.getByRole('button', { name: 'Proposer des circuits' }))

    const card = (await screen.findByText('Les incontournables')).closest('li')!
    expect(calls[0].body).toEqual({ start: sjo, end: null })
    expect(within(card).getByText('La Fortuna')).toBeInTheDocument()
    expect(within(card).getByText('≈ 6 h 40 de route entre les étapes · ≈ 2 h 30 pour rejoindre l’aéroport de départ')).toBeInTheDocument()
    expect(screen.getByText('Variante 1')).toBeInTheDocument()
    expect(screen.getByText(/en expédition\) : Base Wafer/)).toBeInTheDocument()

    fireEvent.click(within(card).getByRole('button', { name: 'Appliquer le circuit « Les incontournables »' }))
    expect(await within(card).findByRole('status')).toHaveTextContent('Circuit appliqué')
    expect(calls.map((c) => c.url)).toEqual(['circuits', 'apply', 'poi', 'poi'])
    expect(calls[1].body).toMatchObject({ stays: [{ name: 'La Fortuna', first_night: '2026-12-01', nights: 1, poi_ids: [] }, { name: 'Santa Elena' }] })
    expect(calls.slice(2).map((c) => c.body)).toEqual(['Volcán Arenal', 'Reserva Monteverde'])
  })

  it('choisit un hôtel de la zone : ajouté puis à la place de la zone', async () => {
    const calls: string[] = []
    let replaced: Record<string, unknown> = {}
    const hotel: Suggestion = {
      place: place(6, 'Hotel Arenal Springs', { category: 'lodging', osm_tag: 'tourism=hotel', website: 'https://springs.example' }),
      score: 30,
      date: null,
      travel_min: 2,
      reasons: [{ kind: 'near', date: null, minutes: 2 }],
    }
    mockApi(
      routes('editor', () => ({ status: 'ready', suggestions: [hotel] }), {
        'POST /trips/trip-1/pois': () => {
          calls.push('poi')
          return { status: 201, body: { ...fortuna, id: 'h1', name: 'Hotel Arenal Springs', placeholder: false } }
        },
        'POST /trips/trip-1/stays/replace': (init) => {
          calls.push('replace')
          replaced = JSON.parse(String(init?.body))
          return { status: 200, body: [] }
        },
      }),
    )
    renderApp('/trips/trip-1')
    fireEvent.click(await screen.findByRole('tab', { name: 'Transports' }))
    const summary = await screen.findByText('Hébergements dans la zone')
    fireEvent.click(summary)
    // Ouverture contrôlée par l'événement `toggle` de <details>.
    const details = summary.closest('details')!
    details.open = true
    fireEvent(details, new Event('toggle'))
    const choose = await screen.findByRole('button', { name: 'Choisir Hotel Arenal Springs' })
    expect(within(details).getByRole('link', { name: 'Site officiel' })).toHaveAttribute('href', 'https://springs.example')
    fireEvent.click(choose)
    await waitFor(() => expect(calls).toEqual(['poi', 'replace']))
    expect(replaced).toEqual({ from_poi_id: 'area', to_poi_id: 'h1', remove_from: true })
  })
})
