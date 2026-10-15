import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiFetch } from './client'
import { tripKeys, type TransportMode } from './trips'

/** Un jour du voyage avec ses réglages effectifs. */
export interface TripDay {
  date: string
  /** Modes du jour, sinon ceux du voyage. */
  modes: TransportMode[]
  /** `true` : le jour reprend les modes par défaut du voyage. */
  modes_inherited: boolean
  /** Hébergement : départ et arrivée de la journée. */
  base_poi_id: string | null
}

export interface TripDayInput {
  /** `null` : reprendre les modes du voyage. */
  modes: TransportMode[] | null
  base_poi_id: string | null
}

/** `router` : calculé sur le réseau OSM ; les deux autres sont des estimations. */
export type TravelSource = 'router' | 'straight_line' | 'transit_estimate'

export interface TravelOption {
  mode: TransportMode
  duration_s: number
  distance_m: number
  source: TravelSource
}

export interface RouteLeg {
  from_poi_id: string
  to_poi_id: string
  chosen: TravelOption
  options: TravelOption[]
  /** Tracé `[lon, lat]` de l'option retenue. */
  geometry: [number, number][]
}

export interface RoutePlan {
  modes: TransportMode[]
  legs: RouteLeg[]
  total_duration_s: number
  has_estimates: boolean
}

export interface RouteRequest {
  poi_ids: string[]
  modes?: TransportMode[]
  date?: string
}

const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body) })

export const transportKeys = {
  days: tripKeys.days,
  route: (tripId: string, req: RouteRequest) => [...tripKeys.routes(tripId), req] as const,
}

export const useTripDays = (tripId: string) =>
  useQuery({ queryKey: transportKeys.days(tripId), queryFn: () => apiFetch<TripDay[]>(`/trips/${tripId}/days`) })

export function useUpdateTripDay(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ date, ...input }: TripDayInput & { date: string }) =>
      apiFetch<TripDay>(`/trips/${tripId}/days/${date}`, json('PUT', input)),
    onSuccess: (day) =>
      qc.setQueryData<TripDay[]>(transportKeys.days(tripId), (list) => list?.map((d) => (d.date === day.date ? day : d))),
  })
}

/** Trajets entre des lieux (au moins deux) ; `null` : rien à calculer. */
export const useRoutePlan = (tripId: string, req: RouteRequest | null) =>
  useQuery({
    queryKey: transportKeys.route(tripId, req ?? { poi_ids: [] }),
    queryFn: () => apiFetch<RoutePlan>(`/trips/${tripId}/routes`, json('POST', req)),
    enabled: !!req && req.poi_ids.length >= 2,
    // Les temps de trajet changent peu : inutile de recalculer à chaque affichage.
    staleTime: 10 * 60_000,
    placeholderData: keepPreviousData,
  })
