import { useMe } from '../../api/auth'
import { errorMessage } from '../../api/errors'
import { useMembers, useRemoveMember, useUpdateMember, type Trip } from '../../api/trips'
import { t } from '../../i18n/fr'

export function MembersPanel({ trip }: { trip: Trip }) {
  const { data: me } = useMe()
  const { data: members } = useMembers(trip.id)
  const update = useUpdateMember(trip.id)
  const remove = useRemoveMember(trip.id)
  const isOrganizer = trip.my_role === 'organizer'
  const error = update.error ?? remove.error

  return (
    <section className="card">
      <h2>{t.trip.members}</h2>
      <ul className="member-list">
        {members?.map((m) => {
          const isMe = m.user_id === me?.id
          return (
            <li key={m.user_id}>
              <div>
                <strong>{m.display_name}</strong> {isMe && <span className="muted">{t.trip.you}</span>}
                <div className="muted small">
                  {t.roles[m.role]}
                  {m.email && ` · ${m.email}`}
                </div>
              </div>
              {isOrganizer && !isMe && (
                <div className="actions">
                  {m.role === 'viewer' && (
                    <button type="button" onClick={() => update.mutate({ userId: m.user_id, role: 'editor' })}>
                      {t.trip.makeEditor}
                    </button>
                  )}
                  {m.role === 'editor' && (
                    <button type="button" onClick={() => update.mutate({ userId: m.user_id, role: 'viewer' })}>
                      {t.trip.makeViewer}
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() =>
                      confirm(t.trip.transferConfirm(m.display_name)) &&
                      update.mutate({ userId: m.user_id, role: 'organizer' })
                    }
                  >
                    {t.trip.transfer}
                  </button>
                  <button
                    type="button"
                    className="danger-outline"
                    onClick={() => confirm(t.trip.removeConfirm(m.display_name)) && remove.mutate(m.user_id)}
                  >
                    {t.trip.remove}
                  </button>
                </div>
              )}
            </li>
          )
        })}
      </ul>
      {error && <p className="error" role="alert">{errorMessage(error)}</p>}
    </section>
  )
}
