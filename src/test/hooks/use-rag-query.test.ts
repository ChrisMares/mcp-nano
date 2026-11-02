import { beforeEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook, waitFor } from '@testing-library/react'
import { invoke } from '@tauri-apps/api/core'
import { useRagQuery } from '@/hooks/use-rag-query'
import type { RagQueryPayload, RagResponse } from '@/types/rag'

const emptyResponse = (query: string): RagResponse => ({
  results: [],
  total_count: 0,
  user_query: query,
})

const ragPayloads = () =>
  vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === 'rag_query')
    .map(([, args]) => (args as { payload: RagQueryPayload }).payload)

describe('useRagQuery', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === 'get_metadata_values') return { values: ['repo-b', 'repo-a'] }
      if (cmd === 'rag_query') {
        const payload = (args as { payload: RagQueryPayload }).payload
        return emptyResponse(payload.symbol ?? payload.query)
      }
      return undefined
    })
  })

  it('loads sorted filter options for the collection', async () => {
    const { result } = renderHook(() => useRagQuery())
    await waitFor(() => expect(result.current.filterOptions).toEqual(['repo-a', 'repo-b']))
  })

  it('sends a text search with expand and the repo filter', async () => {
    const { result } = renderHook(() => useRagQuery())
    act(() => {
      result.current.setQuery('rank results')
      result.current.setExpand(true)
      result.current.toggleFilter('repo-a')
    })
    await act(() => result.current.submit())
    expect(ragPayloads()[0]).toMatchObject({
      collection: 'codebase',
      query: 'rank results',
      expand: true,
      where: { $or: [{ repo_name: 'repo-a' }] },
    })
  })

  it('sends a symbol lookup in symbol mode', async () => {
    const { result } = renderHook(() => useRagQuery())
    act(() => {
      result.current.setMode('symbol')
      result.current.setSymbol('run_rag_query')
      result.current.setRelation('references')
    })
    await act(() => result.current.submit())
    expect(ragPayloads()[0]).toMatchObject({ symbol: 'run_rag_query', relation: 'references' })
  })

  it('stores the response of the last request', async () => {
    const { result } = renderHook(() => useRagQuery())
    act(() => {
      result.current.setMode('symbol')
      result.current.setSymbol('helper')
    })
    await act(() => result.current.submit())
    expect(result.current.response?.user_query).toBe('helper')
  })

  it('drops the response of a search started before switching collection', async () => {
    let resolve: (value: RagResponse) => void = () => {}
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === 'get_metadata_values') return { values: [] }
      if (cmd === 'rag_query') return new Promise<RagResponse>((r) => (resolve = r))
      return undefined
    })
    const { result } = renderHook(() => useRagQuery())
    act(() => result.current.setQuery('slow'))
    let pending: Promise<void> = Promise.resolve()
    act(() => {
      pending = result.current.submit()
    })
    act(() => result.current.setCollection('general'))
    await act(async () => {
      resolve(emptyResponse('slow'))
      await pending
    })
    expect(result.current.response).toBeUndefined()
    expect(result.current.loading).toBe(false)
  })

  it('does not submit symbol mode without a symbol', () => {
    const { result } = renderHook(() => useRagQuery())
    act(() => result.current.setMode('symbol'))
    expect(result.current.canSubmit).toBe(false)
  })
})
