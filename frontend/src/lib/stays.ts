import type { TripDay } from '../api/transport'

/** Date ISO décalée de `n` jours (sans fuseau). */
export function addDays(iso: string, n: number) {
  const [y, m, d] = iso.split('-').map(Number)
  const date = new Date(Date.UTC(y, m - 1, d + n))
  return date.toISOString().slice(0, 10)
}

/** Nuits consécutives au même hébergement (`poiId` nul : sans hébergement). */
export interface Stay {
  poiId: string | null
  firstNight: string
  lastNight: string
  nights: number
  /** Arrivée le soir de la première nuit, départ le lendemain de la dernière. */
  checkin: string
  checkout: string
}

/** Regroupe les jours du voyage en séjours (l'hébergement d'un jour est celui de sa nuit). */
export function stays(days: Pick<TripDay, 'date' | 'base_poi_id'>[]): Stay[] {
  const result: Stay[] = []
  for (const day of days) {
    const last = result.at(-1)
    if (last && last.poiId === day.base_poi_id && addDays(last.lastNight, 1) === day.date) {
      last.lastNight = day.date
      last.nights += 1
      last.checkout = addDays(day.date, 1)
    } else {
      result.push({
        poiId: day.base_poi_id,
        firstNight: day.date,
        lastNight: day.date,
        nights: 1,
        checkin: day.date,
        checkout: addDays(day.date, 1),
      })
    }
  }
  return result
}
