import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import ToolDetailsStep from '@/components/mcp/ToolDetailsStep'

const noop = () => {}

describe('ToolDetailsStep website badges', () => {
  it('shows website badges in data summary', () => {
    render(
      <ToolDetailsStep
        name="test_tool"
        description=""
        selectedRepos={new Set()}
        selectedGroups={new Set()}
        selectedWebsites={new Set(['docs.example.com'])}
        onNameChange={noop}
        onDescriptionChange={noop}
        onBack={noop}
        onSave={noop}
        saving={false}
        saveLabel="Create Tool"
      />
    )
    expect(screen.getByText('docs.example.com')).toBeInTheDocument()
  })

  it('does not show data summary when nothing selected', () => {
    render(
      <ToolDetailsStep
        name="test_tool"
        description=""
        selectedRepos={new Set()}
        selectedGroups={new Set()}
        selectedWebsites={new Set()}
        onNameChange={noop}
        onDescriptionChange={noop}
        onBack={noop}
        onSave={noop}
        saving={false}
        saveLabel="Create Tool"
      />
    )
    expect(screen.queryByText('Selected Data')).not.toBeInTheDocument()
  })

  it('shows all three badge types together', () => {
    render(
      <ToolDetailsStep
        name="test_tool"
        description=""
        selectedRepos={new Set(['my-repo'])}
        selectedGroups={new Set(['my-group'])}
        selectedWebsites={new Set(['example.com'])}
        onNameChange={noop}
        onDescriptionChange={noop}
        onBack={noop}
        onSave={noop}
        saving={false}
        saveLabel="Create Tool"
      />
    )
    expect(screen.getByText('Selected Data')).toBeInTheDocument()
    expect(screen.getByText('my-repo')).toBeInTheDocument()
    expect(screen.getByText('my-group')).toBeInTheDocument()
    expect(screen.getByText('example.com')).toBeInTheDocument()
  })
})
