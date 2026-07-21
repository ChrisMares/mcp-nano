import { describe, it, expect } from 'vitest'
import { screen } from '@testing-library/react'
import React from 'react'
import { ThemeToggleButton } from '@/components/common/ThemeToggleButton'
import { renderWithProviders } from '../helpers'

describe('ThemeToggleButton', () => {
  it('renders a button', () => {
    renderWithProviders(<ThemeToggleButton />)
    expect(screen.getByRole('button')).toBeInTheDocument()
  })

  it('contains SVG icons', () => {
    renderWithProviders(<ThemeToggleButton />)
    const btn = screen.getByRole('button')
    const svgs = btn.querySelectorAll('svg')
    expect(svgs.length).toBe(2)
  })
})
