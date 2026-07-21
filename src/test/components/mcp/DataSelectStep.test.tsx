import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import { MemoryRouter } from 'react-router-dom'
import DataSelectStep from '@/components/mcp/DataSelectStep'

const noop = () => {}
const emptySet = new Set<string>()

const wrapper = ({ children }: { children: React.ReactNode }) => <MemoryRouter>{children}</MemoryRouter>

const renderStep = (overrides: Record<string, unknown> = {}) =>
  render(
    <DataSelectStep
      repoOptions={[]}
      groupOptions={[]}
      websiteOptions={[]}
      selectedRepos={emptySet}
      selectedGroups={emptySet}
      selectedWebsites={emptySet}
      onToggleRepo={noop}
      onToggleGroup={noop}
      onToggleWebsite={noop}
      onSetRepos={noop}
      onSetGroups={noop}
      onSetWebsites={noop}
      onNext={noop}
      {...overrides}
    />,
    { wrapper }
  )

describe('DataSelectStep website support', () => {
  it('shows website section when websiteOptions provided', () => {
    renderStep({ websiteOptions: ['docs.example.com', 'blog.example.com'] })
    expect(screen.getByText('Add Websites')).toBeInTheDocument()
  })

  it('hides website section when no websiteOptions', () => {
    renderStep({ repoOptions: ['repo-1'] })
    expect(screen.queryByText('Add Websites')).not.toBeInTheDocument()
    expect(screen.getByText('Add Code Repos')).toBeInTheDocument()
  })

  it('shows empty state when no data at all', () => {
    renderStep()
    expect(screen.getByText(/No embedded data found/)).toBeInTheDocument()
  })

  it('disables Next when no selection even with websites', () => {
    renderStep({ websiteOptions: ['example.com'] })
    expect(screen.getByRole('button', { name: 'Next' })).toBeDisabled()
  })

  it('enables Next when a website is selected', () => {
    renderStep({ websiteOptions: ['example.com'], selectedWebsites: new Set(['example.com']) })
    expect(screen.getByRole('button', { name: 'Next' })).not.toBeDisabled()
  })

  it('calls onToggleWebsite when website checkbox clicked', () => {
    const toggleFn = vi.fn()
    renderStep({ websiteOptions: ['docs.example.com'], onToggleWebsite: toggleFn })
    fireEvent.click(screen.getByLabelText('docs.example.com'))
    expect(toggleFn).toHaveBeenCalledWith('docs.example.com')
  })
})
