import { fireEvent, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { safeNext } from '../auth/safeNext'
import { demoUser, mockApi, renderApp, unauthenticated } from '../test/utils'

// La carte nécessite WebGL, indisponible dans jsdom.
vi.mock('../components/MapView', () => ({ MapView: () => <div>carte</div> }))

const health = () => ({ status: 200, body: { status: 'ok', database: 'ok', version: '0.1.0' } })

afterEach(() => vi.restoreAllMocks())

describe('authentification', () => {
  it('redirige vers /login quand aucune session', async () => {
    mockApi({ 'GET /me': unauthenticated, 'GET /health': health })
    renderApp('/account')
    expect(await screen.findByRole('heading', { name: 'Connexion' })).toBeInTheDocument()
  })

  it('connecte avec mot de passe puis revient à la page demandée', async () => {
    let loggedIn = false
    const api = mockApi({
      'GET /me': () => (loggedIn ? { status: 200, body: demoUser } : unauthenticated()),
      'GET /health': health,
      'POST /auth/login': () => {
        loggedIn = true
        return { status: 200, body: demoUser }
      },
    })
    renderApp('/login?next=%2Faccount')
    fireEvent.change(await screen.findByLabelText('Adresse email'), { target: { value: 'ana@example.com' } })
    fireEvent.change(screen.getByLabelText('Mot de passe'), { target: { value: 'motdepasse-solide' } })
    fireEvent.click(screen.getByRole('button', { name: 'Se connecter' }))

    expect(await screen.findByRole('heading', { name: 'Mon compte' })).toBeInTheDocument()
    const loginCall = api.mock.calls.find(([, init]) => init?.method === 'POST')
    expect(JSON.parse(String(loginCall?.[1]?.body))).toEqual({
      email: 'ana@example.com',
      password: 'motdepasse-solide',
    })
    expect(loginCall?.[1]?.credentials).toBe('include')
  })

  it('affiche une erreur générique si les identifiants sont faux', async () => {
    mockApi({
      'GET /me': unauthenticated,
      'GET /health': health,
      'POST /auth/login': unauthenticated,
    })
    renderApp('/login')
    fireEvent.change(await screen.findByLabelText('Adresse email'), { target: { value: 'ana@example.com' } })
    fireEvent.change(screen.getByLabelText('Mot de passe'), { target: { value: 'mauvais' } })
    fireEvent.click(screen.getByRole('button', { name: 'Se connecter' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('Email ou mot de passe incorrect.')
  })

  it('demande un lien magique', async () => {
    mockApi({
      'GET /me': unauthenticated,
      'GET /health': health,
      'POST /auth/magic-link': () => ({ status: 202, body: { message: 'ok' } }),
    })
    renderApp('/login')
    fireEvent.click(await screen.findByRole('tab', { name: 'Par lien magique' }))
    fireEvent.change(screen.getByLabelText('Adresse email'), { target: { value: 'ana@example.com' } })
    fireEvent.click(screen.getByRole('button', { name: 'Recevoir un lien de connexion' }))
    expect(await screen.findByRole('status')).toHaveTextContent('lien de connexion')
  })

  it('échange le jeton du lien magique contre une session', async () => {
    let loggedIn = false
    const api = mockApi({
      'GET /me': () => (loggedIn ? { status: 200, body: demoUser } : unauthenticated()),
      'GET /health': health,
      'GET /trips': () => ({ status: 200, body: [] }),
      'POST /auth/magic-link/verify': () => {
        loggedIn = true
        return { status: 200, body: demoUser }
      },
    })
    renderApp('/auth/magic?token=abc123')
    expect(await screen.findByRole('heading', { name: 'Mes voyages' })).toBeInTheDocument()
    const verify = api.mock.calls.filter(([, init]) => init?.method === 'POST')
    expect(verify).toHaveLength(1)
    expect(JSON.parse(String(verify[0][1]?.body))).toEqual({ token: 'abc123' })
  })

  it('signale un lien magique invalide', async () => {
    mockApi({
      'GET /me': unauthenticated,
      'GET /health': health,
      'POST /auth/magic-link/verify': () => ({ status: 400, body: { code: 'bad_request', message: 'lien invalide ou expiré' } }),
    })
    renderApp('/auth/magic?token=perime')
    expect(await screen.findByRole('alert')).toHaveTextContent('invalide ou a expiré')
  })

  it("n'active la suppression du compte qu'avec le bon email", async () => {
    mockApi({ 'GET /me': () => ({ status: 200, body: demoUser }), 'GET /health': health })
    renderApp('/account')
    const button = await screen.findByRole('button', { name: 'Supprimer définitivement' })
    expect(button).toBeDisabled()
    fireEvent.change(screen.getByLabelText('Tapez votre email pour confirmer'), {
      target: { value: 'ana@example.com' },
    })
    await waitFor(() => expect(button).toBeEnabled())
  })
})

describe('safeNext', () => {
  it('refuse les redirections externes', () => {
    expect(safeNext('/account')).toBe('/account')
    expect(safeNext('//evil.example')).toBe('/')
    expect(safeNext('https://evil.example')).toBe('/')
    expect(safeNext(null)).toBe('/')
  })
})
