import { google } from 'googleapis';
import { getOAuth2Client } from './auth.js';

export async function getGmailClient() {
  const auth = await getOAuth2Client();
  return google.gmail({ version: 'v1', auth });
}

export async function listEmails(maxResults = 10) {
  const gmail = await getGmailClient();
  const res = await gmail.users.messages.list({ userId: 'me', maxResults });
  
  if (!res.data.messages) return [];
  
  const messages = await Promise.all(
    res.data.messages.map(async (msg) => {
      const msgData = await gmail.users.messages.get({ userId: 'me', id: msg.id, format: 'metadata', metadataHeaders: ['Subject', 'From', 'Date'] });
      const headers = msgData.data.payload.headers;
      const subject = headers.find(h => h.name === 'Subject')?.value;
      const from = headers.find(h => h.name === 'From')?.value;
      const date = headers.find(h => h.name === 'Date')?.value;
      return { id: msg.id, snippet: msgData.data.snippet, subject, from, date };
    })
  );
  return messages;
}

export async function readEmail(id) {
  const gmail = await getGmailClient();
  const res = await gmail.users.messages.get({ userId: 'me', id, format: 'full' });
  return res.data;
}

export async function sendEmail(to, subject, body) {
  const gmail = await getGmailClient();
  const message = [
    `To: ${to}`,
    'Content-Type: text/html; charset=utf-8',
    'MIME-Version: 1.0',
    `Subject: ${subject}`,
    '',
    body
  ].join('\n');
  const encodedMessage = Buffer.from(message).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  
  const res = await gmail.users.messages.send({
    userId: 'me',
    requestBody: { raw: encodedMessage }
  });
  return res.data;
}
