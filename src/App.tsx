import { BrowserRouter as Router, Routes, Route, Navigate } from 'react-router-dom';
import { HelmetProvider } from 'react-helmet-async';
import { AuthProvider } from './contexts/AuthContext';
import { ThemeProvider } from './contexts/ThemeContext';
import AppLayout from './components/layout/AppLayout';
import UploadFiles from './pages/UploadFiles';
import DataManagement from './pages/DataManagement';
import FetchContext from './pages/FetchContext';
import McpCreate from './pages/McpCreate';
import McpManage from './pages/McpManage';
import McpConnect from './pages/McpConnect';
import Account from './pages/Account';
import Dashboard from './pages/Dashboard';

function App() {
  return (
    <HelmetProvider>
    <Router>
      <ThemeProvider>
        <AuthProvider>
          <Routes>
            <Route element={<AppLayout />}>
              <Route path="/" element={<Navigate to="/dashboard" replace />} />
              <Route path="/dashboard" element={<Dashboard />} />
              <Route path="/embed/upload" element={<UploadFiles />} />
              <Route path="/embed/data" element={<DataManagement />} />
              <Route path="/query/fetch" element={<FetchContext />} />
              <Route path="/mcp/create" element={<McpCreate />} />
              <Route path="/mcp/manage" element={<McpManage />} />
              <Route path="/mcp/connect" element={<McpConnect />} />
              <Route path="/account" element={<Account />} />
            </Route>
            <Route path="*" element={<Navigate to="/" replace />} />
          </Routes>
        </AuthProvider>
      </ThemeProvider>
    </Router>
    </HelmetProvider>
  );
}

export default App;
