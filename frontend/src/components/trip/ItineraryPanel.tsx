import { useEffect, useMemo, useState } from 'react'
import { errorMessage } from '../../api/errors'
import { dayPoiIds, useGenerateItinerary, useItinerary, type Itinerary, type ItineraryDay } from '../../api/itinerary'
import type { Poi } from '../../api/pois'
import { useRoutePlan, type TravelOption } from '../../api/transport'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { formatDate, formatDistance, formatDuration } from '../../lib/format'

interface Props {
  trip: Trip
  pois: Poi[]
  onRouteChange: (geometry: [number, number][] | null) => void
  onSelectPoi: (id: string) => void
}

/** Le calcul de trajets relie au plus 25 lieux. */
const MAX_ROUTE_POIS = 25

const dayLabel = (date: string) => formatDate(date, { weekday: 'short', day: 'numeric', month: 'short' })

export function ItineraryPanel({ trip, pois, onRouteChange, onSelectPoi }: Props) {
  const { data: itinerary, error, isPending } = useItinerary(trip.id)
  const generate = useGenerateItinerary(trip.id)
  const canEdit = trip.my_role !== 'viewer'
  const names = useMemo(() => new Map(pois.map((p) => [p.id, p.name])), [pois])

  if (isPending) return <p className="muted">{t.common.loading}</p>
  if (error) return <p className="error">{errorMessage(error)}</p>

  const run = () => {
    if (itinerary && !confirm(t.itinerary.regenerateConfirm)) return
    generate.mutate()
  }
  const button = canEdit && (
    <button type="button" className={itinerary ? 'small-button' : 'primary'} onClick={run} disabled={generate.isPending}>
      {generate.isPending ? t.itinerary.generating : itinerary ? t.itinerary.regenerate : t.itinerary.generate}
    </button>
  )
  const generateError = generate.error && <p className="error small" role="alert">{errorMessage(generate.error)}</p>

  if (!itinerary) {
    return (
      <section className="card">
        <h2 className="section-title">{t.itinerary.title}</h2>
        <p>{t.itinerary.none}</p>
        <p className="small muted">{t.itinerary.intro}</p>
        {canEdit ? button : <p className="small">{t.itinerary.viewerHint}</p>}
        {generateError}
      </section>
    )
  }

  return (
    <div className="poi-panel">
      <section className="card itinerary-head">
        <div className="list-header">
          <h2 className="section-title">{t.itinerary.title}</h2>
          {button}
        </div>
        <p className="tiny muted">
          {t.itinerary.generated(
            new Date(itinerary.generated_at).toLocaleString('fr-FR', { dateStyle: 'medium', timeStyle: 'short' }),
            itinerary.generated_by_name,
          )}
        </p>
        {generateError}
        {itinerary.stale && <p className="notice warning small">{t.itinerary.stale}</p>}
        {itinerary.has_estimates && <p className="hint">ⓘ {t.itinerary.estimates}</p>}
      </section>
      <Days itinerary={itinerary} tripId={trip.id} names={names} onRouteChange={onRouteChange} onSelectPoi={onSelectPoi} />
      <Unscheduled itinerary={itinerary} names={names} onSelectPoi={onSelectPoi} />
    </div>
  )
}

interface DaysProps {
  itinerary: Itinerary
  tripId: string
  names: Map<string, string>
  onRouteChange: Props['onRouteChange']
  onSelectPoi: Props['onSelectPoi']
}

function Days({ itinerary, tripId, names, onRouteChange, onSelectPoi }: DaysProps) {
  // Jour affiché sur la carte : par défaut le premier qui a des étapes.
  const [shown, setShown] = useState<string | null>(
    () => itinerary.days.find((d) => d.stops.length > 0)?.date ?? null,
  )
  const day = itinerary.days.find((d) => d.date === shown)
  const ids = day ? dayPoiIds(day) : []
  const { data: plan } = useRoutePlan(
    tripId,
    day && ids.length >= 2 && ids.length <= MAX_ROUTE_POIS ? { poi_ids: ids, date: day.date } : null,
  )
  // Le tracé n'est affiché que s'il correspond bien au jour choisi (pas celui d'avant).
  const matches =
    !!plan &&
    plan.legs.length === ids.length - 1 &&
    plan.legs[0]?.from_poi_id === ids[0] &&
    plan.legs.at(-1)?.to_poi_id === ids.at(-1)
  const geometry = useMemo(() => (matches && plan ? plan.legs.flatMap((l) => l.geometry) : null), [matches, plan])
  useEffect(() => {
    onRouteChange(geometry && geometry.length > 1 ? geometry : null)
  }, [geometry, onRouteChange])
  useEffect(() => () => onRouteChange(null), [onRouteChange])

  return (
    <>
      {itinerary.days.map((d) => (
        <DayCard
          key={d.date}
          day={d}
          names={names}
          shown={d.date === shown}
          onShow={() => setShown(d.date)}
          onSelectPoi={onSelectPoi}
        />
      ))}
    </>
  )
}

