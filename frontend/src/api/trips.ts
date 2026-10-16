import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { apiFetch } from './client'
import type { User } from './auth'

export type Role = 'organizer' | 'editor' | 'viewer'
export type TransportMode = 'walk' | 'bike' | 'car' | 'transit'
export const ALL_MODES: TransportMode[] = ['walk', 'bike', 'car', 'transit']
/** Envies du groupe (questionnaire de création du voyage). */
export type Interest = 'nature' | 'beach' | 'sight' | 'museum' | 'activity' | 'food' | 'shopping'
export const ALL_INTERESTS: Interest[] = ['nature', 'beach', 'sight', 'museum', 'activity', 'food', 'shopping']
export type Pace = 'relaxed' | 'balanced' | 'packed'
export type Budget = 'budget' | 'comfort'

export interface Trip {
  id: string
  name: string
  description: string
  start_date: string
  end_date: string
  timezone: string
  day_start: string
  day_end: string
  default_modes: TransportMode[]
  interests: Interest[]
  pace: Pace
  budget: Budget | null
  version: number
  my_role: Role
  member_count: number
  created_at: string
}

export interface TripInput {
  name: string
  description?: string
  start_date: string
  end_date: string
  timezone?: string
  day_start?: string
  day_end?: string
  default_modes?: TransportMode[]
  interests?: Interest[]
  pace?: Pace
  /** `null` : pas de préférence. */
  budget?: Budget | null
}

export interface Member {
  user_id: string
  display_name: string
  email: string | null
  role: Role
  joined_at: string
}

export interface Invitation {
  id: string
  role: Role
  created_at: string
  expires_at: string
  max_uses: number | null
  uses: number
  revoked_at: string | null
}

export interface InvitationPreview {
  trip_name: string
  start_date: string
  end_date: string
  invited_by: string | null
  role: Role
}

const json = (method: string, body: unknown): RequestInit => ({ method, body: JSON.stringify(body) })

export const tripKeys = {
  all: ['trips'] as const,
  one: (id: string) => ['trips', id] as const,
  members: (id: string) => ['trips', id, 'members'] as const,
  invitations: (id: string) => ['trips', id, 'invitations'] as const,
  days: (id: string) => ['trips', id, 'days'] as const,
  routes: (id: string) => ['trips', id, 'route'] as const,
  itinerary: (id: string) => ['trips', id, 'itinerary'] as const,
}

export const useTrips = () =>
  useQuery({ queryKey: tripKeys.all, queryFn: () => apiFetch<Trip[]>('/trips') })

export const useTrip = (id: string) =>
  useQuery({ queryKey: tripKeys.one(id), queryFn: () => apiFetch<Trip>(`/trips/${id}`) })

export function useCreateTrip() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: TripInput) => apiFetch<Trip>('/trips', json('POST', input)),
    onSuccess: (trip) => {
      qc.setQueryData(tripKeys.one(trip.id), trip)
      qc.invalidateQueries({ queryKey: tripKeys.all, exact: true })
    },
  })
}

export function useUpdateTrip(id: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patch: Partial<TripInput> & { version: number }) =>
      apiFetch<Trip>(`/trips/${id}`, json('PATCH', patch)),
    onSuccess: (trip) => {
      qc.setQueryData(tripKeys.one(id), trip)
      qc.invalidateQueries({ queryKey: tripKeys.all, exact: true })
      // Dates ou modes par défaut modifiés : les jours changent.
      qc.invalidateQueries({ queryKey: tripKeys.days(id) })
      qc.invalidateQueries({ queryKey: tripKeys.routes(id) })
      qc.invalidateQueries({ queryKey: tripKeys.itinerary(id) })
      // Envies et rythme modifiés : suggestions, régions et circuits changent.
      qc.invalidateQueries({ queryKey: [...tripKeys.one(id), 'suggestions'] })
    },
    // Conflit de version : on recharge la version à jour.
    onError: () => qc.invalidateQueries({ queryKey: tripKeys.one(id) }),
  })
}

export function useDeleteTrip(id: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: () => apiFetch<void>(`/trips/${id}`, { method: 'DELETE' }),
    onSuccess: () => {
      qc.removeQueries({ queryKey: tripKeys.one(id) })
      qc.invalidateQueries({ queryKey: tripKeys.all, exact: true })
    },
  })
}

export const useMembers = (id: string) =>
  useQuery({ queryKey: tripKeys.members(id), queryFn: () => apiFetch<Member[]>(`/trips/${id}/members`) })

export function useUpdateMember(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: ({ userId, role }: { userId: string; role: Role }) =>
      apiFetch<void>(`/trips/${tripId}/members/${userId}`, json('PATCH', { role })),
    onSuccess: () => qc.invalidateQueries({ queryKey: tripKeys.one(tripId) }),
  })
}

export function useRemoveMember(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (userId: string) =>
      apiFetch<void>(`/trips/${tripId}/members/${userId}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: tripKeys.all }),
  })
}

export const useInvitations = (tripId: string, enabled: boolean) =>
  useQuery({
    queryKey: tripKeys.invitations(tripId),
    queryFn: () => apiFetch<Invitation[]>(`/trips/${tripId}/invitations`),
    enabled,
  })

export function useCreateInvitation(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (input: { role: Role; expires_in_days?: number; max_uses?: number }) =>
      apiFetch<Invitation & { url: string }>(`/trips/${tripId}/invitations`, json('POST', input)),
    onSuccess: () => qc.invalidateQueries({ queryKey: tripKeys.invitations(tripId) }),
  })
}

export function useRevokeInvitation(tripId: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (invitationId: string) =>
      apiFetch<void>(`/trips/${tripId}/invitations/${invitationId}`, { method: 'DELETE' }),
    onSuccess: () => qc.invalidateQueries({ queryKey: tripKeys.invitations(tripId) }),
  })
}

export const useInvitationPreview = (token: string) =>
  useQuery({
    queryKey: ['invitation', token],
    queryFn: () => apiFetch<InvitationPreview>(`/invitations/${token}`),
    retry: false,
  })

export function useAcceptInvitation(token: string) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (body: { display_name?: string; email?: string }) =>
      apiFetch<{ trip_id: string; role: Role; user: User }>(
        `/invitations/${token}/accept`,
        json('POST', body),
      ),
    onSuccess: (res) => {
      qc.setQueryData(['me'], res.user)
      qc.invalidateQueries({ queryKey: tripKeys.all })
    },
  })
}

/** Statut lisible d'une invitation. */
export function invitationStatus(inv: Invitation, now = new Date()) {
  if (inv.revoked_at) return 'revoked'
  if (new Date(inv.expires_at) <= now) return 'expired'
  if (inv.max_uses !== null && inv.uses >= inv.max_uses) return 'exhausted'
  return 'active'
}
