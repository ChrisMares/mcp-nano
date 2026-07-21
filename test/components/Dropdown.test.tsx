import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import { Dropdown } from '@/components/ui/dropdown/Dropdown'

describe('Dropdown', () => {
  it('renders children when open', () => {
    render(
      <Dropdown isOpen={true} onClose={vi.fn()}>
        <span>Menu content</span>
      </Dropdown>
    )
    expect(screen.getByText('Menu content')).toBeInTheDocument()
  })

  it('renders nothing when closed', () => {
    const { container } = render(
      <Dropdown isOpen={false} onClose={vi.fn()}>
        <span>Menu content</span>
      </Dropdown>
    )
    expect(container.innerHTML).toBe('')
  })

  it('calls onClose when clicking outside', async () => {
    const onClose = vi.fn()
    const user = userEvent.setup()
    render(
      <div>
        <div data-testid="outside">Outside</div>
        <Dropdown isOpen={true} onClose={onClose}>
          <span>Inside</span>
        </Dropdown>
      </div>
    )
    await user.click(screen.getByTestId('outside'))
    expect(onClose).toHaveBeenCalled()
  })

  it('applies custom className', () => {
    render(
      <Dropdown isOpen={true} onClose={vi.fn()} className="custom-class">
        <span>Content</span>
      </Dropdown>
    )
    const dropdown = screen.getByText('Content').parentElement
    expect(dropdown?.className).toContain('custom-class')
  })
})
