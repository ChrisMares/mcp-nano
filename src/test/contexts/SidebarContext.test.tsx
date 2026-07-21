import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import React from 'react'
import { SidebarProvider, useSidebar } from '@/contexts/SidebarContext'

function SidebarConsumer() {
  const { isExpanded, isMobileOpen, toggleSidebar, toggleMobileSidebar } = useSidebar()
  return (
    <div>
      <span data-testid="expanded">{String(isExpanded)}</span>
      <span data-testid="mobileOpen">{String(isMobileOpen)}</span>
      <button onClick={toggleSidebar}>toggleSidebar</button>
      <button onClick={toggleMobileSidebar}>toggleMobile</button>
    </div>
  )
}

describe('SidebarContext', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'innerWidth', { writable: true, value: 1024 })
  })

  it('starts expanded on desktop', () => {
    render(
      <SidebarProvider>
        <SidebarConsumer />
      </SidebarProvider>
    )
    expect(screen.getByTestId('expanded').textContent).toBe('true')
  })

  it('toggles sidebar expanded state', async () => {
    const user = userEvent.setup()
    render(
      <SidebarProvider>
        <SidebarConsumer />
      </SidebarProvider>
    )
    await user.click(screen.getByText('toggleSidebar'))
    expect(screen.getByTestId('expanded').textContent).toBe('false')
  })

  it('toggles mobile sidebar', async () => {
    const user = userEvent.setup()
    render(
      <SidebarProvider>
        <SidebarConsumer />
      </SidebarProvider>
    )
    expect(screen.getByTestId('mobileOpen').textContent).toBe('false')
    await user.click(screen.getByText('toggleMobile'))
    expect(screen.getByTestId('mobileOpen').textContent).toBe('true')
  })

  it('throws when useSidebar is used outside provider', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {})
    expect(() => render(<SidebarConsumer />)).toThrow(
      'useSidebar must be used within a SidebarProvider'
    )
    spy.mockRestore()
  })
})
