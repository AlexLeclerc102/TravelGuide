import { useEffect, useRef } from 'react'
import * as maplibregl from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'

// Tuiles vectorielles OpenFreeMap : gratuites, sans clé, données OpenStreetMap.
const STYLE_URL = 'https://tiles.openfreemap.org/styles/liberty'
// Centre du Costa Rica.
const DEFAULT_CENTER: [number, number] = [-84.1, 9.9]

export function MapView() {
  const container = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!container.current) return
    const map = new maplibregl.Map({
      container: container.current,
      style: STYLE_URL,
      center: DEFAULT_CENTER,
      zoom: 7,
    })
    map.addControl(new maplibregl.NavigationControl(), 'top-right')
    return () => map.remove()
  }, [])

  return <div ref={container} className="map" data-testid="map" />
}
