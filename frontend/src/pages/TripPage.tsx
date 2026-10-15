import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { useMe } from '../api/auth'
import { ApiError } from '../api/client'
import { errorMessage } from '../api/errors'
import { DEFAULT_DURATION, placeToInput, reverseGeocode, usePois, type Bounds, type PoiInput } from '../api/pois'
import { useDeleteTrip, useRemoveMember, useTrip, useUpdateTrip, type Trip } from '../api/trips'
import { MapView } from '../components/MapView'
import { InvitationsPanel } from '../components/trip/InvitationsPanel'
import { MembersPanel } from '../components/trip/MembersPanel'
import { PoiPanel, type Selection } from '../components/trip/PoiPanel'
import { TripForm } from '../components/trip/TripForm'
import { t } from '../i18n/fr'
import { dayCount, formatDateRange } from '../lib/format'

type Tab = 'places' | 'members' | 'settings'

export function TripPage() {
  const { tripId = '' } = useParams()
  const { data: trip, error, isPending } = useTrip(tripId)

  if (isPending) return <p className="centered muted">{t.common.loading}</p>
  if (error || !trip) {
    return (
      <div className="page">
        <Link to="/">{t.trip.back}</Link>
        <p className="error">{errorMessage(error)}</p>
      </div>
    )
  }
  return <TripView trip={trip} />
}

function TripView({ trip }: { trip: Trip }) {
  const [tab, setTab] = useState<Tab>('places')
  const { data: pois = [] } = usePois(trip.id)
  const [selection, setSelection] = useState<Selection>(null)
  const [addMode, setAddMode] = useState(false)
  const [locating, setLocating] = useState(false)
  const [locateError, setLocateError] = useState<unknown>(null)
  const [bounds, setBounds] = useState<Bounds | null>(null)

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
    () => pois.map((p) => ({ id: p.id, name: p.name, category: p.category, lat: p.lat, lon: p.lon })),
    [pois],
  )
  const draft = selection?.kind === 'draft' ? selection.draft : null

  return (
    <div className="trip-layout">
      <aside className="trip-sidebar">
        <Link to="/">{t.trip.back}</Link>
        <header>
          <h1>{trip.name}</h1>
          <p className="muted">
            {formatDateRange(trip.start_date, trip.end_date)} · {dayCount(trip.start_date, trip.end_date)} j ·{' '}
            {trip.default_modes.map((m) => t.modes[m]).join(', ')}
          </p>
          {trip.description && <p>{trip.description}</p>}
          <p className="small">
            {t.trip.myRole} : <strong>{t.roles[trip.my_role]}</strong>
          </p>
        </header>
        <div className="tabs" role="tablist">
          {(['places', 'members', 'settings'] as Tab[]).map((k) => (
            <button key={k} type="button" role="tab" aria-selected={tab === k} onClick={() => setTab(k)}>
              {t.tabs[k]}
            </button>
          ))}
        </div>
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
      </aside>
      <div className="trip-map">
        <MapView
          markers={markers}
          selectedId={selection?.kind === 'poi' ? selection.id : null}
          draft={draft && { lat: draft.lat, lon: draft.lon }}
          addMode={addMode}
          onSelect={(id) => {
            setTab('places')
            setSelection({ kind: 'poi', id })
          }}
          onMapClick={onMapClick}
          onBoundsChange={setBounds}
        />
      </div>
    </div>
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
