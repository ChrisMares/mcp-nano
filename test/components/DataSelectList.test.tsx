import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import DataSelectList from '@/components/mcp/DataSelectList'

const baseProps = {
  title: 'Add Code Repos',
  options: ['repo-alpha', 'repo-beta', 'repo-gamma'],
  selected: new Set<string>(),
  onToggle: vi.fn(),
  onSetSelected: vi.fn(),
}

const getSelectAllCheckbox = () => screen.getAllByRole('checkbox')[0] as HTMLInputElement

describe('DataSelectList', () => {
  it('renders title', () => {
    render(<DataSelectList {...baseProps} />)
    expect(screen.getByText('Add Code Repos')).toBeInTheDocument()
  })

  it('renders nothing when options is empty', () => {
    const { container } = render(<DataSelectList {...baseProps} options={[]} />)
    expect(container.innerHTML).toBe('')
  })

  it('renders all options as checkboxes', () => {
    render(<DataSelectList {...baseProps} />)
    expect(screen.getByText('repo-alpha')).toBeInTheDocument()
    expect(screen.getByText('repo-beta')).toBeInTheDocument()
    expect(screen.getByText('repo-gamma')).toBeInTheDocument()
  })

  it('shows filtered selected count', () => {
    render(<DataSelectList {...baseProps} selected={new Set(['repo-alpha'])} />)
    expect(screen.getByText('1/3')).toBeInTheDocument()
  })

  it('has a fixed-height list container', () => {
    const { container } = render(<DataSelectList {...baseProps} />)
    const listBox = container.querySelector('.h-48')
    expect(listBox).toBeInTheDocument()
  })

  it('filters options by search text', async () => {
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} />)
    await user.type(screen.getByPlaceholderText('Search…'), 'alpha')
    expect(screen.getByText('repo-alpha')).toBeInTheDocument()
    expect(screen.queryByText('repo-beta')).not.toBeInTheDocument()
    expect(screen.queryByText('repo-gamma')).not.toBeInTheDocument()
  })

  it('shows "No matches" when filter has no results', async () => {
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} />)
    await user.type(screen.getByPlaceholderText('Search…'), 'zzz')
    expect(screen.getByText('No matches')).toBeInTheDocument()
  })

  it('calls onToggle when an item checkbox is clicked', async () => {
    const onToggle = vi.fn()
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} onToggle={onToggle} />)
    await user.click(screen.getByText('repo-beta'))
    expect(onToggle).toHaveBeenCalledWith('repo-beta')
  })

  it('select-all selects all items when no filter', async () => {
    const onSetSelected = vi.fn()
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} onSetSelected={onSetSelected} />)
    await user.click(getSelectAllCheckbox())
    const result = onSetSelected.mock.calls[0][0] as Set<string>
    expect(result).toEqual(new Set(['repo-alpha', 'repo-beta', 'repo-gamma']))
  })

  it('select-all deselects all when all are already selected', async () => {
    const onSetSelected = vi.fn()
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} selected={new Set(['repo-alpha', 'repo-beta', 'repo-gamma'])} onSetSelected={onSetSelected} />)
    await user.click(getSelectAllCheckbox())
    const result = onSetSelected.mock.calls[0][0] as Set<string>
    expect(result).toEqual(new Set())
  })

  it('select-all only selects filtered items when search is active', async () => {
    const onSetSelected = vi.fn()
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} selected={new Set(['repo-gamma'])} onSetSelected={onSetSelected} />)
    await user.type(screen.getByPlaceholderText('Search…'), 'alpha')
    await user.click(getSelectAllCheckbox())
    const result = onSetSelected.mock.calls[0][0] as Set<string>
    // repo-gamma stays selected, repo-alpha gets added
    expect(result).toEqual(new Set(['repo-gamma', 'repo-alpha']))
  })

  it('deselect-all only deselects filtered items when search is active', async () => {
    const onSetSelected = vi.fn()
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} selected={new Set(['repo-alpha', 'repo-gamma'])} onSetSelected={onSetSelected} />)
    await user.type(screen.getByPlaceholderText('Search…'), 'alpha')
    await user.click(getSelectAllCheckbox())
    const result = onSetSelected.mock.calls[0][0] as Set<string>
    // repo-gamma stays, repo-alpha removed
    expect(result).toEqual(new Set(['repo-gamma']))
  })

  it('count reflects filtered view', async () => {
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} selected={new Set(['repo-alpha', 'repo-gamma'])} />)
    // Unfiltered: 2 of 3 selected
    expect(screen.getByText('2/3')).toBeInTheDocument()
    // Filter to "alpha": 1 of 1 selected
    await user.type(screen.getByPlaceholderText('Search…'), 'alpha')
    expect(screen.getByText('1/1')).toBeInTheDocument()
  })

  it('select-all checkbox is checked when all filtered items are selected', async () => {
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} selected={new Set(['repo-alpha'])} />)
    expect(getSelectAllCheckbox().checked).toBe(false)
    await user.type(screen.getByPlaceholderText('Search…'), 'alpha')
    expect(getSelectAllCheckbox().checked).toBe(true)
  })

  it('search is case insensitive', async () => {
    const user = userEvent.setup()
    render(<DataSelectList {...baseProps} />)
    await user.type(screen.getByPlaceholderText('Search…'), 'BETA')
    expect(screen.getByText('repo-beta')).toBeInTheDocument()
    expect(screen.queryByText('repo-alpha')).not.toBeInTheDocument()
  })
})
