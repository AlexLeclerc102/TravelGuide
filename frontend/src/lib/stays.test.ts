import { describe, expect, it } from 'vitest'
import { addDays, stays } from './stays'

describe('séjours', () => {
  it('ajoute des jours en passant les mois et les années', () => {
    expect(addDays('2026-12-31', 1)).toBe('2027-01-01')
    expect(addDays('2026-03-01', -1)).toBe('2026-02-28')
  })

  it('regroupe les nuits consécutives au même hébergement', () => {
    const days = [
      { date: '2026-12-01', base_poi_id: 'sj' },
      { date: '2026-12-02', base_poi_id: 'fortuna' },
      { date: '2026-12-03', base_poi_id: 'fortuna' },
      { date: '2026-12-04', base_poi_id: null },
      { date: '2026-12-05', base_poi_id: 'sj' },
    ]
    expect(stays(days).map((s) => [s.poiId, s.nights, s.checkin, s.checkout])).toEqual([
      ['sj', 1, '2026-12-01', '2026-12-02'],
      ['fortuna', 2, '2026-12-02', '2026-12-04'],
      [null, 1, '2026-12-04', '2026-12-05'],
      ['sj', 1, '2026-12-05', '2026-12-06'],
    ])
  })
})
