import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { listen } from '@tauri-apps/api/event'
import { useJobTracking } from '@/hooks/use-job-tracking'
import type { JobProgressEvent } from '@/hooks/use-job-events'

type Handler = (ev: { payload: JobProgressEvent }) => void

const handlers: Record<string, Handler> = {}

const emit = (name: string, payload: Partial<JobProgressEvent> & { job_id: string }) =>
  handlers[name]?.({
    payload: { percentage: 0, ...payload } as JobProgressEvent,
  })

describe('useJobTracking', () => {
  beforeEach(() => {
    for (const key of Object.keys(handlers)) delete handlers[key]
    vi.mocked(listen).mockImplementation((async (name: string, cb: Handler) => {
      handlers[name] = cb
      return () => {}
    }) as typeof listen)
  })

  it('tracks a job from queued through progress to finished', async () => {
    const { result } = renderHook(() => useJobTracking())
    await act(async () => {}) // let the listeners attach

    act(() => emit('job_queued', { job_id: 'j1', file_name: 'a.pdf', status: 'PENDING' }))
    expect(result.current.activeJobs).toHaveLength(1)
    expect(result.current.activeJobs[0].status).toBe('PENDING')
    expect(result.current.completedJobs).toHaveLength(0)

    act(() => emit('job_progress', { job_id: 'j1', percentage: 50, message: 'halfway' }))
    expect(result.current.activeJobs[0].progress_percentage).toBe(50)
    expect(result.current.activeJobs[0].message).toBe('halfway')

    act(() => emit('job_finished', { job_id: 'j1', percentage: 100 }))
    expect(result.current.activeJobs).toHaveLength(0)
    expect(result.current.completedJobs).toHaveLength(1)
    expect(result.current.completedJobs[0].status).toBe('COMPLETED')
    expect(result.current.completedJobs[0].progress_percentage).toBe(100)
  })

  it('moves failed jobs to completedJobs with FAILED status', async () => {
    const { result } = renderHook(() => useJobTracking())
    await act(async () => {})

    act(() => emit('job_queued', { job_id: 'j2', file_name: 'bad.pdf' }))
    act(() => emit('job_failed', { job_id: 'j2', percentage: 100, message: 'boom' }))
    expect(result.current.activeJobs).toHaveLength(0)
    expect(result.current.completedJobs[0].status).toBe('FAILED')
    expect(result.current.completedJobs[0].message).toBe('boom')
  })

  it('trackUploadedJobs registers pending jobs and preserves running state', async () => {
    const { result } = renderHook(() => useJobTracking())
    await act(async () => {})

    act(() => emit('job_queued', { job_id: 'j3', file_name: 'c.pdf', status: 'PENDING' }))
    act(() => emit('job_progress', { job_id: 'j3', percentage: 20, status: 'RUNNING' }))
    expect(result.current.activeJobs[0].status).toBe('RUNNING')

    // A duplicate upload entry for the same job must not rewind it to PENDING.
    act(() =>
      result.current.trackUploadedJobs([
        { filename: 'c.pdf', job_id: 'j3', collection: 'general', status: 'PENDING' },
        { filename: 'd.pdf', job_id: 'j4', collection: 'general', status: 'PENDING' },
      ])
    )
    const j3 = result.current.activeJobs.find((j) => j.job_id === 'j3')
    const j4 = result.current.activeJobs.find((j) => j.job_id === 'j4')
    expect(j3?.status).toBe('RUNNING')
    expect(j3?.progress_percentage).toBe(20)
    expect(j4?.status).toBe('PENDING')
  })

  it('trackQueuedJob adds a job once', async () => {
    const { result } = renderHook(() => useJobTracking())
    await act(async () => {})

    const job = {
      job_id: 'j5',
      status: 'PENDING',
      progress_percentage: 0,
      file_name: 'https://example.com',
      created_at: new Date().toISOString(),
    }
    act(() => result.current.trackQueuedJob(job))
    act(() => result.current.trackQueuedJob(job))
    expect(result.current.activeJobs).toHaveLength(1)
  })
})
