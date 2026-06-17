import { useEffect, useState } from 'react';
import axios from 'axios';
import { PieChart, ListChecks, FileText } from 'lucide-react';

export default function Dashboard() {
  const [stats, setStats] = useState({ reqs: 0, total: 0, passed: 0, failed: 0, draft: 0, ready: 0 });

  useEffect(() => {
    fetchData();
  }, []);

  const fetchData = async () => {
    try {
      const reqRes = await axios.get('/api/requirements');
      const testRes = await axios.get('/api/test-cases');
      
      const testCases = testRes.data;
      const passed = testCases.filter((t: any) => t.status === 'Passed').length;
      const failed = testCases.filter((t: any) => t.status === 'Failed').length;
      const draft = testCases.filter((t: any) => t.status === 'Draft').length;
      const ready = testCases.filter((t: any) => t.status === 'Ready').length;

      setStats({
        reqs: reqRes.data.length,
        total: testCases.length,
        passed, failed, draft, ready
      });
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <h1 className="text-2xl font-bold text-gray-800 mb-6">Dashboard</h1>
      
      <div className="grid grid-cols-1 md:grid-cols-3 gap-6 mb-8">
        <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 flex items-center">
          <div className="p-4 bg-blue-50 text-blue-600 rounded-full mr-4">
            <FileText size={24} />
          </div>
          <div>
            <p className="text-sm text-gray-500 font-medium">Total Requirements</p>
            <p className="text-2xl font-bold text-gray-800">{stats.reqs}</p>
          </div>
        </div>
        
        <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 flex items-center">
          <div className="p-4 bg-purple-50 text-purple-600 rounded-full mr-4">
            <ListChecks size={24} />
          </div>
          <div>
            <p className="text-sm text-gray-500 font-medium">Total Test Cases</p>
            <p className="text-2xl font-bold text-gray-800">{stats.total}</p>
          </div>
        </div>

        <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 flex items-center">
          <div className="p-4 bg-green-50 text-green-600 rounded-full mr-4">
            <PieChart size={24} />
          </div>
          <div>
            <p className="text-sm text-gray-500 font-medium">Verify Pass Rate</p>
            <p className="text-2xl font-bold text-gray-800">
              {stats.total > 0 ? Math.round((stats.passed / stats.total) * 100) : 0}%
            </p>
          </div>
        </div>
      </div>

      <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6">
        <h2 className="text-lg font-semibold text-gray-800 mb-4">Test Execution Status</h2>
        <div className="space-y-4">
          <div>
            <div className="flex justify-between text-sm mb-1">
              <span className="text-gray-600">Passed</span>
              <span className="font-medium">{stats.passed}</span>
            </div>
            <div className="w-full bg-gray-100 rounded-full h-2">
              <div className="bg-green-500 h-2 rounded-full" style={{ width: `${stats.total ? (stats.passed/stats.total)*100 : 0}%` }}></div>
            </div>
          </div>
          <div>
            <div className="flex justify-between text-sm mb-1">
              <span className="text-gray-600">Failed</span>
              <span className="font-medium">{stats.failed}</span>
            </div>
            <div className="w-full bg-gray-100 rounded-full h-2">
              <div className="bg-red-500 h-2 rounded-full" style={{ width: `${stats.total ? (stats.failed/stats.total)*100 : 0}%` }}></div>
            </div>
          </div>
          <div>
            <div className="flex justify-between text-sm mb-1">
              <span className="text-gray-600">Ready</span>
              <span className="font-medium">{stats.ready}</span>
            </div>
            <div className="w-full bg-gray-100 rounded-full h-2">
              <div className="bg-yellow-500 h-2 rounded-full" style={{ width: `${stats.total ? (stats.ready/stats.total)*100 : 0}%` }}></div>
            </div>
          </div>
          <div>
            <div className="flex justify-between text-sm mb-1">
              <span className="text-gray-600">Draft</span>
              <span className="font-medium">{stats.draft}</span>
            </div>
            <div className="w-full bg-gray-100 rounded-full h-2">
              <div className="bg-gray-400 h-2 rounded-full" style={{ width: `${stats.total ? (stats.draft/stats.total)*100 : 0}%` }}></div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
