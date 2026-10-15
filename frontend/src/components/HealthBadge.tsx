import { useHealth } from '../api/health'
import { t } from '../i18n/fr'

export function HealthBadge() {
  const { data, isPending, isError } = useHealth()
  if (isPending) return <span className="badge">{t.health.checking}</span>
  if (isError || data.status !== 'ok') {
    return <span className="badge badge-error">{t.health.down}</span>
  }
  return <span className="badge badge-ok">{t.health.ok} · v{data.version}</span>
}
