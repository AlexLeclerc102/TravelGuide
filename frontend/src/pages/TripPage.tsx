import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { useMe } from '../api/auth'
import { ApiError } from '../api/client'
import { errorMessage } from '../api/errors'
import { DEFAULT_DURATION, placeToInput, reverseGeocode, usePois, type Bounds, type PoiInput } from '../api/pois'
import { tripKeys, useDeleteTrip, useRemoveMember, useTrip, useUpdateTrip, type Trip } from '../api/trips'
import { Icon, type IconName } from '../components/Icon'
import { MapView, type MapMarker } from '../components/MapView'
import { createHoverStore, MapHoverProvider } from '../components/mapHover'
import { CollabBar } from '../components/trip/CollabBar'
import { InvitationsPanel } from '../components/trip/InvitationsPanel'
import { ItineraryPanel } from '../components/trip/ItineraryPanel'
import { MembersPanel } from '../components/trip/MembersPanel'
import { PoiPanel, type Selection } from '../components/trip/PoiPanel'
import { SuggestionsPanel } from '../components/trip/SuggestionsPanel'
import { TransportPanel } from '../components/trip/TransportPanel'
import { TripForm } from '../components/trip/TripForm'
import { t } from '../i18n/fr'
import { dayCount, formatDateRange } from '../lib/format'
import { useTripRealtime } from '../realtime/useTripRealtime'

type Tab = 'places' | 'suggestions' | 'itinerary' | 'transport' | 'members' | 'settings'

const TABS: Tab[] = ['places', 'suggestions', 'itinerary', 'transport', 'members', 'settings']
const TAB_ICONS: Record<Tab, IconName> = {
  places: 'pin',
  suggestions: 'idea',
  itinerary: 'calendar',
  transport: 'route',
  members: 'members',
  settings: 'settings',
}
/**
 * Largeur du panneau selon la tâche : carte visible pour choisir des lieux, panneau étendu pour
 * composer l'itinéraire et les trajets, pleine largeur quand la carte n'apporte rien.
 */
const TAB_MODES: Record<Tab, 'map' | 'wide' | 'full'> = {
  places: 'map',
  suggestions: 'map',
  itinerary: 'wide',
  transport: 'wide',
  members: 'full',
  settings: 'full',
}

export function TripPage() {
  const { tripId = '' } = useParams()
  const { data: trip, error, isPending } = useTrip(tripId)
  const qc = useQueryClient()
  // Voyage dont on vient d'être retiré (la page reste montée d'un voyage à l'autre).
  const [removedFrom, setRemovedFrom] = useState<string | null>(null)

  if (removedFrom === tripId) {
    return (
      <div className="page">
        <Link to="/">{t.trip.back}</Link>
        <p className="notice warning" role="alert">
          {t.realtime.removed}
        </p>
      </div>
    )
  }
  if (isPending) return <p className="centered muted">{t.common.loading}</p>
  if (error || !trip) {
    return (
      <div className="page">
        <Link to="/">{t.trip.back}</Link>
        <p className="error">{errorMessage(error)}</p>
      </div>
    )
  }
  return (
    <TripView
      key={trip.id}
      trip={trip}
      onRemoved={() => {
        setRemovedFrom(tripId)
        qc.removeQueries({ queryKey: tripKeys.one(tripId) })
        qc.invalidateQueries({ queryKey: tripKeys.all, exact: true })
      }}
    />
  )
}

