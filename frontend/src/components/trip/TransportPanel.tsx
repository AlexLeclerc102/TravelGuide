import { useEffect, useState } from 'react'
import { errorMessage } from '../../api/errors'
import type { Poi } from '../../api/pois'
import {
  useRoutePlan,
  useTripDays,
  useUpdateTripDay,
  type RouteLeg,
  type TravelOption,
  type TripDay,
} from '../../api/transport'
import { ALL_MODES, type TransportMode, type Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { StaysSection } from './StaysSection'
import { formatDate, formatDistance, formatDuration } from '../../lib/format'

interface Props {
  trip: Trip
  pois: Poi[]
  /** Tracé `[lon, lat]` du trajet comparé, à afficher sur la carte (`null` : aucun). */
  onRouteChange: (geometry: [number, number][] | null) => void
}

const dayLabel = (date: string) => formatDate(date, { weekday: 'short', day: 'numeric', month: 'short' })

export function TransportPanel({ trip, pois, onRouteChange }: Props) {
  const { data: days = [], error } = useTripDays(trip.id)
  const canEdit = trip.my_role !== 'viewer'
  const usesTransit = trip.default_modes.includes('transit') || days.some((d) => d.modes.includes('transit'))

  return (
    <div className="poi-panel">
      <StaysSection trip={trip} days={days} pois={pois} canEdit={canEdit} />
      <section className="card">
        <h2 className="section-title">{t.transport.daysTitle}</h2>
        <p className="small">
          {t.transport.defaults(trip.default_modes.map((m) => t.modes[m]).join(', '))} {t.transport.defaultsHint}
        </p>
        {error && <p className="error">{errorMessage(error)}</p>}
        <ul className="day-list">
          {days.map((day) => (
            <DayRow key={day.date} trip={trip} day={day} pois={pois} canEdit={canEdit} />
          ))}
        </ul>
        <p className="tiny muted">{t.transport.lodgingHint}</p>
        {usesTransit && <p className="hint">ⓘ {t.transport.transitNotice}</p>}
      </section>
      <RouteCompare trip={trip} days={days} pois={pois} onRouteChange={onRouteChange} />
    </div>
  )
}

function DayRow({ trip, day, pois, canEdit }: { trip: Trip; day: TripDay; pois: Poi[]; canEdit: boolean }) {
  const update = useUpdateTripDay(trip.id)
  const label = dayLabel(day.date)
  // Un hébergement supprimé entre-temps n'est plus proposé.
  const baseId = pois.some((p) => p.id === day.base_poi_id) ? day.base_poi_id : null

  const save = (modes: TransportMode[] | null, base_poi_id: string | null) =>
    update.mutate({ date: day.date, modes, base_poi_id })

  function toggle(mode: TransportMode) {
    const next = day.modes.includes(mode) ? day.modes.filter((m) => m !== mode) : [...day.modes, mode]
    if (next.length === 0) return
    // Revenir exactement aux modes du voyage : le jour les « hérite » de nouveau.
    const same = next.length === trip.default_modes.length && next.every((m) => trip.default_modes.includes(m))
    save(same ? null : ALL_MODES.filter((m) => next.includes(m)), baseId)
  }

  return (
    <li className={`day-row${day.modes_inherited ? '' : ' custom'}`}>
      <div className="day-head">
        <strong>{label}</strong>
        <span className="tiny muted">{day.modes_inherited ? t.transport.inherited : t.transport.custom}</span>
        {canEdit && !day.modes_inherited && (
          <button type="button" className="link-button tiny" onClick={() => save(null, baseId)} disabled={update.isPending}>
            {t.transport.reset}
          </button>
        )}
      </div>
      <fieldset className="chips mode-chips" aria-label={t.transport.modesOf(label)} disabled={!canEdit || update.isPending}>
        {ALL_MODES.map((mode) => {
          const on = day.modes.includes(mode)
          return (
            <label key={mode} className={`chip${on ? ' on' : ''}`} title={on && day.modes.length === 1 ? t.transport.lastMode : undefined}>
              <input type="checkbox" checked={on} onChange={() => toggle(mode)} />
              {t.modes[mode]}
            </label>
          )
        })}
      </fieldset>
      <label className="lodging-select">
        <span className="small">{t.transport.lodging}</span>
        <select
          aria-label={t.transport.lodgingOf(label)}
          value={baseId ?? ''}
          disabled={!canEdit || update.isPending}
          onChange={(e) => save(day.modes_inherited ? null : day.modes, e.target.value || null)}
        >
          <option value="">{t.transport.noLodging}</option>
          <PoiOptions pois={pois} lodgingFirst />
        </select>
      </label>
      {update.error && <p className="error small" role="alert">{errorMessage(update.error)}</p>}
    </li>
  )
}

/** Lieux pour un `<select>`, hébergements regroupés en tête si demandé. */
function PoiOptions({ pois, lodgingFirst = false }: { pois: Poi[]; lodgingFirst?: boolean }) {
  const lodging = pois.filter((p) => p.category === 'lodging')
  if (!lodgingFirst || lodging.length === 0) {
    return pois.map((p) => (
      <option key={p.id} value={p.id}>
        {p.name}
      </option>
    ))
  }
  const others = pois.filter((p) => p.category !== 'lodging')
  return (
    <>
      <optgroup label={t.transport.lodgingGroup}>
        {lodging.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </optgroup>
      {others.length > 0 && (
        <optgroup label={t.transport.otherGroup}>
          {others.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </optgroup>
      )}
    </>
  )
}

interface CompareProps {
  trip: Trip
  days: TripDay[]
  pois: Poi[]
  onRouteChange: (geometry: [number, number][] | null) => void
}

function RouteCompare({ trip, days, pois, onRouteChange }: CompareProps) {
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const [date, setDate] = useState('')
  const ready = from !== '' && to !== '' && from !== to
  const { data: plan, error, isFetching } = useRoutePlan(
    trip.id,
    ready ? { poi_ids: [from, to], ...(date ? { date } : {}) } : null,
  )
  const leg = ready && plan?.legs[0]?.from_poi_id === from && plan.legs[0].to_poi_id === to ? plan.legs[0] : null

  // Le tracé du trajet retenu s'affiche sur la carte tant que le panneau est ouvert.
  useEffect(() => {
    onRouteChange(leg && leg.geometry.length > 1 ? leg.geometry : null)
  }, [leg, onRouteChange])
  useEffect(() => () => onRouteChange(null), [onRouteChange])

  if (pois.length < 2) {
    return (
      <section className="card">
        <h2 className="section-title">{t.transport.compareTitle}</h2>
        <p className="muted small">{t.transport.needTwo}</p>
      </section>
    )
  }

  return (
    <section className="card route-compare">
      <h2 className="section-title">{t.transport.compareTitle}</h2>
      <div className="form">
        <label>
          {t.transport.from}
          <select value={from} onChange={(e) => setFrom(e.target.value)}>
            <option value="">{t.transport.choose}</option>
            <PoiOptions pois={pois} />
          </select>
        </label>
        <label>
          {t.transport.to}
          <select value={to} onChange={(e) => setTo(e.target.value)}>
            <option value="">{t.transport.choose}</option>
            <PoiOptions pois={pois.filter((p) => p.id !== from)} />
          </select>
        </label>
        <label>
          {t.transport.day}
          <select value={date} onChange={(e) => setDate(e.target.value)}>
            <option value="">{t.transport.tripModes}</option>
            {days.map((d) => (
              <option key={d.date} value={d.date}>
                {t.transport.dayModes(dayLabel(d.date))}
              </option>
            ))}
          </select>
        </label>
      </div>
      {ready && isFetching && !leg && <p className="muted small">{t.transport.computing}</p>}
      {ready && error && <p className="error small">{errorMessage(error)}</p>}
      {leg && <LegOptions leg={leg} />}
      <p className="tiny muted">{t.transport.rule}</p>
      <p className="tiny muted">{t.transport.attribution}</p>
    </section>
  )
}

function LegOptions({ leg }: { leg: RouteLeg }) {
  const straight = leg.options.some((o) => o.source === 'straight_line')
  return (
    <>
      <ul className="travel-options" aria-label={t.transport.compareTitle}>
        {leg.options.map((o) => (
          <OptionRow key={o.mode} option={o} chosen={o.mode === leg.chosen.mode} />
        ))}
      </ul>
      {straight && <p className="notice warning small">{t.transport.straightLineNotice}</p>}
    </>
  )
}

function OptionRow({ option, chosen }: { option: TravelOption; chosen: boolean }) {
  const flag = t.transport.sources[option.source]
  return (
    <li className={chosen ? 'chosen' : undefined}>
      <span className="travel-mode">{t.modes[option.mode]}</span>
      <span className="travel-time">
        {option.source !== 'router' && '≈ '}
        {formatDuration(option.duration_s)}
      </span>
      <span className="muted small">{formatDistance(option.distance_m)}</span>
      {flag && <span className="tag tag-estimate">{flag}</span>}
      {chosen && <span className="tag tag-active">{t.transport.chosen}</span>}
    </li>
  )
}
