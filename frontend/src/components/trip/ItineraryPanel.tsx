import { useEffect, useMemo, useState } from 'react'
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core'
import { SortableContext, sortableKeyboardCoordinates, useSortable, verticalListSortingStrategy } from '@dnd-kit/sortable'
import { CSS } from '@dnd-kit/utilities'
import { ApiError } from '../../api/client'
import { errorMessage } from '../../api/errors'
import {
  dayPoiIds,
  useAdjustItinerary,
  useGenerateItinerary,
  useItinerary,
  type Adjustment,
  type Itinerary,
  type ItineraryDay,
  type ItineraryStop,
} from '../../api/itinerary'
import type { Poi } from '../../api/pois'
import { useRoutePlan, type TravelOption } from '../../api/transport'
import type { Trip } from '../../api/trips'
import { t } from '../../i18n/fr'
import { Icon } from '../Icon'
import { pointAt, useMapHover } from '../mapHover'
import { formatDate, formatDistance, formatDuration } from '../../lib/format'

interface Props {
  trip: Trip
  pois: Poi[]
  onRouteChange: (geometry: [number, number][] | null) => void
  onSelectPoi: (id: string) => void
}

/** Le calcul de trajets relie au plus 25 lieux. */
const MAX_ROUTE_POIS = 25
/** Préfixe des zones de dépôt « journée » (pour déposer dans une journée vide). */
const DAY_DROP = 'day:'

const dayLabel = (date: string) => formatDate(date, { weekday: 'short', day: 'numeric', month: 'short' })

/** Actions d'édition partagées par les composants de la frise. */
interface Editor {
  send: (adjustment: Adjustment) => void
  pending: boolean
}

export function ItineraryPanel({ trip, pois, onRouteChange, onSelectPoi }: Props) {
  const { data: itinerary, error, isPending } = useItinerary(trip.id)
  const generate = useGenerateItinerary(trip.id)
  const adjust = useAdjustItinerary(trip.id)
  const canEdit = trip.my_role !== 'viewer'
  const names = useMemo(() => new Map(pois.map((p) => [p.id, p.name])), [pois])
  const byId = useMemo(() => new Map(pois.map((p) => [p.id, p])), [pois])

  if (isPending) return <p className="muted">{t.common.loading}</p>
  if (error) return <p className="error">{errorMessage(error)}</p>

  const run = () => {
    if (itinerary && !confirm(t.itinerary.regenerateConfirm)) return
    generate.mutate()
  }
  const button = canEdit && (
    <button type="button" className={itinerary ? 'small-button' : 'primary'} onClick={run} disabled={generate.isPending}>
      {generate.isPending ? t.itinerary.generating : itinerary ? t.itinerary.regenerate : t.itinerary.generate}
    </button>
  )
  const generateError = generate.error && <p className="error small" role="alert">{errorMessage(generate.error)}</p>

  if (!itinerary) {
    return (
      <section className="card">
        <h2 className="section-title">{t.itinerary.title}</h2>
        <p>{t.itinerary.none}</p>
        <p className="small muted">{t.itinerary.intro}</p>
        {canEdit ? button : <p className="small">{t.itinerary.viewerHint}</p>}
        {generateError}
      </section>
    )
  }

  const editor: Editor | null = canEdit
    ? { send: (adjustment) => adjust.mutate({ version: itinerary.version, adjustment }), pending: adjust.isPending }
    : null
  const adjustError =
    adjust.error &&
    (adjust.error instanceof ApiError && adjust.error.status === 409 ? t.itinerary.conflict : errorMessage(adjust.error))

  return (
    <div className="poi-panel">
      <section className="card itinerary-head">
        <div className="list-header">
          <h2 className="section-title">{t.itinerary.title}</h2>
          {button}
        </div>
        <p className="tiny muted">
          {t.itinerary.generated(
            new Date(itinerary.generated_at).toLocaleString('fr-FR', { dateStyle: 'medium', timeStyle: 'short' }),
            itinerary.generated_by_name,
          )}
        </p>
        {generateError}
        {adjustError && <p className="error small" role="alert">{adjustError}</p>}
        {itinerary.stale && <p className="notice warning small">{t.itinerary.stale}</p>}
        {itinerary.has_estimates && <p className="hint">
            <Icon name="info" size={15} /> {t.itinerary.estimates}
          </p>}
        {canEdit && <p className="tiny muted">{t.itinerary.editHint}</p>}
      </section>
      <Days
        itinerary={itinerary}
        tripId={trip.id}
        names={names}
        byId={byId}
        editor={editor}
        onRouteChange={onRouteChange}
        onSelectPoi={onSelectPoi}
      />
      <Unscheduled itinerary={itinerary} names={names} editor={editor} onSelectPoi={onSelectPoi} />
    </div>
  )
}

