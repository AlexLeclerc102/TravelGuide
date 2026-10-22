import type { ReactNode } from 'react'
import { t } from '../i18n/fr'

/**
 * Écran partagé des pages de compte : le formulaire à gauche, une carte décorative à droite
 * (masquée sur mobile, réduite à un bandeau).
 */
export function AuthShell({ kicker, children, perks }: { kicker?: string; children: ReactNode; perks?: boolean }) {
  return (
    <div className="auth-split">
      <div className="auth-column">
        <div className="auth-card">
          {kicker && <p className="kicker">{kicker}</p>}
          {children}
        </div>
      </div>
      <aside className="auth-aside">
        <svg className="auth-map" viewBox="0 0 1000 900" preserveAspectRatio="xMidYMid slice" aria-hidden="true" focusable="false">
          <defs>
            <pattern id="auth-hatch" width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
              <line x1="0" y1="0" x2="0" y2="8" stroke="#8DA68A" strokeWidth="2" />
            </pattern>
          </defs>
          <path
            d="M70,170 C150,140 260,120 340,110 L420,170 L520,250 L640,340 L760,430 L860,520 L930,600 L960,680 L900,760 L820,790 L760,740 L700,770 L630,730 L560,690 L500,640 L440,610 L400,560 L360,600 L320,570 L330,500 L290,450 L220,420 L160,360 L120,290 Z"
            fill="#EFE8D2"
            stroke="#1F2A37"
            strokeWidth="1.5"
          />
          <path d="M400,230 C450,200 540,240 580,300 C540,350 450,330 410,290 Z" fill="url(#auth-hatch)" />
          <path d="M590,470 C640,440 720,470 780,520 C740,570 650,550 610,520 Z" fill="url(#auth-hatch)" />
          <polyline points="560,470 870,575 660,410 380,520 500,600" fill="none" stroke="#1F2A37" strokeWidth="5" strokeLinejoin="round" />
          <polyline points="560,470 870,575 660,410 380,520 500,600" fill="none" stroke="#E2B33C" strokeWidth="2" strokeDasharray="6 7" />
          {[
            [560, 470],
            [870, 575],
            [660, 410],
            [380, 520],
            [500, 600],
          ].map(([x, y], i) => (
            <g key={i} transform={`translate(${x},${y})`}>
              <rect x="-17" y="-17" width="34" height="34" fill="#1F2A37" />
              <text y="6" textAnchor="middle" fill="#E2B33C" fontFamily="Bitter, serif" fontWeight="800" fontSize="18">
                {i + 1}
              </text>
            </g>
          ))}
        </svg>
        <div className="auth-aside-text">
          <h2>{t.auth.asideTitle}</h2>
          {perks ? (
            <ul>
              {t.auth.perks.map((p) => (
                <li key={p}>{p}</li>
              ))}
            </ul>
          ) : (
            <p>{t.auth.asideText}</p>
          )}
        </div>
      </aside>
    </div>
  )
}
