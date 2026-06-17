import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { WebStandardStreamableHTTPServerTransport } from '@modelcontextprotocol/sdk/server/webStandardStreamableHttp.js';
import { SenclawSpace } from '@senclaw/space-sdk';
import { z } from 'zod';

import { getGoogleSettings, saveGoogleSettings } from '../../lib/google/auth.js';
import { listEmails, readEmail, sendEmail } from '../../lib/google/gmail.js';
import { listEvents, createEvent } from '../../lib/google/calendar.js';
import { listFiles, uploadFile } from '../../lib/google/drive.js';

export const dynamic = 'force-dynamic';
export const runtime = 'nodejs'; // Required for googleapis which relies on node builtins

const ok = (structured, text) => ({ content: [{ type: 'text', text }], structuredContent: structured });
const fail = (e) => ({ isError: true, content: [{ type: 'text', text: `Error: ${e instanceof Error ? e.message : String(e)}` }] });

function buildServer() {
  const server = new McpServer({ name: 'google-workspace-mcp-server', version: '2.0.0' });

  // Settings Tools
  server.registerTool('gworkspace_get_settings', {
    description: 'Read the saved Google Workspace settings including credentials and sync window.',
    inputSchema: {},
    annotations: { readOnlyHint: true }
  }, async () => {
    try {
      const s = await getGoogleSettings();
      // Mask secrets
      const safe = { ...s };
      if (safe.clientSecret) safe.clientSecret = '***';
      if (safe.tokens) safe.tokens = '***';
      return ok(safe, JSON.stringify(safe, null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('gworkspace_set_settings', {
    description: 'Update settings including clientId and clientSecret.',
    inputSchema: {
      clientId: z.string().optional(),
      clientSecret: z.string().optional(),
      days: z.number().int().optional(),
    }
  }, async (args) => {
    try {
      const saved = await saveGoogleSettings(args);
      const safe = { ...saved };
      if (safe.clientSecret) safe.clientSecret = '***';
      if (safe.tokens) safe.tokens = '***';
      return ok(safe, JSON.stringify(safe, null, 2));
    } catch (e) { return fail(e); }
  });

  // Gmail Tools
  server.registerTool('gworkspace_list_emails', {
    description: 'List recent emails from Gmail.',
    inputSchema: { maxResults: z.number().int().optional().default(10) }
  }, async ({ maxResults }) => {
    try {
      const emails = await listEmails(maxResults);
      return ok(emails, JSON.stringify(emails, null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('gworkspace_read_email', {
    description: 'Read the full content of a specific email by ID.',
    inputSchema: { id: z.string() }
  }, async ({ id }) => {
    try {
      const email = await readEmail(id);
      return ok(email, JSON.stringify(email, null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('gworkspace_send_email', {
    description: 'Send an email via Gmail.',
    inputSchema: { 
      to: z.string(), 
      subject: z.string(), 
      body: z.string() 
    }
  }, async ({ to, subject, body }) => {
    try {
      const res = await sendEmail(to, subject, body);
      return ok(res, `Email sent! ID: ${res.id}`);
    } catch (e) { return fail(e); }
  });

  // Calendar Tools
  server.registerTool('gworkspace_list_events', {
    description: 'List upcoming events from Google Calendar.',
    inputSchema: { maxResults: z.number().int().optional().default(10) }
  }, async ({ maxResults }) => {
    try {
      const events = await listEvents(maxResults);
      return ok(events, JSON.stringify(events, null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('gworkspace_create_event', {
    description: 'Create a new event in Google Calendar.',
    inputSchema: { 
      summary: z.string(), 
      description: z.string().optional().default(''), 
      startTime: z.string().describe('ISO string of start time'), 
      endTime: z.string().describe('ISO string of end time') 
    }
  }, async ({ summary, description, startTime, endTime }) => {
    try {
      const res = await createEvent(summary, description, startTime, endTime);
      return ok(res, `Event created! Link: ${res.htmlLink}`);
    } catch (e) { return fail(e); }
  });

  // Drive Tools
  server.registerTool('gworkspace_list_files', {
    description: 'List recently modified files from Google Drive.',
    inputSchema: { maxResults: z.number().int().optional().default(10) }
  }, async ({ maxResults }) => {
    try {
      const files = await listFiles(maxResults);
      return ok(files, JSON.stringify(files, null, 2));
    } catch (e) { return fail(e); }
  });

  server.registerTool('gworkspace_upload_file', {
    description: 'Upload a text file to Google Drive.',
    inputSchema: { 
      name: z.string(), 
      mimeType: z.string().optional().default('text/plain'), 
      textContent: z.string() 
    }
  }, async ({ name, mimeType, textContent }) => {
    try {
      const res = await uploadFile(name, mimeType, textContent);
      return ok(res, `File uploaded! Link: ${res.webViewLink}`);
    } catch (e) { return fail(e); }
  });

  return server;
}

async function handle(req) {
  const headers = new Headers(req.headers);
  const accept = headers.get('accept') || '';
  if (!accept.includes('application/json') || !accept.includes('text/event-stream')) {
    headers.set('accept', 'application/json, text/event-stream');
  }
  const body = req.method === 'POST' ? await req.arrayBuffer() : undefined;
  const patched = new Request(req.url, { method: req.method, headers, body });

  const server = buildServer();
  const transport = new WebStandardStreamableHTTPServerTransport({
    sessionIdGenerator: undefined,
    enableJsonResponse: true,
  });
  await server.connect(transport);
  return transport.handleRequest(patched);
}

export const GET = handle;
export const POST = handle;
