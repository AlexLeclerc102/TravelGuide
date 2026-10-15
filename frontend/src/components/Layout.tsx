import type { ReactNode } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { useLogout, useMe } from '../api/auth'
import { t } from '../i18n/fr'
import { HealthBadge } from './HealthBadge'

export function Layout({ children }: { children: ReactNode }) {
  const { data: user } = useMe()
  const logout = useLogout()
  const navigate = useNavigate()

  return (
    <div className="layout">
      <header className="topbar">
        <Link to="/" className="brand">
          {t.appName}
        </Link>
        <span className="muted hide-mobile">{t.tagline}</span>
        <HealthBadge />
        {user && (
          <nav className="topnav">
            <Link to="/account">{user.display_name}</Link>
            <button
              type="button"
              className="link-button"
              onClick={() => logout.mutate(undefined, { onSuccess: () => navigate('/login') })}
            >
              {t.nav.logout}
            </button>
          </nav>
        )}
      </header>
      <main className="main">{children}</main>
    </div>
  )
}
