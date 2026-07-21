import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import { MemoryRouter } from 'react-router-dom'
import { DropdownItem } from '@/components/ui/dropdown/DropdownItem'

describe('DropdownItem', () => {
  it('renders as a button by default', () => {
    render(<DropdownItem>Action</DropdownItem>)
    expect(screen.getByRole('button', { name: 'Action' })).toBeInTheDocument()
  })

  it('renders as a link when tag=a and to is provided', () => {
    render(
      <MemoryRouter>
        <DropdownItem tag="a" to="/settings">Settings</DropdownItem>
      </MemoryRouter>
    )
    expect(screen.getByRole('link', { name: 'Settings' })).toBeInTheDocument()
  })

  it('calls onClick and onItemClick handlers', async () => {
    const onClick = vi.fn()
    const onItemClick = vi.fn()
    const user = userEvent.setup()
    render(
      <DropdownItem onClick={onClick} onItemClick={onItemClick}>
        Do something
      </DropdownItem>
    )
    await user.click(screen.getByText('Do something'))
    expect(onClick).toHaveBeenCalledOnce()
    expect(onItemClick).toHaveBeenCalledOnce()
  })

  it('applies custom className', () => {
    render(<DropdownItem className="extra">Item</DropdownItem>)
    expect(screen.getByRole('button').className).toContain('extra')
  })
})
