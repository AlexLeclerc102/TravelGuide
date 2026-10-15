import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach, vi } from 'vitest'
import { FakeWebSocket } from './fakeWebSocket'

// Sans `globals: true`, Testing Library ne démonte pas automatiquement entre deux tests.
afterEach(() => cleanup())

// Aucune vraie connexion temps réel dans les tests : les pages ouvrent un WebSocket factice.
vi.stubGlobal('WebSocket', FakeWebSocket)
afterEach(() => {
  FakeWebSocket.instances = []
})
