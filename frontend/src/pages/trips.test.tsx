import { fireEvent, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { invitationStatus, type Invitation } from '../api/trips'
import { demoUser, mockApi, renderApp, unauthenticated } from '../test/utils'

vi.mock('../components/MapView', () => ({ MapView: () => <div>carte</div> }))

const health = () => ({ status: 200, body: { status: 'ok', database: 'ok', version: '0.1.0' } })
const organizer = { ...demoUser, plan: 'organizer' }
const trip = {
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
  my_role: 'organizer',
  member_count: 1,
  created_at: '2026-10-15T00:00:00Z',
}

afterEach(() => vi.restoreAllMocks())

describe('voyages', () => {
  it("explique l'abonnement à un compte gratuit", async () => {
    mockApi({
      'GET /me': () => ({ status: 200, body: demoUser }),
      'GET /health': health,
      'GET /trips': () => ({ status: 200, body: [] }),
    })
    renderApp('/')
    expect(await screen.findByText(/réservée aux organisateurs/)).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Nouveau voyage' })).not.toBeInTheDocument()
  })

  it('crée un voyage puis ouvre sa page', async () => {
    const api = mockApi({
      'GET /me': () => ({ status: 200, body: organizer }),
      'GET /health': health,
      'GET /trips': () => ({ status: 200, body: [] }),
      'POST /trips': () => ({ status: 201, body: trip }),
      'GET /trips/trip-1': () => ({ status: 200, body: trip }),
      'GET /trips/trip-1/members': () => ({
        status: 200,
        body: [{ user_id: organizer.id, display_name: 'Ana', email: 'ana@example.com', role: 'organizer', joined_at: '' }],
      }),
      'GET /trips/trip-1/invitations': () => ({ status: 200, body: [] }),
    })
    renderApp('/')
    fireEvent.click(await screen.findByRole('button', { name: 'Nouveau voyage' }))
    fireEvent.change(screen.getByLabelText('Nom du voyage'), { target: { value: 'Pura Vida' } })
    fireEvent.change(screen.getByLabelText('Du'), { target: { value: '2026-12-01' } })
    fireEvent.change(screen.getByLabelText('Au'), { target: { value: '2026-12-08' } })
    fireEvent.click(screen.getByLabelText('À pied'))
    // Questionnaire d'envies.
    fireEvent.click(screen.getByLabelText('Plages'))
    fireEvent.click(screen.getByLabelText('Gastronomie'))
    fireEvent.click(screen.getByLabelText('Tranquille'))
    fireEvent.click(screen.getByLabelText(/Petit budget/))
    fireEvent.click(screen.getByRole('button', { name: 'Créer le voyage' }))

    expect(await screen.findByRole('heading', { name: 'Pura Vida' })).toBeInTheDocument()
    const post = api.mock.calls.find(([, init]) => init?.method === 'POST')
    expect(JSON.parse(String(post?.[1]?.body))).toMatchObject({
      name: 'Pura Vida',
      start_date: '2026-12-01',
      end_date: '2026-12-08',
      default_modes: ['car', 'walk'],
      interests: ['beach', 'food'],
      pace: 'relaxed',
      budget: 'budget',
    })
    fireEvent.click(screen.getByRole('tab', { name: 'Membres' }))
    expect(screen.getByRole('heading', { name: 'Invitations' })).toBeInTheDocument()
  })

  it("masque invitations et paramètres à un éditeur", async () => {
    const asEditor = { ...trip, my_role: 'editor' }
    mockApi({
      'GET /me': () => ({ status: 200, body: demoUser }),
      'GET /health': health,
      'GET /trips/trip-1': () => ({ status: 200, body: asEditor }),
      'GET /trips/trip-1/members': () => ({ status: 200, body: [] }),
    })
    renderApp('/trips/trip-1')
    expect(await screen.findByRole('heading', { name: 'Pura Vida' })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('tab', { name: 'Membres' }))
    expect(screen.getByRole('heading', { name: 'Membres' })).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: 'Invitations' })).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('tab', { name: 'Paramètres' }))
    expect(screen.queryByRole('heading', { name: 'Paramètres du voyage' })).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Quitter ce voyage' })).toBeInTheDocument()
  })
})

