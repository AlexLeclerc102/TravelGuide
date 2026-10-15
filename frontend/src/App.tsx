import { BrowserRouter, Route, Routes } from 'react-router-dom'
import { RequireAuth } from './auth/RequireAuth'
import { Layout } from './components/Layout'
import { AccountPage } from './pages/AccountPage'
import { HomePage } from './pages/HomePage'
import { LoginPage } from './pages/LoginPage'
import { MagicLinkPage } from './pages/MagicLinkPage'
import { RegisterPage } from './pages/RegisterPage'

export function AppRoutes() {
  return (
    <Layout>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/register" element={<RegisterPage />} />
        <Route path="/auth/magic" element={<MagicLinkPage />} />
        <Route path="/account" element={<RequireAuth><AccountPage /></RequireAuth>} />
        <Route path="/" element={<RequireAuth><HomePage /></RequireAuth>} />
      </Routes>
    </Layout>
  )
}

export default function App() {
  return (
    <BrowserRouter>
      <AppRoutes />
    </BrowserRouter>
  )
}
