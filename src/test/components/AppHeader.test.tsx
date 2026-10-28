import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import React from 'react'
import AppHeader from '@/components/layout/AppHeader'
import { renderWithProviders } from '../helpers'

describe('AppHeader', () => {
  it('renders the account menu and theme toggle', () => {
    renderWithProviders(<AppHeader />)
    expect(screen.getByRole('button', { name: 'Account menu' })).toBeInTheDocument()
  })
})
