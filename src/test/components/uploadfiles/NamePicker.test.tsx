import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import NamePicker from '@/components/uploadfiles/NamePicker'

const baseProps = {
  label: 'Repo Name',
  options: [] as string[],
  value: '',
  mode: 'existing' as const,
  onChange: vi.fn(),
  emptyMessage: 'No repos yet.',
  newPlaceholder: 'Enter new name',
}

describe('NamePicker', () => {
  it('shows empty state with text input when no options and no defaultOption', () => {
    render(<NamePicker {...baseProps} />)
    expect(screen.getByText('No repos yet.')).toBeInTheDocument()
    expect(screen.getByPlaceholderText('Enter new name')).toBeInTheDocument()
  })

  it('uses emptyPlaceholder over newPlaceholder in empty state', () => {
    render(<NamePicker {...baseProps} emptyPlaceholder="Type here" />)
    expect(screen.getByPlaceholderText('Type here')).toBeInTheDocument()
  })

  it('shows select with defaultOption even when options is empty', () => {
    render(<NamePicker {...baseProps} defaultOption="default" />)
    expect(screen.getByRole('combobox')).toBeInTheDocument()
    expect(screen.getByText('default')).toBeInTheDocument()
  })

  it('shows select with options when provided', () => {
    render(<NamePicker {...baseProps} options={['alpha', 'beta']} />)
    const select = screen.getByRole('combobox')
    expect(select).toBeInTheDocument()
    expect(screen.getByText('alpha')).toBeInTheDocument()
    expect(screen.getByText('beta')).toBeInTheDocument()
  })

  it('shows new input when mode is "new" and options exist', () => {
    render(<NamePicker {...baseProps} options={['alpha']} mode="new" />)
    expect(screen.getByPlaceholderText('Enter new name')).toBeInTheDocument()
  })

  it('calls onChange with "new" mode when __new__ is selected', () => {
    const onChange = vi.fn()
    render(<NamePicker {...baseProps} options={['alpha']} onChange={onChange} />)
    fireEvent.change(screen.getByRole('combobox'), { target: { value: '__new__' } })
    expect(onChange).toHaveBeenCalledWith('', 'new')
  })

  it('calls onChange with "existing" mode when a normal option is selected', () => {
    const onChange = vi.fn()
    render(<NamePicker {...baseProps} options={['alpha']} onChange={onChange} />)
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'alpha' } })
    expect(onChange).toHaveBeenCalledWith('alpha', 'existing')
  })

  it('renders required asterisk when required prop is set', () => {
    render(<NamePicker {...baseProps} required />)
    expect(screen.getByText('*')).toBeInTheDocument()
  })
})
