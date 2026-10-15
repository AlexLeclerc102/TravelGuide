import { useState, type FormEvent } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  downloadExport,
  useChangePassword,
  useDeleteAccount,
  useMe,
  useUpdateMe,
  type User,
} from '../api/auth'
import { errorMessage } from '../api/errors'
import { t } from '../i18n/fr'

export function AccountPage() {
  const { data: user } = useMe()
  if (!user) return null
  return (
    <div className="page">
      <h1>{t.account.title}</h1>
      <ProfileSection user={user} />
      <PasswordSection user={user} />
      <DataSection user={user} />
    </div>
  )
}

function ProfileSection({ user }: { user: User }) {
  const [name, setName] = useState(user.display_name)
  const update = useUpdateMe()
  return (
    <section className="card">
      <h2>{t.account.profile}</h2>
      <form
        className="form"
        onSubmit={(e: FormEvent) => {
          e.preventDefault()
          update.mutate({ display_name: name })
        }}
      >
        <label>
          {t.common.email}
          <input value={user.email} disabled />
        </label>
        <label>
          {t.common.displayName}
          <input required maxLength={80} value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <p>
          {t.account.plan} : <strong>{t.account.plans[user.plan] ?? user.plan}</strong>
        </p>
        {update.isError && <p className="error" role="alert">{errorMessage(update.error)}</p>}
        {update.isSuccess && <p className="success" role="status">{t.account.saved}</p>}
        <button type="submit" className="primary" disabled={update.isPending}>
          {t.common.save}
        </button>
      </form>
    </section>
  )
}

function PasswordSection({ user }: { user: User }) {
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const change = useChangePassword()
  return (
    <section className="card">
      <h2>{t.account.passwordSection}</h2>
      {!user.has_password && <p className="muted">{t.account.noPasswordYet}</p>}
      <form
        className="form"
        onSubmit={(e: FormEvent) => {
          e.preventDefault()
          change.mutate(
            { current_password: user.has_password ? current : undefined, new_password: next },
            {
              onSuccess: () => {
                setCurrent('')
                setNext('')
              },
            },
          )
        }}
      >
        {user.has_password && (
          <label>
            {t.account.currentPassword}
            <input
              type="password"
              required
              autoComplete="current-password"
              value={current}
              onChange={(e) => setCurrent(e.target.value)}
            />
          </label>
        )}
        <label>
          {t.account.newPassword}
          <input
            type="password"
            required
            minLength={10}
            maxLength={128}
            autoComplete="new-password"
            value={next}
            onChange={(e) => setNext(e.target.value)}
          />
          <small className="muted">{t.register.passwordHint}</small>
        </label>
        {change.isError && <p className="error" role="alert">{errorMessage(change.error)}</p>}
        {change.isSuccess && <p className="success" role="status">{t.account.passwordChanged}</p>}
        <button type="submit" className="primary" disabled={change.isPending}>
          {user.has_password ? t.account.changePassword : t.account.setPassword}
        </button>
      </form>
    </section>
  )
}

function DataSection({ user }: { user: User }) {
  const navigate = useNavigate()
  const remove = useDeleteAccount()
  const [confirmEmail, setConfirmEmail] = useState('')
  const [password, setPassword] = useState('')
  const [exportError, setExportError] = useState<string | null>(null)

  return (
    <section className="card">
      <h2>{t.account.dataSection}</h2>
      <p className="muted">{t.account.dataIntro}</p>
      <button
        type="button"
        onClick={() => downloadExport().catch((e) => setExportError(errorMessage(e)))}
      >
        {t.account.export}
      </button>
      {exportError && <p className="error" role="alert">{exportError}</p>}

      <h3 className="danger-title">{t.account.deleteTitle}</h3>
      <p className="muted">{t.account.deleteWarning}</p>
      <form
        className="form"
        onSubmit={(e: FormEvent) => {
          e.preventDefault()
          remove.mutate(
            { confirm_email: confirmEmail, password: user.has_password ? password : undefined },
            { onSuccess: () => navigate('/login', { replace: true }) },
          )
        }}
      >
        <label>
          {t.account.confirmEmail}
          <input type="email" required value={confirmEmail} onChange={(e) => setConfirmEmail(e.target.value)} />
        </label>
        {user.has_password && (
          <label>
            {t.common.password}
            <input
              type="password"
              required
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </label>
        )}
        {remove.isError && <p className="error" role="alert">{errorMessage(remove.error)}</p>}
        <button
          type="submit"
          className="danger"
          disabled={remove.isPending || confirmEmail.trim().toLowerCase() !== user.email}
        >
          {t.account.deleteSubmit}
        </button>
      </form>
    </section>
  )
}
