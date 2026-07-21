import { describe, it, expect } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { useToolForm } from '@/hooks/useToolForm'

describe('useToolForm', () => {
  it('starts with empty form by default', () => {
    const { result } = renderHook(() => useToolForm())
    expect(result.current.form.name).toBe('')
    expect(result.current.form.description).toBe('')
    expect(result.current.form.selectedRepos.size).toBe(0)
    expect(result.current.form.selectedGroups.size).toBe(0)
  })

  it('initializes with provided data', () => {
    const initial = { name: 'test', description: 'desc', selectedRepos: new Set(['r1']), selectedGroups: new Set(['g1']) }
    const { result } = renderHook(() => useToolForm(initial))
    expect(result.current.form.name).toBe('test')
    expect(result.current.form.selectedRepos.has('r1')).toBe(true)
  })

  it('updateForm merges partial updates', () => {
    const { result } = renderHook(() => useToolForm())
    act(() => result.current.updateForm({ name: 'hello' }))
    expect(result.current.form.name).toBe('hello')
    expect(result.current.form.description).toBe('')
  })

  it('toggleRepo adds and removes repos', () => {
    const { result } = renderHook(() => useToolForm())
    act(() => result.current.toggleRepo('repo-1'))
    expect(result.current.form.selectedRepos.has('repo-1')).toBe(true)
    act(() => result.current.toggleRepo('repo-1'))
    expect(result.current.form.selectedRepos.has('repo-1')).toBe(false)
  })

  it('toggleGroup adds and removes groups', () => {
    const { result } = renderHook(() => useToolForm())
    act(() => result.current.toggleGroup('group-1'))
    expect(result.current.form.selectedGroups.has('group-1')).toBe(true)
    act(() => result.current.toggleGroup('group-1'))
    expect(result.current.form.selectedGroups.has('group-1')).toBe(false)
  })

  it('resetForm clears all fields', () => {
    const { result } = renderHook(() => useToolForm())
    act(() => {
      result.current.updateForm({ name: 'test', description: 'desc' })
      result.current.toggleRepo('repo-1')
    })
    act(() => result.current.resetForm())
    expect(result.current.form.name).toBe('')
    expect(result.current.form.selectedRepos.size).toBe(0)
  })

  it('setRepos replaces all selected repos', () => {
    const { result } = renderHook(() => useToolForm())
    act(() => result.current.toggleRepo('repo-1'))
    act(() => result.current.setRepos(new Set(['repo-2', 'repo-3'])))
    expect(result.current.form.selectedRepos.has('repo-1')).toBe(false)
    expect(result.current.form.selectedRepos.has('repo-2')).toBe(true)
    expect(result.current.form.selectedRepos.has('repo-3')).toBe(true)
  })

  it('setGroups replaces all selected groups', () => {
    const { result } = renderHook(() => useToolForm())
    act(() => result.current.toggleGroup('group-1'))
    act(() => result.current.setGroups(new Set(['group-2'])))
    expect(result.current.form.selectedGroups.has('group-1')).toBe(false)
    expect(result.current.form.selectedGroups.has('group-2')).toBe(true)
  })
})
