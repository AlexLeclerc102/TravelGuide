import { Link, useNavigate, useParams } from 'react-router-dom'
import { useMe } from '../api/auth'
import { ApiError } from '../api/client'
import { errorMessage } from '../api/errors'
import { useDeleteTrip, useRemoveMember, useTrip, useUpdateTrip, type Trip } from '../api/trips'
import { MapView } from '../components/MapView'
import { InvitationsPanel } from '../components/trip/InvitationsPanel'
import { MembersPanel } from '../components/trip/MembersPanel'
import { TripForm } from '../components/trip/TripForm'
import { t } from '../i18n/fr'
import { dayCount, formatDateRange } from '../lib/format'

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
        <MembersPanel trip={trip} />
        {trip.my_role === 'organizer' && <InvitationsPanel trip={trip} />}
        {trip.my_role === 'organizer' && <SettingsPanel key={trip.version} trip={trip} />}
        <DangerZone trip={trip} />
      </aside>
      <div className="trip-map">
        <MapView />
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