interface DaysProps {
  itinerary: Itinerary
  tripId: string
  names: Map<string, string>
  byId: Map<string, Poi>
  editor: Editor | null
  onRouteChange: Props['onRouteChange']
  onSelectPoi: Props['onSelectPoi']
}

function Days({ itinerary, tripId, names, byId, editor, onRouteChange, onSelectPoi }: DaysProps) {
  // Jour affiché sur la carte : par défaut le premier qui a des étapes.
  const [shown, setShown] = useState<string | null>(() => itinerary.days.find((d) => d.stops.length > 0)?.date ?? null)
  const day = itinerary.days.find((d) => d.date === shown)
  const ids = day ? dayPoiIds(day) : []
  const { data: plan } = useRoutePlan(
    tripId,
    day && ids.length >= 2 && ids.length <= MAX_ROUTE_POIS ? { poi_ids: ids, date: day.date } : null,
  )
  // Le tracé n'est affiché que s'il correspond bien au jour choisi (pas celui d'avant).
  const matches =
    !!plan &&
    plan.legs.length === ids.length - 1 &&
    plan.legs[0]?.from_poi_id === ids[0] &&
    plan.legs.at(-1)?.to_poi_id === ids.at(-1)
  const geometry = useMemo(() => (matches && plan ? plan.legs.flatMap((l) => l.geometry) : null), [matches, plan])
  useEffect(() => {
    onRouteChange(geometry && geometry.length > 1 ? geometry : null)
  }, [geometry, onRouteChange])
  useEffect(() => () => onRouteChange(null), [onRouteChange])

  const sensors = useSensors(
    // Un léger déplacement avant de saisir : les clics sur les boutons restent des clics.
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  )
  const name = (id: unknown) => names.get(String(id)) ?? '—'
  const dayOf = (poiId: string) => itinerary.days.find((d) => d.stops.some((s) => s.poi_id === poiId))
  const targetName = (id: unknown) =>
    String(id).startsWith(DAY_DROP) ? dayLabel(String(id).slice(DAY_DROP.length)) : name(id)

  function onDragEnd({ active, over }: DragEndEvent) {
    if (!editor || !over) return
    const poiId = String(active.id)
    const overId = String(over.id)
    const from = dayOf(poiId)
    let date: string
    let position: number
    if (overId.startsWith(DAY_DROP)) {
      date = overId.slice(DAY_DROP.length)
      position = Number.MAX_SAFE_INTEGER
    } else {
      const target = dayOf(overId)
      if (!target) return
      date = target.date
      // Avant l'étape survolée (dans la même journée : à sa place, comme un déplacement de liste).
      position = target.stops.findIndex((s) => s.poi_id === overId)
    }
    const current = from?.stops.findIndex((s) => s.poi_id === poiId)
    if (from?.date === date && (current === position || (position === Number.MAX_SAFE_INTEGER && current === from.stops.length - 1)))
      return
    editor.send({ kind: 'move', poi_id: poiId, date, position })
  }

  const cards = itinerary.days.map((d) => (
    <DayCard
      key={d.date}
      day={d}
      days={itinerary.days}
      names={names}
      byId={byId}
      editor={editor}
      shown={d.date === shown}
      onShow={() => setShown(d.date)}
      onSelectPoi={onSelectPoi}
    />
  ))
  if (!editor) return <>{cards}</>
  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCenter}
      onDragEnd={onDragEnd}
      accessibility={{
        announcements: {
          onDragStart: ({ active }) => t.itinerary.dragStart(name(active.id)),
          onDragOver: ({ active, over }) => (over ? t.itinerary.dragOver(name(active.id), targetName(over.id)) : undefined),
          onDragEnd: ({ active }) => t.itinerary.dragEnd(name(active.id)),
          onDragCancel: ({ active }) => t.itinerary.dragCancel(name(active.id)),
        },
      }}
    >
      {cards}
    </DndContext>
  )
}

interface DayCardProps {
  day: ItineraryDay
  days: ItineraryDay[]
  names: Map<string, string>
  byId: Map<string, Poi>
  editor: Editor | null
  shown: boolean
  onShow: () => void
  onSelectPoi: (id: string) => void
}

