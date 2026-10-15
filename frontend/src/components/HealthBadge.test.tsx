import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { HealthBadge } from './HealthBadge'

function renderWithClient() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return render(
    <QueryClientProvider client={client}>
      <HealthBadge />
    </QueryClientProvider>,
  )
}

afterEach(() => vi.restoreAllMocks())

describe('HealthBadge', () => {
  it('affiche la version quand le serveur répond', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ status: 'ok', database: 'ok', version: '0.1.0' }), {
        status: 200,
      }),
    )
    renderWithClient()
    expect(await screen.findByText(/Serveur opérationnel · v0\.1\.0/)).toBeInTheDocument()
  })

  it('signale un serveur injoignable', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 'internal', message: 'boom' }), { status: 500 }),
    )
    renderWithClient()
    expect(await screen.findByText('Serveur injoignable')).toBeInTheDocument()
  })
})
