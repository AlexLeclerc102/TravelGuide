import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiFetch } from './client'
import { tripKeys } from './trips'

export type Category = 'sight' | 'nature' | 'museum' | 'food' | 'activity' | 'lodging' | 'shopping' | 'other'
export const ALL_CATEGORIES: Category[] = ['sight', 'nature', 'museum', 'food', 'activity', 'lodging', 'shopping', 'other']

/** Durées types (minutes), identiques à celles du backend. */
export const DEFAULT_DURATION: Record<Category, number> = {
  sight: 60,
  nature: 150,
  museum: 90,
  food: 75,
  activity: 120,
  lodging: 0,
  shopping: 45,
  other: 60,
}

export interface OsmRef {
  type: 'node' | 'way' | 'relation'
  id: number
}

/** Lieu trouvé dans OpenStreetMap. */
export interface Place {
  name: string
  display_name: string
  lat: number
  lon: number
  category: Category
  osm_tag: string
  osm: OsmRef | null
  opening_hours: string | null
  website: string | null
  country_code: string | null
}

export type HoursStatus = 'none' | 'valid' | 'invalid'

/** Synthèse des notes du groupe (désaccord : écart d'au moins 2 points). */
export interface GroupRating {
  count: number
  average: number | null
  min: number | null
  max: number | null
  disagreement: boolean
}

export interface MyReview {
  rating: number
  comment: string
}

export interface Review {
  user_id: string
  display_name: string
  rating: number
  comment: string
  updated_at: string
}

export interface Poi {
  id: string
  trip_id: string
  name: string
  category: Category
  lat: number
  lon: number
  address: string
  duration_min: number
  opening_hours: string | null
  opening_hours_status: HoursStatus
  closed_dates: string[]
  country_code: string | null
  website: string | null
  notes: string
  osm: OsmRef | null
  rating: GroupRating
  my_review: MyReview | null
  version: number
  created_by: string | null
  created_by_name: string | null
  created_at: string
  updated_at: string
}

export interface PoiInput {
  name: string
  category: Category
  lat: number
  lon: number
  address?: string
  duration_min?: number
  opening_hours?: string
  country_code?: string | null
  website?: string
  notes?: string
  osm?: OsmRef | null
}

export interface OpeningInterval {
  start: string
  end: string
  unknown: boolean
  comment?: string
}

export interface DaySchedule {
  date: string
  intervals: OpeningInterval[]
  holiday: boolean
}

export interface OpeningPreview {
  status: HoursStatus
  error: string | null
  days: DaySchedule[]
}

/** Zone affichée : `[minLon, minLat, maxLon, maxLat]`. */
export type Bounds = [number, number, number, number]

const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body) })

export const poiKeys = {
  list: (tripId: string) => [...tripKeys.one(tripId), 'pois'] as const,
  reviews: (tripId: string, poiId: string) => [...tripKeys.one(tripId), 'pois', poiId, 'reviews'] as const,
  opening: (tripId: string, value: string, country: string | null) =>
    [...tripKeys.one(tripId), 'opening', value, country] as const,
}

/** Prépare l'ajout d'un lieu OSM avec les valeurs par défaut de sa catégorie. */
export function placeToInput(place: Place, name = place.name): PoiInput {
  return {
    name,
    category: place.category,
    lat: place.lat,
    lon: place.lon,
    address: place.display_name,
    duration_min: DEFAULT_DURATION[place.category],
    opening_hours: place.opening_hours ?? undefined,
    country_code: place.country_code,
    website: place.website ?? undefined,
    osm: place.osm,
  }
}

export const sameOsm = (a: OsmRef | null, b: OsmRef | null) => !!a && !!b && a.type === b.type && a.id === b.id

export const usePois = (tripId: string) =>
  useQuery({ queryKey: poiKeys.list(tripId), queryFn: () => apiFetch<Poi[]>(`/trips/${tripId}/pois`) })

export function useCreatePoi(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: PoiInput) => apiFetch<Poi>(`/trips/${tripId}/pois`, json('POST', input)),
    onSuccess: (poi) => {
      qc.setQueryData<Poi[]>(poiKeys.list(tripId), (list) => [...(list ?? []), poi])
      // L'itinéraire signale les lieux ajoutés depuis sa génération.
      qc.invalidateQueries({ queryKey: tripKeys.itinerary(tripId) })
    },
  })
}

