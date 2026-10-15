import { t } from '../../i18n/fr'
import { formatRating } from '../../lib/format'

/** Affichage d'une note sur 5 (arrondie à l'étoile la plus proche). */
export function Stars({ value }: { value: number }) {
  const full = Math.round(value)
  return (
    <span className="stars" role="img" aria-label={t.reviews.stars(formatRating(value))}>
      {'★'.repeat(full)}
      <span className="stars-empty">{'★'.repeat(5 - full)}</span>
    </span>
  )
}

/** Saisie d'une note de 1 à 5. */
export function StarInput({ value, onChange }: { value: number; onChange: (n: number) => void }) {
  return (
    <span className="star-input" role="radiogroup" aria-label={t.reviews.mine}>
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          type="button"
          role="radio"
          aria-checked={value === n}
          aria-label={t.reviews.rate(n)}
          className={n <= value ? 'on' : undefined}
          onClick={() => onChange(n)}
        >
          ★
        </button>
      ))}
    </span>
  )
}
