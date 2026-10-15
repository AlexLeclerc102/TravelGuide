import { BrowserRouter, Route, Routes } from 'react-router-dom'
import { HealthBadge } from './components/HealthBadge'
import { MapView } from './components/MapView'
import { t } from './i18n/fr'

function Home() {
  return (
    <div className="layout">
      <header className="topbar">
        <strong>{t.appName}</strong>
        <span className="muted">{t.tagline}</span>
        <HealthBadge />
      </header>
      <main className="main">
        <MapView />
      </main>
    </div>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<Home />} />
      </Routes>
    </BrowserRouter>
  )
}
