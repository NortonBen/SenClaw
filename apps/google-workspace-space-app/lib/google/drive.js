import { google } from 'googleapis';
import { getOAuth2Client } from './auth.js';
import stream from 'stream';

export async function getDriveClient() {
  const auth = await getOAuth2Client();
  return google.drive({ version: 'v3', auth });
}

export async function listFiles(maxResults = 10) {
  const drive = await getDriveClient();
  const res = await drive.files.list({
    pageSize: maxResults,
    fields: 'nextPageToken, files(id, name, mimeType, modifiedTime)',
    orderBy: 'modifiedTime desc'
  });
  return res.data.files || [];
}

export async function uploadFile(name, mimeType, textContent) {
  const drive = await getDriveClient();
  const bufferStream = new stream.PassThrough();
  bufferStream.end(Buffer.from(textContent));
  
  const res = await drive.files.create({
    requestBody: { name, mimeType },
    media: { mimeType, body: bufferStream },
    fields: 'id, name, webViewLink'
  });
  return res.data;
}
