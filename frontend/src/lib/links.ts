import type { Poi } from '../api/pois'

export type LinkKind = 'website' | 'phone' | 'email' | 'wikipedia' | 'osm' | 'googleMaps' | 'directions'

export interface PoiLink {
  kind: LinkKind
  href: string
  /** Texte affiché quand la valeur elle-même est parlante (numéro, adresse email). */
  value?: string
}

type LinkSource = Pick<Poi, 'name' | 'lat' | 'lon' | 'address' | 'website' | 'phone' | 'email' | 'wikipedia' | 'osm'>

/** `es:Volcán Arenal` → https://es.wikipedia.org/wiki/Volc%C3%A1n_Arenal */
export function wikipediaUrl(tag: string): string | null {
  const sep = tag.indexOf(':')
  if (sep < 2) return null
  const lang = tag.slice(0, sep)
  const title = tag.slice(sep + 1).trim()
  if (!/^[a-z-]+$/.test(lang) || !title) return null
  return `https://${lang}.wikipedia.org/wiki/${encodeURIComponent(title.replace(/ /g, '_'))}`
}

/** Texte de recherche d'un lieu : nom et adresse (l'adresse OSM commence souvent par le nom). */
export function placeQuery(p: Pick<Poi, 'name' | 'address'>) {
  const address = p.address.trim()
  if (!address) return p.name
  return address.startsWith(p.name) ? address : `${p.name}, ${address}`
}

export interface StaySearch {
  query: string
  checkin: string
  checkout: string
  adults: number
}

/**
 * Recherche d'hébergement pour ces dates sur Booking.com et Airbnb : simples liens de
 * recherche, sans affiliation ni suivi (rien n'est envoyé avant le clic).
 */
export function lodgingSearchLinks({ query, checkin, checkout, adults }: StaySearch) {
  const booking = new URLSearchParams({ ss: query, checkin, checkout, group_adults: String(adults), no_rooms: '1' })
  const airbnb = new URLSearchParams({ checkin, checkout, adults: String(adults) })
  return {
    booking: `https://www.booking.com/searchresults.html?${booking}`,
    airbnb: `https://www.airbnb.com/s/${encodeURIComponent(query)}/homes?${airbnb}`,
  }
}

/**
 * Liens utiles d'un lieu, du plus précis au plus général : site officiel, contact, Wikipédia,
 * fiche OpenStreetMap, puis recherche et itinéraire dans Google Maps. Rien n'est appelé
 * tant que l'utilisateur ne clique pas.
 */
export function poiLinks(p: LinkSource): PoiLink[] {
  const links: PoiLink[] = []
  const at = `${p.lat.toFixed(6)},${p.lon.toFixed(6)}`
  if (p.website) links.push({ kind: 'website', href: p.website })
  if (p.phone) links.push({ kind: 'phone', href: `tel:${p.phone.replace(/[^\d+]/g, '')}`, value: p.phone })
  if (p.email) links.push({ kind: 'email', href: `mailto:${p.email}`, value: p.email })
  const wiki = p.wikipedia && wikipediaUrl(p.wikipedia)
  if (wiki) links.push({ kind: 'wikipedia', href: wiki })
  links.push({
    kind: 'osm',
    href: p.osm
      ? `https://www.openstreetmap.org/${p.osm.type}/${p.osm.id}`
      : `https://www.openstreetmap.org/?mlat=${p.lat}&mlon=${p.lon}#map=17/${p.lat}/${p.lon}`,
  })
  // Le nom et l'adresse retrouvent la fiche du lieu ; sans adresse, les coordonnées.
  const query = p.address.trim() ? placeQuery(p) : at
  links.push({ kind: 'googleMaps', href: `https://www.google.com/maps/search/?api=1&query=${encodeURIComponent(query)}` })
  links.push({ kind: 'directions', href: `https://www.google.com/maps/dir/?api=1&destination=${at}` })
  return links
}
