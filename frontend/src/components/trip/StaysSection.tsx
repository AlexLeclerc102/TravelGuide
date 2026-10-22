import { useState, type FormEvent } from 'react'
import { errorMessage } from '../../api/errors'
import { placeToInput, useCreatePoi, usePlaceSearch, type Place, type Poi } from '../../api/pois'
import { useSuggestions } from '../../api/suggestions'
import { useReplaceStay, useSetStay, type TripDay } from '../../api/transport'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { Icon } from '../Icon'
import { pointAt, useMapHover } from '../mapHover'
import { formatDate } from '../../lib/format'
import { lodgingSearchLinks, placeQuery } from '../../lib/links'
import { stays, type Stay } from '../../lib/stays'
import { StayProposal } from './StayProposal'

const short = (date: string) => formatDate(date, { weekday: 'short', day: 'numeric', month: 'short' })
const external = { target: '_blank', rel: 'noopener noreferrer nofollow' } as const

interface Props {
  trip: Trip
  days: TripDay[]
  pois: Poi[]
  canEdit: boolean
}

/** Hébergements par suite de nuits : zones à préciser, recherche d'hôtel, remplacement. */
export function StaysSection({ trip, days, pois, canEdit }: Props) {
  const byId = new Map(pois.map((p) => [p.id, p]))
  // Un hébergement supprimé entre-temps compte comme une nuit sans hébergement.
  const list = stays(days.map((d) => ({ ...d, base_poi_id: d.base_poi_id && byId.has(d.base_poi_id) ? d.base_poi_id : null })))

  return (
    <section className="card">
      <h2 className="section-title">{t.stays.title}</h2>
      <p className="small muted">{t.stays.hint}</p>
      <StayProposal trip={trip} pois={pois} canEdit={canEdit} />
      <ul className="stay-list">
        {list.map((stay) => (
          <StayRow key={stay.firstNight} trip={trip} stay={stay} poi={stay.poiId ? byId.get(stay.poiId) : undefined} pois={pois} canEdit={canEdit} />
        ))}
      </ul>
      {canEdit && <AddStay trip={trip} days={days} pois={pois} />}
    </section>
  )
}

function StayRow({ trip, stay, poi, pois, canEdit }: { trip: Trip; stay: Stay; poi?: Poi; pois: Poi[]; canEdit: boolean }) {
  const hover = useMapHover()
  const replace = useReplaceStay(trip.id)
  const hotels = pois.filter((p) => p.category === 'lodging' && !p.placeholder && p.id !== poi?.id)
  // Recherche autour de la zone ou de l'hôtel choisi, pour ces dates et tout le groupe.
  const search = poi && lodgingSearchLinks({ query: placeQuery(poi), checkin: stay.checkin, checkout: stay.checkout, adults: trip.member_count })

  return (
    <li className={`stay-row${poi?.placeholder ? ' area' : ''}`} {...hover(poi ? pointAt(poi, poi.id) : null)}>
      <div className="stay-head">
        <strong>
          <Icon name="lodging" /> {poi ? poi.name : t.stays.none}
          {poi?.placeholder && <span className="tag tag-area"> {t.stays.area}</span>}
        </strong>
        <span className="small muted">
          {t.stays.nights(stay.nights)} · {t.stays.range(short(stay.firstNight), short(stay.lastNight))}
        </span>
      </div>
      {search && (
        <p className="small stay-links">
          {t.stays.searchOn}{' '}
          <a href={search.booking} {...external}>
            Booking.com
          </a>
          {' · '}
          <a href={search.airbnb} {...external}>
            Airbnb
          </a>
        </p>
      )}
      {canEdit && poi?.placeholder && (
        hotels.length > 0 ? (
          <label className="small">
            {t.stays.replace}{' '}
            <select
              aria-label={t.stays.replaceOf(poi.name)}
              value=""
              disabled={replace.isPending}
              onChange={(e) => e.target.value && replace.mutate({ from_poi_id: poi.id, to_poi_id: e.target.value, remove_from: true })}
            >
              <option value="">{t.stays.choose}</option>
              {hotels.map((h) => (
                <option key={h.id} value={h.id}>
                  {h.name}
                </option>
              ))}
            </select>
          </label>
        ) : (
          <p className="tiny muted">{t.stays.replaceHint}</p>
        )
      )}
      {replace.error && (
        <p className="error small" role="alert">
          {errorMessage(replace.error)}
        </p>
      )}
      {poi?.placeholder && <AreaHotels trip={trip} area={poi} canEdit={canEdit} />}
    </li>
  )
}

