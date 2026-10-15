import { describe, expect, it } from 'vitest'
import { dayCount, formatDate, formatDistance, formatDuration, formatRating } from './format'

describe('format', () => {
  it('formate une date ISO sans décalage', () => {
    expect(formatDate('2026-12-01')).toBe('1 décembre 2026')
  })
  it('compte les jours bornes incluses', () => {
    expect(dayCount('2026-12-01', '2026-12-01')).toBe(1)
    expect(dayCount('2026-12-01', '2026-12-08')).toBe(8)
  })
})

describe('formatRating', () => {
  it('arrondit à une décimale avec la virgule', () => {
    expect(formatRating(13 / 3)).toBe('4,3')
    expect(formatRating(5)).toBe('5')
    expect(formatRating(3.5)).toBe('3,5')
  })
})

describe('durées et distances de trajet', () => {
  it('formate les durées à la minute', () => {
    expect(formatDuration(0)).toBe('0 min')
    expect(formatDuration(20)).toBe('1 min')
    expect(formatDuration(840)).toBe('14 min')
    expect(formatDuration(3600)).toBe('1 h')
    expect(formatDuration(3900)).toBe('1 h 05')
  })

  it('formate les distances', () => {
    expect(formatDistance(847)).toBe('850 m')
    expect(formatDistance(1060)).toBe('1,1 km')
    expect(formatDistance(128_400)).toBe('128 km')
  })
})
