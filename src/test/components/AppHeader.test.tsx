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
    expect(screen.getByText('MCP Nano')).toBeInTheDocument()
  })

  it('renders the account menu', () => {
    renderWithProviders(<AppHeader />)
    expect(screen.getByRole('button', { name: 'Account menu' })).toBeInTheDocument()
  })
})
