import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import ServerSelectStep from '@/components/mcp/ServerSelectStep'

vi.mock('@/components/ui/CustomSelect', () => ({
  default: ({ value, onChange, options, placeholder }: { value: string; onChange: (v: string) => void; options: { value: string; label: string }[]; placeholder?: string }) => (
    <select data-testid="server-select" value={value} onChange={(e) => onChange(e.target.value)}>
      {placeholder && <option value="">{placeholder}</option>}
      {options.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
    </select>
  ),
}))

const servers = [
  { id: 'srv-1', name: 'Alpha', active: true, created_at: '', updated_at: '' },
  { id: 'srv-2', name: 'Beta', active: true, created_at: '', updated_at: '' },
]

const baseProps = {
  serverMode: 'new' as const,
  serverName: '',
  serverNameError: null,
  existingServers: [],
  onServerModeChange: vi.fn(),
  onServerNameChange: vi.fn(),
  onNext: vi.fn(),
}

describe('ServerSelectStep', () => {
  describe('no existing servers', () => {
    it('shows only a name input (no dropdown or create button)', () => {
      render(<ServerSelectStep {...baseProps} />)
      expect(screen.getByPlaceholderText('e.g. MyProjectMcp')).toBeInTheDocument()
      expect(screen.queryByTestId('server-select')).not.toBeInTheDocument()
      expect(screen.queryByText('+ Create New Server')).not.toBeInTheDocument()
    })

    it('shows first-time messaging', () => {
      render(<ServerSelectStep {...baseProps} />)
      expect(screen.getByText(/Name your first MCP server/)).toBeInTheDocument()
    })

    it('disables Create Tool when name is empty', () => {
      render(<ServerSelectStep {...baseProps} />)
      expect(screen.getByRole('button', { name: /Create Tool/ })).toBeDisabled()
    })

    it('enables Create Tool when name is provided', () => {
      render(<ServerSelectStep {...baseProps} serverName="MyMcp" />)
      expect(screen.getByRole('button', { name: /Create Tool/ })).toBeEnabled()
    })

    it('shows validation error', () => {
      render(<ServerSelectStep {...baseProps} serverNameError="Invalid name" />)
      expect(screen.getByText('Invalid name')).toBeInTheDocument()
    })

    it('disables Create Tool when validation error present', () => {
      render(<ServerSelectStep {...baseProps} serverName="bad!" serverNameError="Invalid" />)
      expect(screen.getByRole('button', { name: /Create Tool/ })).toBeDisabled()
    })
  })

  describe('has existing servers (selecting)', () => {
    const withServers = { ...baseProps, existingServers: servers, serverMode: 'srv-1' }

    it('shows dropdown and create button', () => {
      render(<ServerSelectStep {...withServers} />)
      expect(screen.getByTestId('server-select')).toBeInTheDocument()
      expect(screen.getByText('+ Create New Server')).toBeInTheDocument()
    })

    it('does not show name input', () => {
      render(<ServerSelectStep {...withServers} />)
      expect(screen.queryByPlaceholderText('e.g. MyProjectMcp')).not.toBeInTheDocument()
    })

    it('enables Create Tool when a server is selected', () => {
      render(<ServerSelectStep {...withServers} />)
      expect(screen.getByRole('button', { name: /Create Tool/ })).toBeEnabled()
    })

    it('calls onServerModeChange("new") and clears name on create button click', async () => {
      const onModeChange = vi.fn()
      const onNameChange = vi.fn()
      const user = userEvent.setup()
      render(<ServerSelectStep {...withServers} onServerModeChange={onModeChange} onServerNameChange={onNameChange} />)
      await user.click(screen.getByText('+ Create New Server'))
      expect(onModeChange).toHaveBeenCalledWith('new')
      expect(onNameChange).toHaveBeenCalledWith('')
    })
  })

  describe('has existing servers (creating new)', () => {
    const creating = { ...baseProps, existingServers: servers, serverMode: 'new' as const }

    it('shows name input and back link', () => {
      render(<ServerSelectStep {...creating} />)
      expect(screen.getByPlaceholderText('e.g. MyProjectMcp')).toBeInTheDocument()
      expect(screen.getByText('Back to server list')).toBeInTheDocument()
    })

    it('hides dropdown and create button', () => {
      render(<ServerSelectStep {...creating} />)
      expect(screen.queryByTestId('server-select')).not.toBeInTheDocument()
      expect(screen.queryByText('+ Create New Server')).not.toBeInTheDocument()
    })

    it('clicking back link switches to first existing server', async () => {
      const onModeChange = vi.fn()
      const user = userEvent.setup()
      render(<ServerSelectStep {...creating} onServerModeChange={onModeChange} />)
      await user.click(screen.getByText('Back to server list'))
      expect(onModeChange).toHaveBeenCalledWith('srv-1')
    })
  })

  it('calls onNext when Create Tool is clicked', async () => {
    const onNext = vi.fn()
    const user = userEvent.setup()
    render(<ServerSelectStep {...baseProps} serverName="MyMcp" onNext={onNext} />)
    await user.click(screen.getByRole('button', { name: /Create Tool/ }))
    expect(onNext).toHaveBeenCalledOnce()
  })
})
