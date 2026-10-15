import { useState, type FormEvent } from 'react'
import { Link, Navigate, useNavigate, useSearchParams } from 'react-router-dom'
import { useMe, useRegister } from '../api/auth'
import { errorMessage } from '../api/errors'
import { safeNext } from '../auth/safeNext'
import { t } from '../i18n/fr'

export function RegisterPage() {
  const [params] = useSearchParams()
  const next = safeNext(params.get('next'))
  const navigate = useNavigate()
  const { data: user } = useMe()
  const register = useRegister()
  const [form, setForm] = useState({ email: '', password: '', display_name: '' })

  if (user) return <Navigate to={next} replace />

  function onSubmit(e: FormEvent) {
    e.preventDefault()
    register.mutate(form, { onSuccess: () => navigate(next, { replace: true }) })
  }

  const set = (key: keyof typeof form) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm({ ...form, [key]: e.target.value })

  return (
    <div className="card auth-card">
      <h1>{t.register.title}</h1>
      <form onSubmit={onSubmit} className="form">
        <label>
          {t.common.displayName}
          <input required maxLength={80} autoComplete="nickname" value={form.display_name} onChange={set('display_name')} />
        </label>
        <label>
          {t.common.email}
          <input type="email" required autoComplete="email" value={form.email} onChange={set('email')} />
        </label>
        <label>
          {t.common.password}
          <input
            type="password"
            required
            minLength={10}
            maxLength={128}
            autoComplete="new-password"
            value={form.password}
            onChange={set('password')}
          />
          <small className="muted">{t.register.passwordHint}</small>
        </label>
        {register.isError && <p className="error" role="alert">{errorMessage(register.error)}</p>}
        <button type="submit" className="primary" disabled={register.isPending}>
          {t.register.submit}
        </button>
      </form>
      <p className="muted">
        {t.register.haveAccount} <Link to={`/login?next=${encodeURIComponent(next)}`}>{t.register.login}</Link>
      </p>
    </div>
  )
}
