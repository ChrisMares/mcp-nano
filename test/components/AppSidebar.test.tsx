import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import React from 'react'
import AppSidebar from '@/components/layout/AppSidebar'
import { renderWithProviders } from '../helpers'

describe('AppSidebar', () => {
  it('renders navigation groups', () => {
    renderWithProviders(<AppSidebar />, { route: '/embed/upload' })
    expect(screen.getByText('Dashboard')).toBeInTheDocument()
    expect(screen.getByText('Embed')).toBeInTheDocument()
    expect(screen.getByText('Query')).toBeInTheDocument()
    expect(screen.getByText('MCP')).toBeInTheDocument()
  })

  it('renders dashboard link pointing to /dashboard', () => {
    renderWithProviders(<AppSidebar />, { route: '/dashboard' })
    const link = screen.getByText('Dashboard').closest('a')
    expect(link).toHaveAttribute('href', '/dashboard')
  })

  it('renders sub-items', () => {
    renderWithProviders(<AppSidebar />, { route: '/embed/upload' })
    expect(screen.getByText('Upload Files')).toBeInTheDocument()
    expect(screen.getByText('Data Management')).toBeInTheDocument()
    expect(screen.getByText('Fetch Context')).toBeInTheDocument()
    expect(screen.getByText('Create')).toBeInTheDocument()
    expect(screen.getByText('Manage')).toBeInTheDocument()
    expect(screen.getByText('Connect')).toBeInTheDocument()
  })

  it('renders logo', () => {
    renderWithProviders(<AppSidebar />, { route: '/' })
    expect(screen.getByLabelText('NASA MCP')).toBeInTheDocument()
  })
})
