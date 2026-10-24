import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import JobStatusRow from '@/components/uploadfiles/JobStatusRow'
import { renderWithProviders } from '../../helpers'
import { cancelJob } from '@/utils/apicalls'

vi.mock('@/utils/apicalls', () => ({
  cancelJob: vi.fn(),
}))

const runningJob = {
  job_id: 'job-1',
  status: 'RUNNING',
  progress_percentage: 42,
  file_name: 'repo.zip',
  created_at: null,
  message: 'Embedding batches 2/6',
}

describe('JobStatusRow cancellation', () => {
  beforeEach(() => {
    vi.mocked(cancelJob).mockReset()
    vi.mocked(cancelJob).mockResolvedValue(undefined)
  })

  it('shows a cancel button for a running job', () => {
    renderWithProviders(<JobStatusRow embedJob={runningJob} />)
    expect(
      screen.getByRole('button', { name: 'Cancel repo.zip' }),
    ).toBeInTheDocument()
  })

  it('shows a cancel button for a queued job', () => {
    renderWithProviders(
      <JobStatusRow
        embedJob={{ ...runningJob, status: 'PENDING', progress_percentage: 0 }}
      />,
    )
    expect(
      screen.getByRole('button', { name: 'Cancel repo.zip' }),
    ).toBeInTheDocument()
  })

  it('asks for confirmation before cancelling', async () => {
    const user = userEvent.setup()
    renderWithProviders(<JobStatusRow embedJob={runningJob} />)

    await user.click(screen.getByRole('button', { name: 'Cancel repo.zip' }))

    expect(
      screen.getByRole('heading', { name: 'Cancel job' }),
    ).toBeInTheDocument()
    expect(
      screen.getByText(/Anything this run has already embedded will be deleted/),
    ).toBeInTheDocument()
    expect(cancelJob).not.toHaveBeenCalled()
  })

  it('does not cancel when the user keeps the job running', async () => {
    const user = userEvent.setup()
    renderWithProviders(<JobStatusRow embedJob={runningJob} />)

    await user.click(screen.getByRole('button', { name: 'Cancel repo.zip' }))
    await user.click(screen.getByRole('button', { name: 'Keep running' }))

    expect(screen.queryByText('Cancel job')).not.toBeInTheDocument()
    expect(cancelJob).not.toHaveBeenCalled()
  })

  it('cancels the job on confirmation and shows progress feedback', async () => {
    const user = userEvent.setup()
    renderWithProviders(<JobStatusRow embedJob={runningJob} />)

    await user.click(screen.getByRole('button', { name: 'Cancel repo.zip' }))
    await user.click(screen.getByRole('button', { name: 'Cancel job' }))

    await waitFor(() => expect(cancelJob).toHaveBeenCalledWith('job-1'))
    await waitFor(() =>
      expect(screen.getByText('Cancelling...')).toBeInTheDocument(),
    )
    expect(screen.queryByText('Cancel job')).not.toBeInTheDocument()
  })

  it('surfaces an error and lets the user retry when cancelling fails', async () => {
    vi.mocked(cancelJob).mockRejectedValueOnce(new Error('job already FINISHED'))
    const user = userEvent.setup()
    renderWithProviders(<JobStatusRow embedJob={runningJob} />)

    await user.click(screen.getByRole('button', { name: 'Cancel repo.zip' }))
    await user.click(screen.getByRole('button', { name: 'Cancel job' }))

    await waitFor(() =>
      expect(screen.getByText('job already FINISHED')).toBeInTheDocument(),
    )
    expect(screen.getByRole('button', { name: 'Cancel job' })).toBeEnabled()
  })
})
