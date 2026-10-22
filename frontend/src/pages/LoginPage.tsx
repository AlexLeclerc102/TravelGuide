import { useState, type FormEvent } from 'react'
import { Link, Navigate, useNavigate, useSearchParams } from 'react-router-dom'
import { useLogin, useMe, useRequestMagicLink } from '../api/auth'
import { ApiError } from '../api/client'
import { errorMessage } from '../api/errors'
import { safeNext } from '../auth/safeNext'
import { AuthShell } from '../components/AuthShell'
import { t } from '../i18n/fr'

type Mode = 'password' | 'magic'

export function LoginPage() {
  const [params] = useSearchParams()
  const next = safeNext(params.get('next'))
  const navigate = useNavigate()
  const { data: user } = useMe()
  const [mode, setMode] = useState<Mode>('password')
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const login = useLogin()
  const magic = useRequestMagicLink()

  if (user) return <Navigate to={next} replace />

  function onSubmit(e: FormEvent) {
    e.preventDefault()
    if (mode === 'password') {
      login.mutate({ email, password }, { onSuccess: () => navigate(next, { replace: true }) })
    } else {
      magic.mutate({ email, next: next === '/' ? undefined : next })
    }
  }

  const loginError =
    login.error instanceof ApiError && login.error.status === 401
      ? t.login.invalidCredentials
      : login.error && errorMessage(login.error)

  return (
    <AuthShell>
      <h1>{t.login.title}</h1>
      <div className="tabs" role="tablist">
        <button
          role="tab"
          type="button"
          aria-selected={mode === 'password'}
          onClick={() => setMode('password')}
        >
          {t.login.withPassword}
        </button>
        <button
          role="tab"
          type="button"
          aria-selected={mode === 'magic'}
          onClick={() => setMode('magic')}
        >
          {t.login.withMagicLink}
        </button>
      </div>

      <form onSubmit={onSubmit} className="form">
        <label>
          {t.common.email}
          <input
            type="email"
            autoComplete="email"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
          />
        </label>
        {mode === 'password' && (
          <label>
            {t.common.password}
            <input
              type="password"
              autoComplete="current-password"
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
        )}

        {mode === 'password' && loginError && <p className="error" role="alert">{loginError}</p>}
        {mode === 'magic' && magic.isError && (
          <p className="error" role="alert">{errorMessage(magic.error)}</p>
        )}
        {mode === 'magic' && magic.isSuccess && (
          <p className="success" role="status">{t.login.linkSent}</p>
        )}

        <button
          type="submit"
          className="primary"
          disabled={login.isPending || magic.isPending}
        >
          {mode === 'password' ? t.login.submit : t.login.sendLink}
        </button>
      </form>
      <p className="muted">
        {t.login.noAccount} <Link to={`/register?next=${encodeURIComponent(next)}`}>{t.login.register}</Link>
      </p>
    </AuthShell>
  )
}
