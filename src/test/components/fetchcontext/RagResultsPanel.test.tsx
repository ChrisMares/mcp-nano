import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import RagResultsPanel from '@/components/fetchcontext/RagResultsPanel'
import type { RagResponse } from '@/types/rag'

describe('RagResultsPanel', () => {
  const mockData: RagResponse = {
    answer: 'Test answer',
    chunks: [],
    sources: [],
  }

  it('renders Results heading', () => {
    render(<RagResultsPanel data={undefined} theme="light" />)
    expect(screen.getByText('Results')).toBeInTheDocument()
  })

  it('shows placeholder when no data', () => {
    render(<RagResultsPanel data={undefined} theme="light" />)
    expect(screen.getByText(/Enter a query above to fetch/)).toBeInTheDocument()
  })

  it('shows copy button when data exists', () => {
    render(<RagResultsPanel data={mockData} theme="light" />)
    expect(screen.getByText('Copy')).toBeInTheDocument()
  })

  it('shows expand button when data exists', () => {
    render(<RagResultsPanel data={mockData} theme="light" />)
    expect(screen.getByText('Expand All')).toBeInTheDocument()
  })

  it('renders JSON data', () => {
    render(<RagResultsPanel data={mockData} theme="light" />)
    expect(screen.getByText('"Test answer"')).toBeInTheDocument()
  })
})
