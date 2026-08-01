import { useState } from "react";
import { Link } from "react-router-dom";
import { Dropdown } from "../ui/dropdown/Dropdown";
import { UserCircleIcon } from "@/icons";

export default function UserDropdown() {
  const [isOpen, setIsOpen] = useState(false);

  const toggleDropdown = () => setIsOpen(!isOpen);
  const closeDropdown = () => setIsOpen(false);

  return (
    <div className="relative">
      <button
        onClick={toggleDropdown}
        className="flex items-center gap-2 text-foreground hover:text-foreground/80 transition-colors focus:outline-none shadow-none"
      >
        <span className="w-9 h-9 rounded-full bg-primary/10 flex items-center justify-center">
          <UserCircleIcon className="w-6 h-6 text-primary" />
        </span>
        <svg
          className={`w-4 h-4 text-muted-foreground transition-transform duration-200 ${
            isOpen ? "rotate-180" : ""
          }`}
          fill="none"
          stroke="currentColor"
          viewBox="0 0 24 24"
        >
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={2}
            d="M19 9l-7 7-7-7"
          />
        </svg>
      </button>

      <Dropdown
        isOpen={isOpen}
        onClose={closeDropdown}
        className="absolute right-0 mt-2 w-56 rounded-lg border border-border bg-card p-2 shadow-lg"
      >
        <div className="px-3 py-2 border-b border-border mb-2">
          <p className="text-sm font-medium text-foreground">Local</p>
        </div>

        <Link
          to="/settings"
          onClick={closeDropdown}
          className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-sm text-foreground hover:bg-muted"
        >
          <svg
            className="w-4 h-4"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
            aria-hidden="true"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M12 15.5a3.5 3.5 0 100-7 3.5 3.5 0 000 7z"
            />
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M19.4 15a1.7 1.7 0 00.34 1.88l.06.06-2.12 2.12-.06-.06a1.7 1.7 0 00-1.88-.34 1.7 1.7 0 00-1.03 1.55V20.3h-3v-.09a1.7 1.7 0 00-1.03-1.55 1.7 1.7 0 00-1.88.34l-.06.06-2.12-2.12.06-.06A1.7 1.7 0 007.02 15 1.7 1.7 0 005.47 14H5.4v-3h.07a1.7 1.7 0 001.55-1.03A1.7 1.7 0 006.73 8.1l-.06-.06 2.12-2.12.06.06a1.7 1.7 0 001.88.34A1.7 1.7 0 0011.76 4.8v-.1h3v.1a1.7 1.7 0 001.03 1.55 1.7 1.7 0 001.88-.34l.06-.06 2.12 2.12-.06.06a1.7 1.7 0 00-.34 1.88A1.7 1.7 0 0021 11.04h.1v3H21a1.7 1.7 0 00-1.6.96z"
            />
          </svg>
          Settings
        </Link>
      </Dropdown>
    </div>
  );
}
