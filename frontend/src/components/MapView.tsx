import { useEffect, useRef, useState } from 'react'
import * as maplibregl from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'
// MapLibre cherche son worker à côté de son propre fichier, que Vite déplace (pré-bundling
// en dev, hachage en production) : on lui donne l'URL du fichier servi par Vite.
import workerUrl from 'maplibre-gl/dist/maplibre-gl-worker.mjs?url'
import type { Bounds, Category } from '../api/pois'
import { t } from '../i18n/fr'
import { useHoverStore, useMapHovered, type MapHover } from './mapHover'

// Tuiles vectorielles OpenFreeMap : gratuites, sans clé, données OpenStreetMap.
const STYLE_URL = 'https://tiles.openfreemap.org/styles/liberty'
// Centre du Costa Rica.
const DEFAULT_CENTER: [number, number] = [-84.1, 9.9]

// Charte « Carnet de route » : le fond OpenFreeMap est recoloré en carte ancienne.
const PALETTE = {
  land: '#EFE8D2',
  water: '#BFD0CE',
  waterway: '#9FB8B5',
  green: '#DCDFC6',
  building: '#E3DAC3',
  ink: '#1F2A37',
  mustard: '#E2B33C',
}

/** Recolore les couches du style (terre, eau, verdure, bâti) sans toucher aux routes ni aux libellés. */
function applyPalette(map: maplibregl.Map) {
  for (const layer of map.getStyle().layers ?? []) {
    const sourceLayer = 'source-layer' in layer ? layer['source-layer'] : undefined
    try {
      if (layer.type === 'background') map.setPaintProperty(layer.id, 'background-color', PALETTE.land)
      else if (layer.type === 'fill' && sourceLayer === 'water') map.setPaintProperty(layer.id, 'fill-color', PALETTE.water)
      else if (layer.type === 'line' && sourceLayer === 'waterway') map.setPaintProperty(layer.id, 'line-color', PALETTE.waterway)
      else if (layer.type === 'fill' && (sourceLayer === 'park' || sourceLayer === 'landcover'))
        map.setPaintProperty(layer.id, 'fill-color', PALETTE.green)
      else if (layer.type === 'fill' && sourceLayer === 'building') map.setPaintProperty(layer.id, 'fill-color', PALETTE.building)
    } catch {
      // Une couche aux propriétés inattendues garde ses couleurs d'origine.
    }
  }
}

maplibregl.setWorkerUrl(workerUrl)

export interface MapMarker {
  id: string
  name: string
  category: Category
  lat: number
  lon: number
  /** Suggestion (pas encore dans le voyage) : marqueur distinct. */
  suggestion?: boolean
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
  // Forme distincte par type (pas la couleur seule) : carré pour un hébergement, losange sinon.
  const kind = m.suggestion ? 'suggestion' : m.category === 'lodging' ? 'stay' : 'place'
  el.className = `poi-marker ${kind} cat-${m.category}`
  el.title = m.name
  el.setAttribute('aria-label', m.name)
  const shape = document.createElement('span')
  shape.className = 'poi-marker-shape'
  el.append(shape)
  el.addEventListener('click', (e) => {
    e.stopPropagation()
    onClick()
  })
  return el
}

const ROUTE_SOURCE = 'route'
const HOVER_SOURCE = 'hover-area'

/** Disque de `radiusM` mètres autour d'un point, en polygone `[lon, lat]`. */
function circle(lat: number, lon: number, radiusM: number, steps = 64): [number, number][] {
  const dLat = radiusM / 111_320
  const dLon = dLat / Math.cos((lat * Math.PI) / 180)
  return Array.from({ length: steps + 1 }, (_, i) => {
    const a = (i / steps) * 2 * Math.PI
    return [lon + dLon * Math.cos(a), lat + dLat * Math.sin(a)] as [number, number]
  })
}

function hoverBounds(h: MapHover) {
  const b = new maplibregl.LngLatBounds()
  if (h.kind === 'area') circle(h.lat, h.lon, h.radiusM, 16).forEach((c) => b.extend(c))
  else b.extend([h.lon, h.lat])
  return b
}

/** Repère provisoire d'un lieu survolé qui n'a pas de marqueur sur la carte. */
function ghostElement() {
  const el = document.createElement('div')
  el.className = 'poi-marker hover-ghost hovered'
  el.setAttribute('aria-hidden', 'true')
  const shape = document.createElement('span')
  shape.className = 'poi-marker-shape'
  el.append(shape)
  return el
}

/** Délai avant de déplacer la carte vers un élément survolé hors champ (survol « voulu »). */
const HOVER_PAN_DELAY_MS = 400

/** Durée du « vol » vers un lieu : 400 ms, rien si l'utilisateur réduit les animations. */
const flyDuration = () => (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 0 : 400)

