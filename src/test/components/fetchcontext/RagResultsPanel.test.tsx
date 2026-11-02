import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import RagResultsPanel from '@/components/fetchcontext/RagResultsPanel'
import type { RagResponse } from '@/types/rag'

const mockData: RagResponse = {
  results: [
    {
      id: '1',
      document: '',
      metadata: {
        file_name: 'rag_service.rs',
        symbol: 'run_rag_query',
        calls: ['semantic_search'],
        code: 'pub async fn run_rag_query() {}',
      },
      score: 0.9,
    },
  ],
  total_count: 1,
  user_query: 'definition of run_rag_query',
}

describe('RagResultsPanel', () => {
  it('shows a placeholder when there is no data', () => {
    render(<RagResultsPanel data={undefined} theme="light" />)
    expect(screen.getByText('Results')).toBeInTheDocument()
    expect(screen.getByText(/Enter a query above to fetch/)).toBeInTheDocument()
  })

  it('renders the response as a JSON tree with copy and expand controls', () => {
    render(<RagResultsPanel data={mockData} theme="light" />)
    expect(screen.queryByText(/Enter a query above/)).not.toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Copy/ })).toBeInTheDocument()
    expect(screen.getByText('user_query:')).toBeInTheDocument()
  })

  it('expands nested symbol lookup fields', async () => {
    render(<RagResultsPanel data={mockData} theme="light" />)
    await userEvent.click(screen.getByRole('button', { name: /Expand All/ }))
    expect(screen.getByText('"run_rag_query"')).toBeInTheDocument()
    expect(screen.getByText('"semantic_search"')).toBeInTheDocument()
  })
})