describe('invitation', () => {
  const preview = () => ({
    status: 200,
    body: { trip_name: 'Pura Vida', start_date: '2026-12-01', end_date: '2026-12-08', invited_by: 'Orga', role: 'editor' },
  })

  it('un invité rejoint avec prénom et email', async () => {
    let joined = false
    const api = mockApi({
      'GET /me': () => (joined ? { status: 200, body: demoUser } : unauthenticated()),
      'GET /health': health,
      'GET /invitations/tok': preview,
      'POST /invitations/tok/accept': () => {
        joined = true
        return { status: 200, body: { trip_id: 'trip-1', role: 'editor', user: demoUser } }
      },
      'GET /trips/trip-1': () => ({ status: 200, body: { ...trip, my_role: 'editor' } }),
      'GET /trips/trip-1/members': () => ({ status: 200, body: [] }),
    })
    renderApp('/invite/tok')
    expect(await screen.findByText('Orga vous invite à rejoindre')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText('Prénom ou pseudo'), { target: { value: 'Lucía' } })
    fireEvent.change(screen.getByLabelText('Adresse email'), { target: { value: 'lucia@example.com' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rejoindre ce voyage' }))
    expect(await screen.findByText(/Mon rôle/)).toBeInTheDocument()
    const accept = api.mock.calls.find(([, init]) => init?.method === 'POST')
    expect(JSON.parse(String(accept?.[1]?.body))).toEqual({ display_name: 'Lucía', email: 'lucia@example.com' })
  })

  it('propose un lien magique si le compte existe déjà', async () => {
    const api = mockApi({
      'GET /me': unauthenticated,
      'GET /health': health,
      'GET /invitations/tok': preview,
      'POST /invitations/tok/accept': () => ({ status: 409, body: { code: 'account_exists', message: 'compte existant' } }),
      'POST /auth/magic-link': () => ({ status: 202, body: { message: 'ok' } }),
    })
    renderApp('/invite/tok')
    fireEvent.change(await screen.findByLabelText('Prénom ou pseudo'), { target: { value: 'Ana' } })
    fireEvent.change(screen.getByLabelText('Adresse email'), { target: { value: 'ana@example.com' } })
    fireEvent.click(screen.getByRole('button', { name: 'Rejoindre ce voyage' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Recevoir un lien de connexion' }))
    expect(await screen.findByRole('status')).toHaveTextContent('Lien envoyé')
    const magic = api.mock.calls.filter(([, init]) => init?.method === 'POST').at(-1)
    expect(JSON.parse(String(magic?.[1]?.body))).toEqual({ email: 'ana@example.com', next: '/invite/tok' })
  })

  it('signale une invitation invalide', async () => {
    mockApi({
      'GET /me': unauthenticated,
      'GET /health': health,
      'GET /invitations/bad': () => ({ status: 400, body: { code: 'bad_request', message: 'invitation invalide' } }),
    })
    renderApp('/invite/bad')
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('invalide, révoquée ou expirée'))
  })
})

describe('invitationStatus', () => {
  const base: Invitation = {
    id: 'i', role: 'viewer', created_at: '', expires_at: '2030-01-01T00:00:00Z',
    max_uses: null, uses: 0, revoked_at: null,
  }
  it('calcule le statut', () => {
    const now = new Date('2026-10-15T00:00:00Z')
    expect(invitationStatus(base, now)).toBe('active')
    expect(invitationStatus({ ...base, revoked_at: '2026-01-01' }, now)).toBe('revoked')
    expect(invitationStatus({ ...base, expires_at: '2026-01-01T00:00:00Z' }, now)).toBe('expired')
    expect(invitationStatus({ ...base, max_uses: 2, uses: 2 }, now)).toBe('exhausted')
  })
})
