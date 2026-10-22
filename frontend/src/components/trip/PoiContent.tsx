import { useState, type FormEvent } from 'react'
import { errorMessage } from '../../api/errors'
import {
  domainOf,
  LINK_KINDS,
  useCreateLink,
  useCreateNote,
  useDeleteLink,
  useDeleteNote,
  useLinks,
  useNotes,
  useUpdateNote,
  type LinkKind,
  type PoiLink,
  type PoiNote,
} from '../../api/poiContent'
import { t } from '../../i18n/fr'
import { formatDate } from '../../lib/format'

interface Props {
  tripId: string
  poiId: string
  canEdit: boolean
}

const external = { target: '_blank', rel: 'noopener noreferrer nofollow' } as const

/** Auteur, date et provenance d'une note ou d'un lien. */
function Byline({ item }: { item: Pick<PoiNote, 'author_name' | 'via_assistant' | 'created_at'> }) {
  return (
    <>
      {item.author_name ?? t.poiContent.formerMember} · {formatDate(item.created_at, { day: 'numeric', month: 'short' })}
      {item.via_assistant && <span className="tag tag-assistant">{t.poiContent.assistant}</span>}
    </>
  )
}

/** Notes du lieu : informations pratiques partagées (horaires, prix, conseils), avec leur source. */
export function NotesSection({ tripId, poiId, canEdit }: Props) {
  const { data: notes = [], isError } = useNotes(tripId, poiId)
  const create = useCreateNote(tripId, poiId)
  const remove = useDeleteNote(tripId, poiId)
  const [editing, setEditing] = useState<string | null>(null)
  // Formulaire d'ajout recréé (vidé) après chaque ajout réussi.
  const [formKey, setFormKey] = useState(0)

  return (
    <section className="poi-content" aria-label={t.poiContent.notes}>
      <h3>{t.poiContent.notes}</h3>
      {isError ? (
        <p className="muted small">{t.poiContent.loadError}</p>
      ) : notes.length === 0 ? (
        <p className="muted small">{t.poiContent.noNotes}</p>
      ) : (
        <ul className="content-list">
          {notes.map((note) =>
            editing === note.id ? (
              <li key={note.id}>
                <NoteForm
                  tripId={tripId}
                  poiId={poiId}
                  note={note}
                  onDone={() => setEditing(null)}
                />
              </li>
            ) : (
              <li key={note.id}>
                <p className="small pre-line">{note.body}</p>
                <p className="muted tiny content-meta">
                  <Byline item={note} />
                  {note.source_url && (
                    <a href={note.source_url} {...external}>
                      {t.poiContent.source(domainOf(note.source_url))}
                    </a>
                  )}
                  {canEdit && (
                    <>
                      <button type="button" className="link-button" onClick={() => setEditing(note.id)}>
                        {t.poiContent.edit}
                      </button>
                      <button
                        type="button"
                        className="link-button"
                        disabled={remove.isPending}
                        onClick={() => confirm(t.poiContent.deleteNoteConfirm) && remove.mutate(note.id)}
                      >
                        {t.poiContent.delete}
                      </button>
                    </>
                  )}
                </p>
              </li>
            ),
          )}
        </ul>
      )}
      {!!remove.error && (
        <p className="error" role="alert">
          {errorMessage(remove.error)}
        </p>
      )}
      {canEdit && editing === null && (
        <NoteForm key={formKey} tripId={tripId} poiId={poiId} create={create} onDone={() => setFormKey((k) => k + 1)} />
      )}
    </section>
  )
}

interface NoteFormProps {
  tripId: string
  poiId: string
  /** Note modifiée ; absente : nouvelle note. */
  note?: PoiNote
  /** Mutation d'ajout, partagée avec la section. */
  create?: ReturnType<typeof useCreateNote>
  onDone: () => void
}

