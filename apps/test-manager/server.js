import express from 'express';
import multer from 'multer';
import csvParser from 'csv-parser';
import { createObjectCsvWriter } from 'csv-writer';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import { initDb, runQuery, getQuery } from './db.js';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { z } from 'zod';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const PORT = Number(process.env.PORT) || 4108;
const MCP_PATH = process.env.MCP_PATH || '/api/mcp/sse';

const app = express();
app.use(express.json());

// Set up uploads dir
const uploadsDir = path.join(__dirname, 'uploads');
if (!fs.existsSync(uploadsDir)) {
  fs.mkdirSync(uploadsDir);
}
const upload = multer({ dest: 'uploads/' });

// Wait for DB to init
await initDb();

// --- REST APIs for Frontend ---

// Requirements
app.get('/api/requirements', async (req, res) => {
  try {
    const rows = await getQuery('SELECT * FROM requirements');
    res.json(rows);
  } catch (e) {
    res.status(500).json({ error: e.message });
  }
});

app.post('/api/requirements', async (req, res) => {
  const { id, title, description, status } = req.body;
  try {
    await runQuery('INSERT OR REPLACE INTO requirements (id, title, description, status) VALUES (?, ?, ?, ?)', [id, title, description, status || 'Open']);
    res.json({ success: true });
  } catch (e) {
    res.status(500).json({ error: e.message });
  }
});

app.delete('/api/requirements/:id', async (req, res) => {
  try {
    await runQuery('DELETE FROM requirements WHERE id = ?', [req.params.id]);
    res.json({ success: true });
  } catch (e) {
    res.status(500).json({ error: e.message });
  }
});

// Test Cases
app.get('/api/test-cases', async (req, res) => {
  try {
    const rows = await getQuery('SELECT * FROM test_cases');
    res.json(rows);
  } catch (e) {
    res.status(500).json({ error: e.message });
  }
});

app.post('/api/test-cases', async (req, res) => {
  const { id, req_id, title, steps, expected_result, status, last_run_log } = req.body;
  try {
    await runQuery('INSERT OR REPLACE INTO test_cases (id, req_id, title, steps, expected_result, status, last_run_log) VALUES (?, ?, ?, ?, ?, ?, ?)', 
      [id, req_id, title, steps, expected_result, status || 'Draft', last_run_log || '']);
    res.json({ success: true });
  } catch (e) {
    res.status(500).json({ error: e.message });
  }
});

app.delete('/api/test-cases/:id', async (req, res) => {
  try {
    await runQuery('DELETE FROM test_cases WHERE id = ?', [req.params.id]);
    res.json({ success: true });
  } catch (e) {
    res.status(500).json({ error: e.message });
  }
});

// CSV Export Test Cases
app.get('/api/test-cases/export', async (req, res) => {
  try {
    const rows = await getQuery('SELECT * FROM test_cases');
    const tempFile = path.join(uploadsDir, `export_${Date.now()}.csv`);
    const csvWriter = createObjectCsvWriter({
      path: tempFile,
      header: [
        {id: 'id', title: 'ID'},
        {id: 'req_id', title: 'Requirement_ID'},
        {id: 'title', title: 'Title'},
        {id: 'steps', title: 'Steps'},
        {id: 'expected_result', title: 'Expected_Result'},
        {id: 'status', title: 'Status'},
        {id: 'last_run_log', title: 'Log'}
      ]
    });
    await csvWriter.writeRecords(rows);
    res.download(tempFile, 'test-cases.csv', () => {
      fs.unlinkSync(tempFile);
    });
  } catch (e) {
    res.status(500).json({ error: e.message });
  }
});

// CSV Import Test Cases
app.post('/api/test-cases/import', upload.single('file'), (req, res) => {
  if (!req.file) return res.status(400).json({ error: 'No file uploaded' });
  
  const results = [];
  fs.createReadStream(req.file.path)
    .pipe(csvParser())
    .on('data', (data) => results.push(data))
    .on('end', async () => {
      try {
        for (const row of results) {
          const id = row.ID || row.id;
          const req_id = row.Requirement_ID || row.req_id || '';
          const title = row.Title || row.title || '';
          const steps = row.Steps || row.steps || '';
          const expected = row.Expected_Result || row.expected_result || '';
          const status = row.Status || row.status || 'Draft';
          const log = row.Log || row.last_run_log || '';
          if (id) {
            await runQuery('INSERT OR REPLACE INTO test_cases (id, req_id, title, steps, expected_result, status, last_run_log) VALUES (?, ?, ?, ?, ?, ?, ?)', 
              [id, req_id, title, steps, expected, status, log]);
          }
        }
        fs.unlinkSync(req.file.path);
        res.json({ success: true, count: results.length });
      } catch (e) {
        res.status(500).json({ error: e.message });
      }
    });
});

