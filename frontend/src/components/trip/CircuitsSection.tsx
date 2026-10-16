import { useState, type FormEvent } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { errorMessage } from '../../api/errors'
import { placeToInput, sameOsm, useCreatePoi, type Poi } from '../../api/pois'
import { suggestionKeys, useAirports, useProposeCircuits, type Circuit } from '../../api/suggestions'
import { useApplyProposal } from '../../api/transport'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { dayCount, formatDate, formatDuration } from '../../lib/format'

const short = (date: string) => formatDate(date, { weekday: 'short', day: 'numeric', month: 'short' })

interface Props {
  trip: Trip
  pois: Poi[]
  canEdit: boolean
}

/** Circuits types : quelles régions enchaîner pour la durée du voyage, entre deux aéroports. */
export function CircuitsSection({ trip, pois, canEdit }: Props) {
  const [open, setOpen] = useState(false)
  const [start, setStart] = useState('')
  const [end, setEnd] = useState('')
  const { data: airports = [] } = useAirports(trip.id, open)
  const propose = useProposeCircuits(trip.id)
  const result = propose.data
  const nights = dayCount(trip.start_date, trip.end_date)
  const osmOf = (key: string) => airports.find((a) => `${a.osm.type}/${a.osm.id}` === key)?.osm ?? null

  function submit(e: FormEvent) {
    e.preventDefault()
    propose.mutate({ start: osmOf(start), end: osmOf(end) })
  }

  return (
    <details className="card circuits" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary className="section-title">{t.circuits.title(nights)}</summary>
      <p className="small muted">{t.circuits.intro}</p>
      <form className="form" onSubmit={submit} aria-label={t.circuits.title(nights)}>
        <div className="row">
          {(['start', 'end'] as const).map((which) => (
            <label key={which}>
              {t.circuits[which]}
              <select value={which === 'start' ? start : end} onChange={(e) => (which === 'start' ? setStart : setEnd)(e.target.value)}>
                <option value="">{t.proposal.none}</option>
                {airports.map((a) => (
                  <option key={`${a.osm.type}/${a.osm.id}`} value={`${a.osm.type}/${a.osm.id}`}>
                    {a.name}
                  </option>
                ))}
              </select>
            </label>
          ))}
        </div>
        <div className="actions">
          <button type="submit" disabled={propose.isPending}>
            {propose.isPending ? t.proposal.computing : t.circuits.compute}
          </button>
        </div>
      </form>
      {result?.status === 'not_imported' && <p className="notice small">{t.regions.notImported}</p>}
      {result?.status === 'ready' && result.circuits.length === 0 && <p className="muted small">{t.circuits.none}</p>}
      {result && result.circuits.length > 0 && (
        <ul className="circuit-list">
          {result.circuits.map((c, i) => (
            <CircuitCard key={c.stops.map((s) => s.name).join('>')} index={i} circuit={c} trip={trip} pois={pois} canEdit={canEdit} />
          ))}
        </ul>
      )}
      {result && result.boat_or_plane.length > 0 && <p className="small muted">{t.circuits.expeditions(result.boat_or_plane.join(', '))}</p>}
      {!!propose.error && (
        <p className="error small" role="alert">
          {errorMessage(propose.error)}
        </p>
      )}
    </details>
  )
}

interface CardProps {
  index: number
  circuit: Circuit
  trip: Trip
  pois: Poi[]
  canEdit: boolean
}

function CircuitCard({ index, circuit: c, trip, pois, canEdit }: CardProps) {
  const qc = useQueryClient()
  const apply = useApplyProposal(trip.id)
  const create = useCreatePoi(trip.id)
  const [state, setState] = useState<'idle' | 'applying' | 'done'>('idle')
  const title = index === 0 ? t.circuits.essentials : t.circuits.variant(index)
  const error = apply.error ?? create.error

  async function applyCircuit() {
    setState('applying')
    try {
      // Une zone d'hébergement par étape, sur ses nuits ; puis les points forts manquants.
      await apply.mutateAsync({
        stays: c.stops.map(({ name, lat, lon, osm, first_night, last_night, nights, transfer_min }) => ({
          name,
          lat,
          lon,
          osm,
          first_night,
          last_night,
          nights,
          transfer_min,
          poi_ids: [],
        })),
      })
      const added = [...pois]
      for (const place of c.stops.flatMap((s) => s.highlights)) {
        if (added.some((p) => sameOsm(p.osm, place.osm))) continue
        added.push(await create.mutateAsync(placeToInput(place)))
      }
      setState('done')
    } catch {
      // Erreur affichée via `apply.error` / `create.error`.
      setState('idle')
    } finally {
      qc.invalidateQueries({ queryKey: suggestionKeys.all(trip.id) })
    }
  }

  return (
    <li className="circuit-card">
      <div className="region-head">
        <strong>{title}</strong>
        <span className="muted small">{c.themes.map((cat) => t.categories[cat]).join(', ')}</span>
      </div>
      <ol>
        {c.stops.map((s) => (
          <li key={s.first_night}>
            <span>
              🛏️ <strong>{s.name}</strong>{' '}
              <span className="small muted">
                {t.stays.nights(s.nights)} · {t.stays.range(short(s.first_night), short(s.last_night))}
              </span>
            </span>
            {s.transfer_min > 0 && <span className="tiny muted proposal-transfer">{t.proposal.transfer(s.transfer_min)}</span>}
            {s.highlights.length > 0 && <span className="tiny muted"> · {s.highlights.map((h) => h.name).join(', ')}</span>}
          </li>
        ))}
      </ol>
      <p className="small">
        {t.circuits.drive(formatDuration(c.drive_min * 60))}
        {c.return_min > 0 && ` · ${t.circuits.back(formatDuration(c.return_min * 60))}`}
      </p>
      {canEdit &&
        (state === 'done' ? (
          <p className="success small" role="status">
            {t.circuits.applied}
          </p>
        ) : (
          <div className="actions">
            <button
              type="button"
              className="small-button primary"
              disabled={state === 'applying'}
              aria-label={t.circuits.applyNamed(title)}
              onClick={applyCircuit}
            >
              {state === 'applying' ? t.regions.adding : t.circuits.apply}
            </button>
            <span className="tiny muted">{t.circuits.applyHint}</span>
          </div>
        ))}
      {!!error && (
        <p className="error small" role="alert">
          {errorMessage(error)}
        </p>
      )}
    </li>
  )
}
