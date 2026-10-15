import { useMe } from '../../api/auth'
import { describeChange, type LastChange, type Person, type RealtimeStatus } from '../../realtime/useTripRealtime'
import { t } from '../../i18n/fr'

interface Props {
  status: RealtimeStatus
  online: Person[]
  lastChange: LastChange | null
}

/** État de la collaboration : connexion, autres membres en ligne, dernière modification reçue. */
export function CollabBar({ status, online, lastChange }: Props) {
  const { data: me } = useMe()
  const others = online.filter((p) => p.user_id !== me?.id)
  const label = status === 'online' ? t.realtime.live : status === 'connecting' ? t.realtime.connecting : t.realtime.offline
  return (
    <div className="collab-bar small" aria-live="polite">
      <p>
        <span className={`status-dot ${status}`} aria-hidden="true" /> {label}
        {status === 'online' && (
          <span className="muted">
            {' · '}
            {others.length > 0 ? t.realtime.online(others.map((p) => p.name).join(', ')) : t.realtime.alone}
          </span>
        )}
      </p>
      {lastChange && lastChange.by.user_id !== me?.id && <p className="muted">{describeChange(lastChange)}</p>}
    </div>
  )
}
