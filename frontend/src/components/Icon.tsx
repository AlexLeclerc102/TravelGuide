/**
 * Pictogrammes au trait (charte « Carnet de route » : pas d'emoji, traits de 1,8 px).
 * Décoratifs par défaut : le texte voisin porte le sens.
 */
const PATHS = {
  // Catégories de lieux
  sight: 'M12 3l2.6 5.6 6 .7-4.5 4.1 1.2 6L12 16.4 6.7 19.4l1.2-6L3.4 9.3l6-.7z',
  nature: 'M3 20l6-11 4 7 2.5-4L21 20zM9 9l1.6-3',
  museum: 'M3 9l9-5 9 5M5 10v8M9.5 10v8M14.5 10v8M19 10v8M3 20h18',
  food: 'M7 3v8M5 3v5a2 2 0 004 0V3M7 11v10M16 3c-2 1.5-2.5 4-2.5 7H17V3zM17 10v11',
  activity: 'M4 8h16v3a2 2 0 000 4v3H4v-3a2 2 0 000-4zM10 8v10',
  lodging: 'M3 19V6M3 15h18v4M21 15v-3a3 3 0 00-3-3h-7v6M7 12.5a1.5 1.5 0 100-.01',
  shopping: 'M5 8h14l-1 12H6zM9 8V6a3 3 0 016 0v2',
  other: 'M12 21s-6-6.2-6-11a6 6 0 0112 0c0 4.8-6 11-6 11zM12 12a2 2 0 100-.01',
  // Modes de déplacement
  walk: 'M13 4.5a1.5 1.5 0 100-.01M10 21l2-6 3 3v3M12 15l-1-5 3-1 2 4 3 1M11 10l-3 2v3',
  bike: 'M6 17a3 3 0 100-.01M18 17a3 3 0 100-.01M6 17l4-8h5l3 8M10 9l3 8M14 6h2',
  car: 'M4 16v-4l2-5h12l2 5v4zM4 16v2h3v-2M17 16v2h3v-2M7.5 13h.01M16.5 13h.01M4 12h16',
  transit: 'M6 4h12v12H6zM6 11h12M8.5 14h.01M15.5 14h.01M8 16l-2 4M16 16l2 4',
  boat: 'M3 16l2 4h14l2-4zM6 16V10h12v6M12 10V4M12 5l5 3',
  // Liens d'un lieu
  website: 'M12 21a9 9 0 100-18 9 9 0 000 18zM3 12h18M12 3c3 3 3 15 0 18M12 3c-3 3-3 15 0 18',
  phone: 'M5 4h4l2 5-2.5 1.5a11 11 0 005 5L15 13l5 2v4a1 1 0 01-1 1A16 16 0 014 5a1 1 0 011-1z',
  email: 'M3 6h18v12H3zM3 7l9 7 9-7',
  wikipedia: 'M4 5h6M14 5h6M5.5 5l4 14 2.5-8 2.5 8 4-14',
  osm: 'M3 6l6-2 6 2 6-2v14l-6 2-6-2-6 2zM9 4v14M15 6v14',
  googleMaps: 'M12 21s-6-6.2-6-11a6 6 0 0112 0c0 4.8-6 11-6 11zM12 12a2 2 0 100-.01',
  directions: 'M12 3l9 9-9 9-9-9zM9 14v-3h5M12 9l2 2-2 2',
  // Interface
  pin: 'M12 21s-6-6.2-6-11a6 6 0 0112 0c0 4.8-6 11-6 11zM12 12a2 2 0 100-.01',
  lock: 'M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 017 0v3',
  unlock: 'M6 11h12v9H6zM8.5 11V8a3.5 3.5 0 016.8-1.2',
  warning: 'M12 4l9 16H3zM12 10v4M12 17h.01',
  split: 'M12 3v18M5 8l-2 4 2 4M19 8l2 4-2 4',
  heritage: 'M3 9l9-5 9 5M5 10v8M9.5 10v8M14.5 10v8M19 10v8M3 20h18',
  info: 'M12 21a9 9 0 100-18 9 9 0 000 18zM12 11v6M12 7.5h.01',
  map: 'M3 6l6-2 6 2 6-2v14l-6 2-6-2-6 2zM9 4v14M15 6v14',
  expand: 'M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5',
  idea: 'M12 21a9 9 0 100-18 9 9 0 000 18zM15.5 8.5l-2 5-5 2 2-5z',
  calendar: 'M4 6h16v14H4zM4 10h16M8 3v4M16 3v4',
  route: 'M6 19a2 2 0 100-.01M18 7a2 2 0 100-.01M8 19h7a3 3 0 000-6H9a3 3 0 010-6h7',
  members: 'M9 11a3.5 3.5 0 100-7 3.5 3.5 0 000 7zM2.5 20a6.5 6.5 0 0113 0M16 4.5a3.5 3.5 0 010 6.5M18 14a6 6 0 013.5 6',
  settings: 'M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M10 15v4',
  search: 'M10.5 17a6.5 6.5 0 100-13 6.5 6.5 0 000 13zM15.5 15.5L20 20',
} as const

export type IconName = keyof typeof PATHS

export function Icon({ name, size = 18, label }: { name: IconName | string; size?: number; label?: string }) {
  const d = (PATHS as Record<string, string>)[name] ?? PATHS.other
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      {...(label ? { role: 'img', 'aria-label': label } : { 'aria-hidden': true, focusable: 'false' })}
    >
      <path d={d} />
    </svg>
  )
}
