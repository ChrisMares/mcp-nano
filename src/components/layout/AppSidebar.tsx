import { useCallback } from "react";
import { Link, useLocation } from "react-router-dom";
import { LayoutDashboard } from "lucide-react";
import { FolderIcon, ChatIcon, PlugIn } from "@/icons";
import BlockLogo from "@/components/shared/BlockLogo";

type NavItem = {
  name: string;
  icon: React.ReactNode;
  path?: string;
  subItems?: { name: string; path: string }[];
};

const navItems: NavItem[] = [
  {
    icon: <LayoutDashboard className="w-6 h-6" />,
    name: "Dashboard",
    path: "/dashboard",
  },
  {
    icon: <FolderIcon />,
    name: "Embed",
    subItems: [
      { name: "Upload Files", path: "/embed/upload" },
      { name: "Data Management", path: "/embed/data" },
    ],
  },
  {
    icon: <ChatIcon />,
    name: "Query",
    subItems: [{ name: "Fetch Context", path: "/query/fetch" }],
  },
  {
    icon: <PlugIn />,
    name: "MCP",
    subItems: [
      { name: "Create", path: "/mcp/create" },
      { name: "Manage", path: "/mcp/manage" },
      { name: "Connect", path: "/mcp/connect" },
    ],
  },
];

const AppSidebar: React.FC = () => {
  const location = useLocation();
  const isActive = useCallback(
    (path: string) => {
      return location.pathname === path;
    },
    [location.pathname],
  );

  return (
    <aside className="w-[225px] shrink-0 flex flex-col bg-sidebar px-4 text-sidebar-foreground border-r border-sidebar-border shadow-lg">
      <div className="py-4 flex justify-center">
        <Link to="/" className="logo-container w-full flex items-center justify-center px-3 py-5 rounded-xl">
          <BlockLogo className="sidebar-block-logo max-w-[190px]" />
        </Link>
      </div>
      <div className="flex flex-col overflow-y-auto no-scrollbar mt-2.5">
        <nav className="mb-6">
          <ul className="flex flex-col gap-4">
            {navItems.map((nav) => (
              <li key={nav.name}>
                {!nav.subItems && nav.path === "/dashboard" ? (
                  <>
                    <Link
                      to={nav.path}
                      className={`menu-item-dashboard group ${isActive(nav.path) ? "menu-item-dashboard-active" : "menu-item-dashboard-inactive"}`}
                    >
                      <span className={`menu-item-icon-size ${isActive(nav.path) ? "menu-item-icon-active" : "menu-item-icon-inactive"}`}>{nav.icon}</span>
                      <span className="menu-item-text font-semibold">{nav.name}</span>
                    </Link>
                    <div className="mx-3 mt-4 border-b border-sidebar-border" />
                  </>
                ) : nav.subItems ? (
                  <div className="menu-item group menu-item-active cursor-pointer">
                    <span className="menu-item-icon-size menu-item-icon-active">{nav.icon}</span>
                    <span className="menu-item-text">{nav.name}</span>
                  </div>
                ) : (
                  nav.path && (
                    <Link to={nav.path} className={`menu-item group ${isActive(nav.path) ? "menu-item-active" : "menu-item-inactive"}`}>
                      <span className={`menu-item-icon-size ${isActive(nav.path) ? "menu-item-icon-active" : "menu-item-icon-inactive"}`}>{nav.icon}</span>
                      <span className="menu-item-text">{nav.name}</span>
                    </Link>
                  )
                )}
                {nav.subItems && (
                  <ul className="mt-2 space-y-1 ml-8">
                    {nav.subItems.map((sub) => (
                      <li key={sub.name}>
                        <Link
                          to={sub.path}
                          className={`menu-dropdown-item ${isActive(sub.path) ? "menu-dropdown-item-active" : "menu-dropdown-item-inactive"}`}
                        >
                          {sub.name}
                        </Link>
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            ))}
          </ul>
        </nav>
      </div>
    </aside>
  );
};

export default AppSidebar;
