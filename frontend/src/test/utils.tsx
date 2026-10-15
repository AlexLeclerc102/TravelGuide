import { render } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router-dom'
import { vi } from 'vitest'
import { AppRoutes } from '../App'

type Handler = (init: RequestInit | undefined) => { status: number; body?: unknown }

/** Simule l'API : `routes` associe « MÉTHODE /chemin » à une réponse. */
export function mockApi(routes: Record<string, Handler>) {
  return vi.spyOn(globalThis, 'fetch').mockImplementation(async (input, init) => {
    const url = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url
    const path = url.replace(/^.*\/api\/v1/, '').split('?')[0]
    const key = `${(init?.method ?? 'GET').toUpperCase()} ${path}`
    const handler = routes[key]
    if (!handler) return new Response(JSON.stringify({ code: 'not_found', message: key }), { status: 404 })
    const { status, body } = handler(init)
    return new Response(body === undefined ? null : JSON.stringify(body), { status })
  })
}

export function renderApp(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={[path]}>
        <AppRoutes />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

export const demoUser = {
  id: '00000000-0000-0000-0000-000000000001',
  email: 'ana@example.com',
  display_name: 'Ana',
  plan: 'free',
  has_password: true,
  created_at: '2026-10-15T00:00:00Z',
}

export const unauthenticated = () => ({ status: 401, body: { code: 'unauthorized', message: 'authentification requise' } })
