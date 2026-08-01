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

  it('shows Account as a disabled placeholder without a link', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)
    await user.click(screen.getByText('Local'))
    const item = screen.getByText('Account')
    expect(item.closest('a')).toBeNull()
    expect(item.closest('[aria-disabled="true"]')).not.toBeNull()
  })
})
