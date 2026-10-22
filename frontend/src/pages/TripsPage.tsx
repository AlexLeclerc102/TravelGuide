import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { useMe } from '../api/auth'
import { errorMessage } from '../api/errors'
import { useCreateTrip, useTrips } from '../api/trips'
import { TripForm } from '../components/trip/TripForm'
import { t } from '../i18n/fr'
import { dayCount, formatDateRange } from '../lib/format'

export function TripsPage() {
  const { data: user } = useMe()
  const { data: trips, isPending, error } = useTrips()
  const create = useCreateTrip()
  const navigate = useNavigate()
  const [showForm, setShowForm] = useState(false)
  const canCreate = user?.plan === 'organizer'

  return (
    <div className="page page-wide">
      <div className="page-header">
        <div>
          {user && <p className="kicker">{t.trips.hello(user.display_name)}</p>}
          <h1>{t.trips.title}</h1>
        </div>
        {canCreate && !showForm && (
          <button type="button" className="primary" onClick={() => setShowForm(true)}>
            <span aria-hidden="true">+ </span>
            {t.trips.create}
          </button>
        )}
      </div>

      {!canCreate && <p className="notice">{t.trips.planRequired}</p>}

      {showForm && (
        <section className="card form-card">
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
              <span className="trip-card-map" aria-hidden="true" />
              <span className="trip-card-body">
                <span className="trip-card-head">
                  <strong>{trip.name}</strong>
                  <span className={`stamp${trip.my_role === 'organizer' ? '' : ' stamp-muted'}`}>{t.roles[trip.my_role]}</span>
                </span>
                <span className="mono-data">
                  {formatDateRange(trip.start_date, trip.end_date)} · {dayCount(trip.start_date, trip.end_date)} j
                </span>
                <span className="muted small">{t.trips.members(trip.member_count)}</span>
              </span>
            </Link>
          </li>
        ))}
        {canCreate && !showForm && (
          <li>
            <button type="button" className="trip-card trip-card-new" onClick={() => setShowForm(true)}>
              <span className="trip-card-plus" aria-hidden="true">
                +
              </span>
              <strong>{t.trips.newCardTitle}</strong>
              <span className="muted small">{t.trips.newCardHint}</span>
            </button>
          </li>
        )}
      </ul>
    </div>
  )
}