// --- MCP Server Setup ---

// Compatibility Shim
app.use(MCP_PATH, (req, _res, next) => {
  const desired = 'application/json, text/event-stream';
  const raw = req.rawHeaders;
  let idx = -1;
  for (let i = 0; i < raw.length; i += 2) {
    if (raw[i]?.toLowerCase() === 'accept') {
      idx = i + 1;
      break;
    }
  }
  const current = idx >= 0 ? String(raw[idx]) : '';
  if (!current.includes('application/json') || !current.includes('text/event-stream')) {
    if (idx >= 0) raw[idx] = desired;
    else raw.push('Accept', desired);
    req.headers.accept = desired;
  }
  next();
});

const createServer = () => {
  const server = new McpServer({ name: 'test-manager-mcp', version: '1.0.0' });

  server.tool('get_requirements', {}, async () => {
    const rows = await getQuery('SELECT * FROM requirements');
    return { content: [{ type: 'text', text: JSON.stringify(rows, null, 2) }] };
  });

  server.tool('get_test_cases', { req_id: z.string().optional() }, async ({ req_id }) => {
    let rows;
    if (req_id) {
      rows = await getQuery('SELECT * FROM test_cases WHERE req_id = ?', [req_id]);
    } else {
      rows = await getQuery('SELECT * FROM test_cases');
    }
    return { content: [{ type: 'text', text: JSON.stringify(rows, null, 2) }] };
  });

  server.tool('update_test_status', { 
    test_id: z.string(), 
    status: z.enum(['Passed', 'Failed', 'Ready', 'Draft']), 
    log: z.string().optional() 
  }, async ({ test_id, status, log }) => {
    const check = await getQuery('SELECT * FROM test_cases WHERE id = ?', [test_id]);
    if (check.length === 0) {
      return { isError: true, content: [{ type: 'text', text: `Test case ${test_id} not found.` }] };
    }
    await runQuery('UPDATE test_cases SET status = ?, last_run_log = ? WHERE id = ?', [status, log || '', test_id]);
    return { content: [{ type: 'text', text: `Successfully updated test case ${test_id} to ${status}.` }] };
  });

  server.tool('generate_report', {}, async () => {
    const total = await getQuery('SELECT COUNT(*) as count FROM test_cases');
    const passed = await getQuery('SELECT COUNT(*) as count FROM test_cases WHERE status = "Passed"');
    const failed = await getQuery('SELECT COUNT(*) as count FROM test_cases WHERE status = "Failed"');
    const report = `Test Execution Report:
Total Test Cases: ${total[0].count}
Passed: ${passed[0].count}
Failed: ${failed[0].count}`;
    return { content: [{ type: 'text', text: report }] };
  });

  return server;
};

// MCP Endpoint
app.post(MCP_PATH, async (req, res) => {
  const server = createServer();
  const transport = new StreamableHTTPServerTransport({
    sessionIdGenerator: undefined,
    enableJsonResponse: true,
  });
  res.on('close', () => {
    transport.close();
    server.close();
  });
  try {
    await server.connect(transport);
    await transport.handleRequest(req, res, req.body);
  } catch (error) {
    if (!res.headersSent) {
      res.status(500).json({ jsonrpc: '2.0', error: { code: -32603, message: 'Internal server error' }, id: null });
    }
  }
});

// Serve frontend
const distPath = path.join(__dirname, 'web/dist');
app.use(express.static(distPath));

// Fallback to index.html for React Router (Single Page App)
app.use((req, res, next) => {
  if (req.path.startsWith('/api')) {
    return res.status(404).json({error: 'Not found'});
  }
  res.sendFile(path.join(distPath, 'index.html'));
});

app.listen(PORT, '127.0.0.1', () => {
  console.log(`test-manager server listening on port ${PORT}`);
});
