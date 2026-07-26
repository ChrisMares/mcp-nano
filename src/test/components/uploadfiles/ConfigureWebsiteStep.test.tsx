import { render, screen, fireEvent } from '@testing-library/react'
import { describe, it, expect, vi } from 'vitest'
import ConfigureWebsiteStep from '@/components/uploadfiles/ConfigureWebsiteStep'

const defaults = {
  websiteUrl: '',
  depth: 1,
  sameDomainOnly: true,
  isCrawling: false,
  onUrlChange: vi.fn(),
  onDepthChange: vi.fn(),
  onSameDomainChange: vi.fn(),
  onBack: vi.fn(),
  onNext: vi.fn(),
}

describe('ConfigureWebsiteStep', () => {
  it('renders the URL input, depth slider, and same-domain checkbox', () => {
    render(<ConfigureWebsiteStep {...defaults} />)
    expect(screen.getByPlaceholderText('https://docs.example.com')).toBeInTheDocument()
    expect(screen.getByText(/Crawl Depth/)).toBeInTheDocument()
    expect(screen.getByLabelText('Only crawl current domain')).toBeInTheDocument()
  })

  it('checkbox is checked by default', () => {
    render(<ConfigureWebsiteStep {...defaults} />)
    const checkbox = screen.getByLabelText('Only crawl current domain')
    expect(checkbox).toBeChecked()
  })

  it('checkbox shows unchecked when sameDomainOnly is false', () => {
    render(<ConfigureWebsiteStep {...defaults} sameDomainOnly={false} />)
    const checkbox = screen.getByLabelText('Only crawl current domain')
    expect(checkbox).not.toBeChecked()
  })

  it('calls onSameDomainChange when checkbox toggled', () => {
    const onSameDomainChange = vi.fn()
    render(<ConfigureWebsiteStep {...defaults} onSameDomainChange={onSameDomainChange} />)
    const checkbox = screen.getByLabelText('Only crawl current domain')
    fireEvent.click(checkbox)
    expect(onSameDomainChange).toHaveBeenCalledWith(false)
  })

  it('checkbox has a title tooltip', () => {
    render(<ConfigureWebsiteStep {...defaults} />)
    const checkbox = screen.getByLabelText('Only crawl current domain')
    expect(checkbox.closest('label')).toHaveAttribute('title')
    expect(checkbox.closest('label')!.getAttribute('title')).toContain('same domain')
  })

  it('depth label has a title tooltip', () => {
    render(<ConfigureWebsiteStep {...defaults} />)
    const depthLabel = screen.getByText(/Crawl Depth/)
    expect(depthLabel).toHaveAttribute('title')
    expect(depthLabel.getAttribute('title')).toContain('link levels')
  })

  it('disables inputs when isCrawling', () => {
    render(<ConfigureWebsiteStep {...defaults} isCrawling={true} />)
    expect(screen.getByPlaceholderText('https://docs.example.com')).toBeDisabled()
    expect(screen.getByLabelText('Only crawl current domain')).toBeDisabled()
  })

  it('shows current crawl URL on one line while crawling', () => {
    render(
      <ConfigureWebsiteStep
        {...defaults}
        websiteUrl="https://gojs.net/latest/api/"
        isCrawling={true}
        crawlFoundCount={2}
        crawlCurrentUrl="https://gojs.net/latest/api/symbols/Diagram.html"
      />
    )
    expect(
      screen.getByText(/Crawling https:\/\/gojs\.net\/latest\/api\/symbols\/Diagram\.html/)
    ).toBeInTheDocument()
    expect(screen.getByText(/2 found/)).toBeInTheDocument()
  })

  it('shows Start Crawl button when not crawling', () => {
    render(<ConfigureWebsiteStep {...defaults} />)
    expect(screen.getByText('Start Crawl')).toBeInTheDocument()
  })

  it('Next button is disabled when URL is empty', () => {
    render(<ConfigureWebsiteStep {...defaults} websiteUrl="" />)
    // Next is the "Start Crawl" button in the wizard nav
    expect(screen.getByText('Start Crawl').closest('button')).toBeDisabled()
  })

  it('Next button is enabled when URL is provided', () => {
    render(<ConfigureWebsiteStep {...defaults} websiteUrl="https://example.com" />)
    expect(screen.getByText('Start Crawl').closest('button')).not.toBeDisabled()
  })

  it('calls onUrlChange when URL input changes', () => {
    const onUrlChange = vi.fn()
    render(<ConfigureWebsiteStep {...defaults} onUrlChange={onUrlChange} />)
    fireEvent.change(screen.getByPlaceholderText('https://docs.example.com'), {
      target: { value: 'https://mysite.com' },
    })
    expect(onUrlChange).toHaveBeenCalledWith('https://mysite.com')
  })
})
