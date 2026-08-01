import { describe, expect, it, vi } from 'vitest'
import { screen } from '@testing-library/react'
import React from 'react'
import { renderWithProviders } from '../helpers'
import packageJson from '../../../package.json'

const { useBackendStatus } = vi.hoisted(() => ({ useBackendStatus: vi.fn() }))

vi.mock('@/hooks/use-backend-status', () => ({ useBackendStatus }))

import Settings from '@/pages/Settings'

describe('Settings', () => {
  it('shows each model and its CPU reason', () => {
    useBackendStatus.mockReturnValue({
      qdrant_storage_path: '/home/local/.local/share/mcp-nano/qdrant',
      sqlite_path: '/home/local/.local/share/mcp-nano/app.db',
      logs_path: '/home/local/.local/share/mcp-nano/logs',
      logs_size_bytes: 1536,
      model_statuses: [
        { role: 'Sparse', model: 'BM25', device: 'CPU', cpu_reason: 'CPU-only lexical scorer.' },
        { role: 'Dense', model: 'Snowflake Arctic Embed XS', device: 'CUDA (GPU)', cpu_reason: null },
        { role: 'Reranking', model: 'MS MARCO MiniLM L6 v2', device: 'CPU', cpu_reason: 'CUDA initialization failed.' },
      ],
    })

    renderWithProviders(<Settings />)

    expect(screen.getByRole('heading', { name: 'Model Status' })).toBeInTheDocument()
    expect(screen.getByText('Application')).toBeInTheDocument()
    expect(screen.getByText(packageJson.version)).toBeInTheDocument()
    expect(screen.getByText('BM25')).toBeInTheDocument()
    expect(screen.getByText('Snowflake Arctic Embed XS')).toBeInTheDocument()
    expect(screen.getByText('MS MARCO MiniLM L6 v2')).toBeInTheDocument()
    expect(screen.getByText('CPU-only lexical scorer.')).toBeInTheDocument()
    expect(screen.getByText('CUDA initialization failed.')).toBeInTheDocument()
    expect(screen.getByText('Qdrant')).toBeInTheDocument()
    expect(screen.getByText('/home/local/.local/share/mcp-nano/qdrant')).toBeInTheDocument()
    expect(screen.getByText('SQLite')).toBeInTheDocument()
    expect(screen.getByText('/home/local/.local/share/mcp-nano/app.db')).toBeInTheDocument()
    expect(screen.getByText('Logs')).toBeInTheDocument()
    expect(screen.getByText('/home/local/.local/share/mcp-nano/logs')).toBeInTheDocument()
    expect(screen.getByText('1.5 KB')).toBeInTheDocument()
  })
})