/** Hébergements OSM autour d'une zone ; « Choisir » l'ajoute et remplace la zone. */
function AreaHotels({ trip, area, canEdit }: { trip: Trip; area: Poi; canEdit: boolean }) {
  const hover = useMapHover()
  const [open, setOpen] = useState(false)
  const { data, error, isPending } = useSuggestions(trip.id, { lodgingNear: area.id }, open)
  const create = useCreatePoi(trip.id)
  const replace = useReplaceStay(trip.id)
  const busy = create.isPending || replace.isPending
  const hotels = (data?.suggestions ?? []).slice(0, 6)
  const failure = error ?? create.error ?? replace.error

  function choose(place: Place) {
    create.mutate(placeToInput(place), {
      onSuccess: (hotel) => replace.mutate({ from_poi_id: area.id, to_poi_id: hotel.id, remove_from: true }),
    })
  }

  return (
    <details className="area-hotels" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary className="small">{t.suggestions.hotelsInArea}</summary>
      {open && isPending && <p className="muted small">{t.suggestions.loading}</p>}
      {data && hotels.length === 0 && <p className="muted small">{t.suggestions.noHotels}</p>}
      {hotels.length > 0 && (
        <ul className="result-list">
          {hotels.map(({ place, travel_min }) => (
            <li key={`${place.osm?.type}${place.osm?.id}`} {...hover(pointAt(place))}>
              <span className="poi-text">
                <strong>{place.name}</strong>
                <span className="muted small">
                  ≈ {travel_min} min
                  {place.website && (
                    <>
                      {' · '}
                      <a href={place.website} {...external}>
                        {t.links.labels.website}
                      </a>
                    </>
                  )}
                </span>
              </span>
              {canEdit && (
                <button
                  type="button"
                  className="small-button primary"
                  disabled={busy}
                  aria-label={t.suggestions.chooseNamed(place.name)}
                  onClick={() => choose(place)}
                >
                  {t.suggestions.choose}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {!!failure && (
        <p className="error small" role="alert">
          {errorMessage(failure)}
        </p>
      )}
    </details>
  )
}

/** Nouveau séjour : un hébergement du voyage, ou une ville (zone à préciser), et ses nuits. */
function AddStay({ trip, days, pois }: { trip: Trip; days: TripDay[]; pois: Poi[] }) {
  const hover = useMapHover()
  const lodging = pois.filter((p) => p.category === 'lodging')
  const [poiId, setPoiId] = useState('')
  const [town, setTown] = useState<Place | null>(null)
  const [text, setText] = useState('')
  const [query, setQuery] = useState('')
  // Les jours peuvent arriver après le premier affichage : par défaut, la première nuit.
  const [firstChoice, setFirst] = useState('')
  const [lastChoice, setLast] = useState('')
  const first = firstChoice || days[0]?.date || ''
  const last = lastChoice && lastChoice >= first ? lastChoice : first
  const search = usePlaceSearch(query, null)
  const create = useCreatePoi(trip.id)
  const setStay = useSetStay(trip.id)
  const error = create.error ?? setStay.error
  const busy = create.isPending || setStay.isPending
  const ready = (poiId || town) && first && last && first <= last

  async function submit(e: FormEvent) {
    e.preventDefault()
    if (!ready) return
    let id = poiId
    if (!id && town) {
      const name = town.name || town.display_name.split(',')[0]
      let area: Poi
      try {
        area = await create.mutateAsync({
        name,
        category: 'lodging',
        placeholder: true,
        lat: town.lat,
        lon: town.lon,
        address: town.display_name,
        duration_min: 0,
        country_code: town.country_code,
        osm: null,
        })
      } catch {
        return // erreur affichée sous le formulaire
      }
      id = area.id
    }
    setStay.mutate(
      { poi_id: id, first_night: first, last_night: last },
      {
        onSuccess: () => {
          setPoiId('')
          setTown(null)
          setText('')
          setQuery('')
        },
      },
    )
  }

  return (
    <details className="add-stay">
      <summary>{t.stays.add}</summary>
      <form className="form" onSubmit={submit} aria-label={t.stays.add}>
        {lodging.length > 0 && (
          <label>
            {t.stays.existing}
            <select
              value={poiId}
              onChange={(e) => {
                setPoiId(e.target.value)
                setTown(null)
              }}
            >
              <option value="">{t.stays.choose}</option>
              {lodging.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                  {p.placeholder ? ` (${t.stays.area})` : ''}
                </option>
              ))}
            </select>
          </label>
        )}
        <div className="row">
          <label>
            {t.stays.searchTown}
            <input type="search" value={text} onChange={(e) => setText(e.target.value)} placeholder={t.stays.searchTownPlaceholder} maxLength={200} />
          </label>
          <button type="button" disabled={text.trim().length < 2 || search.isFetching} onClick={() => setQuery(text.trim())}>
            {search.isFetching ? t.pois.searching : t.pois.search}
          </button>
        </div>
        {search.error && <p className="error small">{errorMessage(search.error)}</p>}
        {search.data && !town && (
          search.data.length === 0 ? (
            <p className="muted small">{t.pois.noResults}</p>
          ) : (
            <ul className="result-list" aria-label={t.stays.searchTown}>
              {search.data.map((place, i) => (
                <li key={place.osm ? `${place.osm.type}${place.osm.id}` : i} {...hover(pointAt(place))}>
                  <span className="poi-text">
                    <strong>{place.name || place.display_name.split(',')[0]}</strong>
                    <span className="muted small">{place.display_name}</span>
                  </span>
                  <button
                    type="button"
                    className="small-button"
                    onClick={() => {
                      setTown(place)
                      setPoiId('')
                    }}
                  >
                    {t.stays.pickTown}
                  </button>
                </li>
              ))}
            </ul>
          )
        )}
        {town && (
          <p className="small">
            <Icon name="lodging" /> <strong>{town.name || town.display_name.split(',')[0]}</strong> <span className="tag tag-area">{t.stays.area}</span>
          </p>
        )}
        <div className="row">
          <label>
            {t.stays.firstNight}
            <select value={first} onChange={(e) => setFirst(e.target.value)}>
              {days.map((d) => (
                <option key={d.date} value={d.date}>
                  {short(d.date)}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t.stays.lastNight}
            <select value={last} onChange={(e) => setLast(e.target.value)}>
              {days.map((d) => (
                <option key={d.date} value={d.date} disabled={d.date < first}>
                  {short(d.date)}
                </option>
              ))}
            </select>
          </label>
        </div>
        {!!error && (
          <p className="error small" role="alert">
            {errorMessage(error)}
          </p>
        )}
        <div className="actions">
          <button type="submit" className="primary" disabled={!ready || busy}>
            {t.stays.save}
          </button>
        </div>
      </form>
    </details>
  )
}
