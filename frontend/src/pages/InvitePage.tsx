import { useState, type FormEvent } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { useMe, useRequestMagicLink } from '../api/auth'
import { ApiError } from '../api/client'
import { errorMessage } from '../api/errors'
import { useAcceptInvitation, useInvitationPreview } from '../api/trips'
import { AuthShell } from '../components/AuthShell'
import { t } from '../i18n/fr'
import { formatDateRange } from '../lib/format'

/** Page publique ouverte par un lien d'invitation. */
export function InvitePage() {
  const { token = '' } = useParams()
  const navigate = useNavigate()
  const { data: me, isPending: mePending } = useMe()
  const preview = useInvitationPreview(token)
  const accept = useAcceptInvitation(token)
  const magic = useRequestMagicLink()
  const [name, setName] = useState('')
  const [email, setEmail] = useState('')

  const next = `/invite/${token}`
  const goToTrip = (res: { trip_id: string }) => navigate(`/trips/${res.trip_id}`, { replace: true })
  const accountExists = accept.error instanceof ApiError && accept.error.code === 'account_exists'

  if (preview.isPending || mePending) return <p className="centered muted">{t.common.loading}</p>
  if (preview.isError) {
    return (
      <AuthShell perks>
        <p className="error" role="alert">{t.invite.invalid}</p>
      </AuthShell>
    )
  }

  const inv = preview.data
  return (
    <AuthShell perks>
      <p className="invite-banner">{inv.invited_by ? t.invite.invitedBy(inv.invited_by) : t.invite.invited}</p>
      <h1>{inv.trip_name}</h1>
      <p className="muted">
        {formatDateRange(inv.start_date, inv.end_date)} · {t.invite.asRole(t.roles[inv.role])}
      </p>

      {me ? (
        <button type="button" className="primary" disabled={accept.isPending} onClick={() => accept.mutate({}, { onSuccess: goToTrip })}>
          {t.invite.join}
        </button>
      ) : (
        <form
          className="form"
          onSubmit={(e: FormEvent) => {
            e.preventDefault()
            accept.mutate({ display_name: name, email }, { onSuccess: goToTrip })
          }}
        >
          <p className="muted">{t.invite.guestIntro}</p>
          <label>
            {t.common.displayName}
            <input required maxLength={80} autoComplete="given-name" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label>
            {t.common.email}
            <input type="email" required autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} />
          </label>
          {!accountExists && (
            <button type="submit" className="primary" disabled={accept.isPending}>
              {t.invite.join}
            </button>
          )}
        </form>
      )}

      {accountExists ? (
        <div className="form">
          <p role="alert">{t.invite.accountExists}</p>
          {magic.isSuccess ? (
            <p className="success" role="status">{t.invite.linkSent}</p>
          ) : (
            <button type="button" className="primary" disabled={magic.isPending} onClick={() => magic.mutate({ email, next })}>
              {t.invite.sendLink}
            </button>
          )}
          <Link to={`/login?next=${encodeURIComponent(next)}`}>{t.invite.orLogin}</Link>
        </div>
      ) : (
        accept.isError && <p className="error" role="alert">{errorMessage(accept.error)}</p>
      )}
    </AuthShell>
  )
}
