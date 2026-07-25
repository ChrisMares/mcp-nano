import React from 'react'
import { render, type RenderOptions } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { HelmetProvider } from 'react-helmet-async'
import { ThemeProvider } from '@/contexts/ThemeContext'
import { SidebarProvider } from '@/contexts/SidebarContext'

interface WrapperOptions {
  route?: string
}

export function createWrapper(options: WrapperOptions = {}) {
  const route = options.route || '/'

  return function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <HelmetProvider>
        <MemoryRouter initialEntries={[route]}>
          <ThemeProvider>
            <SidebarProvider>
              {children}
            </SidebarProvider>
          </ThemeProvider>
        </MemoryRouter>
      </HelmetProvider>
    )
  }
}

export function renderWithProviders(
  ui: React.ReactElement,
  options?: WrapperOptions & Omit<RenderOptions, 'wrapper'>
) {
  const { route, ...renderOptions } = options || {}
  return render(ui, {
    wrapper: createWrapper({ route }),
    ...renderOptions,
  })
}
