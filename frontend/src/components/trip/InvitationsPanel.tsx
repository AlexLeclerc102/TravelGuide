import { useState, type FormEvent } from 'react'
import { errorMessage } from '../../api/errors'
import {
  invitationStatus,
  useCreateInvitation,
  useInvitations,
  useRevokeInvitation,
  type Role,
  type Trip,
} from '../../api/trips'
import { t } from '../../i18n/fr'
import { formatDate } from '../../lib/format'

export function InvitationsPanel({ trip }: { trip: Trip }) {
  const { data: invitations } = useInvitations(trip.id, true)
  const create = useCreateInvitation(trip.id)
  const revoke = useRevokeInvitation(trip.id)
  const [role, setRole] = useState<Role>('editor')
  const [days, setDays] = useState(7)
  const [maxUses, setMaxUses] = useState('')
  const [copied, setCopied] = useState(false)

  function onSubmit(e: FormEvent) {
    e.preventDefault()
    setCopied(false)
    create.mutate({
      role,
      expires_in_days: days,
      max_uses: maxUses ? Number(maxUses) : undefined,
    })
  }

  const link = create.data?.url

  return (
    <section className="card">
      <h2>{t.trip.invitations}</h2>
      <p className="muted">{t.invitations.intro}</p>
      <form className="form" onSubmit={onSubmit}>
        <div className="row">
          <label>
            {t.invitations.role}
            <select value={role} onChange={(e) => setRole(e.target.value as Role)}>
              <option value="editor">{t.roles.editor}</option>
              <option value="viewer">{t.roles.viewer}</option>
            </select>
          </label>
          <label>
            {t.invitations.expiresInDays}
            <input type="number" min={1} max={30} value={days} onChange={(e) => setDays(Number(e.target.value))} />
          </label>
          <label>
            {t.invitations.maxUses}
            <input type="number" min={1} max={100} value={maxUses} onChange={(e) => setMaxUses(e.target.value)} />
          </label>
        </div>
        {create.isError && <p className="error" role="alert">{errorMessage(create.error)}</p>}
        <button type="submit" className="primary" disabled={create.isPending}>
          {t.invitations.create}
        </button>
      </form>

      {link && (
        <div className="invite-link">
          <p>{t.invitations.newLink}</p>
          <div className="row">
            <input readOnly value={link} aria-label={t.invitations.newLink} onFocus={(e) => e.target.select()} />
            <button
              type="button"
              onClick={() => navigator.clipboard?.writeText(link).then(() => setCopied(true))}
            >
              {copied ? t.invitations.copied : t.invitations.copy}
            </button>
          </div>
        </div>
      )}

      {invitations?.length === 0 && <p className="muted">{t.invitations.none}</p>}
      <ul className="member-list">
        {invitations?.map((inv) => {
          const status = invitationStatus(inv)
          return (
            <li key={inv.id}>
              <div>
                <strong>{t.roles[inv.role]}</strong>{' '}
                <span className={`tag tag-${status}`}>{t.invitations.status[status]}</span>
                <div className="muted small">
                  {t.invitations.uses(inv.uses, inv.max_uses)} ·{' '}
                  {t.invitations.expires(formatDate(inv.expires_at))}
                </div>
              </div>
              {status === 'active' && (
                <button type="button" className="danger-outline" onClick={() => revoke.mutate(inv.id)}>
                  {t.invitations.revoke}
                </button>
              )}
            </li>
          )
        })}
      </ul>
    </section>
  )
}
