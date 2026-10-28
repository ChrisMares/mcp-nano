interface BlockLogoProps {
  className?: string;
}

const letters = [
  { pattern: ["10001", "11011", "10101", "10101", "10001", "10001", "10001"], color: "logo-block-5" },
  { pattern: ["01111", "10000", "10000", "10000", "10000", "10000", "01111"], color: "logo-block-6" },
  { pattern: ["11110", "10001", "10001", "11110", "10000", "10000", "10000"], color: "logo-block-7" },
  { pattern: ["10001", "11001", "10101", "10011", "10001", "10001", "10001"], color: "logo-block-7" },
  { pattern: ["01110", "10001", "10001", "11111", "10001", "10001", "10001"], color: "logo-block-8" },
  { pattern: ["10001", "11001", "10101", "10011", "10001", "10001", "10001"], color: "logo-block-8" },
  { pattern: ["01110", "10001", "10001", "10001", "10001", "10001", "01110"], color: "logo-block-9" },
];

const BlockLogo: React.FC<BlockLogoProps> = ({ className = "" }) => (
  <svg viewBox="0 0 217 35" className={className} aria-label="mcp nano" role="img">
    {letters.map(({ pattern, color }, letterIndex) => {
      const xOffset = letterIndex * 31 + (letterIndex > 2 ? 6 : 0);

      return pattern.flatMap((row, rowIndex) =>
        [...row].flatMap((cell, columnIndex) =>
          cell === "1"
            ? [
                <rect
                  key={`${letterIndex}-${rowIndex}-${columnIndex}`}
                  x={xOffset + columnIndex * 5}
                  y={rowIndex * 5}
                  width="5"
                  height="5"
                  className={color}
                />,
              ]
            : [],
        ),
      );
    })}
  </svg>
);

export default BlockLogo;
