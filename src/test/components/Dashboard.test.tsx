import { beforeEach, describe, expect, it, vi } from 'vitest'
import { screen, waitFor } from '@testing-library/react'
import { invoke } from '@tauri-apps/api/core'
import React from 'react'
import { renderWithProviders } from '../helpers'

const { useBackendStatus } = vi.hoisted(() => ({ useBackendStatus: vi.fn() }))

vi.mock('@/hooks/use-backend-status', () => ({ useBackendStatus }))

import Dashboard from '@/pages/Dashboard'

const notReadyStatus = {
  qdrant_ready: false,
  qdrant_error: null,
  db_ready: false,
}

const readyStatus = {
  qdrant_ready: true,
  qdrant_error: null,
  db_ready: true,
}

const mockInvoke = (impl: (cmd: string) => unknown) => {
  vi.mocked(invoke).mockImplementation(async (cmd: string) => impl(cmd))
}

describe('Dashboard', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear()
    useBackendStatus.mockReset()
  })

  it('does not fetch stats while the backend is still starting', () => {
    useBackendStatus.mockReturnValue(notReadyStatus)

    renderWithProviders(<Dashboard />)

    expect(screen.getByText('Loading stats...')).toBeInTheDocument()
    expect(vi.mocked(invoke)).not.toHaveBeenCalled()
    expect(screen.queryByText('Embed data first')).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: /Try a Search/ })).toBeInTheDocument()
    expect(screen.getByRole('link', { name: /Create a Server/ })).toBeInTheDocument()
  })

  it('fetches and renders stats once the backend is ready', async () => {
    useBackendStatus.mockReturnValue(readyStatus)
    mockInvoke((cmd) => {
      switch (cmd) {
        case 'get_files':
          return {
            repos: [{ repo_name: 'alpha' }, { repo_name: 'beta' }],
            documents: [{ filename: 'a.pdf' }, { filename: 'b.md' }],
          }
        case 'get_mcp_servers':
          return { servers: [{ id: 'srv1' }] }
        case 'get_mcp_server':
          return { server: { tools: [{ id: 't1' }, { id: 't2' }] } }
        case 'get_websites':
          return { websites: [{ url: 'https://example.com' }] }
        default:
          return undefined
      }
    })

    renderWithProviders(<Dashboard />)

    expect(await screen.findByText('Repositories')).toBeInTheDocument()
    await waitFor(() => {
      expect(vi.mocked(invoke)).toHaveBeenCalledWith('get_mcp_servers')
      expect(vi.mocked(invoke)).toHaveBeenCalledWith('get_mcp_server', { serverId: 'srv1' })
      expect(vi.mocked(invoke)).toHaveBeenCalledWith('get_websites')
    })

    const cardValue = (label: string) =>
      screen.getByText(label).closest('a')?.querySelector('span.text-2xl')?.textContent
    expect(cardValue('Repositories')).toBe('2')
    expect(cardValue('Documents')).toBe('3')
    expect(cardValue('MCP Servers')).toBe('1')
    expect(cardValue('MCP Tools')).toBe('2')

    expect(screen.queryByText('Embed data first')).not.toBeInTheDocument()
    expect(screen.queryByText('Loading stats...')).not.toBeInTheDocument()
  })

  it('disables steps 2 and 3 when the backend is ready but has no data', async () => {
    useBackendStatus.mockReturnValue(readyStatus)
    mockInvoke((cmd) => {
      switch (cmd) {
        case 'get_files':
          return { repos: [], documents: [] }
        case 'get_mcp_servers':
          return { servers: [] }
        case 'get_websites':
          return { websites: [] }
        default:
          return undefined
      }
    })

    renderWithProviders(<Dashboard />)

    await waitFor(() => {
      expect(screen.getAllByText('Embed data first')).toHaveLength(2)
    })
    expect(screen.queryByRole('link', { name: /Try a Search/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('link', { name: /Create a Server/ })).not.toBeInTheDocument()
    expect(screen.getByRole('link', { name: /Start Embedding/ })).toBeInTheDocument()
  })
})
