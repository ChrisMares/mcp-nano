import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import ToolDetailsStep from '@/components/mcp/ToolDetailsStep'

const baseProps = {
  name: '',
  description: '',
  selectedRepos: new Set<string>(),
  selectedGroups: new Set<string>(),
  selectedWebsites: new Set<string>(),
  onNameChange: vi.fn(),
  onDescriptionChange: vi.fn(),
  onBack: vi.fn(),
  onSave: vi.fn(),
  saving: false,
  saveLabel: 'Create Tool',
}

describe('ToolDetailsStep', () => {
  beforeEach(() => vi.clearAllMocks())

  it('renders heading', () => {
    render(<ToolDetailsStep {...baseProps} />)
    expect(screen.getByText('Tool Name & Description')).toBeInTheDocument()
  })

  it('renders name input and description textarea', () => {
    render(<ToolDetailsStep {...baseProps} />)
    expect(screen.getByPlaceholderText('e.g. search_backend_code')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('What this tool searches for...')).toBeInTheDocument()
  })

  it('shows data summary when repos/groups selected', () => {
    render(<ToolDetailsStep {...baseProps} selectedRepos={new Set(['my-repo'])} selectedGroups={new Set(['my-group'])} />)
    expect(screen.getByText('my-repo')).toBeInTheDocument()
    expect(screen.getByText('my-group')).toBeInTheDocument()
  })

  it('hides data summary when nothing selected', () => {
    render(<ToolDetailsStep {...baseProps} />)
    expect(screen.queryByText('Selected data:')).not.toBeInTheDocument()
  })

  it('disables save when name is empty', () => {
    render(<ToolDetailsStep {...baseProps} />)
    expect(screen.getByText('Create Tool')).toBeDisabled()
  })

  it('enables save when name has content', () => {
    render(<ToolDetailsStep {...baseProps} name="test" />)
    expect(screen.getByText('Create Tool')).not.toBeDisabled()
  })

  it('disables save when name contains invalid characters', () => {
    render(<ToolDetailsStep {...baseProps} name="test-tool" />)
    expect(screen.getByText('Create Tool')).toBeDisabled()
  })

  it('shows validation error for invalid tool name', async () => {
    const user = userEvent.setup()
    const onNameChange = vi.fn()
    render(<ToolDetailsStep {...baseProps} onNameChange={onNameChange} />)
    const input = screen.getByPlaceholderText('e.g. search_backend_code')
    // Type a hyphen which is invalid
    await user.type(input, '-')
    expect(screen.getByText('Only letters, numbers, and underscores allowed')).toBeInTheDocument()
  })

  it('strips spaces from tool name input', async () => {
    const user = userEvent.setup()
    const onNameChange = vi.fn()
    render(<ToolDetailsStep {...baseProps} onNameChange={onNameChange} />)
    const input = screen.getByPlaceholderText('e.g. search_backend_code')
    // Type a space — should be stripped to empty, so onNameChange is not called with a space
    await user.type(input, ' ')
    expect(onNameChange).toHaveBeenCalledWith('')
  })

  it('shows hint text for tool name format', () => {
    render(<ToolDetailsStep {...baseProps} />)
    expect(screen.getByText(/Alphanumeric and underscores only/)).toBeInTheDocument()
  })

  it('calls onSave when save button clicked', async () => {
    const user = userEvent.setup()
    const onSave = vi.fn()
    render(<ToolDetailsStep {...baseProps} name="test" onSave={onSave} />)
    await user.click(screen.getByText('Create Tool'))
    expect(onSave).toHaveBeenCalledOnce()
  })

  it('shows Saving state', () => {
    render(<ToolDetailsStep {...baseProps} name="test" saving={true} />)
    expect(screen.getByText('Saving...')).toBeInTheDocument()
  })

  it('calls onBack when Back is clicked', async () => {
    const user = userEvent.setup()
    const onBack = vi.fn()
    render(<ToolDetailsStep {...baseProps} onBack={onBack} />)
    await user.click(screen.getByText('Back'))
    expect(onBack).toHaveBeenCalledOnce()
  })

  it('uses provided saveLabel', () => {
    render(<ToolDetailsStep {...baseProps} name="t" saveLabel="Save Changes" />)
    expect(screen.getByText('Save Changes')).toBeInTheDocument()
  })

})
