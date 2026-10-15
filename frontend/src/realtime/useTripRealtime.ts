import { useEffect, useRef, useState } from 'react'
import { useQueryClient, type QueryClient } from '@tanstack/react-query'
import { meKey } from '../api/auth'
import { ApiError, apiFetch, CLIENT_ID } from '../api/client'
import { t } from '../i18n/fr'
import { poiKeys } from '../api/pois'
import { suggestionKeys } from '../api/suggestions'
import { tripKeys } from '../api/trips'

/** Ce qui a changé dans le voyage (voir `realtime::Topic` côté serveur). */
export type Topic = 'trip' | 'members' | 'invitations' | 'pois' | 'reviews' | 'days' | 'itinerary' | 'suggestions'

export interface Person {
  user_id: string
  name: string
}

type ServerMessage =
  | { type: 'changed'; topics: Topic[]; by: Person; client_id: string | null }
  | { type: 'presence'; online: Person[] }
  | { type: 'removed' }
  | { type: 'resync' }

export type RealtimeStatus = 'connecting' | 'online' | 'offline'

export interface LastChange {
  by: Person
  topics: Topic[]
  at: number
}

/** Premier délai de reconnexion, doublé à chaque échec (plafonné). */
const RECONNECT_BASE_MS = 500
const RECONNECT_MAX_MS = 30_000

/** Recharge les données concernées par une notification. */
export function invalidateTopics(qc: QueryClient, tripId: string, topics: Topic[]) {
  const invalidate = (queryKey: readonly unknown[], exact = false) => qc.invalidateQueries({ queryKey, exact })
  for (const topic of topics) {
    switch (topic) {
      case 'trip':
        invalidate(tripKeys.one(tripId), true)
        invalidate(tripKeys.all, true)
        break
      case 'members':
        invalidate(tripKeys.members(tripId))
        break
      case 'invitations':
        invalidate(tripKeys.invitations(tripId))
        break
      case 'pois':
        invalidate(poiKeys.list(tripId), true)
        invalidate(tripKeys.routes(tripId))
        break
      case 'reviews':
        // Notes de la liste et avis détaillés de chaque lieu.
        invalidate(poiKeys.list(tripId))
        break
      case 'days':
        invalidate(tripKeys.days(tripId))
        invalidate(tripKeys.routes(tripId))
        break
      case 'itinerary':
        invalidate(tripKeys.itinerary(tripId))
        break
    }
  }
  // Les suggestions dépendent des lieux, des jours, des avis, de l'itinéraire et des refus.
  if (topics.some((topic) => topic !== 'members' && topic !== 'invitations')) invalidate(suggestionKeys.all(tripId))
}

/** Sujet le plus parlant d'une notification (une modification en touche souvent plusieurs). */
const PRIORITY: Topic[] = ['members', 'reviews', 'pois', 'trip', 'days', 'itinerary', 'invitations']

/** « Ana a modifié les lieux (14:05) ». */
export function describeChange(change: LastChange) {
  const topic = PRIORITY.find((p) => change.topics.includes(p)) ?? change.topics[0]
  const time = new Date(change.at).toLocaleTimeString('fr-FR', { hour: '2-digit', minute: '2-digit' })
  return t.realtime.changed(change.by.name, t.realtime.topics[topic] ?? '', time)
}

function socketUrl(tripId: string) {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${protocol}//${window.location.host}/api/v1/trips/${tripId}/ws`
}

/**
 * Canal temps réel d'un voyage : recharge ce que les autres membres modifient, donne la
 * liste des membres en ligne et se reconnecte seul (délai croissant, puis tout est rechargé
 * pour rattraper ce qui a pu être manqué).
 */
export function useTripRealtime(tripId: string, onRemoved: () => void) {
  const qc = useQueryClient()
  const [status, setStatus] = useState<RealtimeStatus>('connecting')
  const [online, setOnline] = useState<Person[]>([])
  const [lastChange, setLastChange] = useState<LastChange | null>(null)
  const removed = useRef(onRemoved)
  useEffect(() => {
    removed.current = onRemoved
  })

  useEffect(() => {
    if (typeof WebSocket === 'undefined') return
    let socket: WebSocket | null = null
    let timer: ReturnType<typeof setTimeout> | undefined
    let attempts = 0
    let stopped = false

    const scheduleReconnect = () => {
      const delay = Math.min(RECONNECT_MAX_MS, RECONNECT_BASE_MS * 2 ** attempts)
      attempts += 1
      timer = setTimeout(connect, delay)
    }

    // Connexion refusée : le navigateur ne donne pas le statut HTTP de la poignée de main,
    // on demande donc le voyage. Session expirée ou accès perdu : inutile de réessayer.
    const checkAccess = async () => {
      try {
        await apiFetch(`/trips/${tripId}`)
      } catch (e) {
        if (stopped) return
        if (e instanceof ApiError && e.status === 401) {
          stopped = true
          qc.invalidateQueries({ queryKey: meKey })
          return
        }
        if (e instanceof ApiError && (e.status === 403 || e.status === 404)) {
          stopped = true
          removed.current()
          return
        }
      }
      if (!stopped) scheduleReconnect()
    }

    const connect = () => {
      let opened = false
      setStatus('connecting')
      socket = new WebSocket(socketUrl(tripId))
      socket.onopen = () => {
        opened = true
        setStatus('online')
        // Après une coupure, des notifications ont pu être perdues.
        if (attempts > 0) qc.invalidateQueries({ queryKey: tripKeys.one(tripId) })
        attempts = 0
      }
      socket.onmessage = (event) => {
        let message: ServerMessage
        try {
          message = JSON.parse(String(event.data)) as ServerMessage
        } catch {
          return
        }
        switch (message.type) {
          case 'changed':
            // Nos propres modifications sont déjà à jour dans cet onglet.
            if (message.client_id === CLIENT_ID) return
            invalidateTopics(qc, tripId, message.topics)
            setLastChange({ by: message.by, topics: message.topics, at: Date.now() })
            break
          case 'presence':
            setOnline(message.online)
            break
          case 'resync':
            qc.invalidateQueries({ queryKey: tripKeys.one(tripId) })
            break
          case 'removed':
            stopped = true
            removed.current()
            break
        }
      }
      socket.onclose = () => {
        socket = null
        if (stopped) return
        setStatus('offline')
        setOnline([])
        if (opened) scheduleReconnect()
        else void checkAccess()
      }
    }

    connect()
    return () => {
      stopped = true
      clearTimeout(timer)
      socket?.close()
    }
  }, [qc, tripId])

  return { status, online, lastChange }
}
