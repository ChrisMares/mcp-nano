import { ThemeToggleButton } from "@/components/common/ThemeToggleButton";
import EmbeddingDeviceBadge from "@/components/header/EmbeddingDeviceBadge";
import UserDropdown from "@/components/header/UserDropdown";

const AppHeader: React.FC = () => {
  return (
    <header className="flex h-16 w-full shrink-0 bg-card border-b border-border shadow-sm z-50">
      <div className="flex items-center justify-end w-full px-4 lg:px-6">
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
