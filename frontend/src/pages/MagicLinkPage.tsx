import { useEffect, useRef } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router-dom'
import { useVerifyMagicLink } from '../api/auth'
import { safeNext } from '../auth/safeNext'
import { t } from '../i18n/fr'

/**
 * Le lien de l'email ouvre cette page, qui échange le jeton par un POST.
 * (Un simple GET serait consommé par les scanners de liens des messageries.)
 */
export function MagicLinkPage() {
  const [params] = useSearchParams()
  const token = params.get('token')
  const next = safeNext(params.get('next'))
  const navigate = useNavigate()
  const verify = useVerifyMagicLink()
  const started = useRef(false)

  useEffect(() => {
    if (!token || started.current) return
    started.current = true
    verify.mutate(token, { onSuccess: () => navigate(next, { replace: true }) })
  }, [token, next, verify, navigate])

  if (!token || verify.isError) {
    return (
      <div className="card auth-card">
        <p className="error" role="alert">{t.magic.invalid}</p>
        <Link to="/login">{t.magic.retry}</Link>
      </div>
    )
  }
  return <p className="centered muted">{t.magic.verifying}</p>
}
