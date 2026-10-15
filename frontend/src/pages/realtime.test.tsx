import { act, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { CLIENT_ID } from '../api/client'
import { FakeWebSocket } from '../test/fakeWebSocket'
import { demoUser, mockApi, renderApp } from '../test/utils'

vi.mock('../components/MapView', () => ({ MapView: () => <div>carte</div> }))

const trip = {
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
  my_role: 'editor',
  member_count: 2,
  created_at: '2026-10-15T00:00:00Z',
}
const ana = { user_id: demoUser.id, name: 'Ana' }
const bob = { user_id: '00000000-0000-0000-0000-000000000002', name: 'Bob' }

function setup(access = { trip: 200 }) {
  const calls = { trip: 0, pois: 0 }
  mockApi({
    'GET /me': () => ({ status: 200, body: demoUser }),
    'GET /health': () => ({ status: 200, body: { status: 'ok', database: 'ok', version: '0.1.0' } }),
    'GET /trips/trip-1': () => {
      calls.trip += 1
      // Premier chargement toujours accepté ; ensuite, selon le scénario.
      if (calls.trip > 1 && access.trip !== 200) {
        return { status: access.trip, body: { code: 'forbidden', message: 'accès refusé' } }
      }
      return { status: 200, body: trip }
    },
    'GET /trips/trip-1/pois': () => {
      calls.pois += 1
      return { status: 200, body: [] }
    },
  })
  return calls
}

async function connected() {
  renderApp('/trips/trip-1')
  await screen.findByRole('heading', { name: 'Pura Vida' })
  const socket = FakeWebSocket.last!
  act(() => socket.open())
  return socket
}

afterEach(() => vi.restoreAllMocks())

describe('collaboration en temps réel', () => {
  it('se connecte au canal du voyage et affiche qui est en ligne', async () => {
    setup()
    const socket = await connected()
    expect(socket.url).toMatch(/^ws:\/\/[^/]+\/api\/v1\/trips\/trip-1\/ws$/)
    expect(screen.getByText('En direct')).toBeInTheDocument()
    expect(screen.getByText(/Personne d’autre en ligne/)).toBeInTheDocument()

    act(() => socket.receive({ type: 'presence', online: [ana, bob] }))
    expect(screen.getByText(/Aussi en ligne : Bob/)).toBeInTheDocument()
  })

  it('recharge ce qu’un autre membre a modifié, mais pas ses propres changements', async () => {
    const calls = setup()
    const socket = await connected()
    await waitFor(() => expect(calls.pois).toBe(1))

    act(() => socket.receive({ type: 'changed', topics: ['pois', 'itinerary'], by: bob, client_id: 'autre-onglet' }))
    await waitFor(() => expect(calls.pois).toBe(2))
    expect(screen.getByText(/Bob a modifié les lieux \(\d\d:\d\d\)/)).toBeInTheDocument()

    // Notification de cet onglet-ci : déjà à jour, rien à recharger.
    act(() => socket.receive({ type: 'changed', topics: ['pois'], by: ana, client_id: CLIENT_ID }))
    await new Promise((r) => setTimeout(r, 50))
    expect(calls.pois).toBe(2)
  })

  it('se reconnecte après une coupure et rattrape ce qui a été manqué', async () => {
    const calls = setup()
    const socket = await connected()
    await waitFor(() => expect(calls.trip).toBe(1))

    act(() => socket.drop())
    expect(screen.getByText('Hors ligne · reconnexion…')).toBeInTheDocument()
    await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2), { timeout: 2000 })
    act(() => FakeWebSocket.last!.open())
    expect(screen.getByText('En direct')).toBeInTheDocument()
    await waitFor(() => expect(calls.trip).toBe(2))
  })

  it('vérifie l’accès quand la reconnexion est refusée, puis se reconnecte ou s’arrête', async () => {
    const access = { trip: 200 }
    const calls = setup(access)
    const socket = await connected()
    await waitFor(() => expect(calls.trip).toBe(1))

    // Coupure, puis reconnexion refusée alors que l’accès est intact : nouvel essai.
    act(() => socket.drop())
    await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(2), { timeout: 2000 })
    act(() => FakeWebSocket.last!.drop())
    await waitFor(() => expect(calls.trip).toBe(2))
    await waitFor(() => expect(FakeWebSocket.instances).toHaveLength(3), { timeout: 3000 })

    // Membre retiré entre-temps : la poignée de main échoue, le voyage répond 403.
    access.trip = 403
    act(() => FakeWebSocket.last!.drop())
    expect(await screen.findByRole('alert')).toHaveTextContent('Vous n’avez plus accès à ce voyage')
    await new Promise((r) => setTimeout(r, 1200))
    expect(FakeWebSocket.instances).toHaveLength(3)
  })

  it('prévient quand l’accès au voyage est perdu', async () => {
    setup()
    const socket = await connected()
    act(() => socket.receive({ type: 'removed' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Vous n’avez plus accès à ce voyage')
    // Pas de reconnexion.
    await new Promise((r) => setTimeout(r, 700))
    expect(FakeWebSocket.instances).toHaveLength(1)
  })
})
