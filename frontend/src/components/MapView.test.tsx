import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { MapView } from './MapView'

describe('carte', () => {
  it('affiche un message au lieu de planter quand WebGL est indisponible', () => {
    // jsdom n'a pas WebGL : MapLibre lève une exception à l'initialisation.
    vi.spyOn(console, 'warn').mockImplementation(() => undefined)
    render(<MapView markers={[]} />)
    expect(screen.getByRole('status')).toHaveTextContent(/WebGL/)
  })
})
