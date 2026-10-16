import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { errorMessage } from '../../api/errors'
import { placeToInput, sameOsm, useCreatePoi, type Category, type Poi } from '../../api/pois'
import {
  suggestionId,
  suggestionKeys,
  useDismissSuggestion,
  useResetDismissals,
  useSuggestions,
  type Suggestion,
} from '../../api/suggestions'
import { useTripDays, type TripDay } from '../../api/transport'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { poiLinks } from '../../lib/links'
import { reasonText } from '../../lib/suggestions'
import type { MapMarker } from '../MapView'
import { CircuitsSection } from './CircuitsSection'
import { RegionsSection } from './RegionsSection'

const FILTERS: (Category | null)[] = [null, 'nature', 'sight', 'museum', 'activity', 'food']
const external = { target: '_blank', rel: 'noopener noreferrer nofollow' } as const

interface Props {
  trip: Trip
  pois: Poi[]
  /** Suggestion mise en avant (clic sur la carte). */
  focusedId: string | null
  onFocus: (id: string | null) => void
  /** Marqueurs des suggestions à afficher sur la carte. */
  onMarkersChange: (markers: MapMarker[]) => void
}

/** Lieux proposés autour des journées du voyage, avec les raisons de chaque proposition. */
export function SuggestionsPanel({ trip, pois, focusedId, onFocus, onMarkersChange }: Props) {
  const [category, setCategory] = useState<Category | null>(null)
  const { data, error, isPending } = useSuggestions(trip.id, { category })
  const { data: days = [] } = useTripDays(trip.id)
  const reset = useResetDismissals(trip.id)
  const canEdit = trip.my_role !== 'viewer'
  const list = data?.suggestions ?? []
  const discovery = data?.status === 'discovery'

  useEffect(() => {
    onMarkersChange(
      (data?.suggestions ?? []).map((s) => ({
        id: suggestionId(s),
        name: s.place.name,
        category: s.place.category,
        lat: s.place.lat,
        lon: s.place.lon,
        suggestion: true,
      })),
    )
  }, [data, onMarkersChange])
  useEffect(() => () => onMarkersChange([]), [onMarkersChange])
  // La suggestion choisie sur la carte défile jusque dans la liste.
  useEffect(() => {
    if (focusedId) document.getElementById(focusedId)?.scrollIntoView?.({ block: 'nearest', behavior: 'smooth' })
  }, [focusedId])

  return (
    <div className="poi-panel">
      <section className="card">
        <h2 className="section-title">{t.tabs.suggestions}</h2>
        <p className="small muted">{t.suggestions.intro}</p>
        <div className="chips" role="radiogroup" aria-label={t.suggestions.filterLabel}>
          {FILTERS.map((c) => (
            <button
              key={c ?? 'all'}
              type="button"
              role="radio"
              aria-checked={category === c}
              className={`chip${category === c ? ' on' : ''}`}
              onClick={() => setCategory(c)}
            >
              {t.suggestions.filters[c ?? 'all']}
            </button>
          ))}
        </div>
      </section>

      {isPending && <p className="muted small">{t.suggestions.loading}</p>}
      {error && (
        <p className="error" role="alert">
          {errorMessage(error)}
        </p>
      )}
      {data?.status === 'not_imported' && <p className="notice small">{t.suggestions.notImported}</p>}
      {data && <RegionsSection key={String(discovery)} trip={trip} pois={pois} canEdit={canEdit} defaultOpen={discovery} />}
      {data && <CircuitsSection trip={trip} pois={pois} canEdit={canEdit} />}
      {discovery && <p className="notice small">{t.suggestions.discovery}</p>}
      {data?.status === 'ready' && list.length === 0 && <p className="muted small">{t.suggestions.none}</p>}

      {list.length > 0 && (
        <ul className="suggestion-list">
          {list.map((s) => (
            <SuggestionCard
              key={suggestionId(s)}
              trip={trip}
              suggestion={s}
              pois={pois}
              days={days}
              canEdit={canEdit}
              focused={focusedId === suggestionId(s)}
              onFocus={() => onFocus(suggestionId(s))}
            />
          ))}
        </ul>
      )}
      {(data?.status === 'ready' || discovery) && (
        <p className="muted tiny">
          {t.suggestions.attribution}
          {canEdit && (
            <>
              {' · '}
              <button type="button" className="link-button tiny" disabled={reset.isPending} onClick={() => reset.mutate()}>
                {t.suggestions.reset}
              </button>
            </>
          )}
        </p>
      )}
    </div>
  )
}

interface CardProps {
  trip: Trip
  suggestion: Suggestion
  pois: Poi[]
  days: TripDay[]
  canEdit: boolean
  focused: boolean
  onFocus: () => void
}

function SuggestionCard({ trip, suggestion: s, pois, days, canEdit, focused, onFocus }: CardProps) {
  const qc = useQueryClient()
  const create = useCreatePoi(trip.id)
  const dismiss = useDismissSuggestion(trip.id)
  const added = pois.some((p) => sameOsm(p.osm, s.place.osm))
  const { place } = s
  const links = poiLinks({ ...place, address: '' }).filter((l) => l.kind === 'website' || l.kind === 'wikipedia' || l.kind === 'osm')
  const error = create.error ?? dismiss.error

  return (
    <li id={suggestionId(s)} className={`suggestion-card${focused ? ' focused' : ''}`}>
      <button type="button" className="suggestion-head" onClick={onFocus}>
        <span className="poi-icon" aria-hidden>
          {t.categoryIcons[place.category]}
        </span>
        <span className="poi-text">
          <strong>{place.name}</strong>
          <span className="muted small">{t.categories[place.category]}</span>
        </span>
      </button>
      <ul className="reasons small">
        {s.reasons.map((r, i) => (
          <li key={i}>{reasonText(r, days, pois)}</li>
        ))}
      </ul>
      <p className="small suggestion-links">
        {links.map((l, i) => (
          <span key={l.kind}>
            {i > 0 && ' · '}
            <a href={l.href} {...external}>
              {t.links.labels[l.kind]}
            </a>
          </span>
        ))}
      </p>
      {canEdit && (
        <div className="actions">
          <button
            type="button"
            className="small-button primary"
            disabled={added || create.isPending}
            aria-label={t.suggestions.addNamed(place.name)}
            onClick={() =>
              create.mutate(placeToInput(place), {
                onSuccess: () => qc.invalidateQueries({ queryKey: suggestionKeys.all(trip.id) }),
              })
            }
          >
            {added ? t.suggestions.added : t.suggestions.add}
          </button>
          {place.osm && (
            <button
              type="button"
              className="small-button"
              disabled={dismiss.isPending}
              aria-label={t.suggestions.dismissNamed(place.name)}
              onClick={() => place.osm && dismiss.mutate({ osm: place.osm, category: place.category })}
            >
              {t.suggestions.dismiss}
            </button>
          )}
        </div>
      )}
      {!!error && (
        <p className="error small" role="alert">
          {errorMessage(error)}
        </p>
      )}
    </li>
  )
}
