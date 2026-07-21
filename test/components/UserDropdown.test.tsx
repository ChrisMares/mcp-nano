import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import UserDropdown from '@/components/header/UserDropdown'
import { renderWithProviders } from '../helpers'

describe('UserDropdown', () => {
  it('renders display name from email', () => {
    renderWithProviders(<UserDropdown />)
    expect(screen.getByText('test')).toBeInTheDocument()
  })

  it('opens dropdown on click', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('test'))
    expect(screen.getByText('test@example.com')).toBeInTheDocument()
  })

  it('shows local mode button in dropdown', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('test'))
    expect(screen.getByText('Local Mode')).toBeInTheDocument()
  })

  it('shows Account link in dropdown', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('test'))
    expect(screen.getByText('Account')).toBeInTheDocument()
  })

  it('Account link points to /account', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('test'))
    const link = screen.getByText('Account').closest('a')
    expect(link).toHaveAttribute('href', '/account')
  })
})
