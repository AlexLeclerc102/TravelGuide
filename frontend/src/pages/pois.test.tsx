import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Place, Poi } from '../api/pois'
import { demoUser, mockApi, renderApp } from '../test/utils'

// Carte simulée : un bouton reproduit un clic aux coordonnées du parc national Arenal.
vi.mock('../components/MapView', () => ({
  MapView: ({ onMapClick }: { onMapClick?: (lat: number, lon: number) => void }) => (
    <button type="button" onClick={() => onMapClick?.(10.46, -84.69)}>
      clic carte
    </button>
  ),
}))

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
  member_count: 2,
  created_at: '2026-10-15T00:00:00Z',
})

const arenal: Place = {
  name: 'Volcán Arenal',
  display_name: 'Volcán Arenal, La Fortuna, Costa Rica',
  lat: 10.4621,
  lon: -84.7034,
  category: 'nature',
  osm_tag: 'natural=volcano',
  osm: { type: 'node', id: 276216895 },
  opening_hours: null,
  website: null,
  country_code: 'CR',
}

const museum: Poi = {
  id: 'poi-1',
  trip_id: 'trip-1',
  name: 'Museo Nacional',
  category: 'museum',
  lat: 9.9327,
  lon: -84.0716,
  address: 'San José',
  duration_min: 90,
  opening_hours: 'Tu-Sa 08:30-16:30; Su 09:00-16:30',
  opening_hours_status: 'valid',
  closed_dates: ['2026-12-07'],
  country_code: 'CR',
  website: null,
  notes: '',
  osm: { type: 'relation', id: 4144800 },
  rating: { count: 0, average: null, min: null, max: null, disagreement: false },
  my_review: null,
  version: 1,
  created_by: demoUser.id,
  created_by_name: 'Ana',
  created_at: '2026-10-15T00:00:00Z',
  updated_at: '2026-10-15T00:00:00Z',
}

const preview = {
  status: 'valid',
  error: null,
  days: [
    { date: '2026-12-01', intervals: [{ start: '08:30', end: '16:30', unknown: false }], holiday: true },
    { date: '2026-12-07', intervals: [], holiday: false },
  ],
}

function baseRoutes(role: string, pois: Poi[]) {
  return {
    'GET /me': () => ({ status: 200, body: demoUser }),
    'GET /health': health,
    'GET /trips/trip-1': () => ({ status: 200, body: trip(role) }),
    'GET /trips/trip-1/pois': () => ({ status: 200, body: pois }),
    'POST /trips/trip-1/opening-hours/preview': () => ({ status: 200, body: preview }),
  }
}

afterEach(() => vi.restoreAllMocks())

