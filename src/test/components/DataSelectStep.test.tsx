import { describe, it, expect, vi } from 'vitest'
import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import DataSelectStep from '@/components/mcp/DataSelectStep'
import { renderWithProviders } from '../helpers'

const baseProps = {
  repoOptions: ['repo-1', 'repo-2'],
  groupOptions: ['group-1'],
  websiteOptions: [] as string[],
  selectedRepos: new Set<string>(),
  selectedGroups: new Set<string>(),
  selectedWebsites: new Set<string>(),
  onToggleRepo: vi.fn(),
  onToggleGroup: vi.fn(),
  onToggleWebsite: vi.fn(),
  onSetRepos: vi.fn(),
  onSetGroups: vi.fn(),
  onSetWebsites: vi.fn(),
  onBack: vi.fn(),
  onNext: vi.fn(),
}

describe('DataSelectStep', () => {
  it('renders heading', () => {
    renderWithProviders(<DataSelectStep {...baseProps} />)
    expect(screen.getByText('Select Data for Your Tool')).toBeInTheDocument()
  })

  it('renders both data lists when options exist', () => {
    renderWithProviders(<DataSelectStep {...baseProps} />)
    expect(screen.getByText('Add Code Repos')).toBeInTheDocument()
    expect(screen.getByText('Add Document Groups')).toBeInTheDocument()
  })

  it('shows repo checkboxes directly (no accordion)', () => {
    renderWithProviders(<DataSelectStep {...baseProps} />)
    expect(screen.getByText('repo-1')).toBeInTheDocument()
    expect(screen.getByText('repo-2')).toBeInTheDocument()
  })

  it('shows empty state with link to upload when no data', () => {
    renderWithProviders(<DataSelectStep {...baseProps} repoOptions={[]} groupOptions={[]} />)
    expect(screen.getByText(/upload and embed files/i)).toBeInTheDocument()
    const link = screen.getByText(/Go to Upload Files/)
    expect(link).toBeInTheDocument()
    expect(link.closest('a')).toHaveAttribute('href', '/embed/upload')
  })

  it('hides repo list when repoOptions is empty but shows groups', () => {
    renderWithProviders(<DataSelectStep {...baseProps} repoOptions={[]} />)
    expect(screen.queryByText('Add Code Repos')).not.toBeInTheDocument()
    expect(screen.getByText('Add Document Groups')).toBeInTheDocument()
  })

  it('calls onBack when Back is clicked', async () => {
    const user = userEvent.setup()
    const onBack = vi.fn()
    renderWithProviders(<DataSelectStep {...baseProps} onBack={onBack} />)
    await user.click(screen.getByText('Back'))
    expect(onBack).toHaveBeenCalledOnce()
  })

  it('calls onNext when Next is clicked with selections', async () => {
    const user = userEvent.setup()
    const onNext = vi.fn()
    renderWithProviders(<DataSelectStep {...baseProps} selectedRepos={new Set(['repo-1'])} onNext={onNext} />)
    await user.click(screen.getByText('Next'))
    expect(onNext).toHaveBeenCalledOnce()
  })

  it('disables Next when nothing is selected', () => {
    renderWithProviders(<DataSelectStep {...baseProps} />)
    expect(screen.getByText('Next')).toBeDisabled()
  })

  it('enables Next when a repo is selected', () => {
    renderWithProviders(<DataSelectStep {...baseProps} selectedRepos={new Set(['repo-1'])} />)
    expect(screen.getByText('Next')).not.toBeDisabled()
  })

  it('enables Next when a group is selected', () => {
    renderWithProviders(<DataSelectStep {...baseProps} selectedGroups={new Set(['group-1'])} />)
    expect(screen.getByText('Next')).not.toBeDisabled()
  })
})
