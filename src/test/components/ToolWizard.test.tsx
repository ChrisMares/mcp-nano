import { describe, it, expect, vi } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import ToolWizard from '@/components/mcp/ToolWizard'
import { renderWithProviders } from '../helpers'

describe('ToolWizard', () => {
  const baseProps = {
    repoOptions: ['repo-1'],
    groupOptions: ['group-1'],
    websiteOptions: [] as string[],
    onSave: vi.fn(),
    saving: false,
    saveLabel: 'Add Tool',
    onCancel: vi.fn(),
  }

  it('starts on step 1 (data selection)', () => {
    renderWithProviders(<ToolWizard {...baseProps} />)
    expect(screen.getByText('Select Data for Your Tool')).toBeInTheDocument()
  })

  it('shows back to tool list link', () => {
    renderWithProviders(<ToolWizard {...baseProps} />)
    expect(screen.getByText(/Back to tool list/)).toBeInTheDocument()
  })

  it('shows delete button when onDelete provided', () => {
    renderWithProviders(<ToolWizard {...baseProps} onDelete={vi.fn()} editMode />)
    expect(screen.getByText('Delete Tool')).toBeInTheDocument()
  })

  it('does not show delete button by default', () => {
    renderWithProviders(<ToolWizard {...baseProps} editMode />)
    expect(screen.queryByText('Delete Tool')).not.toBeInTheDocument()
  })

  it('navigates from step 1 to step 2', async () => {
    const user = userEvent.setup()
    renderWithProviders(<ToolWizard {...baseProps} />)
    await user.click(screen.getByText('repo-1'))
    await user.click(screen.getByText('Next'))
    expect(screen.getByText('Tool Name & Description')).toBeInTheDocument()
  })

  it('navigates back from step 2 to step 1', async () => {
    const user = userEvent.setup()
    renderWithProviders(<ToolWizard {...baseProps} />)
    await user.click(screen.getByText('repo-1'))
    await user.click(screen.getByText('Next'))
    await user.click(screen.getByText('Back'))
    expect(screen.getByText('Select Data for Your Tool')).toBeInTheDocument()
  })

  it('calls onCancel when back-to-list is clicked', async () => {
    const user = userEvent.setup()
    const onCancel = vi.fn()
    renderWithProviders(<ToolWizard {...baseProps} onCancel={onCancel} />)
    await user.click(screen.getByText(/Back to tool list/))
    expect(onCancel).toHaveBeenCalledOnce()
  })

  it('pre-populates form when initialData is provided', async () => {
    const user = userEvent.setup()
    renderWithProviders(
      <ToolWizard
        {...baseProps}
        initialData={{ name: 'my-tool', description: 'my desc', selectedRepos: new Set(['repo-1']), selectedGroups: new Set(), selectedWebsites: new Set() }}
      />
    )
    await user.click(screen.getByText('Next'))
    expect(screen.getByDisplayValue('my-tool')).toBeInTheDocument()
    expect(screen.getByDisplayValue('my desc')).toBeInTheDocument()
  })

  it('shows step indicator with correct labels', () => {
    renderWithProviders(<ToolWizard {...baseProps} />)
    expect(screen.getByText('Select Data')).toBeInTheDocument()
    expect(screen.getByText('Tool Details')).toBeInTheDocument()
  })
})
