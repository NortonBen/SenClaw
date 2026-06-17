import { google } from 'googleapis';
import { SenclawSpace } from '@senclaw/space-sdk';

const APP_ID = process.env.SENCLAW_SPACE_APP_ID || 'google-workspace';
const BASE_URL = (process.env.SENCLAW_BASE_URL || 'http://127.0.0.1:18788').replace(/\/$/, '');
const SETTINGS_KEY = 'google-workspace-settings';
const space = SenclawSpace.forDaemon(APP_ID, BASE_URL);

export async function getGoogleSettings() {
  const settings = await space.getConfig(SETTINGS_KEY);
  return settings || {};
}

export async function saveGoogleSettings(newSettings) {
  const current = await getGoogleSettings();
  const merged = { ...current, ...newSettings };
  await space.setConfig(SETTINGS_KEY, merged);
  return merged;
}

export async function getOAuth2Client(redirectUri = 'http://127.0.0.1:4310/api/auth/callback') {
  const settings = await getGoogleSettings();
  if (!settings.clientId || !settings.clientSecret) {
    throw new Error('Google Cloud credentials (clientId, clientSecret) are not configured.');
  }
  
  const oauth2Client = new google.auth.OAuth2(
    settings.clientId,
    settings.clientSecret,
    redirectUri
  );

  if (settings.tokens) {
    oauth2Client.setCredentials(settings.tokens);
  }

  // Handle token refresh automatically
  oauth2Client.on('tokens', async (tokens) => {
    const currentSettings = await getGoogleSettings();
    if (tokens.refresh_token) {
      currentSettings.tokens = { ...currentSettings.tokens, ...tokens };
    } else {
      currentSettings.tokens = { ...currentSettings.tokens, access_token: tokens.access_token, expiry_date: tokens.expiry_date };
    }
    await space.setConfig(SETTINGS_KEY, currentSettings);
  });

  return oauth2Client;
}

export const SCOPES = [
  'https://www.googleapis.com/auth/gmail.readonly',
  'https://www.googleapis.com/auth/gmail.send',
  'https://www.googleapis.com/auth/calendar.events',
  'https://www.googleapis.com/auth/drive.file',
  'https://www.googleapis.com/auth/drive.readonly'
];