interface DayCardProps {
  day: ItineraryDay
  names: Map<string, string>
  shown: boolean
  onShow: () => void
  onSelectPoi: (id: string) => void
}

function DayCard({ day, names, shown, onShow, onSelectPoi }: DayCardProps) {
  const label = dayLabel(day.date)
  const name = (id: string) => names.get(id) ?? '—'
  return (
    <section className={`card itinerary-day${shown ? ' shown' : ''}`} aria-label={label}>
      <button type="button" className="day-title" onClick={onShow} aria-pressed={shown} title={t.itinerary.showOnMap(label)}>
        <strong>{label}</strong>
        <span className="tiny muted">{day.modes.map((m) => t.modeIcons[m]).join(' ')}</span>
        {day.stale && <span className="tag tag-estimate">{t.itinerary.staleDay}</span>}
      </button>
      {day.stops.length > 0 ? (
        <p className="tiny muted">
          {t.itinerary.summary(
            day.stops.length,
            formatDuration(day.total_visit_s),
            formatDuration(day.total_travel_s),
            day.finish,
          )}
        </p>
      ) : (
        <p className="small muted">{day.back ? t.itinerary.transferDay : t.itinerary.freeDay}</p>
      )}
      <ol className="timeline">
        {day.start_poi_id && (day.stops.length > 0 || day.back) && (
          <li className="lodging">
            <span className="time">{day.day_start}</span>
            <span>
              🛏 {t.itinerary.departure} : {name(day.start_poi_id)}
            </span>
          </li>
        )}
        {day.stops.map((s) => (
          <StopItem key={s.poi_id} stop={s} name={name(s.poi_id)} onSelect={() => onSelectPoi(s.poi_id)} />
        ))}
        {day.back && <TravelItem travel={day.back} />}
        {day.end_poi_id && day.back && (
          <li className="lodging">
            <span className="time">{day.finish}</span>
            <span>
              🛏 {t.itinerary.arrival} : {name(day.end_poi_id)}
            </span>
          </li>
        )}
      </ol>
    </section>
  )
}

function StopItem({ stop, name, onSelect }: { stop: ItineraryDay['stops'][number]; name: string; onSelect: () => void }) {
  return (
    <>
      {stop.travel && <TravelItem travel={stop.travel} />}
      <li className="stop">
        <span className="time">
          {stop.start}–{stop.end}
        </span>
        <button type="button" className="link-button stop-name" onClick={onSelect}>
          {name}
        </button>
        {stop.wait_s >= 60 && <span className="tiny muted">{t.itinerary.wait(formatDuration(stop.wait_s))}</span>}
      </li>
    </>
  )
}

function TravelItem({ travel }: { travel: TravelOption }) {
  const estimate = travel.source !== 'router'
  return (
    <li className="travel">
      <span className="time" />
      <span className="tiny muted">
        {t.modeIcons[travel.mode]} {t.modes[travel.mode]} · {estimate && '≈ '}
        {formatDuration(travel.duration_s)} · {formatDistance(travel.distance_m)}
        {estimate && ` · ${t.transport.sources[travel.source]}`}
      </span>
    </li>
  )
}

function Unscheduled({
  itinerary,
  names,
  onSelectPoi,
}: {
  itinerary: Itinerary
  names: Map<string, string>
  onSelectPoi: (id: string) => void
}) {
  if (itinerary.unscheduled.length === 0) return <p className="tiny muted">{t.itinerary.lodgingNote}</p>
  return (
    <section className="card">
      <h2 className="section-title">{t.itinerary.unscheduledTitle(itinerary.unscheduled.length)}</h2>
      <ul className="unscheduled">
        {itinerary.unscheduled.map((u) => (
          <li key={u.poi_id}>
            <button type="button" className="link-button" onClick={() => onSelectPoi(u.poi_id)}>
              {names.get(u.poi_id) ?? '—'}
            </button>{' '}
            <span className="tiny muted">{t.itinerary.reasons[u.reason]}</span>
          </li>
        ))}
      </ul>
      <p className="tiny muted">{t.itinerary.lodgingNote}</p>
    </section>
  )
}
