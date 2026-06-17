import { google } from 'googleapis';
import { getOAuth2Client } from './auth.js';

export async function getCalendarClient() {
  const auth = await getOAuth2Client();
  return google.calendar({ version: 'v3', auth });
}

export async function listEvents(maxResults = 10) {
  const calendar = await getCalendarClient();
  const res = await calendar.events.list({
    calendarId: 'primary',
    timeMin: new Date().toISOString(),
    maxResults,
    singleEvents: true,
    orderBy: 'startTime',
  });
  return res.data.items || [];
}

export async function createEvent(summary, description, startTime, endTime) {
  const calendar = await getCalendarClient();
  const event = {
    summary,
    description,
    start: { dateTime: startTime },
    end: { dateTime: endTime },
  };
  const res = await calendar.events.insert({
    calendarId: 'primary',
    requestBody: event,
  });
  return res.data;
}
