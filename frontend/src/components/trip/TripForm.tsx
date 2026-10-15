import { useState, type FormEvent } from 'react'
import { ALL_MODES, type TransportMode, type TripInput } from '../../api/trips'
import { t } from '../../i18n/fr'

interface Props {
  initial?: Partial<TripInput>
  submitLabel: string
  pending: boolean
  error?: string | null
  onSubmit: (input: TripInput) => void
}

const DEFAULTS: TripInput = {
  name: '',
  description: '',
  start_date: '',
  end_date: '',
  timezone: 'America/Costa_Rica',
  day_start: '09:00',
  day_end: '19:00',
  default_modes: ['car'],
}

/** Formulaire partagé entre la création et les paramètres d'un voyage. */
export function TripForm({ initial, submitLabel, pending, error, onSubmit }: Props) {
  const [form, setForm] = useState<TripInput>({ ...DEFAULTS, ...initial })
  const set = (key: keyof TripInput) => (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) =>
    setForm({ ...form, [key]: e.target.value })

  function toggleMode(mode: TransportMode) {
    const modes = form.default_modes ?? []
    setForm({
      ...form,
      default_modes: modes.includes(mode) ? modes.filter((m) => m !== mode) : [...modes, mode],
    })
  }

  return (
    <form
      className="form"
      onSubmit={(e: FormEvent) => {
        e.preventDefault()
        onSubmit(form)
      }}
    >
      <label>
        {t.trips.name}
        <input required maxLength={120} value={form.name} onChange={set('name')} />
      </label>
      <label>
        {t.trips.description}
        <textarea rows={2} maxLength={2000} value={form.description} onChange={set('description')} />
      </label>
      <div className="row">
        <label>
          {t.trips.startDate}
          <input type="date" required value={form.start_date} onChange={set('start_date')} />
        </label>
        <label>
          {t.trips.endDate}
          <input
            type="date"
            required
            min={form.start_date || undefined}
            value={form.end_date}
            onChange={set('end_date')}
          />
        </label>
      </div>
      <div className="row">
        <label>
          {t.trips.dayStart}
          <input type="time" required value={form.day_start} onChange={set('day_start')} />
        </label>
        <label>
          {t.trips.dayEnd}
          <input type="time" required value={form.day_end} onChange={set('day_end')} />
        </label>
      </div>
      <fieldset className="chips">
        <legend>{t.trips.modes}</legend>
        {ALL_MODES.map((mode) => (
          <label key={mode} className="chip">
            <input
              type="checkbox"
              checked={form.default_modes?.includes(mode) ?? false}
              onChange={() => toggleMode(mode)}
            />
            {t.modes[mode]}
          </label>
        ))}
      </fieldset>
      {form.default_modes?.includes('transit') && <p className="hint">ⓘ {t.transitEstimate}</p>}
      <label>
        {t.trips.timezone}
        <input required value={form.timezone} onChange={set('timezone')} />
      </label>
      {error && <p className="error" role="alert">{error}</p>}
      <button
        type="submit"
        className="primary"
        disabled={pending || (form.default_modes?.length ?? 0) === 0}
      >
        {submitLabel}
      </button>
    </form>
  )
}
