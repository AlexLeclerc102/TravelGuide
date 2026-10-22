import { QueryClient } from '@tanstack/react-query'
import { fireEvent, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { contentKeys, type PoiLink, type PoiNote } from '../api/poiContent'
import type { Poi } from '../api/pois'
import { suggestionKeys } from '../api/suggestions'
import { invalidateTopics } from '../realtime/useTripRealtime'
import { demoUser, mockApi, renderApp } from '../test/utils'

vi.mock('../components/MapView', () => ({ MapView: () => <div>carte</div> }))

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

const arenal: Poi = {
  id: 'p1',
  trip_id: 'trip-1',
  name: 'Volcán Arenal',
  category: 'nature',
  placeholder: false,
  lat: 10.46,
  lon: -84.7,
  address: '',
  duration_min: 150,
  opening_hours: null,
  opening_hours_status: 'none',
  closed_dates: [],
  country_code: 'CR',
  website: null,
  phone: null,
  email: null,
  wikidata: null,
  wikipedia: null,
  osm: null,
  rating: { count: 0, average: null, min: null, max: null, disagreement: false },
  my_review: null,
  version: 1,
  created_by: null,
  created_by_name: null,
  created_at: '2026-10-15T00:00:00Z',
  updated_at: '2026-10-15T00:00:00Z',
}

const note = (id: string, body: string, extra: Partial<PoiNote> = {}): PoiNote => ({
  id,
  poi_id: 'p1',
  body,
  source_url: null,
  author_id: 'u2',
  author_name: 'Lucía',
  via_assistant: false,
  created_at: '2026-10-20T10:00:00Z',
  updated_at: '2026-10-20T10:00:00Z',
  ...extra,
})

const link = (id: string, title: string, extra: Partial<PoiLink> = {}): PoiLink => ({
  id,
  poi_id: 'p1',
  url: 'https://www.arenal.net/billets',
  title,
  kind: 'booking',
  author_id: 'u2',
  author_name: 'Lucía',
  via_assistant: false,
  created_at: '2026-10-20T10:00:00Z',
  ...extra,
})

type Routes = Parameters<typeof mockApi>[0]

function routes(role: string, extra: Routes = {}): Routes {
  return {
    'GET /me': () => ({ status: 200, body: demoUser }),
    'GET /health': () => ({ status: 200, body: { status: 'ok', database: 'ok', version: '0.1.0' } }),
    'GET /trips/trip-1': () => ({ status: 200, body: trip(role) }),
    'GET /trips/trip-1/pois': () => ({ status: 200, body: [arenal] }),
    'GET /trips/trip-1/pois/p1/reviews': () => ({ status: 200, body: [] }),
    ...extra,
  }
}

async function openArenal() {
  renderApp('/trips/trip-1')
  fireEvent.click(await screen.findByRole('button', { name: /Volcán Arenal/ }))
}

afterEach(() => vi.restoreAllMocks())

describe('notes des lieux', () => {
  it('affiche auteur, source et provenance « assistant »', async () => {
    mockApi(
      routes('viewer', {
        'GET /trips/trip-1/pois/p1/notes': () => ({
          status: 200,
          body: [
            note('n1', 'Prévoir des chaussures de marche'),
            note('n2', 'Ouvert de 8 h à 16 h', {
              via_assistant: true,
              source_url: 'https://www.arenal.net/horaires',
            }),
          ],
        }),
        'GET /trips/trip-1/pois/p1/links': () => ({ status: 200, body: [] }),
      }),
    )
    await openArenal()
    const section = await screen.findByRole('region', { name: 'Notes' })
    expect(await within(section).findByText('Prévoir des chaussures de marche')).toBeInTheDocument()
    const sourced = within(section).getByText('Ouvert de 8 h à 16 h').closest('li')!
    expect(within(sourced).getByText('assistant')).toBeInTheDocument()
    const source = within(sourced).getByRole('link', { name: 'Source : arenal.net' })
    expect(source).toHaveAttribute('href', 'https://www.arenal.net/horaires')
    expect(source).toHaveAttribute('rel', 'noopener noreferrer nofollow')
    // Lecteur : ni ajout ni modification, et pas de section de liens vide.
    expect(within(section).queryByRole('form')).not.toBeInTheDocument()
    expect(within(section).queryByRole('button', { name: 'Modifier' })).not.toBeInTheDocument()
    expect(screen.queryByRole('region', { name: 'Liens utiles' })).not.toBeInTheDocument()
  })

  it('un éditeur ajoute, modifie et supprime une note', async () => {
    let notes = [note('n1', 'Parking payant')]
    const sent: unknown[] = []
    mockApi(
      routes('editor', {
        'GET /trips/trip-1/pois/p1/notes': () => ({ status: 200, body: notes }),
        'GET /trips/trip-1/pois/p1/links': () => ({ status: 200, body: [] }),
        'POST /trips/trip-1/pois/p1/notes': (init) => {
          const input = JSON.parse(String(init?.body))
          sent.push(input)
          const created = note('n2', input.body, { author_name: 'Ana', source_url: input.source_url || null })
          notes = [...notes, created]
          return { status: 201, body: created }
        },
        'PUT /trips/trip-1/pois/p1/notes/n1': (init) => {
          const input = JSON.parse(String(init?.body))
          sent.push(input)
          return { status: 200, body: note('n1', input.body) }
        },
        'DELETE /trips/trip-1/pois/p1/notes/n1': () => ({ status: 204, body: undefined }),
      }),
    )
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true)
    await openArenal()
    const section = await screen.findByRole('region', { name: 'Notes' })
    await within(section).findByText('Parking payant')

    const add = within(section).getByRole('form', { name: 'Ajouter une note' })
    fireEvent.change(within(add).getByLabelText('Ajouter une note'), { target: { value: 'Pluie l’après-midi' } })
    fireEvent.change(within(add).getByLabelText('Source (facultatif)'), { target: { value: 'https://meteo.cr' } })
    fireEvent.click(within(add).getByRole('button', { name: 'Ajouter la note' }))
    // Texte d'une note affichée (une zone de saisie contient aussi le texte tapé).
    expect(await within(section).findByText('Pluie l’après-midi', { selector: 'p' })).toBeInTheDocument()
    expect(sent[0]).toEqual({ body: 'Pluie l’après-midi', source_url: 'https://meteo.cr' })
    // Le formulaire est vidé après l'ajout.
    await waitFor(() =>
      expect(within(within(section).getByRole('form', { name: 'Ajouter une note' })).getByLabelText('Ajouter une note')).toHaveValue(''),
    )

    const first = within(section).getByText('Parking payant').closest('li')!
    fireEvent.click(within(first).getByRole('button', { name: 'Modifier' }))
    const edit = within(section).getByRole('form', { name: 'Modifier la note' })
    fireEvent.change(within(edit).getByLabelText('Modifier la note'), { target: { value: 'Parking gratuit' } })
    fireEvent.click(within(edit).getByRole('button', { name: 'Enregistrer' }))
    expect(await within(section).findByText('Parking gratuit', { selector: 'p' })).toBeInTheDocument()
    expect(sent[1]).toEqual({ body: 'Parking gratuit', source_url: '' })

    const edited = within(section).getByText('Parking gratuit', { selector: 'p' }).closest('li')!
    fireEvent.click(within(edited).getByRole('button', { name: 'Supprimer' }))
    expect(confirm).toHaveBeenCalledWith('Supprimer cette note ?')
    await waitFor(() => expect(within(section).queryByText('Parking gratuit', { selector: 'p' })).not.toBeInTheDocument())
  })

  it('garde le texte saisi si l’ajout échoue', async () => {
    mockApi(
      routes('editor', {
        'GET /trips/trip-1/pois/p1/notes': () => ({ status: 200, body: [] }),
        'GET /trips/trip-1/pois/p1/links': () => ({ status: 200, body: [] }),
        'POST /trips/trip-1/pois/p1/notes': () => ({
          status: 400,
          body: { code: 'bad_request', message: 'la source doit être un lien http(s) valide' },
        }),
      }),
    )
    await openArenal()
    const add = await screen.findByRole('form', { name: 'Ajouter une note' })
    fireEvent.change(within(add).getByLabelText('Ajouter une note'), { target: { value: 'Ma note' } })
    fireEvent.click(within(add).getByRole('button', { name: 'Ajouter la note' }))
    expect(await within(add).findByRole('alert')).toHaveTextContent('La source doit être un lien http(s) valide.')
    expect(within(add).getByLabelText('Ajouter une note')).toHaveValue('Ma note')
  })
})

