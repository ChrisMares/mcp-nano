import { useCallback } from "react";
import { Link, useLocation } from "react-router-dom";
import { LayoutDashboard } from "lucide-react";
import { FolderIcon, ChatIcon, PlugIn } from "@/icons";
import { useSidebar } from "@/contexts/SidebarContext";
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
    subItems: [
      { name: "Fetch Context", path: "/query/fetch" },
    ],
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

const sidebarWidth = (expanded: boolean) => expanded ? "w-[290px]" : "w-[90px]";

const AppSidebar: React.FC = () => {
  const { isExpanded, isMobileOpen, isHovered, setIsHovered } = useSidebar();
  const location = useLocation();
  const isActive = useCallback((path: string) => {
    return location.pathname === path;
  }, [location.pathname]);

  const showLabels = isExpanded || isHovered || isMobileOpen;

  return (
    <aside
      className={`fixed mt-16 flex flex-col lg:mt-0 top-0 px-5 left-0 bg-sidebar text-sidebar-foreground h-screen transition-all duration-300 ease-in-out z-50 border-r border-sidebar-border shadow-lg
        ${sidebarWidth(showLabels)}
        ${isMobileOpen ? "translate-x-0" : "-translate-x-full"}
        lg:translate-x-0`}
      onMouseEnter={() => !isExpanded && setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      <div className={`py-4 flex ${!showLabels ? "lg:justify-center" : "justify-center"}`}>
        <Link 
          to="/" 
          className="logo-container w-full flex items-center justify-center px-3 py-5 rounded-xl transition-all duration-300"
        >
          {showLabels ? (
            <BlockLogo className="sidebar-block-logo max-w-[200px]" />
          ) : (
            <BlockLogo className="sidebar-block-logo w-14" />
          )}
        </Link>
      </div>
      <div className="flex flex-col overflow-y-auto duration-300 ease-linear no-scrollbar mt-2.5">
        <nav className="mb-6">
          <div className="flex flex-col gap-4">
            <div>
              <ul className="flex flex-col gap-4">
                {navItems.map((nav) => (
                  <li key={nav.name}>
                    {/* Dashboard: standalone top-level link with accent styling */}
                    {!nav.subItems && nav.path === "/dashboard" ? (
                      <>
                        <Link
                          to={nav.path}
                          className={`menu-item-dashboard group ${isActive(nav.path) ? "menu-item-dashboard-active" : "menu-item-dashboard-inactive"}`}
                        >
                          <span className={`menu-item-icon-size ${isActive(nav.path) ? "menu-item-icon-active" : "menu-item-icon-inactive"}`}>
                            {nav.icon}
                          </span>
                          {showLabels && <span className="menu-item-text font-semibold">{nav.name}</span>}
                        </Link>
                        <div className="mx-3 mt-4 border-b border-sidebar-border" />
                      </>
                    ) : nav.subItems ? (
                      <div className="menu-item group menu-item-active cursor-pointer">
                        <span className="menu-item-icon-size menu-item-icon-active">{nav.icon}</span>
                        {showLabels && <span className="menu-item-text">{nav.name}</span>}
                      </div>
                    ) : nav.path && (
                      <Link
                        to={nav.path}
                        className={`menu-item group ${isActive(nav.path) ? "menu-item-active" : "menu-item-inactive"}`}
                      >
                        <span className={`menu-item-icon-size ${isActive(nav.path) ? "menu-item-icon-active" : "menu-item-icon-inactive"}`}>
                          {nav.icon}
                        </span>
                        {showLabels && <span className="menu-item-text">{nav.name}</span>}
                      </Link>
                    )}
                    {nav.subItems && (
                      <ul className="mt-2 space-y-1 ml-9">
                        {nav.subItems.map((sub) => (
                          <li key={sub.name}>
                            <Link
                              to={sub.path}
                              className={`menu-dropdown-item ${
                                isActive(sub.path) ? "menu-dropdown-item-active" : "menu-dropdown-item-inactive"
                              }`}
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
            </div>
          </div>
        </nav>
      </div>
    </aside>
  );
};

export default AppSidebar;
