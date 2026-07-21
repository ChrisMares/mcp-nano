import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import EmbeddingTypeStep from '@/components/uploadfiles/EmbeddingTypeStep'

describe('EmbeddingTypeStep', () => {
  it('renders both option cards', () => {
    render(<EmbeddingTypeStep onSelect={vi.fn()} />)
    expect(screen.getByText('Code / Code Repository')).toBeInTheDocument()
    expect(screen.getByText('General Documents')).toBeInTheDocument()
  })

  it('calls onSelect with "codebase" when code card is clicked', () => {
    const onSelect = vi.fn()
    render(<EmbeddingTypeStep onSelect={onSelect} />)
    fireEvent.click(screen.getByText('Code / Code Repository'))
    expect(onSelect).toHaveBeenCalledWith('codebase')
  })

  it('calls onSelect with "general" when documents card is clicked', () => {
    const onSelect = vi.fn()
    render(<EmbeddingTypeStep onSelect={onSelect} />)
    fireEvent.click(screen.getByText('General Documents'))
    expect(onSelect).toHaveBeenCalledWith('general')
  })
})
