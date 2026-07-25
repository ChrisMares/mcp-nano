import { Link } from "react-router-dom";
import { useSidebar } from "@/contexts/SidebarContext";
import { ThemeToggleButton } from "@/components/common/ThemeToggleButton";
import EmbeddingDeviceBadge from "@/components/header/EmbeddingDeviceBadge";
import UserDropdown from "@/components/header/UserDropdown";

const AppHeader: React.FC = () => {
  const { isMobileOpen, toggleSidebar, toggleMobileSidebar } = useSidebar();

  const handleToggle = () => {
    if (window.innerWidth >= 1024) {
      toggleSidebar();
    } else {
      toggleMobileSidebar();
    }
  };

  return (
    <header className="flex h-16 w-full shrink-0 bg-card border-b border-border shadow-sm z-50">
      <div className="flex items-center justify-between w-full px-4 lg:px-6">
        {/* Left side - Toggle button */}
        <div className="flex items-center gap-4">
          <button
            className="flex items-center justify-center w-10 h-10 text-muted-foreground hover:text-foreground rounded-lg border border-border hover:bg-accent transition-colors"
            onClick={handleToggle}
            aria-label="Toggle Sidebar"
          >
            {isMobileOpen ? (
              <svg
                className="w-5 h-5"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M6 18L18 6M6 6l12 12"
                />
              </svg>
            ) : (
              <svg
                className="w-5 h-5"
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M4 6h16M4 12h16M4 18h16"
                />
              </svg>
            )}
          </button>

          {/* Mobile logo */}
          <Link to="/" className="lg:hidden">
            <span className="text-lg font-semibold text-foreground">NASA MCP</span>
          </Link>
        </div>

        {/* Right side - device badge, theme, account */}
        <div className="flex items-center gap-3">
          <EmbeddingDeviceBadge />
          <ThemeToggleButton />
          <UserDropdown />
        </div>
      </div>
    </header>
  );
};

export default AppHeader;
