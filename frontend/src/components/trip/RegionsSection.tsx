import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { errorMessage } from '../../api/errors'
import { placeToInput, sameOsm, useCreatePoi, type Poi } from '../../api/pois'
import { suggestionKeys, useRegions, type Region } from '../../api/suggestions'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { Icon } from '../Icon'
import { pointAt, useHoverStore, useMapHover, type MapHover } from '../mapHover'
import { poiLinks } from '../../lib/links'
import { PlaceWikiCard } from './WikiCard'

const external = { target: '_blank', rel: 'noopener noreferrer nofollow' } as const

interface Props {
  trip: Trip
  pois: Poi[]
  canEdit: boolean
  /** Ouvert d'emblée (voyage encore vide). */
  defaultOpen: boolean
}

/** Régions du pays à découvrir, pour choisir où aller avant de choisir quoi visiter. */
export function RegionsSection({ trip, pois, canEdit, defaultOpen }: Props) {
  const [open, setOpen] = useState(defaultOpen)
  const { data, error, isPending } = useRegions(trip.id, open)
  const regions = data?.regions ?? []

  return (
    <details className="card regions" open={defaultOpen} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary className="section-title">{t.regions.title}</summary>
      <p className="small muted">{t.regions.intro}</p>
      {open && isPending && <p className="muted small">{t.suggestions.loading}</p>}
      {error && (
        <p className="error small" role="alert">
          {errorMessage(error)}
        </p>
      )}
      {data?.status === 'not_imported' && <p className="notice small">{t.regions.notImported}</p>}
      {data?.status === 'ready' && regions.length === 0 && <p className="muted small">{t.regions.none}</p>}
      {regions.length > 0 && (
        <ul className="region-list">
          {regions.map((r) => (
            <RegionCard key={`${r.name}:${r.lat}`} trip={trip} region={r} pois={pois} canEdit={canEdit} />
          ))}
        </ul>
      )}
      {data?.status === 'ready' && <p className="muted tiny">{t.regions.attribution}</p>}
    </details>
  )
}

function RegionCard({ trip, region: r, pois, canEdit }: { trip: Trip; region: Region; pois: Poi[]; canEdit: boolean }) {
  const qc = useQueryClient()
  const create = useCreatePoi(trip.id)
  const [adding, setAdding] = useState(false)
  const missing = r.highlights.filter((h) => !pois.some((p) => sameOsm(p.osm, h.osm)))

  async function addHighlights() {
    setAdding(true)
    try {
      // Un par un : chaque ajout est validé (limite de lieux, doublons) par le serveur.
      for (const place of missing) await create.mutateAsync(placeToInput(place))
    } catch {
      // Erreur affichée via `create.error`.
    } finally {
      setAdding(false)
      qc.invalidateQueries({ queryKey: suggestionKeys.all(trip.id) })
    }
  }

  const hover = useMapHover()
  const store = useHoverStore()
  const [expanded, setExpanded] = useState(false)
  const area: MapHover = { kind: 'area', lat: r.lat, lon: r.lon, radiusM: r.radius_m }
  const detailsId = `region-${r.osm ? `${r.osm.type}-${r.osm.id}` : r.name.replace(/\W+/g, '-')}`

  function toggle() {
    setExpanded(!expanded)
    if (!expanded) store.focus(area)
  }

  return (
    <li className={`region-card${expanded ? ' expanded' : ''}`} {...hover(area)}>
      <div className="region-head">
        <button
          type="button"
          className="region-toggle"
          aria-expanded={expanded}
          aria-controls={detailsId}
          title={t.regions.details(r.name)}
          onClick={toggle}
        >
          <strong>{r.name}</strong>
        </button>
        <span className="muted small">
          {r.access === 'boat_or_plane' ? t.regions.expedition : t.regions.nights(r.nights)} ·{' '}
          {r.themes.map((c) => t.categories[c]).join(', ')}
        </span>
        {r.wikivoyage && (
          <span className="tag" title={t.regions.wikivoyageTitle(r.name)}>
            {t.regions.wikivoyage}
          </span>
        )}
        {r.in_trip > 0 && <span className="tag tag-in-trip">{t.regions.inTrip(r.in_trip)}</span>}
      </div>
      {expanded && (
        <div id={detailsId} className="region-details">
          <PlaceWikiCard tripId={trip.id} wiki={r} />
        </div>
      )}
      {r.access === 'boat_or_plane' && <p className="notice small">{t.regions.boatOrPlane}</p>}
      {r.world_heritage.length > 0 && (
        <p className="small">{t.regions.worldHeritage(r.world_heritage.join(', '))}</p>
      )}
      {expanded && <h3 className="small region-subtitle">{t.regions.highlights}</h3>}
      <ul className={`small region-highlights${expanded ? ' rich' : ''}`}>
        {r.highlights.map((h) => {
          const wiki = poiLinks({ ...h, address: '' }).find((l) => l.kind === 'wikipedia')
          return (
            <li key={h.osm ? `${h.osm.type}/${h.osm.id}` : h.name} {...hover(pointAt(h), area)}>
              <span className="region-highlight-name">
                <Icon name={h.category} size={14} />{' '}
                {wiki ? (
                  <a href={wiki.href} {...external}>
                    {h.name}
                  </a>
                ) : (
                  h.name
                )}
              </span>
              {expanded && <PlaceWikiCard tripId={trip.id} wiki={h} compact />}
            </li>
          )
        })}
      </ul>
      {r.place_count > r.highlights.length && (
        <p className="muted tiny">{t.regions.more(r.place_count - r.highlights.length)}</p>
      )}
      {canEdit && (
        <div className="actions">
          <button
            type="button"
            className="small-button primary"
            disabled={adding || missing.length === 0}
            aria-label={t.regions.addNamed(r.name)}
            onClick={addHighlights}
          >
            {adding ? t.regions.adding : missing.length === 0 ? t.regions.added : t.regions.add(missing.length)}
          </button>
        </div>
      )}
      {!!create.error && (
        <p className="error small" role="alert">
          {errorMessage(create.error)}
        </p>
      )}
    </li>
  )
}
