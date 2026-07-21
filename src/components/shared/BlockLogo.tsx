interface BlockLogoProps {
  className?: string;
}

// Pixel-art block font, 8px grid, 5 cols × 6 rows per letter (fills 40×48px each)
// Color classes sweep cyan → purple across letters for futuristic gradient feel
const BlockLogo: React.FC<BlockLogoProps> = ({ className = "" }) => (
  <svg viewBox="0 0 336 48" className={className} aria-label="NASA MCP" role="img">

    {/* N — full left/right cols + 3-step diagonal */}
    <rect x="0"  y="0"  width="8" height="48" className="logo-block-1" />
    <rect x="32" y="0"  width="8" height="48" className="logo-block-1" />
    <rect x="8"  y="8"  width="8" height="8"  className="logo-block-1" />
    <rect x="16" y="16" width="8" height="8"  className="logo-block-1" />
    <rect x="24" y="24" width="8" height="8"  className="logo-block-1" />

    {/* A (first) — peaked top bar, full side cols, crossbar */}
    <rect x="56"  y="0"  width="24" height="8"  className="logo-block-2" />
    <rect x="48"  y="8"  width="8"  height="40" className="logo-block-2" />
    <rect x="80"  y="8"  width="8"  height="40" className="logo-block-2" />
    <rect x="56"  y="16" width="24" height="8"  className="logo-block-2" />

    {/* S — top bar, left bump, mid bar, right bump, bottom bar */}
    <rect x="96"  y="0"  width="40" height="8"  className="logo-block-3" />
    <rect x="96"  y="8"  width="8"  height="8"  className="logo-block-3" />
    <rect x="96"  y="16" width="40" height="8"  className="logo-block-3" />
    <rect x="128" y="24" width="8"  height="16" className="logo-block-3" />
    <rect x="96"  y="40" width="40" height="8"  className="logo-block-3" />

    {/* A (second) */}
    <rect x="152" y="0"  width="24" height="8"  className="logo-block-4" />
    <rect x="144" y="8"  width="8"  height="40" className="logo-block-4" />
    <rect x="176" y="8"  width="8"  height="40" className="logo-block-4" />
    <rect x="152" y="16" width="24" height="8"  className="logo-block-4" />

    {/* M — full side cols + V-shaped inner peaks */}
    <rect x="200" y="0"  width="8" height="48" className="logo-block-5" />
    <rect x="232" y="0"  width="8" height="48" className="logo-block-5" />
    <rect x="208" y="8"  width="8" height="8"  className="logo-block-5" />
    <rect x="224" y="8"  width="8" height="8"  className="logo-block-5" />
    <rect x="216" y="16" width="8" height="8"  className="logo-block-5" />

    {/* C — top bar, left col, bottom bar */}
    <rect x="248" y="0"  width="40" height="8"  className="logo-block-7" />
    <rect x="248" y="8"  width="8"  height="32" className="logo-block-7" />
    <rect x="248" y="40" width="40" height="8"  className="logo-block-7" />

    {/* P — full left col, top/mid bars, right bump */}
    <rect x="296" y="0"  width="8"  height="48" className="logo-block-9" />
    <rect x="304" y="0"  width="24" height="8"  className="logo-block-9" />
    <rect x="328" y="8"  width="8"  height="8"  className="logo-block-9" />
    <rect x="304" y="16" width="24" height="8"  className="logo-block-9" />

  </svg>
);

export default BlockLogo;
