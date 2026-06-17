import { Routes, Route, Link, useLocation } from 'react-router-dom';
import { LayoutDashboard, FileText, CheckSquare } from 'lucide-react';
import Dashboard from './pages/Dashboard';
import Requirements from './pages/Requirements';
import TestCases from './pages/TestCases';

function App() {
  const location = useLocation();

  const navItems = [
    { path: '/', label: 'Dashboard', icon: <LayoutDashboard size={20} /> },
    { path: '/requirements', label: 'Requirements', icon: <FileText size={20} /> },
    { path: '/test-cases', label: 'Test Cases', icon: <CheckSquare size={20} /> },
  ];

  return (
    <div className="flex h-screen bg-gray-100 text-gray-900">
      {/* Sidebar */}
      <aside className="w-64 bg-white border-r border-gray-200 flex flex-col">
        <div className="p-4 border-b border-gray-200 flex items-center space-x-2">
          <div className="w-8 h-8 rounded bg-blue-600 flex items-center justify-center text-white font-bold">TM</div>
          <h1 className="font-semibold text-lg text-gray-800">Test Manager</h1>
        </div>
        <nav className="flex-1 p-4 space-y-1">
          {navItems.map((item) => {
            const isActive = location.pathname === item.path;
            return (
              <Link
                key={item.path}
                to={item.path}
                className={`flex items-center space-x-3 px-3 py-2 rounded-md transition-colors ${
                  isActive 
                    ? 'bg-blue-50 text-blue-700 font-medium' 
                    : 'text-gray-600 hover:bg-gray-50 hover:text-gray-900'
                }`}
              >
                {item.icon}
                <span>{item.label}</span>
              </Link>
            );
          })}
        </nav>
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-auto">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/requirements" element={<Requirements />} />
          <Route path="/test-cases" element={<TestCases />} />
        </Routes>
      </main>
    </div>
  );
}

export default App;
