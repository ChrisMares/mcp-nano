import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import ExpandableCard from '@/components/mcp/ExpandableCard'

describe('ExpandableCard', () => {
  const defaultProps = {
    title: 'Server A',
    expanded: false,
    onToggle: vi.fn(),
    children: <div>Card body</div>,
  }

  it('renders title', () => {
    render(<ExpandableCard {...defaultProps} />)
    expect(screen.getByText('Server A')).toBeInTheDocument()
  })

  it('shows subtitle when provided', () => {
    render(<ExpandableCard {...defaultProps} subtitle="A description" />)
    expect(screen.getByText('A description')).toBeInTheDocument()
  })

  it('hides children when collapsed', () => {
    render(<ExpandableCard {...defaultProps} expanded={false} />)
    expect(screen.queryByText('Card body')).not.toBeInTheDocument()
  })

  it('shows children when expanded', () => {
    render(<ExpandableCard {...defaultProps} expanded={true} />)
    expect(screen.getByText('Card body')).toBeInTheDocument()
  })

  it('calls onToggle when clicking header button', async () => {
    const onToggle = vi.fn()
    const user = userEvent.setup()
    render(<ExpandableCard {...defaultProps} onToggle={onToggle} />)
    await user.click(screen.getByText('Server A'))
    expect(onToggle).toHaveBeenCalledOnce()
  })

  it('renders badge when provided', () => {
    render(<ExpandableCard {...defaultProps} badge={<span>2 tools</span>} />)
    expect(screen.getByText('2 tools')).toBeInTheDocument()
  })

  it('renders actions when provided', () => {
    render(<ExpandableCard {...defaultProps} expanded={true} actions={<button>Delete</button>} />)
    expect(screen.getByText('Delete')).toBeInTheDocument()
  })
})
