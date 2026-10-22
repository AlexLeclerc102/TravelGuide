import { useState, type FormEvent } from 'react'
import { errorMessage } from '../../api/errors'
import type { Poi } from '../../api/pois'
import { useApplyProposal, useProposeStays } from '../../api/transport'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { Icon } from '../Icon'
import { pointAt, useMapHover } from '../mapHover'
import { formatDate, formatDuration } from '../../lib/format'

const short = (date: string) => formatDate(date, { weekday: 'short', day: 'numeric', month: 'short' })

/** « Où dormir ? » : étapes proposées d'après les lieux du voyage, appliquées en un clic. */
export function StayProposal({ trip, pois, canEdit }: { trip: Trip; pois: Poi[]; canEdit: boolean }) {
  const hover = useMapHover()
  // Défaut selon le rythme du voyage (comme le serveur).
  const [minNights, setMinNights] = useState<number>(({ relaxed: 3, balanced: 2, packed: 1 } as const)[trip.pace] ?? 2)
  const [start, setStart] = useState('')
  const [end, setEnd] = useState('')
  const propose = useProposeStays(trip.id)
  const apply = useApplyProposal(trip.id)
  const proposal = propose.data
  const error = propose.error ?? apply.error
  // Arrivée et départ : tout lieu du voyage (souvent un aéroport ajouté comme lieu).
  const endpoints = pois.filter((p) => !p.placeholder)

  function submit(e: FormEvent) {
    e.preventDefault()
    apply.reset()
    propose.mutate({ min_nights: minNights, start_poi_id: start || null, end_poi_id: end || null })
  }

  return (
    <details className="stay-proposal">
      <summary>{t.proposal.open}</summary>
      <p className="small muted">{t.proposal.hint}</p>
      <form className="form" onSubmit={submit} aria-label={t.proposal.open}>
        <div className="row">
          <label>
            {t.proposal.minNights}
            <select value={minNights} onChange={(e) => setMinNights(Number(e.target.value))}>
              {[1, 2, 3, 4].map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
          {(['start', 'end'] as const).map((which) => (
            <label key={which}>
              {t.proposal[which]}
              <select value={which === 'start' ? start : end} onChange={(e) => (which === 'start' ? setStart : setEnd)(e.target.value)}>
                <option value="">{t.proposal.none}</option>
                {endpoints.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </label>
          ))}
        </div>
        <div className="actions">
          <button type="submit" disabled={propose.isPending}>
            {propose.isPending ? t.proposal.computing : t.proposal.compute}
          </button>
        </div>
      </form>

      {proposal && proposal.stays.length === 0 && <p className="muted small">{t.proposal.empty}</p>}
      {proposal && proposal.stays.length > 0 && (
        <>
          <ol className="proposal-list">
            {proposal.stays.map((s) => (
              <li key={s.first_night} {...hover(pointAt(s))}>
                <strong><Icon name="lodging" /> {s.name}</strong>{' '}
                <span className="small muted">
                  {t.stays.nights(s.nights)} · {t.stays.range(short(s.first_night), short(s.last_night))} · {t.proposal.places(s.poi_ids.length)}
                </span>
                {s.transfer_min > 0 && <span className="tiny muted proposal-transfer">{t.proposal.transfer(s.transfer_min)}</span>}
              </li>
            ))}
          </ol>
          <p className="small">{t.proposal.total(formatDuration(proposal.travel_min * 60))}</p>
          {canEdit &&
            (apply.isSuccess ? (
              <p className="success small" role="status">
                {t.proposal.applied}
              </p>
            ) : (
              <div className="actions">
                <button type="button" className="primary" disabled={apply.isPending} onClick={() => apply.mutate({ stays: proposal.stays })}>
                  {t.proposal.apply}
                </button>
                <span className="tiny muted">{t.proposal.applyHint}</span>
              </div>
            ))}
        </>
      )}
      {!!error && (
        <p className="error small" role="alert">
          {errorMessage(error)}
        </p>
      )}
    </details>
  )
}
