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