function NoteForm({ tripId, poiId, note, create, onDone }: NoteFormProps) {
  const update = useUpdateNote(tripId, poiId)
  const mutation = note ? update : create!
  const [body, setBody] = useState(note?.body ?? '')
  const [source, setSource] = useState(note?.source_url ?? '')

  function submit(e: FormEvent) {
    e.preventDefault()
    const input = { body, source_url: source }
    if (note) update.mutate({ id: note.id, ...input }, { onSuccess: onDone })
    else create!.mutate(input, { onSuccess: onDone })
  }

  return (
    <form className="form content-form" onSubmit={submit} aria-label={note ? t.poiContent.editNote : t.poiContent.addNote}>
      <label>
        {note ? t.poiContent.editNote : t.poiContent.addNote}
        <textarea value={body} onChange={(e) => setBody(e.target.value)} maxLength={4000} rows={2} required />
      </label>
      <label>
        {t.poiContent.sourceLabel}
        <input type="url" value={source} onChange={(e) => setSource(e.target.value)} maxLength={500} placeholder="https://" />
      </label>
      {!!mutation.error && (
        <p className="error" role="alert">
          {errorMessage(mutation.error)}
        </p>
      )}
      <div className="actions">
        <button type="submit" className="small-button primary" disabled={!body.trim() || mutation.isPending}>
          {note ? t.poiContent.save : t.poiContent.addNoteButton}
        </button>
        {note && (
          <button type="button" className="small-button" onClick={onDone}>
            {t.poiContent.cancel}
          </button>
        )}
      </div>
    </form>
  )
}

/** Liens ajoutés par le groupe (réservation, articles, vidéos…). */
export function LinksSection({ tripId, poiId, canEdit }: Props) {
  const { data: links = [], isError } = useLinks(tripId, poiId)
  const create = useCreateLink(tripId, poiId)
  const remove = useDeleteLink(tripId, poiId)
  // Formulaire recréé (vidé) après chaque ajout réussi.
  const [formKey, setFormKey] = useState(0)

  if (!canEdit && links.length === 0 && !isError) return null
  return (
    <section className="poi-content" aria-label={t.poiContent.links}>
      <h3>{t.poiContent.links}</h3>
      {isError && <p className="muted small">{t.poiContent.loadError}</p>}
      {links.length > 0 && (
        <ul className="content-list">
          {links.map((link: PoiLink) => (
            <li key={link.id}>
              <p className="small">
                <span className="tag">{t.poiContent.kinds[link.kind]}</span>{' '}
                <a href={link.url} {...external}>
                  {link.title}
                </a>{' '}
                <span className="muted">({domainOf(link.url)})</span>
              </p>
              <p className="muted tiny content-meta">
                <Byline item={link} />
                {canEdit && (
                  <button
                    type="button"
                    className="link-button"
                    disabled={remove.isPending}
                    aria-label={t.poiContent.deleteLinkNamed(link.title)}
                    onClick={() => remove.mutate(link.id)}
                  >
                    {t.poiContent.delete}
                  </button>
                )}
              </p>
            </li>
          ))}
        </ul>
      )}
      {!!remove.error && (
        <p className="error" role="alert">
          {errorMessage(remove.error)}
        </p>
      )}
      {canEdit && (
        <LinkForm
          key={formKey}
          pending={create.isPending}
          error={create.error}
          onSubmit={(input) => create.mutate(input, { onSuccess: () => setFormKey((k) => k + 1) })}
        />
      )}
    </section>
  )
}

function LinkForm({
  pending,
  error,
  onSubmit,
}: {
  pending: boolean
  error: unknown
  onSubmit: (input: { url: string; title: string; kind: LinkKind }) => void
}) {
  const [url, setUrl] = useState('')
  const [title, setTitle] = useState('')
  const [kind, setKind] = useState<LinkKind>('other')

  function submit(e: FormEvent) {
    e.preventDefault()
    onSubmit({ url: url.trim(), title, kind })
  }

  return (
    <form className="form content-form" onSubmit={submit} aria-label={t.poiContent.addLink}>
      <label>
        {t.poiContent.linkUrl}
        <input type="url" value={url} onChange={(e) => setUrl(e.target.value)} maxLength={500} placeholder="https://" required />
      </label>
      <div className="row">
        <label>
          {t.poiContent.linkTitle}
          <input value={title} onChange={(e) => setTitle(e.target.value)} maxLength={200} />
        </label>
        <label>
          {t.poiContent.linkKind}
          <select value={kind} onChange={(e) => setKind(e.target.value as LinkKind)}>
            {LINK_KINDS.map((k) => (
              <option key={k} value={k}>
                {t.poiContent.kinds[k]}
              </option>
            ))}
          </select>
        </label>
      </div>
      {!!error && (
        <p className="error" role="alert">
          {errorMessage(error)}
        </p>
      )}
      <div className="actions">
        <button type="submit" className="small-button primary" disabled={!url.trim() || pending}>
          {t.poiContent.addLinkButton}
        </button>
      </div>
    </form>
  )
}
