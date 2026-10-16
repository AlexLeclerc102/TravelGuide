import type { Poi } from '../api/pois'
import type { SuggestionReason } from '../api/suggestions'
import type { TripDay } from '../api/transport'
import { t } from '../i18n/fr'
import { formatDate, formatRating } from './format'

const short = (date: string) => formatDate(date, { weekday: 'short', day: 'numeric', month: 'short' })

/** « 2 h 30 », « 45 min ». */
function duration(minutes: number) {
  if (minutes < 60) return `${minutes} min`
  const rest = minutes % 60
  return `${Math.floor(minutes / 60)} h${rest ? ` ${String(rest).padStart(2, '0')}` : ''}`
}

/** Consultations arrondies à la centaine (« ≈ 5 200 ») : un ordre de grandeur. */
const roundViews = (n: number) => (n >= 1000 ? Math.round(n / 100) * 100 : n)

/** Point de départ d'un jour, pour l'afficher : son hébergement, sinon celui de la veille. */
function startName(date: string | null, days: TripDay[], pois: Poi[]) {
  const i = days.findIndex((d) => d.date === date)
  const id = i >= 0 ? (days[i].base_poi_id ?? days[i - 1]?.base_poi_id) : null
  return pois.find((p) => p.id === id)?.name ?? t.suggestions.yourPlaces
}

/** Texte d'une raison de suggestion. */
export function reasonText(r: SuggestionReason, days: TripDay[], pois: Poi[]) {
  switch (r.kind) {
    case 'near':
      return t.suggestions.near(r.minutes, startName(r.date, days, pois), r.date && short(r.date))
    case 'notable':
      return t.suggestions.notable(r.languages)
    case 'popular':
      return t.suggestions.popular(roundViews(r.monthly_views).toLocaleString('fr-FR'))
    case 'wikivoyage':
      return t.suggestions.wikivoyage
    case 'world_heritage':
      return t.suggestions.worldHeritage
    case 'protected_area':
      return t.suggestions.protectedArea(r.km2.toLocaleString('fr-FR'), r.national_park)
    case 'taste':
      return t.suggestions.taste(t.categories[r.category].toLowerCase(), formatRating(r.average))
    case 'free_time':
      return t.suggestions.freeTime(duration(r.minutes), short(r.date))
    case 'interest':
      return t.suggestions.interest(t.interests[r.interest].toLowerCase())
    case 'budget':
      return t.suggestions.budget(t.budgets[r.budget].replace(/ \(.*\)$/, '').toLowerCase())
  }
}
