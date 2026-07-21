import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import React from 'react'
import AppHeader from '@/components/layout/AppHeader'
import { renderWithProviders } from '../helpers'

describe('AppHeader', () => {
  it('renders sidebar toggle button', () => {
    renderWithProviders(<AppHeader />)
    expect(screen.getByLabelText('Toggle Sidebar')).toBeInTheDocument()
  })

  it('renders the mobile logo link', () => {
    renderWithProviders(<AppHeader />)
    expect(screen.getByText('NASA MCP')).toBeInTheDocument()
  })

  it('renders user dropdown (display name from email)', () => {
    renderWithProviders(<AppHeader />)
    expect(screen.getByText('test')).toBeInTheDocument()
  })
})
