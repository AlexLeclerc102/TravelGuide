import { describe, expect, it } from 'vitest'
import { dayCount, formatDate, formatRating } from './format'

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
