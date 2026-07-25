import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import UserDropdown from '@/components/header/UserDropdown'
import { renderWithProviders } from '../helpers'

describe('UserDropdown', () => {
  it('renders Local label', () => {
    renderWithProviders(<UserDropdown />)
    expect(screen.getByText('Local')).toBeInTheDocument()
  })

  it('opens dropdown on click', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('Local'))
    expect(screen.getByText('Account')).toBeInTheDocument()
  })

  it('shows Account link in dropdown', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('Local'))
    expect(screen.getByText('Account')).toBeInTheDocument()
  })

  it('Account link points to /account', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('Local'))
    const link = screen.getByText('Account').closest('a')
    expect(link).toHaveAttribute('href', '/account')
  })
})
