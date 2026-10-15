import { useEffect, useState, type FormEvent } from 'react'
import { ApiError } from '../../api/client'
import { errorMessage } from '../../api/errors'
import {
  ALL_CATEGORIES,
  DEFAULT_DURATION,
  useCreatePoi,
  useDeletePoi,
  useUpdatePoi,
  type Category,
  type Poi,
  type PoiInput,
} from '../../api/pois'
import { t } from '../../i18n/fr'
import { OpeningHoursView } from './OpeningHoursView'

/** Valeur stabilisée après `delay` ms sans changement (évite une requête par frappe). */
function useDebounced<T>(value: T, delay = 400) {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay)
    return () => clearTimeout(id)
  }, [value, delay])
  return debounced
}

interface Form {
  name: string
  category: Category
  duration_min: string
  address: string
  opening_hours: string
  website: string
  notes: string
}

const toForm = (v: PoiInput | Poi): Form => ({
  name: v.name,
  category: v.category,
  duration_min: String(v.duration_min ?? DEFAULT_DURATION[v.category]),
  address: v.address ?? '',
  opening_hours: v.opening_hours ?? '',
  website: v.website ?? '',
  notes: v.notes ?? '',
})

interface Props {
  tripId: string
  /** Lieu existant à modifier, ou valeurs initiales d'un nouveau lieu. */
  poi?: Poi
  draft?: PoiInput
  onSaved: (poi: Poi) => void
  onClose: () => void
}

export function PoiEditor({ tripId, poi, draft, onSaved, onClose }: Props) {
  const initial = poi ?? draft
  const [form, setForm] = useState<Form>(() => toForm(initial!))
  const [durationTouched, setDurationTouched] = useState(false)
  const create = useCreatePoi(tripId)
  const update = useUpdatePoi(tripId)
  const del = useDeletePoi(tripId)
  const hours = useDebounced(form.opening_hours)
  const mutation = poi ? update : create
  const conflict = update.error instanceof ApiError && update.error.status === 409
  const error = conflict ? t.pois.conflict : (mutation.error ?? del.error) && errorMessage(mutation.error ?? del.error)

  const set = (key: keyof Form) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) =>
    setForm({ ...form, [key]: e.target.value })

  function setCategory(category: Category) {
    // Tant que la durée n'a pas été choisie à la main, elle suit la catégorie.
    setForm({ ...form, category, duration_min: durationTouched ? form.duration_min : String(DEFAULT_DURATION[category]) })
  }

  function submit(e: FormEvent) {
    e.preventDefault()
    const values = {
      name: form.name,
      category: form.category,
      duration_min: Number(form.duration_min),
      address: form.address,
      opening_hours: form.opening_hours,
      website: form.website,
      notes: form.notes,
    }
    if (poi) {
      update.mutate({ id: poi.id, version: poi.version, ...values }, { onSuccess: onSaved })
    } else if (draft) {
      create.mutate({ ...draft, ...values }, { onSuccess: onSaved })
    }
  }

  return (
    <form className="form poi-editor" onSubmit={submit} aria-label={poi ? poi.name : t.pois.newPlace}>
      {!poi && <h3>{t.pois.newPlace}</h3>}
      <label>
        {t.pois.name}
        <input value={form.name} onChange={set('name')} required maxLength={200} autoFocus={!poi} />
      </label>
      <div className="row">
        <label>
          {t.pois.category}
          <select value={form.category} onChange={(e) => setCategory(e.target.value as Category)}>
            {ALL_CATEGORIES.map((c) => (
              <option key={c} value={c}>
                {t.categoryIcons[c]} {t.categories[c]}
              </option>
            ))}
          </select>
        </label>
        <label>
          {t.pois.duration}
          <input
            type="number"
            min={0}
            max={720}
            step={5}
            value={form.duration_min}
            onChange={(e) => {
              setDurationTouched(true)
              set('duration_min')(e)
            }}
            required
          />
        </label>
      </div>
      <label>
        {t.pois.address}
        <input value={form.address} onChange={set('address')} maxLength={500} />
      </label>
      <label>
        {t.pois.openingHours}
        <input value={form.opening_hours} onChange={set('opening_hours')} maxLength={500} placeholder={t.pois.openingHoursHint} />
      </label>
      <OpeningHoursView tripId={tripId} value={hours} countryCode={initial?.country_code ?? null} />
      <label>
        {t.pois.website}
        <input type="url" value={form.website} onChange={set('website')} maxLength={500} placeholder="https://" />
      </label>
      <label>
        {t.pois.notes}
        <textarea value={form.notes} onChange={set('notes')} maxLength={2000} rows={2} />
      </label>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="actions">
        <button type="submit" className="primary" disabled={mutation.isPending}>
          {poi ? t.pois.save : t.pois.add}
        </button>
        <button type="button" onClick={onClose}>
          {poi ? t.pois.close : t.pois.cancel}
        </button>
        {poi && (
          <button
            type="button"
            className="danger-outline push-right"
            disabled={del.isPending}
            onClick={() => confirm(t.pois.deleteConfirm(poi.name)) && del.mutate(poi.id, { onSuccess: onClose })}
          >
            {t.pois.delete}
          </button>
        )}
      </div>
    </form>
  )
}

/** Fiche d'un lieu en lecture seule (lecteurs). */
export function PoiDetails({ tripId, poi }: { tripId: string; poi: Poi }) {
  return (
    <div className="poi-details">
      {poi.address && <p className="small">{poi.address}</p>}
      {poi.opening_hours ? (
        <>
          <p className="small">
            <code>{poi.opening_hours}</code>
          </p>
          <OpeningHoursView tripId={tripId} value={poi.opening_hours} countryCode={poi.country_code} />
        </>
      ) : (
        <p className="muted small">{t.pois.hoursUnknown}</p>
      )}
      {poi.website && (
        <p className="small">
          <a href={poi.website} target="_blank" rel="noopener noreferrer nofollow">
            {poi.website}
          </a>
        </p>
      )}
      {poi.notes && <p className="small pre-line">{poi.notes}</p>}
    </div>
  )
}
