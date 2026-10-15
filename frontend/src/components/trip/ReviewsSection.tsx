import { useState, type FormEvent } from 'react'
import { useMe } from '../../api/auth'
import { errorMessage } from '../../api/errors'
import { useReviews, useSaveMyReview, type Poi } from '../../api/pois'
import { t } from '../../i18n/fr'
import { formatRating } from '../../lib/format'
import { StarInput, Stars } from './Stars'

/** Moyenne du groupe, désaccord éventuel, mon avis et celui de chaque membre. */
export function ReviewsSection({ tripId, poi }: { tripId: string; poi: Poi }) {
  const { data: me } = useMe()
  const { data: reviews = [] } = useReviews(tripId, poi.id)
  const { rating } = poi
  const extremes = rating.disagreement ? [rating.min, rating.max] : []

  return (
    <section className="reviews" aria-label={t.reviews.title}>
      <h3>{t.reviews.title}</h3>
      {rating.average === null ? (
        <p className="muted small">{t.reviews.none}</p>
      ) : (
        <p className="group-rating">
          <Stars value={rating.average} /> <strong>{t.reviews.average(formatRating(rating.average))}</strong>{' '}
          <span className="muted small">· {t.reviews.count(rating.count)}</span>
          {rating.disagreement && <span className="tag tag-disagreement">⚡ {t.reviews.disagreement}</span>}
        </p>
      )}
      {rating.disagreement && rating.min !== null && rating.max !== null && (
        <p className="notice warning small">{t.reviews.disagreementHint(rating.min, rating.max)}</p>
      )}

      <MyReviewForm key={`${poi.id}:${poi.my_review?.rating}:${poi.my_review?.comment}`} tripId={tripId} poi={poi} />

      {reviews.length > 0 && (
        <>
          <h4 className="small">{t.reviews.others}</h4>
          <ul className="review-list">
            {reviews.map((r) => (
              <li key={r.user_id} className={extremes.includes(r.rating) ? 'extreme' : undefined}>
                <div>
                  <strong>{r.display_name}</strong> {r.user_id === me?.id && <span className="muted">{t.reviews.you}</span>}{' '}
                  <Stars value={r.rating} />
                </div>
                {r.comment && <p className="small pre-line">{r.comment}</p>}
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  )
}

function MyReviewForm({ tripId, poi }: { tripId: string; poi: Poi }) {
  const [value, setValue] = useState(poi.my_review?.rating ?? 0)
  const [comment, setComment] = useState(poi.my_review?.comment ?? '')
  const save = useSaveMyReview(tripId, poi.id)

  function submit(e: FormEvent) {
    e.preventDefault()
    if (value) save.mutate({ rating: value, comment })
  }

  return (
    <form className="form my-review" onSubmit={submit} aria-label={t.reviews.mine}>
      <div className="row">
        <span className="small">{t.reviews.mine}</span>
        <StarInput value={value} onChange={setValue} />
      </div>
      <label>
        {t.reviews.comment}
        <textarea value={comment} onChange={(e) => setComment(e.target.value)} maxLength={2000} rows={2} />
      </label>
      {save.error && (
        <p className="error" role="alert">
          {errorMessage(save.error)}
        </p>
      )}
      <div className="actions">
        <button type="submit" className="primary" disabled={!value || save.isPending}>
          {t.reviews.save}
        </button>
        {poi.my_review && (
          <button type="button" disabled={save.isPending} onClick={() => save.mutate(null)}>
            {t.reviews.remove}
          </button>
        )}
      </div>
    </form>
  )
}
