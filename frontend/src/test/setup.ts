import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

// Sans `globals: true`, Testing Library ne démonte pas automatiquement entre deux tests.
afterEach(() => cleanup())
