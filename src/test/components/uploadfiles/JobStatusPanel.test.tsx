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

  it('renders active jobs with status', () => {
    const jobs = [
      { job_id: 'j1', status: 'RUNNING', progress_percentage: 50, file_name: 'test.pdf', created_at: null },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={jobs} completedJobs={[]} />)
    expect(screen.getByText('test.pdf')).toBeInTheDocument()
    expect(screen.getByText('RUNNING')).toBeInTheDocument()
    expect(screen.getByText('50%')).toBeInTheDocument()
  })

  it('renders completed jobs', () => {
    const jobs = [
      { job_id: 'j2', status: 'COMPLETED', progress_percentage: 100, file_name: 'done.pdf', created_at: null },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={[]} completedJobs={jobs} />)
    expect(screen.getByText('done.pdf')).toBeInTheDocument()
    expect(screen.getByText('Completed')).toBeInTheDocument()
  })

  it('shows job_id when file_name is null', () => {
    const jobs = [
      { job_id: 'abc-123', status: 'PENDING', progress_percentage: 0, file_name: null, created_at: null },
    ]
    renderWithProviders(<JobStatusPanel processing={false} activeJobs={jobs} completedJobs={[]} />)
    expect(screen.getByText('abc-123')).toBeInTheDocument()
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
