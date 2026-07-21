import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import CustomSelect from '@/components/ui/CustomSelect'
import type { SelectOption } from '@/components/ui/CustomSelect'

const options: SelectOption[] = [
  { value: 'a', label: 'Alpha' },
  { value: 'b', label: 'Beta' },
  { value: 'c', label: 'Gamma' },
]

describe('CustomSelect', () => {
  it('renders selected option label', () => {
    render(<CustomSelect value="b" onChange={vi.fn()} options={options} />)
    expect(screen.getByRole('combobox')).toHaveTextContent('Beta')
  })

  it('renders placeholder when no value matches', () => {
    render(<CustomSelect value="" onChange={vi.fn()} options={options} placeholder="Pick one" />)
    expect(screen.getByRole('combobox')).toHaveTextContent('Pick one')
  })

  it('opens dropdown on click and shows all options', () => {
    render(<CustomSelect value="a" onChange={vi.fn()} options={options} />)
    fireEvent.click(screen.getByRole('combobox'))
    expect(screen.getAllByRole('option')).toHaveLength(3)
    // Alpha appears in both trigger (selected value) and dropdown list
    expect(screen.getAllByText('Alpha')).toHaveLength(2)
    expect(screen.getByText('Beta')).toBeInTheDocument()
    expect(screen.getByText('Gamma')).toBeInTheDocument()
  })

  it('calls onChange when an option is clicked', () => {
    const onChange = vi.fn()
    render(<CustomSelect value="a" onChange={onChange} options={options} />)
    fireEvent.click(screen.getByRole('combobox'))
    fireEvent.mouseDown(screen.getByText('Gamma'))
    expect(onChange).toHaveBeenCalledWith('c')
  })

  it('closes dropdown after selection', () => {
    render(<CustomSelect value="a" onChange={vi.fn()} options={options} />)
    fireEvent.click(screen.getByRole('combobox'))
    expect(screen.queryAllByRole('option')).toHaveLength(3)
    fireEvent.mouseDown(screen.getByText('Beta'))
    expect(screen.queryAllByRole('option')).toHaveLength(0)
  })

  it('does not call onChange for disabled options', () => {
    const onChange = vi.fn()
    const opts: SelectOption[] = [
      { value: 'a', label: 'Alpha' },
      { value: 'b', label: 'Beta', disabled: true },
    ]
    render(<CustomSelect value="a" onChange={onChange} options={opts} />)
    fireEvent.click(screen.getByRole('combobox'))
    fireEvent.mouseDown(screen.getByText('Beta'))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('navigates with keyboard arrows and selects with Enter', () => {
    const onChange = vi.fn()
    render(<CustomSelect value="a" onChange={onChange} options={options} />)
    const trigger = screen.getByRole('combobox')
    // Open with Enter
    fireEvent.keyDown(trigger, { key: 'Enter' })
    expect(screen.getAllByRole('option')).toHaveLength(3)
    // Arrow down to next option
    fireEvent.keyDown(trigger, { key: 'ArrowDown' })
    // Select with Enter
    fireEvent.keyDown(trigger, { key: 'Enter' })
    expect(onChange).toHaveBeenCalledWith('b')
  })

  it('closes on Escape', () => {
    render(<CustomSelect value="a" onChange={vi.fn()} options={options} />)
    const trigger = screen.getByRole('combobox')
    fireEvent.click(trigger)
    expect(screen.getAllByRole('option')).toHaveLength(3)
    fireEvent.keyDown(trigger, { key: 'Escape' })
    expect(screen.queryAllByRole('option')).toHaveLength(0)
  })
})
