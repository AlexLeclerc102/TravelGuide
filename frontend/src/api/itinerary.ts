import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiFetch } from './client'
import type { TravelOption } from './transport'
import { tripKeys, type TransportMode } from './trips'

export type UnscheduledReason = 'closed_all_days' | 'opening_hours' | 'no_time' | 'removed' | 'added_after'

/** Contrainte non respectée par une étape placée ou verrouillée à la main. */
export type StopIssue = 'outside_opening_hours' | 'after_day_end'

/** Étape d'une journée ; heures `HH:MM` locales. */
export interface ItineraryStop {
  poi_id: string
  arrival: string
  start: string
  end: string
  wait_s: number
  /** Trajet depuis l'étape précédente ou l'hébergement. */
  travel: TravelOption | null
  /** Garde son jour et son ordre quand on régénère. */
  locked: boolean
  issue: StopIssue | null
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
  /** La journée se termine après `day_end`. */
  overtime: boolean
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

/** Ajustement manuel ; `move` avec `date: null` retire le lieu de l'itinéraire. */
export type Adjustment =
  | { kind: 'move'; poi_id: string; date: string | null; position: number }
  | { kind: 'lock'; poi_id: string; locked: boolean }
  | { kind: 'recompute'; date: string }

function request(tripId: string, version: number, a: Adjustment): [string, RequestInit] {
  const base = `/trips/${tripId}/itinerary`
  const body = (method: string, data: object) => ({ method, body: JSON.stringify({ version, ...data }) })
  switch (a.kind) {
    case 'move':
      return [`${base}/moves`, body('POST', { poi_id: a.poi_id, date: a.date, position: a.position })]
    case 'lock':
      return [`${base}/stops/${a.poi_id}`, body('PATCH', { locked: a.locked })]
    case 'recompute':
      return [`${base}/days/${a.date}/recompute`, body('POST', {})]
  }
}

/**
 * Applique un déplacement en local, en attendant la réponse du serveur : l'étape change
 * de place (ses horaires seront recalculés par le serveur).
 */
export function moveLocally(it: Itinerary, poiId: string, date: string | null, position: number): Itinerary {
  const existing = it.days.flatMap((d) => d.stops).find((s) => s.poi_id === poiId)
  const stop: ItineraryStop = existing ?? {
    poi_id: poiId,
    arrival: '',
    start: '',
    end: '',
    wait_s: 0,
    travel: null,
    locked: false,
    issue: null,
  }
  const days = it.days.map((d) => {
    const stops = d.stops.filter((s) => s.poi_id !== poiId)
    if (d.date === date) stops.splice(Math.min(position, stops.length), 0, stop)
    return stops.length === d.stops.length && d.date !== date ? d : { ...d, stops }
  })
  const unscheduled = it.unscheduled.filter((u) => u.poi_id !== poiId)
  if (date === null) unscheduled.push({ poi_id: poiId, reason: 'removed' })
  return { ...it, days, unscheduled }
}

export function useAdjustItinerary(tripId: string) {
  const qc = useQueryClient()
  const key = tripKeys.itinerary(tripId)
  return useMutation({
    mutationFn: ({ version, adjustment }: { version: number; adjustment: Adjustment }) =>
      apiFetch<Itinerary>(...request(tripId, version, adjustment)),
    onMutate: async ({ adjustment: a }) => {
      if (a.kind !== 'move') return {}
      await qc.cancelQueries({ queryKey: key })
      const previous = qc.getQueryData<Itinerary | null>(key)
      if (previous) qc.setQueryData(key, moveLocally(previous, a.poi_id, a.date, a.position))
      return { previous }
    },
    onSuccess: (it) => qc.setQueryData(key, it),
    onError: (_e, _v, context) => {
      if (context?.previous) qc.setQueryData(key, context.previous)
      // Conflit de version (modifié par quelqu'un d'autre) : on recharge.
      qc.invalidateQueries({ queryKey: key })
    },
  })
}

/** Lieux d'une journée dans l'ordre, hébergements compris, sans doublon consécutif. */
export function dayPoiIds(day: ItineraryDay): string[] {
  const ids = [day.start_poi_id, ...day.stops.map((s) => s.poi_id), day.end_poi_id].filter(
    (id): id is string => id !== null,
  )
  return ids.filter((id, i) => i === 0 || id !== ids[i - 1])
}