describe('liens des lieux', () => {
  it('un éditeur ajoute et retire un lien', async () => {
    let body: Record<string, unknown> = {}
    mockApi(
      routes('editor', {
        'GET /trips/trip-1/pois/p1/notes': () => ({ status: 200, body: [] }),
        'GET /trips/trip-1/pois/p1/links': () => ({ status: 200, body: [] }),
        'POST /trips/trip-1/pois/p1/links': (init) => {
          body = JSON.parse(String(init?.body))
          return { status: 201, body: link('l1', 'Billets officiels', { author_name: 'Ana' }) }
        },
        'DELETE /trips/trip-1/pois/p1/links/l1': () => ({ status: 204, body: undefined }),
      }),
    )
    await openArenal()
    const section = await screen.findByRole('region', { name: 'Liens utiles' })
    const form = within(section).getByRole('form', { name: 'Ajouter un lien' })
    fireEvent.change(within(form).getByLabelText('Adresse du lien (https)'), { target: { value: ' https://www.arenal.net/billets ' } })
    fireEvent.change(within(form).getByLabelText('Titre (facultatif)'), { target: { value: 'Billets officiels' } })
    fireEvent.change(within(form).getByLabelText('Type'), { target: { value: 'booking' } })
    fireEvent.click(within(form).getByRole('button', { name: 'Ajouter le lien' }))

    const added = await within(section).findByRole('link', { name: 'Billets officiels' })
    expect(added).toHaveAttribute('href', 'https://www.arenal.net/billets')
    expect(added).toHaveAttribute('target', '_blank')
    expect(within(section).getByText('(arenal.net)')).toBeInTheDocument()
    expect(within(within(section).getByRole('list')).getByText('Réservation')).toBeInTheDocument()
    expect(body).toEqual({ url: 'https://www.arenal.net/billets', title: 'Billets officiels', kind: 'booking' })

    fireEvent.click(within(section).getByRole('button', { name: 'Supprimer le lien Billets officiels' }))
    await waitFor(() => expect(within(section).queryByRole('link', { name: 'Billets officiels' })).not.toBeInTheDocument())
  })
})

describe('temps réel', () => {
  it('poi_content recharge notes et liens, sans recalculer les suggestions', () => {
    const qc = new QueryClient()
    const spy = vi.spyOn(qc, 'invalidateQueries')
    invalidateTopics(qc, 'trip-1', ['poi_content'])
    const keys = spy.mock.calls.map(([filters]) => filters?.queryKey)
    expect(keys).toContainEqual(contentKeys.all('trip-1'))
    expect(keys).not.toContainEqual(suggestionKeys.all('trip-1'))
  })
})
