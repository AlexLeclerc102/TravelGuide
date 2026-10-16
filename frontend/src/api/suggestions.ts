import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiFetch } from './client'
import type { Category, OsmRef, Place } from './pois'
import { tripKeys } from './trips'

/** Pourquoi un lieu est proposé (voir `planner::recommend`). */
export type SuggestionReason =
  | { kind: 'near'; date: string | null; minutes: number }
  | { kind: 'notable'; languages: number }
  | { kind: 'taste'; category: Category; average: number }
  | { kind: 'free_time'; date: string; minutes: number }

export interface Suggestion {
  place: Place
  score: number
  /** Jour le plus proche. */
  date: string | null
  /** Trajet estimé depuis le point de départ de ce jour. */
  travel_min: number
  reasons: SuggestionReason[]
}

/** `discovery` : voyage encore vide, les lieux les plus notables de tout le pays. */
export type SuggestionStatus = 'ready' | 'not_imported' | 'discovery'

export interface Suggestions {
  status: SuggestionStatus
  suggestions: Suggestion[]
}

/** Lieux à visiter (d'une catégorie), ou hébergements autour d'un lieu du voyage. */
export type SuggestionFilter = { category: Category | null } | { lodgingNear: string }

/** Région du pays à découvrir (voir `planner::regions`). */
export interface Region {
  /** Localité où loger. */
  name: string
  lat: number
  lon: number
  osm: OsmRef | null
  themes: Category[]
  /** Nuits sur place pour les points forts (sans le trajet pour y aller). */
  nights: number
  /** `boat_or_plane` : aucune route depuis les autres régions (île…). */
  access: 'road' | 'boat_or_plane' | 'unknown'
  place_count: number
  highlights: Place[]
  /** Lieux du voyage déjà dans la région. */
  in_trip: number
}

export interface Regions {
  status: 'ready' | 'not_imported'
  regions: Region[]
}

export const suggestionKeys = {
  all: (tripId: string) => [...tripKeys.one(tripId), 'suggestions'] as const,
  list: (tripId: string, filter: SuggestionFilter) => [...suggestionKeys.all(tripId), filter] as const,
  regions: (tripId: string) => [...suggestionKeys.all(tripId), 'regions'] as const,
}

export const useRegions = (tripId: string, enabled = true) =>
  useQuery({
    queryKey: suggestionKeys.regions(tripId),
    queryFn: () => apiFetch<Regions>(`/trips/${tripId}/regions`),
    enabled,
    staleTime: 5 * 60_000,
  })

/** Identifiant stable d'une suggestion (objet OSM). */
export const suggestionId = (s: Suggestion) => (s.place.osm ? `s:${s.place.osm.type}/${s.place.osm.id}` : `s:${s.place.name}`)

export const useSuggestions = (tripId: string, filter: SuggestionFilter, enabled = true) =>
  useQuery({
    queryKey: suggestionKeys.list(tripId, filter),
    queryFn: () => {
      const params = new URLSearchParams()
      if ('lodgingNear' in filter) params.set('lodging_near', filter.lodgingNear)
      else if (filter.category) params.set('category', filter.category)
      const query = params.size ? `?${params}` : ''
      return apiFetch<Suggestions>(`/trips/${tripId}/suggestions${query}`)
    },
    enabled,
    staleTime: 60_000,
  })

export function useDismissSuggestion(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { osm: OsmRef; category: Category }) =>
      apiFetch<void>(`/trips/${tripId}/suggestions/dismissals`, { method: 'POST', body: JSON.stringify(input) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: suggestionKeys.all(tripId) }),
  })
}

export function useResetDismissals(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => apiFetch<void>(`/trips/${tripId}/suggestions/dismissals`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: suggestionKeys.all(tripId) }),
  })
}
