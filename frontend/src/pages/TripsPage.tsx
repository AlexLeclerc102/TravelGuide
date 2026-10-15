import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { useMe } from '../api/auth'
import { errorMessage } from '../api/errors'
import { useCreateTrip, useTrips } from '../api/trips'
import { TripForm } from '../components/trip/TripForm'
import { t } from '../i18n/fr'
import { formatDateRange } from '../lib/format'

export function TripsPage() {
  const { data: user } = useMe()
  const { data: trips, isPending, error } = useTrips()
  const create = useCreateTrip()
  const navigate = useNavigate()
  const [showForm, setShowForm] = useState(false)
  const canCreate = user?.plan === 'organizer'

  return (
    <div className="page">
      <div className="page-header">
        <h1>{t.trips.title}</h1>
        {canCreate && !showForm && (
          <button type="button" className="primary" onClick={() => setShowForm(true)}>
            {t.trips.create}
          </button>
        )}
      </div>

      {!canCreate && <p className="notice">{t.trips.planRequired}</p>}

      {showForm && (
        <section className="card">
          <h2>{t.trips.create}</h2>
          <TripForm
            submitLabel={t.trips.createSubmit}
            pending={create.isPending}
            error={create.error && errorMessage(create.error)}
            onSubmit={(input) =>
              create.mutate(input, { onSuccess: (trip) => navigate(`/trips/${trip.id}`) })
            }
          />
        </section>
      )}

      {isPending && <p className="muted">{t.common.loading}</p>}
      {error && <p className="error">{errorMessage(error)}</p>}
      {trips?.length === 0 && <p className="muted">{t.trips.empty}</p>}
      <ul className="trip-list">
        {trips?.map((trip) => (
          <li key={trip.id}>
            <Link to={`/trips/${trip.id}`} className="card trip-card">
              <strong>{trip.name}</strong>
              <span className="muted">{formatDateRange(trip.start_date, trip.end_date)}</span>
              <span className="muted">
                {t.roles[trip.my_role]} · {t.trips.members(trip.member_count)}
              </span>
            </Link>
          </li>
        ))}
      </ul>
    </div>
  )
}