function TripView({ trip, onRemoved }: { trip: Trip; onRemoved: () => void }) {
  const [tab, setTab] = useState<Tab>('places')
  const realtime = useTripRealtime(trip.id, onRemoved)
  const { data: pois = [] } = usePois(trip.id)
  const [selection, setSelection] = useState<Selection>(null)
  const [addMode, setAddMode] = useState(false)
  const [locating, setLocating] = useState(false)
  const [locateError, setLocateError] = useState<unknown>(null)
  const [bounds, setBounds] = useState<Bounds | null>(null)
  const [route, setRoute] = useState<[number, number][] | null>(null)
  const [suggestionMarkers, setSuggestionMarkers] = useState<MapMarker[]>([])
  const [focusedSuggestion, setFocusedSuggestion] = useState<string | null>(null)
  // « Plein écran carte » : le panneau étendu se replie.
  const [mapOnly, setMapOnly] = useState(false)
  // Élément du panneau survolé, mis en évidence sur la carte.
  const [hoverStore] = useState(createHoverStore)
  const mode = TAB_MODES[tab]
  const compact = mode !== 'map'
  const pickTab = (k: Tab) => {
    setTab(k)
    setMapOnly(false)
  }
  // L'élément survolé disparaît avec son onglet.
  useEffect(() => hoverStore.set(null), [tab, hoverStore])

  // Échap annule le mode « ajout par clic ».
  useEffect(() => {
    if (!addMode) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setAddMode(false)
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [addMode])

  const onMapClick = useCallback(async (lat: number, lon: number) => {
    setAddMode(false)
    setTab('places')
    setLocating(true)
    setLocateError(null)
    // Lieu provisoire tout de suite, enrichi si OpenStreetMap connaît un lieu à cet endroit.
    const blank: PoiInput = { name: '', category: 'other', lat, lon, duration_min: DEFAULT_DURATION.other }
    setSelection({ kind: 'draft', draft: blank })
    try {
      const { place } = await reverseGeocode(lat, lon)
      // On garde le point cliqué ; un résultat « autre » (rue, adresse) ne sert que d'adresse.
      const draft =
        place && place.category !== 'other' && place.name
          ? { ...placeToInput(place), lat, lon }
          : { ...blank, address: place?.display_name ?? '', country_code: place?.country_code }
      setSelection((cur) => (cur?.kind === 'draft' && cur.draft.lat === lat && cur.draft.lon === lon ? { kind: 'draft', draft } : cur))
    } catch (e) {
      setLocateError(e)
    } finally {
      setLocating(false)
    }
  }, [])

  const markers = useMemo(
    () => [
      ...pois.map((p) => ({ id: p.id, name: p.name, category: p.category, lat: p.lat, lon: p.lon })),
      ...(tab === 'suggestions' ? suggestionMarkers : []),
    ],
    [pois, tab, suggestionMarkers],
  )
  const draft = selection?.kind === 'draft' ? selection.draft : null

  return (
    <MapHoverProvider value={hoverStore}>
      <div className="trip-layout" data-mode={mode} data-tab={tab} data-map-only={mapOnly || undefined}>
        <aside className="trip-sidebar">
          <header className={`trip-header${compact ? ' compact' : ''}`}>
            <Link to="/" className="back-link">
              {t.trip.back}
            </Link>
            <div className="trip-title">
              <h1>{trip.name}</h1>
              <span className="stamp">
                <span className="sr-only">{t.trip.myRole} : </span>
                {t.roles[trip.my_role]}
              </span>
            </div>
            <p className="trip-meta">
              {formatDateRange(trip.start_date, trip.end_date)} · {dayCount(trip.start_date, trip.end_date)} j ·{' '}
              {trip.default_modes.map((m) => t.modes[m]).join(', ')}
            </p>
            {trip.description && !compact && <p className="small trip-description">{trip.description}</p>}
            <CollabBar {...realtime} />
            {mode === 'wide' && (
              <button type="button" className="small-button map-toggle" aria-pressed={mapOnly} onClick={() => setMapOnly(!mapOnly)}>
                <Icon name="expand" size={15} /> {mapOnly ? t.map.showPanel : t.map.fullscreen}
              </button>
            )}
            {mode === 'full' && (
              <button type="button" className="small-button map-toggle" onClick={() => pickTab('places')}>
                <Icon name="map" size={15} /> {t.map.show}
              </button>
            )}
          </header>
          <div className="tabs folder-tabs" role="tablist">
            {TABS.map((k) => (
              <button key={k} type="button" role="tab" aria-selected={tab === k} onClick={() => pickTab(k)}>
                <Icon name={TAB_ICONS[k]} size={20} />
                <span>{t.tabs[k]}</span>
              </button>
            ))}
          </div>
          <div className="trip-panel">
            {tab === 'places' && (
              <PoiPanel
                trip={trip}
                pois={pois}
                selection={selection}
                onSelect={(s) => {
                  setSelection(s)
                  setLocateError(null)
                }}
                addMode={addMode}
                onAddModeChange={setAddMode}
                locating={locating}
                locateError={locateError}
                bounds={bounds}
              />
            )}
            {tab === 'itinerary' && (
              <ItineraryPanel
                trip={trip}
                pois={pois}
                onRouteChange={setRoute}
                onSelectPoi={(id) => {
                  setTab('places')
                  setSelection({ kind: 'poi', id })
                }}
              />
            )}
            {tab === 'suggestions' && (
              <SuggestionsPanel
                trip={trip}
                pois={pois}
                focusedId={focusedSuggestion}
                onFocus={setFocusedSuggestion}
                onMarkersChange={setSuggestionMarkers}
              />
            )}
            {tab === 'transport' && <TransportPanel trip={trip} pois={pois} onRouteChange={setRoute} />}
            {tab === 'members' && (
              <>
                <MembersPanel trip={trip} />
                {trip.my_role === 'organizer' && <InvitationsPanel trip={trip} />}
              </>
            )}
            {tab === 'settings' && (
              <>
                {trip.my_role === 'organizer' && <SettingsPanel key={trip.version} trip={trip} />}
                <DangerZone trip={trip} />
              </>
            )}
          </div>
        </aside>
        <div className="trip-map">
          <ul className="map-legend" aria-hidden="true">
            <li>
              <span className="legend-stay" /> {t.map.legendStay}
            </li>
            <li>
              <span className="legend-place" /> {t.map.legendPlace}
            </li>
            {tab === 'suggestions' && (
              <li>
                <span className="legend-suggestion" /> {t.map.legendSuggestion}
              </li>
            )}
          </ul>
          <MapView
            markers={markers}
            selectedId={tab === 'suggestions' ? focusedSuggestion : selection?.kind === 'poi' ? selection.id : null}
            draft={draft && { lat: draft.lat, lon: draft.lon }}
            addMode={addMode}
            route={route}
            onSelect={(id) => {
              if (id.startsWith('s:')) {
                setFocusedSuggestion(id)
                return
              }
              setTab('places')
              setSelection({ kind: 'poi', id })
            }}
            onMapClick={onMapClick}
            onBoundsChange={setBounds}
          />
        </div>
      </div>
    </MapHoverProvider>
  )
}

function SettingsPanel({ trip }: { trip: Trip }) {
  const update = useUpdateTrip(trip.id)
  const conflict = update.error instanceof ApiError && update.error.status === 409
  return (
    <section className="card">
      <h2>{t.trip.settings}</h2>
      <TripForm
        initial={trip}
        submitLabel={t.common.save}
        pending={update.isPending}
        error={conflict ? t.trip.conflict : update.error && errorMessage(update.error)}
        onSubmit={(input) => update.mutate({ ...input, version: trip.version })}
      />
    </section>
  )
}

function DangerZone({ trip }: { trip: Trip }) {
  const navigate = useNavigate()
  const { data: me } = useMe()
  const del = useDeleteTrip(trip.id)
  const leave = useRemoveMember(trip.id)
  const error = del.error ?? leave.error
  return (
    <section className="card">
      <h2 className="danger-title">{t.trip.dangerZone}</h2>
      {trip.my_role === 'organizer' ? (
        <button
          type="button"
          className="danger"
          onClick={() => confirm(t.trip.deleteConfirm) && del.mutate(undefined, { onSuccess: () => navigate('/') })}
        >
          {t.trip.delete}
        </button>
      ) : (
        <button
          type="button"
          className="danger-outline"
          onClick={() =>
            me && confirm(t.trip.leaveConfirm) && leave.mutate(me.id, { onSuccess: () => navigate('/') })
          }
        >
          {t.trip.leave}
        </button>
      )}
      {error && <p className="error" role="alert">{errorMessage(error)}</p>}
    </section>
  )
}
