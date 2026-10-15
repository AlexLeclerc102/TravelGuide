import { describe, expect, it } from 'vitest'
import { dayCount, formatDate } from './format'

describe('format', () => {
  it('formate une date ISO sans décalage', () => {
    expect(formatDate('2026-12-01')).toBe('1 décembre 2026')
  })
  it('compte les jours bornes incluses', () => {
    expect(dayCount('2026-12-01', '2026-12-01')).toBe(1)
    expect(dayCount('2026-12-01', '2026-12-08')).toBe(8)
  })
})
