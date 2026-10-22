import type { Poi } from '../../api/pois'
import { t } from '../../i18n/fr'
import { Icon } from '../Icon'
import { poiLinks } from '../../lib/links'

/** Liens d'un lieu (site, contact, Wikipédia, cartes), visibles par tous les membres. */
export function PoiLinks({ poi }: { poi: Poi }) {
  return (
    <nav className="poi-links" aria-label={t.links.title}>
      <ul>
        {poiLinks(poi).map((link) => {
          const external = link.href.startsWith('http')
          return (
            <li key={link.kind}>
              <a
                href={link.href}
                {...(external && { target: '_blank', rel: 'noopener noreferrer nofollow' })}
              >
                <Icon name={link.kind} size={14} /> {link.value ?? t.links.labels[link.kind]}
              </a>
            </li>
          )
        })}
      </ul>
    </nav>
  )
}
