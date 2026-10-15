import type { ReactNode } from 'react'
import { Navigate, useLocation } from 'react-router-dom'
import { useMe } from '../api/auth'
import { t } from '../i18n/fr'

/** Redirige vers /login si aucune session, en mémorisant la page demandée. */
export function RequireAuth({ children }: { children: ReactNode }) {
  const { data: user, isPending } = useMe()
  const location = useLocation()
  if (isPending) return <p className="centered muted">{t.common.loading}</p>
  if (!user) {
    const next = encodeURIComponent(location.pathname + location.search)
    return <Navigate to={`/login?next=${next}`} replace />
  }
  return <>{children}</>
}
