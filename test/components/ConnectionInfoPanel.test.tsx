import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import React from 'react'
import ConnectionInfoPanel from '@/components/mcp/ConnectionInfoPanel'
import type { ConnectionInfo } from '@/types/mcp'

const mockInfo: ConnectionInfo = {
  mcp_url: 'http://localhost:18653/mcp',
  user_id: 'local-user',
  server_id: 'server-1',
  server_name: 'TestServer',
  full_url: 'http://localhost:18653/mcp?server_id=server-1',
  config_snippets: {
    claude_desktop: { url: 'http://localhost:18653/mcp' },
    opencode: { url: 'http://localhost:18653/mcp' },
    vscode: { url: 'http://localhost:18653/mcp' },
  },
}

describe('ConnectionInfoPanel', () => {
  it('renders the server URL', () => {
    render(<ConnectionInfoPanel info={mockInfo} />)
    expect(screen.getByText(mockInfo.full_url)).toBeInTheDocument()
  })

  it('renders config section labels', () => {
    render(<ConnectionInfoPanel info={mockInfo} />)
    expect(screen.getByText('Claude Desktop Config')).toBeInTheDocument()
    expect(screen.getByText('OpenCode Config')).toBeInTheDocument()
    expect(screen.getByText('VS Code Config')).toBeInTheDocument()
  })

  it('renders copy buttons', () => {
    render(<ConnectionInfoPanel info={mockInfo} />)
    const copyButtons = screen.getAllByText('Copy')
    // 1 for URL + 3 for config snippets
    expect(copyButtons.length).toBe(4)
  })

  it('renders formatted JSON for config snippets', () => {
    render(<ConnectionInfoPanel info={mockInfo} />)
    const pre = document.querySelectorAll('pre')
    expect(pre.length).toBe(3)
  })
})
