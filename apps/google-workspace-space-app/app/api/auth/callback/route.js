import { NextResponse } from 'next/server';
import { getOAuth2Client, saveGoogleSettings } from '../../../../lib/google/auth.js';

export async function GET(req) {
  const url = new URL(req.url);
  const code = url.searchParams.get('code');
  if (!code) {
    return NextResponse.json({ error: 'No code provided.' }, { status: 400 });
  }

  try {
    const host = req.headers.get('host') || '127.0.0.1:4310';
    const protocol = req.headers.get('x-forwarded-proto') || 'http';
    const redirectUri = `${protocol}://${host}/api/auth/callback`;

    const oauth2Client = await getOAuth2Client(redirectUri);
    const { tokens } = await oauth2Client.getToken(code);
    
    await saveGoogleSettings({ tokens });

    // Redirect back to home
    return NextResponse.redirect(`${protocol}://${host}/`);
  } catch (error) {
    console.error('Error fetching tokens:', error);
    return NextResponse.json({ error: 'Failed to retrieve tokens.' }, { status: 500 });
  }
}
