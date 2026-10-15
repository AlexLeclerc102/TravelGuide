/** Date ISO `YYYY-MM-DD` → texte lisible, sans décalage de fuseau. */
export function formatDate(iso: string, opts: Intl.DateTimeFormatOptions = { day: 'numeric', month: 'long', year: 'numeric' }) {
  const [y, m, d] = iso.slice(0, 10).split('-').map(Number)
  return new Date(y, m - 1, d).toLocaleDateString('fr-FR', opts)
}

export function formatDateRange(start: string, end: string) {
  return `${formatDate(start, { day: 'numeric', month: 'short' })} → ${formatDate(end)}`
}

/** Nombre de jours d'un voyage, bornes incluses. */
export function dayCount(start: string, end: string) {
  const toUtc = (iso: string) => {
    const [y, m, d] = iso.split('-').map(Number)
    return Date.UTC(y, m - 1, d)
  }
  return Math.round((toUtc(end) - toUtc(start)) / 86_400_000) + 1
}

/** Note formatée à la française, une décimale au plus (ex. « 4,3 »). */
export const formatRating = (n: number) => n.toLocaleString('fr-FR', { maximumFractionDigits: 1 })

/** Durée de trajet : « 12 min », « 1 h 05 » (arrondie à la minute, au moins 1 min si non nulle). */
export function formatDuration(seconds: number) {
  const min = seconds > 0 ? Math.max(1, Math.round(seconds / 60)) : 0
  if (min < 60) return `${min} min`
  const rest = min % 60
  return `${Math.floor(min / 60)} h${rest ? ` ${String(rest).padStart(2, '0')}` : ''}`
}

/** Distance : « 850 m », « 12,3 km », « 128 km ». */
export function formatDistance(meters: number) {
  if (meters < 1000) return `${Math.round(meters / 10) * 10} m`
  const km = meters / 1000
  return `${km.toLocaleString('fr-FR', { maximumFractionDigits: km < 100 ? 1 : 0 })} km`
}
