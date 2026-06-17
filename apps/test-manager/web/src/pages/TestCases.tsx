import { useEffect, useState, useRef } from 'react';
import axios from 'axios';
import { Plus, Trash2, Download, Upload } from 'lucide-react';

export default function TestCases() {
  const [testCases, setTestCases] = useState<any[]>([]);
  const [loading, setLoading] = useState(true);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Form
  const [id, setId] = useState('');
  const [reqId, setReqId] = useState('');
  const [title, setTitle] = useState('');
  const [steps, setSteps] = useState('');
  const [expected, setExpected] = useState('');

  useEffect(() => {
    fetchTestCases();
  }, []);

  const fetchTestCases = async () => {
    try {
      const res = await axios.get('/api/test-cases');
      setTestCases(res.data);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!id || !title) return;
    try {
      await axios.post('/api/test-cases', { 
        id, 
        req_id: reqId, 
        title, 
        steps, 
        expected_result: expected 
      });
      setId('');
      setReqId('');
      setTitle('');
      setSteps('');
      setExpected('');
      fetchTestCases();
    } catch (e) {
      console.error(e);
    }
  };

  const handleDelete = async (deleteId: string) => {
    try {
      await axios.delete(`/api/test-cases/${deleteId}`);
      fetchTestCases();
    } catch (e) {
      console.error(e);
    }
  };

  const handleExport = () => {
    window.open('/api/test-cases/export', '_blank');
  };

  const handleImport = async (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!e.target.files || e.target.files.length === 0) return;
    const file = e.target.files[0];
    const formData = new FormData();
    formData.append('file', file);
    
    try {
      setLoading(true);
      await axios.post('/api/test-cases/import', formData, {
        headers: { 'Content-Type': 'multipart/form-data' }
      });
      fetchTestCases();
      alert('CSV Imported Successfully!');
    } catch (err) {
      console.error(err);
      alert('Failed to import CSV');
    }
    
    // reset file input
    if (fileInputRef.current) fileInputRef.current.value = '';
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'Passed': return 'bg-green-100 text-green-800';
      case 'Failed': return 'bg-red-100 text-red-800';
      case 'Ready': return 'bg-yellow-100 text-yellow-800';
      default: return 'bg-gray-100 text-gray-800';
    }
  };

  return (
    <div className="p-8 max-w-6xl mx-auto">
      <div className="flex justify-between items-center mb-6">
        <h1 className="text-2xl font-bold text-gray-800">Test Cases</h1>
        <div className="flex space-x-3">
          <input 
            type="file" 
            accept=".csv" 
            className="hidden" 
            ref={fileInputRef} 
            onChange={handleImport} 
          />
          <button 
            onClick={() => fileInputRef.current?.click()}
            className="bg-white border border-gray-300 text-gray-700 px-4 py-2 rounded-md hover:bg-gray-50 flex items-center space-x-2 shadow-sm"
          >
            <Upload size={18} />
            <span>Import CSV</span>
          </button>
          <button 
            onClick={handleExport}
            className="bg-white border border-gray-300 text-gray-700 px-4 py-2 rounded-md hover:bg-gray-50 flex items-center space-x-2 shadow-sm"
          >
            <Download size={18} />
            <span>Export CSV</span>
          </button>
        </div>
      </div>

      <div className="bg-white rounded-xl shadow-sm border border-gray-100 p-6 mb-8">
        <h2 className="text-lg font-semibold text-gray-800 mb-4">Add Test Case</h2>
        <form onSubmit={handleSubmit} className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">ID (e.g., TC-001)</label>
              <input required value={id} onChange={e => setId(e.target.value)} type="text" className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">Requirement ID</label>
              <input value={reqId} onChange={e => setReqId(e.target.value)} type="text" className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">Title</label>
              <input required value={title} onChange={e => setTitle(e.target.value)} type="text" className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" />
            </div>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">Steps</label>
              <textarea value={steps} onChange={e => setSteps(e.target.value)} className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" rows={3}></textarea>
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">Expected Result</label>
              <textarea value={expected} onChange={e => setExpected(e.target.value)} className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500" rows={3}></textarea>
            </div>
          </div>
          <button type="submit" className="bg-blue-600 text-white px-4 py-2 rounded-md hover:bg-blue-700 flex items-center space-x-2">
            <Plus size={18} />
            <span>Save Test Case</span>
          </button>
        </form>
      </div>

      <div className="bg-white rounded-xl shadow-sm border border-gray-100 overflow-hidden">
        <table className="min-w-full divide-y divide-gray-200">
          <thead className="bg-gray-50">
            <tr>
              <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">ID</th>
              <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Req ID</th>
              <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Title</th>
              <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Status</th>
              <th className="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase tracking-wider">Actions</th>
            </tr>
          </thead>
          <tbody className="bg-white divide-y divide-gray-200">
            {testCases.map((tc) => (
              <tr key={tc.id}>
                <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">{tc.id}</td>
                <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{tc.req_id}</td>
                <td className="px-6 py-4 text-sm text-gray-500">{tc.title}</td>
                <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                  <span className={`px-2 inline-flex text-xs leading-5 font-semibold rounded-full ${getStatusColor(tc.status)}`}>
                    {tc.status}
                  </span>
                </td>
                <td className="px-6 py-4 whitespace-nowrap text-right text-sm font-medium">
                  <button onClick={() => handleDelete(tc.id)} className="text-red-600 hover:text-red-900">
                    <Trash2 size={18} />
                  </button>
                </td>
              </tr>
            ))}
            {testCases.length === 0 && !loading && (
              <tr>
                <td colSpan={5} className="px-6 py-4 text-center text-sm text-gray-500">No test cases found.</td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
