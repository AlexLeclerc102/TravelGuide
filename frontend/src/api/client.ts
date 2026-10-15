/** Erreur renvoyée par l'API (format `{ code, message }`). */
export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, code: string, message: string) {
    super(message)
    this.status = status
    this.code = code
  }
}

const BASE = '/api/v1'

/**
 * Identifiant de cet onglet, envoyé avec chaque requête (`X-Client-Id`) : le serveur le
 * renvoie dans les notifications temps réel, pour que l'onglet ignore ses propres changements.
 */
export const CLIENT_ID: string =
  typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`

export async function apiFetch<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers)
  headers.set('X-Client-Id', CLIENT_ID)
  if (init.body !== undefined && !headers.has('Content-Type')) {
    headers.set('Content-Type', 'application/json')
  }
  const res = await fetch(`${BASE}${path}`, { ...init, headers, credentials: 'include' })
  if (!res.ok) {
    let code = 'http_error'
    let message = res.statusText
    try {
      const body = (await res.json()) as { code?: string; message?: string }
      code = body.code ?? code
      message = body.message ?? message
    } catch {
      // corps non JSON : on garde le statut HTTP
    }
    throw new ApiError(res.status, code, message)
  }
  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}
