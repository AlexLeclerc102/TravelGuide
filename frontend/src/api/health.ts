import { useQuery } from '@tanstack/react-query'
import { apiFetch } from './client'

export interface Health {
  status: string
  database: string
  version: string
}

export function useHealth() {
  return useQuery({
    queryKey: ['health'],
    queryFn: () => apiFetch<Health>('/health'),
    refetchInterval: 30_000,
    retry: false,
  })
}