export function MapView({ markers = [], selectedId, draft, addMode, route, onSelect, onMapClick, onBoundsChange }: Props) {
  const container = useRef<HTMLDivElement>(null)
  const mapRef = useRef<maplibregl.Map | null>(null)
  const markerRefs = useRef(new Map<string, { marker: maplibregl.Marker; key: string }>())
  const draftRef = useRef<maplibregl.Marker | null>(null)
  const ghostRef = useRef<maplibregl.Marker | null>(null)
  const hover = useMapHovered()
  const hoverStore = useHoverStore()
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
    map.on('load', () => {
      applyPalette(map)
      setStyleReady(true)
    })
    map.on('moveend', reportBounds)
    map.on('click', (e) => {
      if (callbacks.current.addMode) callbacks.current.onMapClick?.(e.lngLat.lat, e.lngLat.lng)
    })
    // Le panneau change de largeur selon l'onglet : la carte suit son conteneur.
    const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => map.resize())
    resize?.observe(container.current)
    const refs = markerRefs.current
    return () => {
      resize?.disconnect()
      refs.clear()
      draftRef.current = null
      ghostRef.current = null
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
      const key = `${m.lat},${m.lon},${m.category},${m.name},${m.suggestion ?? false}`
      const existing = refs.get(m.id)
      if (existing?.key === key) continue
      existing?.marker.remove()
      const marker = new maplibregl.Marker({ element: markerElement(m, () => callbacks.current.onSelect?.(m.id)) })
        .setLngLat([m.lon, m.lat])
        .addTo(map)
      refs.set(m.id, { marker, key })
    }
    // Premier affichage : cadrer sur les lieux du voyage (pas sur des suggestions).
    if (!fitted.current && markers.some((m) => !m.suggestion)) {
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
    if (selected) map.easeTo({ center: [selected.lon, selected.lat], zoom: Math.max(map.getZoom(), 12), duration: flyDuration() })
    // `markers` volontairement absent : on ne recentre pas à chaque modification de la liste.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedId])

  // Survol d'un élément du panneau : marqueur mis en évidence, repère provisoire ou disque de la
  // région ; la carte ne bouge que si l'élément, survolé un moment, est hors champ.
  useEffect(() => {
    const map = mapRef.current
    if (!map) return
    const match =
      hover?.kind === 'point'
        ? [...markerRefs.current].find(([id, { marker }]) => {
            if (hover.id) return id === hover.id
            const at = marker.getLngLat()
            return Math.abs(at.lat - hover.lat) < 1e-6 && Math.abs(at.lng - hover.lon) < 1e-6
          })
        : undefined
    for (const [id, { marker }] of markerRefs.current) marker.getElement().classList.toggle('hovered', id === match?.[0])
    ghostRef.current?.remove()
    ghostRef.current =
      hover?.kind === 'point' && !match
        ? new maplibregl.Marker({ element: ghostElement() }).setLngLat([hover.lon, hover.lat]).addTo(map)
        : null
    if (styleReady) {
      const data: GeoJSON.FeatureCollection = {
        type: 'FeatureCollection',
        features:
          hover?.kind === 'area'
            ? [
                {
                  type: 'Feature',
                  properties: {},
                  geometry: { type: 'Polygon', coordinates: [circle(hover.lat, hover.lon, hover.radiusM)] },
                },
              ]
            : [],
      }
      const source = map.getSource<maplibregl.GeoJSONSource>(HOVER_SOURCE)
      if (source) {
        source.setData(data)
      } else {
        map.addSource(HOVER_SOURCE, { type: 'geojson', data })
        map.addLayer({
          id: HOVER_SOURCE,
          type: 'fill',
          source: HOVER_SOURCE,
          paint: { 'fill-color': PALETTE.mustard, 'fill-opacity': 0.22 },
        })
        map.addLayer({
          id: `${HOVER_SOURCE}-line`,
          type: 'line',
          source: HOVER_SOURCE,
          paint: { 'line-color': PALETTE.ink, 'line-width': 2, 'line-dasharray': [2, 2] },
        })
      }
    }
  }, [hover, markers, styleReady])

  // Déplacement différé vers l'élément survolé : indépendant des mises à jour de `markers`.
  useEffect(() => {
    const map = mapRef.current
    if (!map || !hover) return
    const timer = setTimeout(() => {
      const view = map.getBounds()
      const b = hoverBounds(hover)
      if (view.contains(b.getSouthWest()) && view.contains(b.getNorthEast())) return
      if (hover.kind === 'area') map.fitBounds(b, { padding: 60, maxZoom: map.getZoom(), duration: flyDuration() })
      else map.easeTo({ center: [hover.lon, hover.lat], duration: flyDuration() })
    }, HOVER_PAN_DELAY_MS)
    return () => clearTimeout(timer)
  }, [hover])

  // Cadrage demandé par le panneau (région ouverte…).
  useEffect(
    () =>
      hoverStore.onFocus((target) => {
        const map = mapRef.current
        if (!map) return
        if (target.kind === 'area') map.fitBounds(hoverBounds(target), { padding: 60, duration: flyDuration() })
        else map.easeTo({ center: [target.lon, target.lat], zoom: Math.max(map.getZoom(), 12), duration: flyDuration() })
      }),
    [hoverStore],
  )

  // Marqueur provisoire du lieu en cours d'ajout.
  useEffect(() => {
    const map = mapRef.current
    if (!map) return
    draftRef.current?.remove()
    draftRef.current = draft ? new maplibregl.Marker({ color: PALETTE.mustard }).setLngLat([draft.lon, draft.lat]).addTo(map) : null
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
      // Trait encre doublé d'un pointillé moutarde.
      map.addLayer({
        id: ROUTE_SOURCE,
        type: 'line',
        source: ROUTE_SOURCE,
        layout: { 'line-cap': 'round', 'line-join': 'round' },
        paint: { 'line-color': PALETTE.ink, 'line-width': 5 },
      })
      map.addLayer({
        id: `${ROUTE_SOURCE}-dash`,
        type: 'line',
        source: ROUTE_SOURCE,
        layout: { 'line-join': 'round' },
        paint: { 'line-color': PALETTE.mustard, 'line-width': 2, 'line-dasharray': [3, 3.5] },
      })
    }
    if (route && route.length > 1) {
      const b = new maplibregl.LngLatBounds()
      route.forEach((c) => b.extend(c))
      map.fitBounds(b, { padding: 80, maxZoom: 15, duration: flyDuration() })
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