describe('lieux', () => {
  it('recherche un lieu, l’ajoute puis le signale comme déjà ajouté', async () => {
    const api = mockApi({
      ...baseRoutes('editor', [museum]),
      'GET /places/search': () => ({ status: 200, body: [arenal] }),
      'POST /trips/trip-1/pois': (init) => ({
        status: 201,
        body: { ...museum, id: 'poi-2', name: 'Volcán Arenal', category: 'nature', duration_min: 150, ...JSON.parse(String(init?.body)), closed_dates: [], opening_hours: null, opening_hours_status: 'none' },
      }),
    })
    renderApp('/trips/trip-1')
    // La liste affiche les jours de fermeture pendant le voyage.
    expect(await screen.findByText(/Fermé le lun\. 7 déc\./)).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Rechercher un lieu'), { target: { value: 'arenal' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rechercher' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Ajouter Volcán Arenal' }))

    await waitFor(() => expect(screen.getByRole('button', { name: 'Déjà ajouté' })).toBeInTheDocument())
    const post = api.mock.calls.find(([, init]) => init?.method === 'POST' && String(init.body).includes('osm'))
    const body = JSON.parse(String(post![1]!.body))
    expect(body).toMatchObject({ name: 'Volcán Arenal', category: 'nature', duration_min: 150, osm: arenal.osm, country_code: 'CR' })
    expect(screen.getByText('Lieux du voyage (2)')).toBeInTheDocument()
    // La recherche part avec le texte saisi, sans autocomplétion.
    const searches = api.mock.calls.filter(([url]) => String(url).includes('/places/search'))
    expect(searches).toHaveLength(1)
    expect(String(searches[0][0])).toContain('q=arenal')
  })

  it('ajoute un lieu par clic sur la carte, prérempli par OpenStreetMap', async () => {
    let created: Record<string, unknown> = {}
    const api = mockApi({
      ...baseRoutes('editor', []),
      'GET /places/reverse': () => ({
        status: 200,
        body: { place: { ...arenal, name: 'Parque Nacional Volcán Arenal', category: 'nature', lat: 10.44, lon: -84.72 } },
      }),
      'POST /trips/trip-1/pois': (init) => {
        created = JSON.parse(String(init?.body))
        return { status: 201, body: { ...museum, id: 'poi-3', ...created, closed_dates: [], opening_hours_status: 'none' } }
      },
    })
    renderApp('/trips/trip-1')
    fireEvent.click(await screen.findByRole('button', { name: /Ajouter en cliquant/ }))
    fireEvent.click(screen.getByRole('button', { name: 'clic carte' }))
    const form = await screen.findByRole('form', { name: 'Nouveau lieu' })
    expect(within(form).getByLabelText('Nom')).toHaveValue('Parque Nacional Volcán Arenal')
    expect(within(form).getByLabelText('Durée de visite (minutes)')).toHaveValue(150)
    fireEvent.click(within(form).getByRole('button', { name: 'Ajouter' }))
    await waitFor(() => expect(created.name).toBe('Parque Nacional Volcán Arenal'))
    // Le point cliqué est conservé, pas celui du résultat OSM.
    expect(created).toMatchObject({ lat: 10.46, lon: -84.69, category: 'nature' })
    expect(String(api.mock.calls.find(([u]) => String(u).includes('reverse'))![0])).toContain('lat=10.460000')
  })

  it('un lecteur consulte les lieux sans pouvoir les modifier', async () => {
    mockApi(baseRoutes('viewer', [museum]))
    renderApp('/trips/trip-1')
    fireEvent.click(await screen.findByRole('button', { name: /Museo Nacional/ }))
    expect(await screen.findByText('Horaires pendant le voyage')).toBeInTheDocument()
    expect(screen.getByText('férié')).toBeInTheDocument()
    expect(screen.queryByLabelText('Rechercher un lieu')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Enregistrer' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Ajouter en cliquant/ })).not.toBeInTheDocument()
  })

  it('modifie un lieu avec sa version et gère un conflit', async () => {
    let patchBody: Record<string, unknown> = {}
    mockApi({
      ...baseRoutes('editor', [museum]),
      'PATCH /trips/trip-1/pois/poi-1': (init) => {
        patchBody = JSON.parse(String(init?.body))
        return { status: 409, body: { code: 'conflict', message: 'ce lieu a été modifié entre-temps' } }
      },
    })
    renderApp('/trips/trip-1')
    fireEvent.click(await screen.findByRole('button', { name: /Museo Nacional/ }))
    const form = await screen.findByRole('form', { name: 'Museo Nacional' })
    fireEvent.change(within(form).getByLabelText('Durée de visite (minutes)'), { target: { value: '120' } })
    fireEvent.click(within(form).getByRole('button', { name: 'Enregistrer' }))
    expect(await screen.findByText(/modifié par quelqu’un d’autre/)).toBeInTheDocument()
    expect(patchBody).toMatchObject({ version: 1, duration_min: 120 })
  })

  it('signale des horaires non reconnus', async () => {
    mockApi({
      ...baseRoutes('editor', [{ ...museum, opening_hours: 'le matin', opening_hours_status: 'invalid', closed_dates: [] }]),
      'POST /trips/trip-1/opening-hours/preview': () => ({
        status: 200,
        body: { status: 'invalid', error: 'erreur de syntaxe vers le caractère 1', days: [] },
      }),
    })
    renderApp('/trips/trip-1')
    expect(await screen.findByText('Horaires à vérifier')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Museo Nacional/ }))
    expect(await screen.findByText(/Syntaxe non reconnue/)).toBeInTheDocument()
  })
})
