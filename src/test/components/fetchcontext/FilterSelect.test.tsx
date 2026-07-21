import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import FilterSelect from '@/components/fetchcontext/FilterSelect'

describe('FilterSelect', () => {
  it('renders label', () => {
    render(
      <FilterSelect
        label="Repositories"
        options={['repo1', 'repo2']}
        selected={new Set()}
        onToggle={vi.fn()}
      />
    )
    expect(screen.getByText('Repositories')).toBeInTheDocument()
  })

  it('renders all options', () => {
    render(
      <FilterSelect
        label="Repositories"
        options={['repo1', 'repo2', 'repo3']}
        selected={new Set()}
        onToggle={vi.fn()}
      />
    )
    expect(screen.getByText('repo1')).toBeInTheDocument()
    expect(screen.getByText('repo2')).toBeInTheDocument()
    expect(screen.getByText('repo3')).toBeInTheDocument()
  })

  it('shows "(none = query all)" helper text', () => {
    render(
      <FilterSelect
        label="Repositories"
        options={['repo1']}
        selected={new Set()}
        onToggle={vi.fn()}
      />
    )
    expect(screen.getByText('(none = query all)')).toBeInTheDocument()
  })

  it('calls onToggle when checkbox is clicked', async () => {
    const user = userEvent.setup()
    const onToggle = vi.fn()
    render(
      <FilterSelect
        label="Repositories"
        options={['repo1']}
        selected={new Set()}
        onToggle={onToggle}
      />
    )
    await user.click(screen.getByRole('checkbox'))
    expect(onToggle).toHaveBeenCalledWith('repo1')
  })

  it('checks boxes for selected items', () => {
    render(
      <FilterSelect
        label="Repositories"
        options={['repo1', 'repo2']}
        selected={new Set(['repo1'])}
        onToggle={vi.fn()}
      />
    )
    const checkboxes = screen.getAllByRole('checkbox')
    expect(checkboxes[0]).toBeChecked()
    expect(checkboxes[1]).not.toBeChecked()
  })
})
