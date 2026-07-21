import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import React from 'react'
import { AuthProvider } from '@/contexts/AuthContext'
import { useAuth } from '@/hooks/useAuth'

function AuthConsumer() {
  const { user, loading } = useAuth()
  return (
    <div>
      <span data-testid="loading">{String(loading)}</span>
      <span data-testid="user-id">{user?.id ?? 'null'}</span>
      <span data-testid="user-email">{user?.email ?? 'null'}</span>
    </div>
  )
}

describe('AuthContext', () => {
  it('provides local user with loading=false', () => {
    render(
      <AuthProvider>
        <AuthConsumer />
      </AuthProvider>
    )

    expect(screen.getByTestId('loading').textContent).toBe('false')
    expect(screen.getByTestId('user-id').textContent).toBe('local-user')
    expect(screen.getByTestId('user-email').textContent).toBe('local@vectorflow')
  })
})
