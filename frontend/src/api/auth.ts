import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ApiError, apiFetch } from './client'

export interface User {
  id: string
  email: string
  display_name: string
  plan: 'free' | 'organizer'
  has_password: boolean
  created_at: string
}

export const meKey = ['me'] as const

/** Utilisateur connecté, ou `null` si aucune session. */
export function useMe() {
  return useQuery({
    queryKey: meKey,
    queryFn: async () => {
      try {
        return await apiFetch<User>('/me')
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) return null
        throw e
      }
    },
    staleTime: 60_000,
  })
}

function useSetsMe<TVars>(fn: (vars: TVars) => Promise<User>) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: fn,
    onSuccess: (user) => qc.setQueryData(meKey, user),
  })
}

const post = <T>(path: string, body: unknown) =>
  apiFetch<T>(path, { method: 'POST', body: JSON.stringify(body) })

export const useLogin = () =>
  useSetsMe((v: { email: string; password: string }) => post<User>('/auth/login', v))

export const useRegister = () =>
  useSetsMe((v: { email: string; password: string; display_name: string }) =>
    post<User>('/auth/register', v),
  )

export const useVerifyMagicLink = () =>
  useSetsMe((token: string) => post<User>('/auth/magic-link/verify', { token }))

export const useRequestMagicLink = () =>
  useMutation({
    mutationFn: (v: { email: string; next?: string }) =>
      post<{ message: string }>('/auth/magic-link', v),
  })

export const useUpdateMe = () =>
  useSetsMe((v: { display_name: string }) =>
    apiFetch<User>('/me', { method: 'PATCH', body: JSON.stringify(v) }),
  )

export function useChangePassword() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (v: { current_password?: string; new_password: string }) =>
      apiFetch<void>('/me/password', { method: 'PUT', body: JSON.stringify(v) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: meKey }),
  })
}

/** Après déconnexion ou suppression : on vide tout le cache. */
function useClearsSession<TVars>(fn: (vars: TVars) => Promise<void>) {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      qc.clear()
      qc.setQueryData(meKey, null)
    },
  })
}

export const useLogout = () => useClearsSession(() => post<void>('/auth/logout', {}))

export const useDeleteAccount = () =>
  useClearsSession((v: { confirm_email: string; password?: string }) =>
    apiFetch<void>('/me', { method: 'DELETE', body: JSON.stringify(v) }),
  )

/** Télécharge l'export RGPD sous forme de fichier JSON. */
export async function downloadExport() {
  const data = await apiFetch<unknown>('/me/export')
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = 'travelguide-export.json'
  a.click()
  URL.revokeObjectURL(url)
}
