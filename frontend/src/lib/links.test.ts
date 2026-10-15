import { describe, expect, it } from 'vitest'
import { lodgingSearchLinks, poiLinks, wikipediaUrl } from './links'

const base = {
  name: 'Volcán Arenal',
  lat: 10.4621,
  lon: -84.7034,
  address: '',
  website: null,
  phone: null,
  email: null,
  wikipedia: null,
  osm: null,
}

describe('liens d’un lieu', () => {
  it('construit l’adresse Wikipédia depuis le tag OSM', () => {
    expect(wikipediaUrl('es:Volcán Arenal')).toBe('https://es.wikipedia.org/wiki/Volc%C3%A1n_Arenal')
    expect(wikipediaUrl('Volcán Arenal')).toBeNull()
    expect(wikipediaUrl('ES:Arenal')).toBeNull()
  })

  it('propose toujours la carte et l’itinéraire, même sans autre information', () => {
    const links = poiLinks(base)
    expect(links.map((l) => l.kind)).toEqual(['osm', 'googleMaps', 'directions'])
    expect(links[0].href).toBe('https://www.openstreetmap.org/?mlat=10.4621&mlon=-84.7034#map=17/10.4621/-84.7034')
    // Sans adresse, Google Maps cherche les coordonnées.
    expect(links[1].href).toBe('https://www.google.com/maps/search/?api=1&query=10.462100%2C-84.703400')
  })

  it('cherche le lieu par son nom et son adresse', () => {
    const [, maps] = poiLinks({ ...base, address: 'La Fortuna, Costa Rica' })
    expect(decodeURIComponent(maps.href)).toContain('query=Volcán Arenal, La Fortuna, Costa Rica')
    // Adresse OSM commençant déjà par le nom : pas de doublon.
    const [, again] = poiLinks({ ...base, address: 'Volcán Arenal, La Fortuna' })
    expect(decodeURIComponent(again.href)).toContain('query=Volcán Arenal, La Fortuna')
    expect(decodeURIComponent(again.href)).not.toContain('Arenal, Volcán')
  })

  it('ordonne site, contacts, Wikipédia puis cartes', () => {
    const links = poiLinks({
      ...base,
      website: 'https://arenal.example',
      phone: '+506 2479-1111',
      email: 'info@arenal.example',
      wikipedia: 'es:Volcán Arenal',
      osm: { type: 'node', id: 276216895 },
    })
    expect(links.map((l) => l.kind)).toEqual(['website', 'phone', 'email', 'wikipedia', 'osm', 'googleMaps', 'directions'])
    expect(links[1]).toMatchObject({ href: 'tel:+50624791111', value: '+506 2479-1111' })
    expect(links[2].href).toBe('mailto:info@arenal.example')
    expect(links[4].href).toBe('https://www.openstreetmap.org/node/276216895')
  })
})

describe('recherche d’hébergement', () => {
  it('reprend la ville, les dates et le nombre de voyageurs', () => {
    const { booking, airbnb } = lodgingSearchLinks({
      query: 'La Fortuna, Costa Rica',
      checkin: '2026-12-02',
      checkout: '2026-12-05',
      adults: 3,
    })
    const b = new URL(booking)
    expect(b.origin + b.pathname).toBe('https://www.booking.com/searchresults.html')
    expect(Object.fromEntries(b.searchParams)).toEqual({
      ss: 'La Fortuna, Costa Rica',
      checkin: '2026-12-02',
      checkout: '2026-12-05',
      group_adults: '3',
      no_rooms: '1',
    })
    expect(airbnb).toBe('https://www.airbnb.com/s/La%20Fortuna%2C%20Costa%20Rica/homes?checkin=2026-12-02&checkout=2026-12-05&adults=3')
  })
})
