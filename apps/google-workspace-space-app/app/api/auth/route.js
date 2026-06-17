import { NextResponse } from 'next/server';
import { getOAuth2Client, SCOPES, getGoogleSettings } from '../../../lib/google/auth.js';

export async function GET(req) {
  try {
    const settings = await getGoogleSettings();
    if (!settings.clientId || !settings.clientSecret) {
      return NextResponse.json({ error: 'Please configure Client ID and Client Secret in settings first.' }, { status: 400 });
    }

    const host = req.headers.get('host') || '127.0.0.1:4310';
    const protocol = req.headers.get('x-forwarded-proto') || 'http';
    const redirectUri = `${protocol}://${host}/api/auth/callback`;

    const oauth2Client = await getOAuth2Client(redirectUri);
    const url = oauth2Client.generateAuthUrl({
      access_type: 'offline',
      prompt: 'consent',
      scope: SCOPES,
    });
    return NextResponse.redirect(url);
  } catch (error) {
    return NextResponse.json({ error: error.message }, { status: 500 });
  }
}
