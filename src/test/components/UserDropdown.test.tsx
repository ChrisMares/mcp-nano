import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import UserDropdown from '@/components/header/UserDropdown'
import { renderWithProviders } from '../helpers'

describe('UserDropdown', () => {
  it('starts with the account menu closed', () => {
    renderWithProviders(<UserDropdown />)
    expect(screen.queryByText('Local')).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Account menu' })).toHaveAttribute(
      'aria-expanded',
      'false',
    )
  })

  it('opens the account menu on click', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)

    await user.click(screen.getByRole('button', { name: 'Account menu' }))

    expect(screen.getByText('Local')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Account menu' })).toHaveAttribute(
      'aria-expanded',
      'true',
    )
  })

  it('links to settings', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)

    await user.click(screen.getByRole('button', { name: 'Account menu' }))

    expect(screen.getByRole('link', { name: 'Settings' })).toHaveAttribute('href', '/settings')
  })

  it('closes the menu after choosing settings', async () => {
    const user = userEvent.setup()
    renderWithProviders(<UserDropdown />)

    await user.click(screen.getByRole('button', { name: 'Account menu' }))
    await user.click(screen.getByRole('link', { name: 'Settings' }))

    expect(screen.queryByText('Local')).not.toBeInTheDocument()
  })
})
