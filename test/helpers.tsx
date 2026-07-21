import React from 'react'
import { render, type RenderOptions } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { HelmetProvider } from 'react-helmet-async'
import { AuthContext } from '@/contexts/AuthContext'
import { ThemeProvider } from '@/contexts/ThemeContext'
import { SidebarProvider } from '@/contexts/SidebarContext'
import { vi } from 'vitest'

type TestAuthContext = {
  user: { id: string; email: string } | null
  loading: boolean
  signIn: (email: string, password: string) => Promise<void>
  signUp: (email: string, password: string) => Promise<void>
  signOut: () => Promise<void>
}

export const mockAuthContext: TestAuthContext = {
  user: {
    id: 'test-user-id',
    email: 'test@example.com',
  },
  loading: false,
  signIn: vi.fn(),
  signUp: vi.fn(),
  signOut: vi.fn(),
}

interface WrapperOptions {
  auth?: Partial<TestAuthContext>
  route?: string
}

export function createWrapper(options: WrapperOptions = {}) {
  const authValue = { ...mockAuthContext, ...options.auth }
  const route = options.route || '/'

  return function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <HelmetProvider>
        <MemoryRouter initialEntries={[route]}>
          <AuthContext.Provider value={authValue}>
            <ThemeProvider>
              <SidebarProvider>
                {children}
              </SidebarProvider>
            </ThemeProvider>
          </AuthContext.Provider>
        </MemoryRouter>
      </HelmetProvider>
    )
  }
}

export function renderWithProviders(
  ui: React.ReactElement,
  options?: WrapperOptions & Omit<RenderOptions, 'wrapper'>
) {
  const { auth, route, ...renderOptions } = options || {}
  return render(ui, {
    wrapper: createWrapper({ auth, route }),
    ...renderOptions,
  })
}