function DayCard({ day, days, names, byId, editor, shown, onShow, onSelectPoi }: DayCardProps) {
  const label = dayLabel(day.date)
  const name = (id: string) => names.get(id) ?? '—'
  const hover = useMapHover()
  const hoverPoi = (id: string | null) => {
    const p = id ? byId.get(id) : undefined
    return hover(p ? pointAt(p, p.id) : null)
  }
  // Toute la carte accepte un dépôt : c'est ainsi qu'on remplit une journée vide.
  const { setNodeRef, isOver } = useDroppable({ id: DAY_DROP + day.date, disabled: !editor })
  const stopIds = day.stops.map((s) => s.poi_id)
  return (
    <section
      ref={setNodeRef}
      className={`card itinerary-day${shown ? ' shown' : ''}${isOver ? ' drop-target' : ''}`}
      aria-label={label}
    >
      <div className="day-head">
        <button type="button" className="day-title" onClick={onShow} aria-pressed={shown} title={t.itinerary.showOnMap(label)}>
          <strong>{label}</strong>
          <span className="tiny muted day-modes" aria-label={day.modes.map((m) => t.modes[m]).join(', ')}>
            {day.modes.map((m) => (
              <Icon key={m} name={m} size={15} />
            ))}
          </span>
          {day.stale && <span className="tag tag-estimate">{t.itinerary.staleDay}</span>}
        </button>
        {editor && day.stale && (
          <button
            type="button"
            className="link-button tiny"
            disabled={editor.pending}
            onClick={() => editor.send({ kind: 'recompute', date: day.date })}
          >
            {t.itinerary.recompute}
          </button>
        )}
      </div>
      {day.stops.length > 0 ? (
        <p className="tiny muted">
          {t.itinerary.summary(day.stops.length, formatDuration(day.total_visit_s), formatDuration(day.total_travel_s), day.finish)}
        </p>
      ) : (
        <p className="small muted">{day.back ? t.itinerary.transferDay : t.itinerary.freeDay}</p>
      )}
      {day.overtime && <p className="notice warning small">{t.itinerary.overtime(day.day_end)}</p>}
      <ol className="timeline">
        {day.start_poi_id && (day.stops.length > 0 || day.back) && (
          <li className="lodging" {...hoverPoi(day.start_poi_id)}>
            <span className="time">{day.day_start}</span>
            <span>
              <Icon name="lodging" size={15} /> {t.itinerary.departure} : {name(day.start_poi_id)}
            </span>
          </li>
        )}
        <SortableContext id={day.date} items={stopIds} strategy={verticalListSortingStrategy}>
          {day.stops.map((s, i) => (
            <StopItem
              key={s.poi_id}
              stop={s}
              name={name(s.poi_id)}
              index={i}
              day={day}
              days={days}
              editor={editor}
              hover={hoverPoi(s.poi_id)}
              onSelect={() => onSelectPoi(s.poi_id)}
            />
          ))}
        </SortableContext>
        {editor && day.stops.length === 0 && <li className="drop-hint tiny muted">{t.itinerary.dropHere}</li>}
        {day.back && <TravelItem travel={day.back} />}
        {day.end_poi_id && day.back && (
          <li className="lodging" {...hoverPoi(day.end_poi_id)}>
            <span className="time">{day.finish}</span>
            <span>
              <Icon name="lodging" size={15} /> {t.itinerary.arrival} : {name(day.end_poi_id)}
            </span>
          </li>
        )}
      </ol>
    </section>
  )
}

interface StopProps {
  stop: ItineraryStop
  name: string
  index: number
  day: ItineraryDay
  days: ItineraryDay[]
  editor: Editor | null
  /** Mise en évidence du lieu sur la carte. */
  hover: ReturnType<ReturnType<typeof useMapHover>>
  onSelect: () => void
}

