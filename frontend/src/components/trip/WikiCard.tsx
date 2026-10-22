import type { UseQueryResult } from '@tanstack/react-query'
import { usePlaceWiki, usePoiWiki, type Poi, type WikiRef, type WikiSummary } from '../../api/pois'
import { t } from '../../i18n/fr'

const external = { target: '_blank', rel: 'noopener noreferrer' } as const

/** Présentation Wikipédia d'un lieu du voyage (résumé, photo créditée), s'il a une référence. */
export function WikiCard({ tripId, poi }: { tripId: string; poi: Poi }) {
  return <WikiView query={usePoiWiki(tripId, poi)} />
}

/** Présentation Wikipédia d'un lieu hors du voyage (région, suggestion). */
export function PlaceWikiCard({ tripId, wiki, compact }: { tripId: string; wiki: WikiRef; compact?: boolean }) {
  return <WikiView query={usePlaceWiki(tripId, wiki)} compact={compact} />
}

/**
 * `compact` : vignette et description seulement (points forts d'une région), sans chargement affiché.
 */
function WikiView({ query, compact }: { query: UseQueryResult<WikiSummary | null>; compact?: boolean }) {
  const { data: summary, isPending, isError, fetchStatus } = query
  if (fetchStatus === 'idle' && isPending) return null // aucune référence Wikipédia
  if (compact) {
    if (isError) return <span className="muted small">{t.wiki.unavailable}</span>
    return summary ? <CompactWiki summary={summary} /> : null
  }
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

function CompactWiki({ summary }: { summary: WikiSummary }) {
  const { image } = summary
  return (
    <span className="wiki-compact">
      {image && (
        <a href={image.page_url} {...external} title={[t.wiki.photo, image.artist, image.license].filter(Boolean).join(' · ')}>
          <img src={image.url} alt={t.wiki.photoAlt(summary.title)} loading="lazy" referrerPolicy="no-referrer" />
        </a>
      )}
      <span className="small" lang={summary.lang}>
        {summary.description && <span className="muted">{capitalize(summary.description)}. </span>}
        {firstSentence(summary.extract)}{' '}
        <a href={summary.url} {...external}>
          {t.wiki.more}
        </a>
      </span>
    </span>
  )
}

const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1)

/** Première phrase d'un résumé (ou ses 220 premiers caractères). */
function firstSentence(text: string) {
  const end = text.search(/[.!?](\s|$)/)
  const cut = end > 0 && end < 220 ? text.slice(0, end + 1) : text.slice(0, 220)
  return cut.length < text.length && !/[.!?]$/.test(cut) ? `${cut.trimEnd()}…` : cut
}
