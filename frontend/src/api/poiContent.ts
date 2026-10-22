import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiFetch } from './client'
import { tripKeys } from './trips'

/** Note d'un lieu, écrite par un membre (ou par l'assistant pour son compte). */
export interface PoiNote {
  id: string
  poi_id: string
  body: string
  /** Page d'où vient l'information. */
  source_url: string | null
  /** Absent si le compte de l'auteur a été supprimé. */
  author_id: string | null
  author_name: string | null
  via_assistant: boolean
  created_at: string
  updated_at: string
}

export interface NoteInput {
  body: string
  source_url?: string
}

export const LINK_KINDS = ['official', 'booking', 'article', 'video', 'map', 'other'] as const
export type LinkKind = (typeof LINK_KINDS)[number]

/** Lien ajouté à un lieu (distinct des liens tirés d'OpenStreetMap : site, téléphone…). */
export interface PoiLink {
  id: string
  poi_id: string
  url: string
  title: string
  kind: LinkKind
  author_id: string | null
  author_name: string | null
  via_assistant: boolean
  created_at: string
}

export interface LinkInput {
  url: string
  /** Défaut : le nom de domaine. */
  title?: string
  kind?: LinkKind
}

const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body) })

/** Toutes les clés sous `content` sont rechargées par la notification temps réel `poi_content`. */
export const contentKeys = {
  all: (tripId: string) => [...tripKeys.one(tripId), 'poi-content'] as const,
  notes: (tripId: string, poiId: string) => [...contentKeys.all(tripId), poiId, 'notes'] as const,
  links: (tripId: string, poiId: string) => [...contentKeys.all(tripId), poiId, 'links'] as const,
  photos: (tripId: string, poiId: string) => [...contentKeys.all(tripId), poiId, 'photos'] as const,
}

const base = (tripId: string, poiId: string) => `/trips/${tripId}/pois/${poiId}`

export const useNotes = (tripId: string, poiId: string) =>
  useQuery({
    queryKey: contentKeys.notes(tripId, poiId),
    queryFn: () => apiFetch<PoiNote[]>(`${base(tripId, poiId)}/notes`),
  })

export function useCreateNote(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: NoteInput) => apiFetch<PoiNote>(`${base(tripId, poiId)}/notes`, json('POST', input)),
    onSuccess: (note) => qc.setQueryData<PoiNote[]>(contentKeys.notes(tripId, poiId), (list) => [...(list ?? []), note]),
  })
}

export function useUpdateNote(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ id, ...input }: NoteInput & { id: string }) =>
      apiFetch<PoiNote>(`${base(tripId, poiId)}/notes/${id}`, json('PUT', input)),
    onSuccess: (note) =>
      qc.setQueryData<PoiNote[]>(contentKeys.notes(tripId, poiId), (list) => list?.map((n) => (n.id === note.id ? note : n))),
  })
}

export function useDeleteNote(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiFetch<void>(`${base(tripId, poiId)}/notes/${id}`, { method: 'DELETE' }),
    onSuccess: (_, id) =>
      qc.setQueryData<PoiNote[]>(contentKeys.notes(tripId, poiId), (list) => list?.filter((n) => n.id !== id)),
  })
}

export const useLinks = (tripId: string, poiId: string) =>
  useQuery({
    queryKey: contentKeys.links(tripId, poiId),
    queryFn: () => apiFetch<PoiLink[]>(`${base(tripId, poiId)}/links`),
  })

export function useCreateLink(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: LinkInput) => apiFetch<PoiLink>(`${base(tripId, poiId)}/links`, json('POST', input)),
    onSuccess: (link) => qc.setQueryData<PoiLink[]>(contentKeys.links(tripId, poiId), (list) => [...(list ?? []), link]),
  })
}

export function useDeleteLink(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiFetch<void>(`${base(tripId, poiId)}/links/${id}`, { method: 'DELETE' }),
    onSuccess: (_, id) =>
      qc.setQueryData<PoiLink[]>(contentKeys.links(tripId, poiId), (list) => list?.filter((l) => l.id !== id)),
  })
}

/** Photo d'un lieu ; image et vignette sont servies aux seuls membres du voyage. */
export interface PoiPhoto {
  id: string
  poi_id: string
  width: number
  height: number
  image_url: string
  thumb_url: string
  /** Image d'origine, si importée depuis le web. */
  source_url: string | null
  /** Page qui présente l'image. */
  source_page: string | null
  author_credit: string | null
  license: string | null
  author_id: string | null
  author_name: string | null
  via_assistant: boolean
  created_at: string
}

export interface PhotoCredit {
  author_credit?: string
  license?: string
  source_page?: string
}

/** Photo à ajouter : fichier envoyé, ou image importée depuis une adresse https. */
export type NewPhoto = ({ file: File } | { image_url: string }) & PhotoCredit

export const usePhotos = (tripId: string, poiId: string) =>
  useQuery({
    queryKey: contentKeys.photos(tripId, poiId),
    queryFn: () => apiFetch<PoiPhoto[]>(`${base(tripId, poiId)}/photos`),
  })

export function useAddPhoto(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (photo: NewPhoto) => {
      const { author_credit, license, source_page } = photo
      if ('file' in photo) {
        const form = new FormData()
        form.append('file', photo.file)
        for (const [key, value] of Object.entries({ author_credit, license, source_page })) {
          if (value?.trim()) form.append(key, value)
        }
        return apiFetch<PoiPhoto>(`${base(tripId, poiId)}/photos`, { method: 'POST', body: form })
      }
      return apiFetch<PoiPhoto>(
        `${base(tripId, poiId)}/photos/import`,
        json('POST', { image_url: photo.image_url, author_credit, license, source_page }),
      )
    },
    onSuccess: (photo) =>
      qc.setQueryData<PoiPhoto[]>(contentKeys.photos(tripId, poiId), (list) => [...(list ?? []), photo]),
  })
}

export function useDeletePhoto(tripId: string, poiId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => apiFetch<void>(`${base(tripId, poiId)}/photos/${id}`, { method: 'DELETE' }),
    onSuccess: (_, id) =>
      qc.setQueryData<PoiPhoto[]>(contentKeys.photos(tripId, poiId), (list) => list?.filter((p) => p.id !== id)),
  })
}

/** Domaine d'une adresse, sans `www.` (affiché à côté des sources et des liens). */
export function domainOf(url: string) {
  try {
    return new URL(url).hostname.replace(/^www\./, '')
  } catch {
    return url
  }
}
