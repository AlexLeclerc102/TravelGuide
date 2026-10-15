import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiFetch } from './client'
import type { TravelOption } from './transport'
import { tripKeys, type TransportMode } from './trips'

export type UnscheduledReason = 'closed_all_days' | 'opening_hours' | 'no_time' | 'added_after'

/** Étape d'une journée ; heures `HH:MM` locales. */
export interface ItineraryStop {
  poi_id: string
  arrival: string
  start: string
  end: string
  wait_s: number
  /** Trajet depuis l'étape précédente ou l'hébergement. */
  travel: TravelOption | null
  locked: boolean
}

export interface ItineraryDay {
  date: string
  modes: TransportMode[]
  /** Hébergement de la veille (sinon du jour). */
  start_poi_id: string | null
  /** Hébergement du soir. */
  end_poi_id: string | null
  day_start: string
  day_end: string
  stops: ItineraryStop[]
  back: TravelOption | null
  total_travel_s: number
  total_visit_s: number
  finish: string
  /** Lieux ou réglages modifiés depuis le calcul. */
  stale: boolean
}

export interface Itinerary {
  version: number
  generated_at: string
  generated_by_name: string | null
  days: ItineraryDay[]
  unscheduled: { poi_id: string; reason: UnscheduledReason }[]
  stale: boolean
  has_estimates: boolean
}

/** `null` : pas encore généré. */
export const useItinerary = (tripId: string) =>
  useQuery({
    queryKey: tripKeys.itinerary(tripId),
    queryFn: () => apiFetch<Itinerary | null>(`/trips/${tripId}/itinerary`),
  })

export function useGenerateItinerary(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => apiFetch<Itinerary>(`/trips/${tripId}/itinerary`, { method: 'POST', body: '{}' }),
    onSuccess: (it) => qc.setQueryData(tripKeys.itinerary(tripId), it),
  })
}

/** Lieux d'une journée dans l'ordre, hébergements compris, sans doublon consécutif. */
export function dayPoiIds(day: ItineraryDay): string[] {
  const ids = [day.start_poi_id, ...day.stops.map((s) => s.poi_id), day.end_poi_id].filter(
    (id): id is string => id !== null,
  )
  return ids.filter((id, i) => i === 0 || id !== ids[i - 1])
}
