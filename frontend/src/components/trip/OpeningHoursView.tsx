import { useOpeningPreview } from '../../api/pois'
import { t } from '../../i18n/fr'
import { formatDate } from '../../lib/format'

interface Props {
  tripId: string
  value: string
  countryCode: string | null
}

/** Horaires interprétés par le serveur pour chaque date du voyage, pour vérification. */
export function OpeningHoursView({ tripId, value, countryCode }: Props) {
  const { data, isFetching, isError } = useOpeningPreview(tripId, value.trim(), countryCode)
  if (!value.trim() || isError) return null
  if (!data) return isFetching ? <p className="muted small">{t.opening.loading}</p> : null
  if (data.status === 'invalid') {
    return (
      <p className="notice warning small" role="status">
        {t.opening.invalid}
        {data.error && <span className="muted"> ({data.error})</span>}
      </p>
    )
  }
  return (
    <div className="opening">
      <p className="small opening-title">{t.opening.title}</p>
      <ul className="opening-days">
        {data.days.map((d) => (
          <li key={d.date} className={d.intervals.length === 0 ? 'closed' : undefined}>
            <span className="opening-date">{formatDate(d.date, { weekday: 'short', day: 'numeric', month: 'short' })}</span>
            <span>
              {d.intervals.length === 0
                ? t.opening.closed
                : d.intervals
                    .map((i) => `${i.start}–${i.end}${i.unknown ? ` (${i.comment ?? t.opening.uncertain})` : ''}`)
                    .join(', ')}
              {d.holiday && <span className="tag tag-holiday">{t.opening.holiday}</span>}
            </span>
          </li>
        ))}
      </ul>
    </div>
  )
}
