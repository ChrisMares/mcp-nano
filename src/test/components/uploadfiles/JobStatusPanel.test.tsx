import { screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import JobStatusPanel from '@/components/uploadfiles/JobStatusPanel'
import { renderWithProviders } from '../../helpers'

describe('JobStatusPanel', () => {
  it('renders nothing when there are no jobs and not processing', () => {
    const { container } = renderWithProviders(
      <JobStatusPanel processing={false} activeJobs={[]} completedJobs={[]} />
    )
    expect(container.innerHTML).toBe('')
  })

  it('shows submitting message when processing with no jobs', () => {
    renderWithProviders(<JobStatusPanel processing={true} activeJobs={[]} completedJobs={[]} />)
    expect(screen.getByText('Submitting files for processing...')).toBeInTheDocument()
  })

  it('renders active jobs with filename and progress message', () => {
    const jobs = [
      {
        job_id: 'j1',
        status: 'RUNNING',
        progress_percentage: 50,
        file_name: 'test.pdf',
        created_at: null,
        message: 'Embedding batches 2/4',
      },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={jobs} completedJobs={[]} />)
    expect(screen.getByText('test.pdf')).toBeInTheDocument()
    expect(screen.getByText('RUNNING')).toBeInTheDocument()
    expect(screen.getByText('50%')).toBeInTheDocument()
    expect(screen.getByText('Embedding batches 2/4')).toBeInTheDocument()
  })

  it('renders completed jobs', () => {
    const jobs = [
      { job_id: 'j2', status: 'COMPLETED', progress_percentage: 100, file_name: 'done.pdf', created_at: null },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={[]} completedJobs={jobs} />)
    expect(screen.getByText('done.pdf')).toBeInTheDocument()
    expect(screen.getByText('Completed')).toBeInTheDocument()
  })

  it('renders failed jobs with error styling', () => {
    const jobs = [
      {
        job_id: 'j-fail',
        status: 'FAILED',
        progress_percentage: 100,
        file_name: 'bad.pdf',
        created_at: null,
        message: 'pdf-extract panicked',
      },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={[]} completedJobs={jobs} />)
    expect(screen.getByText('bad.pdf')).toBeInTheDocument()
    expect(screen.getByText('Failed')).toBeInTheDocument()
    expect(screen.getByText('pdf-extract panicked')).toBeInTheDocument()
  })

  it('shows Untitled when file_name is null instead of a bare GUID', () => {
    const jobs = [
      { job_id: 'abc-123-guid', status: 'PENDING', progress_percentage: 0, file_name: null, created_at: null },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={jobs} completedJobs={[]} />)
    expect(screen.getByText('Untitled')).toBeInTheDocument()
    expect(screen.queryByText('abc-123-guid')).not.toBeInTheDocument()
  })

  it('renders PENDING job with queue position', () => {
    const jobs = [
      { job_id: 'j3', status: 'PENDING', progress_percentage: 0, file_name: 'queued.pdf', created_at: null, queue_position: 3, total_in_queue: 7 },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={jobs} completedJobs={[]} />)
    expect(screen.getByText('#3 of 7 in queue')).toBeInTheDocument()
  })

  it('renders PENDING job without queue position (fallback icon)', () => {
    const jobs = [
      { job_id: 'j4', status: 'PENDING', progress_percentage: 0, file_name: 'pending.pdf', created_at: null },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={jobs} completedJobs={[]} />)
    expect(screen.queryByText(/#\d+ of \d+ in queue/)).not.toBeInTheDocument()
  })
})
