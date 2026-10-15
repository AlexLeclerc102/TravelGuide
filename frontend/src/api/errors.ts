import { ApiError } from './client'
import { t } from '../i18n/fr'

/** Message lisible pour une erreur d'API (le backend renvoie des messages en français). */
export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.status === 429) return t.login.tooManyAttempts
    if (error.status < 500 && error.message) return capitalize(error.message)
  }
  return t.common.unexpectedError
}

function capitalize(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1) + (s.endsWith('.') ? '' : '.')
}
