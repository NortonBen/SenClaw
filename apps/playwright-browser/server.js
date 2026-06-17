import express from 'express';
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { StreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/streamableHttp.js';
import { z } from 'zod';
import { chromium } from 'playwright';
import path from 'path';
import { fileURLToPath } from 'url';
import { WebSocketServer } from 'ws';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const PORT = Number(process.env.PORT) || 4107;
const MCP_PATH = process.env.MCP_PATH || '/api/mcp/sse';

const app = express();
app.use(express.json({ limit: '10mb' }));

// Compatibility shim for MCP
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

// Setup Playwright
let browser;
let context;
let page;

async function initBrowser() {
  if (!browser) {
    browser = await chromium.launch({ headless: true });
    context = await browser.newContext();
    page = await context.newPage();
  }
  return page;
}

// MCP Server creation
const createServer = () => {
  const server = new McpServer({ name: 'playwright-browser-mcp', version: '1.0.0' });

  server.tool('navigate', { url: z.string().url() }, async ({ url }) => {
    const p = await initBrowser();
    await p.goto(url, { waitUntil: 'domcontentloaded' });
    return { content: [{ type: 'text', text: `Navigated to ${url}` }] };
  });

  server.tool('click', { selector: z.string() }, async ({ selector }) => {
    const p = await initBrowser();
    await p.click(selector);
    return { content: [{ type: 'text', text: `Clicked ${selector}` }] };
  });

  server.tool('fill_text', { selector: z.string(), text: z.string() }, async ({ selector, text }) => {
    const p = await initBrowser();
    await p.fill(selector, text);
    return { content: [{ type: 'text', text: `Filled ${selector} with text` }] };
  });

  server.tool('evaluate', { js: z.string() }, async ({ js }) => {
    const p = await initBrowser();
    const result = await p.evaluate(js);
    return { content: [{ type: 'text', text: JSON.stringify(result) }] };
  });

  server.tool('get_html', {}, async () => {
    const p = await initBrowser();
    const html = await p.content();
    return { content: [{ type: 'text', text: html }] };
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

const server = app.listen(PORT, '127.0.0.1', () => {
  console.log(`playwright-browser server listening on port ${PORT}`);
});

// Setup WebSocket for screenshot streaming
const wss = new WebSocketServer({ server });

wss.on('connection', async (ws) => {
  console.log('Client connected to WebSocket');
  
  let interval = setInterval(async () => {
    if (page && !page.isClosed()) {
      try {
        const buffer = await page.screenshot({ type: 'jpeg', quality: 50 });
        ws.send(JSON.stringify({ 
          type: 'screenshot', 
          data: buffer.toString('base64'),
          url: page.url(),
          title: await page.title()
        }));
      } catch (err) {
        // ignore errors if page is busy navigating
      }
    }
  }, 1000); // 1 FPS

  ws.on('close', () => {
    clearInterval(interval);
  });
  
  ws.on('message', async (message) => {
    const data = JSON.parse(message);
    if (data.action === 'navigate') {
      const p = await initBrowser();
      await p.goto(data.url).catch(console.error);
    } else if (data.action === 'click') {
      if (page) {
        // simulate click at coordinates
        await page.mouse.click(data.x, data.y).catch(console.error);
      }
    } else if (data.action === 'scroll') {
      if (page) {
        await page.mouse.wheel(0, data.deltaY).catch(console.error);
      }
    } else if (data.action === 'type') {
      if (page) {
        await page.keyboard.type(data.text).catch(console.error);
      }
    } else if (data.action === 'press') {
      if (page) {
        await page.keyboard.press(data.key).catch(console.error);
      }
    }
  });
});

process.on('SIGTERM', async () => {
  if (browser) await browser.close();
  server.close();
  process.exit(0);
});
