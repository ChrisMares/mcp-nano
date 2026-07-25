import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import React from 'react'
import Account from '@/pages/Account'
import { renderWithProviders } from '../helpers'

describe('Account', () => {
  it('renders local mode heading and description', () => {
    renderWithProviders(<Account />)
    expect(screen.getByText('Local Mode')).toBeInTheDocument()
    expect(screen.getByText(/runs entirely on your machine/)).toBeInTheDocument()
  })
})