function StopItem({ stop, name, index, day, days, editor, hover, onSelect }: StopProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: stop.poi_id,
    disabled: !editor,
  })
  const style = { transform: CSS.Transform.toString(transform), transition }
  const move = (date: string | null, position: number) =>
    editor?.send({ kind: 'move', poi_id: stop.poi_id, date, position })
  const time = stop.start ? `${stop.start}–${stop.end}` : '…'
  return (
    <>
      {stop.travel && <TravelItem travel={stop.travel} />}
      <li
        ref={setNodeRef}
        style={style}
        className={`stop${isDragging ? ' dragging' : ''}${stop.issue ? ' has-issue' : ''}`}
        {...hover}
      >
        {editor && (
          <button type="button" className="drag-handle" aria-label={t.itinerary.drag(name)} {...attributes} {...listeners}>
            ⠿
          </button>
        )}
        <span className="time">{time}</span>
        <button type="button" className="link-button stop-name" onClick={onSelect}>
          {name}
        </button>
        {stop.locked && !editor && (
          <Icon name="lock" size={15} label={t.itinerary.lockedHint} />
        )}
        {stop.wait_s >= 60 && <span className="tiny muted">{t.itinerary.wait(formatDuration(stop.wait_s))}</span>}
        {stop.issue && <span className="tag tag-estimate"><Icon name="warning" size={13} /> {t.itinerary.issues[stop.issue]}</span>}
        {editor && (
          <span className="stop-actions">
            <button
              type="button"
              className="icon-small"
              aria-label={t.itinerary.up(name)}
              disabled={index === 0 || editor.pending}
              onClick={() => move(day.date, index - 1)}
            >
              ↑
            </button>
            <button
              type="button"
              className="icon-small"
              aria-label={t.itinerary.down(name)}
              disabled={index === day.stops.length - 1 || editor.pending}
              onClick={() => move(day.date, index + 1)}
            >
              ↓
            </button>
            <button
              type="button"
              className="icon-small"
              aria-pressed={stop.locked}
              aria-label={stop.locked ? t.itinerary.unlock(name) : t.itinerary.lock(name)}
              title={t.itinerary.lockedHint}
              disabled={editor.pending}
              onClick={() => editor.send({ kind: 'lock', poi_id: stop.poi_id, locked: !stop.locked })}
            >
              <Icon name={stop.locked ? 'lock' : 'unlock'} size={15} />
            </button>
            <select
              className="move-select"
              aria-label={t.itinerary.moveTo(name)}
              value=""
              disabled={editor.pending}
              onChange={(e) => {
                const v = e.target.value
                if (v) move(v === 'remove' ? null : v, Number.MAX_SAFE_INTEGER)
              }}
            >
              <option value="">{t.itinerary.moveToPlaceholder}</option>
              {days
                .filter((d) => d.date !== day.date)
                .map((d) => (
                  <option key={d.date} value={d.date}>
                    {t.itinerary.toDay(dayLabel(d.date))}
                  </option>
                ))}
              <option value="remove">{t.itinerary.remove}</option>
            </select>
          </span>
        )}
      </li>
    </>
  )
}

function TravelItem({ travel }: { travel: TravelOption }) {
  const estimate = travel.source !== 'router'
  return (
    <li className="travel">
      <span className="time" />
      <span className="tiny muted">
        <Icon name={travel.mode} size={14} /> {t.modes[travel.mode]} · {estimate && '≈ '}
        {formatDuration(travel.duration_s)} · {formatDistance(travel.distance_m)}
        {estimate && ` · ${t.transport.sources[travel.source]}`}
      </span>
    </li>
  )
}

interface UnscheduledProps {
  itinerary: Itinerary
  names: Map<string, string>
  editor: Editor | null
  onSelectPoi: (id: string) => void
}

function Unscheduled({ itinerary, names, editor, onSelectPoi }: UnscheduledProps) {
  if (itinerary.unscheduled.length === 0) return <p className="tiny muted">{t.itinerary.lodgingNote}</p>
  return (
    <section className="card">
      <h2 className="section-title">{t.itinerary.unscheduledTitle(itinerary.unscheduled.length)}</h2>
      <ul className="unscheduled">
        {itinerary.unscheduled.map((u) => {
          const name = names.get(u.poi_id) ?? '—'
          return (
            <li key={u.poi_id}>
              <button type="button" className="link-button" onClick={() => onSelectPoi(u.poi_id)}>
                {name}
              </button>{' '}
              <span className="tiny muted">{t.itinerary.reasons[u.reason]}</span>
              {editor && (
                <select
                  className="move-select"
                  aria-label={t.itinerary.addTo(name)}
                  value=""
                  disabled={editor.pending}
                  onChange={(e) =>
                    e.target.value &&
                    editor.send({ kind: 'move', poi_id: u.poi_id, date: e.target.value, position: Number.MAX_SAFE_INTEGER })
                  }
                >
                  <option value="">{t.itinerary.addPlaceholder}</option>
                  {itinerary.days.map((d) => (
                    <option key={d.date} value={d.date}>
                      {dayLabel(d.date)}
                    </option>
                  ))}
                </select>
              )}
            </li>
          )
        })}
      </ul>
      <p className="tiny muted">{t.itinerary.lodgingNote}</p>
    </section>
  )
}
