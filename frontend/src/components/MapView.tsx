import { useEffect, useRef, useState } from 'react'
import * as maplibregl from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'
// MapLibre cherche son worker à côté de son propre fichier, que Vite déplace (pré-bundling
// en dev, hachage en production) : on lui donne l'URL du fichier servi par Vite.
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?url'
import type { Bounds, Category } from '../api/pois'
import { t } from '../i18n/fr'

// Tuiles vectorielles OpenFreeMap : gratuites, sans clé, données OpenStreetMap.
const STYLE_URL = 'https://tiles.openfreemap.org/styles/liberty'
// Centre du Costa Rica.
const DEFAULT_CENTER: [number, number] = [-84.1, 9.9]

maplibregl.setWorkerUrl(workerUrl)

export interface MapMarker {
  id: string
  name: string
  category: Category
  lat: number
  lon: number
}

interface Props {
  markers?: MapMarker[]
  selectedId?: string | null
  /** Lieu en cours d'ajout (marqueur provisoire). */
  draft?: { lat: number; lon: number } | null
  /** Mode « ajout par clic » : curseur en croix, les clics appellent `onMapClick`. */
  addMode?: boolean
  /** Tracé `[lon, lat]` d'un trajet à afficher (`null` : aucun). */
  route?: [number, number][] | null
  onSelect?: (id: string) => void
  onMapClick?: (lat: number, lon: number) => void
  onBoundsChange?: (bounds: Bounds) => void
}

function markerElement(m: MapMarker, onClick: () => void) {
  const el = document.createElement('button')
  el.type = 'button'
  el.className = `poi-marker cat-${m.category}`
  el.title = m.name
  el.setAttribute('aria-label', m.name)
  el.textContent = t.categoryIcons[m.category]
  el.addEventListener('click', (e) => {
    e.stopPropagation()
    onClick()
  })
  return el
}

const ROUTE_SOURCE = 'route'

export function MapView({ markers = [], selectedId, draft, addMode, route, onSelect, onMapClick, onBoundsChange }: Props) {
  const container = useRef<HTMLDivElement>(null)
  const mapRef = useRef<maplibregl.Map | null>(null)
  const markerRefs = useRef(new Map<string, { marker: maplibregl.Marker; key: string }>())
  const draftRef = useRef<maplibregl.Marker | null>(null)
  const fitted = useRef(false)
  const [failed, setFailed] = useState(false)
  // Le style doit être chargé avant d'ajouter une source (tracé du trajet).
  const [styleReady, setStyleReady] = useState(false)
  // Rappels courants, lus par les écouteurs créés une seule fois.
  const callbacks = useRef({ onSelect, onMapClick, onBoundsChange, addMode })
  callbacks.current = { onSelect, onMapClick, onBoundsChange, addMode }

  useEffect(() => {
    if (!container.current) return
    let map: maplibregl.Map
    try {
      map = new maplibregl.Map({ container: container.current, style: STYLE_URL, center: DEFAULT_CENTER, zoom: 7 })
    } catch (e) {
      // Sans WebGL, MapLibre lève une exception : on affiche un repli au lieu de tout casser.
      console.warn('[carte] initialisation impossible', e)
      setFailed(true)
      return
    }
    mapRef.current = map
    map.addControl(new maplibregl.NavigationControl(), 'top-right')
    const reportBounds = () => {
      const b = map.getBounds()
      callbacks.current.onBoundsChange?.([b.getWest(), b.getSouth(), b.getEast(), b.getNorth()])
    }
    map.on('load', reportBounds)
    map.on('load', () => setStyleReady(true))
    map.on('moveend', reportBounds)
    map.on('click', (e) => {
      if (callbacks.current.addMode) callbacks.current.onMapClick?.(e.lngLat.lat, e.lngLat.lng)
    })
    const refs = markerRefs.current
    return () => {
      refs.clear()
      draftRef.current = null
      mapRef.current = null
      map.remove()
    }
  }, [])

  // Synchronise les marqueurs avec la liste des lieux.
  useEffect(() => {
    const map = mapRef.current
    if (!map) return
    const refs = markerRefs.current
    const ids = new Set(markers.map((m) => m.id))
    for (const [id, { marker }] of refs) {
      if (!ids.has(id)) {
        marker.remove()
        refs.delete(id)
      }
    }
    for (const m of markers) {
      const key = `${m.lat},${m.lon},${m.category},${m.name}`
      const existing = refs.get(m.id)
      if (existing?.key === key) continue
      existing?.marker.remove()
      const marker = new maplibregl.Marker({ element: markerElement(m, () => callbacks.current.onSelect?.(m.id)) })
        .setLngLat([m.lon, m.lat])
        .addTo(map)
      refs.set(m.id, { marker, key })
    }
    // Premier affichage : cadrer sur les lieux du voyage.
    if (!fitted.current && markers.length > 0) {
      fitted.current = true
      if (markers.length === 1) {
        map.jumpTo({ center: [markers[0].lon, markers[0].lat], zoom: 13 })
      } else {
        const b = new maplibregl.LngLatBounds()
        markers.forEach((m) => b.extend([m.lon, m.lat]))
        map.fitBounds(b, { padding: 80, maxZoom: 14, duration: 0 })
      }
    }
  }, [markers])

  // Mise en évidence et centrage du lieu sélectionné.
  useEffect(() => {
    const map = mapRef.current
    if (!map) return
    for (const [id, { marker }] of markerRefs.current) {
      marker.getElement().classList.toggle('selected', id === selectedId)
    }
    const selected = markers.find((m) => m.id === selectedId)
    if (selected) map.easeTo({ center: [selected.lon, selected.lat], zoom: Math.max(map.getZoom(), 12) })
    // `markers` volontairement absent : on ne recentre pas à chaque modification de la liste.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId])

  // Marqueur provisoire du lieu en cours d'ajout.
  useEffect(() => {
    const map = mapRef.current
    if (!map) return
    draftRef.current?.remove()
    draftRef.current = draft ? new maplibregl.Marker({ color: '#0f766e' }).setLngLat([draft.lon, draft.lat]).addTo(map) : null
  }, [draft])

  useEffect(() => {
    mapRef.current?.getCanvas().style.setProperty('cursor', addMode ? 'crosshair' : '')
  }, [addMode])

  // Tracé du trajet : une seule source GeoJSON, mise à jour à chaque changement.
  useEffect(() => {
    const map = mapRef.current
    if (!map || !styleReady) return
    const data = {
      type: 'Feature' as const,
      properties: {},
      geometry: { type: 'LineString' as const, coordinates: route ?? [] },
    }
    const source = map.getSource<maplibregl.GeoJSONSource>(ROUTE_SOURCE)
    if (source) {
      source.setData(data)
    } else {
      map.addSource(ROUTE_SOURCE, { type: 'geojson', data })
      map.addLayer({
        id: ROUTE_SOURCE,
        type: 'line',
        source: ROUTE_SOURCE,
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': '#0f766e', 'line-width': 5, 'line-opacity': 0.8 },
      })
    }
    if (route && route.length > 1) {
      const b = new maplibregl.LngLatBounds()
      route.forEach((c) => b.extend(c))
      map.fitBounds(b, { padding: 80, maxZoom: 15 })
    }
  }, [route, styleReady])

  if (failed) {
    return (
      <div className="map map-fallback" data-testid="map" role="status">
        <p>{t.map.unavailable}</p>
      </div>
    )
  }
  return <div ref={container} className="map" data-testid="map" />
}
