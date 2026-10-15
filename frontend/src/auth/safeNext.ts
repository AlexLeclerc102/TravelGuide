/** Destination après connexion : uniquement un chemin interne (pas de redirection ouverte). */
export function safeNext(raw: string | null): string {
  if (raw && raw.startsWith('/') && !raw.startsWith('//')) return raw
  return '/'
}
