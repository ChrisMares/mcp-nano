import { render, screen } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import WebsiteProcessingStep from '@/components/uploadfiles/WebsiteProcessingStep'

const defaults = {
  websiteUrl: 'www.drudgereport.com',
  crawledUrls: [
    'https://www.drudgereport.com/',
    'https://www.drudgereport.com/privacy/',
    'https://www.drudgereport.com/wx.htm',
  ],
  crawlError: false,
  embedJobId: null,
  onEmbed: vi.fn(),
  onStartAnother: vi.fn(),
}

describe('WebsiteProcessingStep', () => {
  it('renders results for a scheme-less website URL', () => {
    render(<WebsiteProcessingStep {...defaults} />)
    expect(screen.getByText('Review Crawl Results')).toBeInTheDocument()
    expect(screen.getByText(/3 of 3/)).toBeInTheDocument()
    expect(screen.getAllByText('www.drudgereport.com').length).toBeGreaterThan(0)
  })

  it('ignores crawled entries that are not valid URLs', () => {
    render(
      <WebsiteProcessingStep
        {...defaults}
        crawledUrls={['https://example.com/docs', 'not a url', 'also:not:a:url']}
      />,
    )
    expect(screen.getByText(/1 of 1/)).toBeInTheDocument()
    expect(screen.getByText('example.com')).toBeInTheDocument()
  })

  it('reports when no crawled entry can be used', () => {
    render(
      <WebsiteProcessingStep
        {...defaults}
        crawledUrls={['not a url']}
      />,
    )
    expect(screen.getByText('No usable URLs were returned by the crawl.')).toBeInTheDocument()
  })
})
