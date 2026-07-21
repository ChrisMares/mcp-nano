import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import StepIndicator from '@/components/mcp/StepIndicator'

const labels = ['Server', 'Data', 'Details']

describe('StepIndicator', () => {
  it('renders all step labels', () => {
    render(<StepIndicator current={1} total={3} labels={labels} onStepClick={() => {}} />)
    expect(screen.getByText('Server')).toBeInTheDocument()
    expect(screen.getByText('Data')).toBeInTheDocument()
    expect(screen.getByText('Details')).toBeInTheDocument()
  })

  it('shows current step number as active', () => {
    render(<StepIndicator current={2} total={3} labels={labels} onStepClick={() => {}} />)
    const buttons = screen.getAllByRole('button')
    expect(buttons[1].textContent).toBe('2')
    expect(buttons[1].className).toContain('bg-primary')
  })

  it('shows completed steps with checkmark icon', () => {
    render(<StepIndicator current={3} total={3} labels={labels} onStepClick={() => {}} />)
    const buttons = screen.getAllByRole('button')
    // Steps 1 and 2 completed
    expect(buttons[0]).not.toBeDisabled()
    expect(buttons[1]).not.toBeDisabled()
    // Step 3 is active (not clickable forward)
    expect(buttons[2]).toBeDisabled()
  })

  it('clicking a completed step calls onStepClick', async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    render(<StepIndicator current={3} total={3} labels={labels} onStepClick={onClick} />)
    await user.click(screen.getAllByRole('button')[0])
    expect(onClick).toHaveBeenCalledWith(1)
  })

  it('does not allow clicking future steps', async () => {
    const user = userEvent.setup()
    const onClick = vi.fn()
    render(<StepIndicator current={1} total={3} labels={labels} onStepClick={onClick} />)
    await user.click(screen.getAllByRole('button')[2])
    expect(onClick).not.toHaveBeenCalled()
  })
})
