import { usePoiWiki, type Poi } from '../../api/pois'
import { t } from '../../i18n/fr'

const external = { target: '_blank', rel: 'noopener noreferrer' } as const

/** Présentation Wikipédia d'un lieu (résumé, photo créditée), s'il a une référence. */
export function WikiCard({ tripId, poi }: { tripId: string; poi: Poi }) {
  const { data: summary, isPending, isError, fetchStatus } = usePoiWiki(tripId, poi)
  if (fetchStatus === 'idle' && isPending) return null // aucune référence Wikipédia
  if (isPending) return <p className="muted small">{t.wiki.loading}</p>
  if (isError) return <p className="muted small">{t.wiki.unavailable}</p>
  if (!summary) return null

  const { image } = summary
  const credit = image && [image.artist, image.license].filter(Boolean).join(' · ')
  return (
    <section className="wiki-card" aria-label={t.wiki.title}>
      {image && (
        <figure>
          <img
            src={image.url}
            alt={t.wiki.photoAlt(summary.title)}
            width={image.width || undefined}
            height={image.height || undefined}
            loading="lazy"
            referrerPolicy="no-referrer"
          />
          <figcaption className="muted tiny">
            <a href={image.page_url} {...external}>
              {t.wiki.photo}
            </a>
            {credit && ` : ${credit}`}
          </figcaption>
        </figure>
      )}
      {summary.description && <p className="small muted wiki-description">{summary.description}</p>}
      <p className="small" lang={summary.lang}>
        {summary.extract}
      </p>
      <p className="small wiki-links">
        <a href={summary.url} {...external}>
          {t.wiki.readMore(summary.lang)}
        </a>
        {summary.wikivoyage_url && (
          <>
            {' · '}
            <a href={summary.wikivoyage_url} {...external}>
              {t.wiki.wikivoyage}
            </a>
          </>
        )}
      </p>
      <p className="muted tiny">{t.wiki.textCredit}</p>
    </section>
  )
}
