import { useState, type FormEvent } from 'react'
import { errorMessage } from '../../api/errors'
import {
  placeToInput,
  sameOsm,
  sortPois,
  useCreatePoi,
  usePlaceSearch,
  type Bounds,
  type Poi,
  type PoiInput,
  type PoiSort,
} from '../../api/pois'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { Icon } from '../Icon'
import { pointAt, useMapHover } from '../mapHover'
import { formatDate, formatRating } from '../../lib/format'
import { PoiDetails, PoiEditor } from './PoiEditor'
import { LinksSection, NotesSection } from './PoiContent'
import { PoiLinks } from './PoiLinks'
import { ReviewsSection } from './ReviewsSection'
import { WikiCard } from './WikiCard'

export type Selection = { kind: 'poi'; id: string } | { kind: 'draft'; draft: PoiInput } | null

interface Props {
  trip: Trip
  pois: Poi[]
  selection: Selection
  onSelect: (selection: Selection) => void
  addMode: boolean
  onAddModeChange: (on: boolean) => void
  locating: boolean
  locateError: unknown
  bounds: Bounds | null
}

function closedSummary(dates: string[]) {
  const shown = dates.slice(0, 3).map((d) => formatDate(d, { weekday: 'short', day: 'numeric', month: 'short' }))
  return t.pois.closedOn(shown.join(', ') + (dates.length > 3 ? '…' : ''))
}

