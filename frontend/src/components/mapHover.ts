import { createContext, useContext, useSyncExternalStore, type FocusEvent, type MouseEvent } from 'react'

/** Ce que le survol d'un élément du panneau met en évidence sur la carte. */
export type MapHover =
  /** Un lieu : son marqueur s'il est affiché (`id`), sinon un repère provisoire. */
  | { kind: 'point'; lat: number; lon: number; id?: string }
  /** Une région : un disque autour de son centre. */
  | { kind: 'area'; lat: number; lon: number; radiusM: number }

const keyOf = (h: MapHover | null) => (h ? `${h.kind}:${h.lat},${h.lon}:${'id' in h ? h.id : ''}` : '')

/**
 * Survol courant, hors de l'état React de la page : seule la carte se redessine quand la souris
 * passe d'un élément à l'autre.
 */
export function createHoverStore() {
  let current: MapHover | null = null
  const listeners = new Set<() => void>()
  const focusListeners = new Set<(target: MapHover) => void>()
  return {
    /** Cadre la carte sur `target` (clic sur une région, par exemple). */
    focus(target: MapHover) {
      focusListeners.forEach((l) => l(target))
    },
    onFocus(listener: (target: MapHover) => void) {
      focusListeners.add(listener)
      return () => {
        focusListeners.delete(listener)
      }
    },
    get: () => current,
    set(next: MapHover | null) {
      if (keyOf(next) === keyOf(current)) return
      current = next
      listeners.forEach((l) => l())
    },
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}

export type HoverStore = ReturnType<typeof createHoverStore>

const HoverContext = createContext<HoverStore>(createHoverStore())

export const MapHoverProvider = HoverContext.Provider

export const useHoverStore = () => useContext(HoverContext)

/** Survol courant (pour la carte). */
export function useMapHovered(): MapHover | null {
  const store = useContext(HoverContext)
  return useSyncExternalStore(store.subscribe, store.get, store.get)
}

/**
 * Gestionnaires à étaler sur un élément de liste : le survoler (ou lui donner le focus au
 * clavier) met `target` en évidence sur la carte. `parent` : élément englobant (région d'un point
 * fort), de nouveau mis en évidence quand on quitte celui-ci.
 */
export function useMapHover() {
  const store = useContext(HoverContext)
  return (target: MapHover | null, parent: MapHover | null = null) => {
    const leave = (e: MouseEvent | FocusEvent) => {
      // Le focus passe à un bouton de la même ligne : on garde la mise en évidence.
      if (e.relatedTarget instanceof Node && e.currentTarget.contains(e.relatedTarget)) return
      if (keyOf(store.get()) === keyOf(target)) store.set(parent)
    }
    return {
      onMouseEnter: () => store.set(target),
      onMouseLeave: leave,
      onFocus: (e: FocusEvent) => {
        // Le focus remonte aux éléments englobants : le plus précis l'emporte.
        e.stopPropagation()
        store.set(target)
      },
      onBlur: leave,
    }
  }
}

/** Repère d'un lieu (ou d'une suggestion) sur la carte. */
export const pointAt = (p: { lat: number; lon: number }, id?: string): MapHover => ({ kind: 'point', lat: p.lat, lon: p.lon, id })