export function useUpdatePoi(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...patch }: Partial<PoiInput> & { id: string; version: number }) =>
      apiFetch<Poi>(`/trips/${tripId}/pois/${id}`, json('PATCH', patch)),
    onSuccess: (poi) => {
      qc.setQueryData<Poi[]>(poiKeys.list(tripId), (list) => list?.map((p) => (p.id === poi.id ? poi : p)))
      // Le lieu a pu être déplacé : les trajets sont à recalculer.
      qc.invalidateQueries({ queryKey: tripKeys.routes(tripId) })
      qc.invalidateQueries({ queryKey: tripKeys.itinerary(tripId) })
    },
    // Conflit de version : on recharge la liste à jour.
    onError: () => qc.invalidateQueries({ queryKey: poiKeys.list(tripId) }),
  })
}

export function useDeletePoi(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiFetch<void>(`/trips/${tripId}/pois/${id}`, { method: 'DELETE' }),
    onSuccess: (_, id) => {
      qc.setQueryData<Poi[]>(poiKeys.list(tripId), (list) => list?.filter((p) => p.id !== id))
      // Un hébergement supprimé disparaît des jours où il servait de départ.
      qc.invalidateQueries({ queryKey: tripKeys.days(tripId) })
      qc.invalidateQueries({ queryKey: tripKeys.itinerary(tripId) })
    },
  })
}

/** Recherche déclenchée explicitement (pas d'autocomplétion : politique d'usage de Nominatim). */
export const usePlaceSearch = (query: string, bounds: Bounds | null) =>
  useQuery({
    queryKey: ['places', query, bounds],
    queryFn: () => {
      const params = new URLSearchParams({ q: query })
      if (bounds) params.set('viewbox', bounds.map((n) => n.toFixed(4)).join(','))
      return apiFetch<Place[]>(`/places/search?${params}`)
    },
    enabled: query.length >= 2,
    staleTime: Infinity,
    retry: false,
  })

export const reverseGeocode = (lat: number, lon: number) =>
  apiFetch<{ place: Place | null }>(`/places/reverse?lat=${lat.toFixed(6)}&lon=${lon.toFixed(6)}`)

export const useOpeningPreview = (tripId: string, value: string, country: string | null) =>
  useQuery({
    queryKey: poiKeys.opening(tripId, value, country),
    queryFn: () =>
      apiFetch<OpeningPreview>(
        `/trips/${tripId}/opening-hours/preview`,
        json('POST', { opening_hours: value, country_code: country }),
      ),
    enabled: value.trim().length > 0,
    staleTime: Infinity,
  })

export const useReviews = (tripId: string, poiId: string) =>
  useQuery({
    queryKey: poiKeys.reviews(tripId, poiId),
    queryFn: () => apiFetch<Review[]>(`/trips/${tripId}/pois/${poiId}/reviews`),
  })

/** Donne, modifie (`review`) ou retire (`null`) mon avis ; le serveur renvoie le POI recalculé. */
export function useSaveMyReview(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (review: MyReview | null) =>
      apiFetch<Poi>(
        `/trips/${tripId}/pois/${poiId}/reviews/me`,
        review ? json('PUT', review) : { method: 'DELETE' },
      ),
    onSuccess: (poi) => {
      qc.setQueryData<Poi[]>(poiKeys.list(tripId), (list) => list?.map((p) => (p.id === poi.id ? poi : p)))
      qc.invalidateQueries({ queryKey: poiKeys.reviews(tripId, poiId) })
    },
  })
}

export type PoiSort = 'added' | 'rating' | 'disagreement'

/** Tri de la liste : ordre d'ajout, meilleure note du groupe, ou désaccords d'abord. */
export function sortPois(pois: Poi[], sort: PoiSort): Poi[] {
  if (sort === 'added') return pois
  const avg = (p: Poi) => p.rating.average ?? -1
  const spread = (p: Poi) => (p.rating.max ?? 0) - (p.rating.min ?? 0)
  return [...pois].sort((a, b) =>
    sort === 'rating'
      ? avg(b) - avg(a) || b.rating.count - a.rating.count
      : Number(b.rating.disagreement) - Number(a.rating.disagreement) || spread(b) - spread(a) || avg(b) - avg(a),
  )
}