export function PoiPanel({ trip, pois, selection, onSelect, addMode, onAddModeChange, locating, locateError, bounds }: Props) {
  const hover = useMapHover()
  const canEdit = trip.my_role !== 'viewer'
  const selectedPoi = selection?.kind === 'poi' ? pois.find((p) => p.id === selection.id) : undefined
  const [sort, setSort] = useState<PoiSort>('added')
  const sorted = sortPois(pois, sort)

  return (
    <div className="poi-panel">
      {canEdit && (
        <PlaceSearch
          trip={trip}
          pois={pois}
          bounds={bounds}
          onAdded={(poi) => onSelect({ kind: 'poi', id: poi.id })}
          onSelectExisting={(id) => onSelect({ kind: 'poi', id })}
        />
      )}
      {canEdit && (
        <div className="click-mode">
          <button type="button" aria-pressed={addMode} onClick={() => onAddModeChange(!addMode)}>
            <Icon name="pin" /> {t.pois.clickMode}
          </button>
          {addMode && <p className="hint">{t.pois.clickModeOn}</p>}
          {locating && <p className="muted small">{t.pois.locating}</p>}
          {locateError != null && (
            <p className="error" role="alert">
              {errorMessage(locateError)}
            </p>
          )}
        </div>
      )}

      {/* Le formulaire attend le géocodage inverse, qui préremplit ses valeurs initiales. */}
      {selection?.kind === 'draft' && !locating && (
        <section className="card selected-card">
          <PoiEditor
            key={`${selection.draft.lat},${selection.draft.lon}`}
            tripId={trip.id}
            draft={selection.draft}
            onSaved={(poi) => onSelect({ kind: 'poi', id: poi.id })}
            onClose={() => onSelect(null)}
          />
        </section>
      )}
      {selectedPoi && (
        <section className="card selected-card" aria-label={selectedPoi.name}>
          <div className="selected-header">
            <div>
              <h3>
                <Icon name={selectedPoi.category} size={20} /> {selectedPoi.name}
              </h3>
              <p className="muted small">
                {t.categories[selectedPoi.category]} · {t.pois.minutes(selectedPoi.duration_min)}
                {selectedPoi.created_by_name && ` · ${t.pois.addedBy(selectedPoi.created_by_name)}`}
              </p>
            </div>
            <button type="button" className="icon-button" aria-label={t.pois.close} onClick={() => onSelect(null)}>
              ×
            </button>
          </div>
          <WikiCard tripId={trip.id} poi={selectedPoi} />
          <PoiLinks poi={selectedPoi} />
          <NotesSection key={`notes:${selectedPoi.id}`} tripId={trip.id} poiId={selectedPoi.id} canEdit={canEdit} />
          {/* Noter est plus fréquent que modifier : les avis d'abord, la modification repliée. */}
          <ReviewsSection key={selectedPoi.id} tripId={trip.id} poi={selectedPoi} />
          <LinksSection key={`links:${selectedPoi.id}`} tripId={trip.id} poiId={selectedPoi.id} canEdit={canEdit} />
          {canEdit ? (
            <details className="edit-details">
              <summary>{t.pois.editDetails}</summary>
              <PoiEditor
                key={`${selectedPoi.id}:${selectedPoi.version}`}
                tripId={trip.id}
                poi={selectedPoi}
                onSaved={() => undefined}
                onClose={() => onSelect(null)}
              />
            </details>
          ) : (
            <PoiDetails tripId={trip.id} poi={selectedPoi} />
          )}
        </section>
      )}

      <section>
        <div className="list-header">
          <h2 className="section-title">{t.pois.listTitle(pois.length)}</h2>
          {pois.length > 1 && (
            <label className="small">
              {t.reviews.sort}{' '}
              <select value={sort} onChange={(e) => setSort(e.target.value as PoiSort)}>
                {(['added', 'rating', 'disagreement'] as PoiSort[]).map((s) => (
                  <option key={s} value={s}>
                    {t.reviews.sorts[s]}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
        {pois.length === 0 ? (
          <p className="muted small">
            {t.pois.empty} {canEdit && t.pois.emptyEditor}
          </p>
        ) : (
          <ul className="poi-list">
            {sorted.map((p) => (
              <li key={p.id} {...hover(pointAt(p, p.id))}>
                <button
                  type="button"
                  className={`poi-item${selectedPoi?.id === p.id ? ' active' : ''}`}
                  onClick={() => onSelect({ kind: 'poi', id: p.id })}
                >
                  <span className="poi-icon" aria-hidden>
                    <Icon name={p.category} />
                  </span>
                  <span className="poi-text">
                    <strong>
                      {p.name}
                      {p.placeholder && <span className="tag tag-area"> {t.pois.areaTag}</span>}
                    </strong>
                    <span className="muted small">
                      {t.categories[p.category]} · {t.pois.minutes(p.duration_min)}
                    </span>
                    <span className="small rating-line">
                      {p.rating.average !== null && (
                        <span>
                          ★ {formatRating(p.rating.average)} · {t.reviews.count(p.rating.count)}
                        </span>
                      )}
                      {p.rating.disagreement && <span className="tag tag-disagreement"><Icon name="split" size={13} /> {t.reviews.disagreement}</span>}
                      {!p.my_review && <span className="muted">{t.reviews.toRate}</span>}
                    </span>
                    {p.closed_dates.length > 0 && <span className="small warning-text">{closedSummary(p.closed_dates)}</span>}
                    {p.opening_hours_status === 'invalid' && <span className="small warning-text">{t.pois.hoursInvalid}</span>}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  )
}

function PlaceSearch({
  trip,
  pois,
  bounds,
  onAdded,
  onSelectExisting,
}: {
  trip: Trip
  pois: Poi[]
  bounds: Bounds | null
  onAdded: (poi: Poi) => void
  onSelectExisting: (id: string) => void
}) {
  const hover = useMapHover()
  const [text, setText] = useState('')
  // La recherche part à la validation du formulaire, avec la zone affichée à ce moment-là.
  const [submitted, setSubmitted] = useState<{ q: string; bounds: Bounds | null }>({ q: '', bounds: null })
  const search = usePlaceSearch(submitted.q, submitted.bounds)
  const create = useCreatePoi(trip.id)

  function submit(e: FormEvent) {
    e.preventDefault()
    setSubmitted({ q: text.trim(), bounds })
  }

  return (
    <section className="place-search">
      <form className="row" onSubmit={submit} role="search">
        <label className="sr-only" htmlFor="place-search">
          {t.pois.searchLabel}
        </label>
        <input
          id="place-search"
          type="search"
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder={t.pois.searchPlaceholder}
          minLength={2}
          maxLength={200}
        />
        <button type="submit" disabled={text.trim().length < 2 || search.isFetching}>
          {search.isFetching ? t.pois.searching : t.pois.search}
        </button>
      </form>
      {search.error && (
        <p className="error" role="alert">
          {errorMessage(search.error)}
        </p>
      )}
      {create.error && (
        <p className="error" role="alert">
          {errorMessage(create.error)}
        </p>
      )}
      {search.data && (
        <>
          {search.data.length === 0 ? (
            <p className="muted small">{t.pois.noResults}</p>
          ) : (
            <ul className="result-list" aria-label={t.pois.searchLabel}>
              {search.data.map((place, i) => {
                const existing = pois.find((p) => sameOsm(p.osm, place.osm))
                const name = place.name || place.display_name.split(',')[0]
                return (
                  <li key={place.osm ? `${place.osm.type}${place.osm.id}` : i} {...hover(pointAt(place, existing?.id))}>
                    <span className="poi-icon" aria-hidden>
                      <Icon name={place.category} />
                    </span>
                    <span className="poi-text">
                      <strong>{name}</strong>
                      <span className="muted small">{place.display_name}</span>
                    </span>
                    {existing ? (
                      <button type="button" className="small-button" onClick={() => onSelectExisting(existing.id)}>
                        {t.pois.added}
                      </button>
                    ) : (
                      <button
                        type="button"
                        className="small-button primary"
                        disabled={create.isPending}
                        aria-label={`${t.pois.add} ${name}`}
                        onClick={() => create.mutate(placeToInput(place, name), { onSuccess: onAdded })}
                      >
                        {t.pois.add}
                      </button>
                    )}
                  </li>
                )
              })}
            </ul>
          )}
          <p className="muted tiny">{t.pois.attribution}</p>
        </>
      )}
    </section>
  )
}
