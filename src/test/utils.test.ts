import { describe, it, expect } from 'vitest'
import { cn } from '@/lib/utils'

describe('cn utility', () => {
  it('merges class names', () => {
    expect(cn('px-2', 'py-2')).toBe('px-2 py-2')
  })

  it('handles conditional classes', () => {
    const shouldHide = false
    expect(cn('base', shouldHide && 'hidden', 'extra')).toBe('base extra')
  })

  it('deduplicates tailwind conflicts', () => {
    const result = cn('px-2', 'px-4')
    expect(result).toBe('px-4')
  })

  it('returns empty string for no input', () => {
    expect(cn()).toBe('')
  })
})
