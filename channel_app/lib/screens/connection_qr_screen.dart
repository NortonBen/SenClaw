import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:qr_flutter/qr_flutter.dart';
import '../theme/tokens.dart';

class ConnectionQRScreen extends StatelessWidget {
  const ConnectionQRScreen({super.key});

  Future<String?> _getQRData() async {
    const storage = FlutterSecureStorage();
    final hub = await storage.read(key: 'hub_url');
    final cid = await storage.read(key: 'channel_id');
    final key = await storage.read(key: 'encryption_key');
    final token = await storage.read(key: 'auth_token');

    if (hub == null || cid == null || key == null) return null;

    final uri = Uri(
      scheme: 'semaclaw',
      host: 'connect',
      queryParameters: {
        'hub': hub,
        'cid': cid,
        'key': key,
        'token': ?token,
      },
    );
    return uri.toString();
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      backgroundColor: c.bg,
      appBar: AppBar(
        backgroundColor: Colors.transparent,
        elevation: 0,
        leading: IconButton(
          icon: Icon(Icons.arrow_back, color: c.textPrimary),
          onPressed: () => Navigator.pop(context),
        ),
        title: Text('Connection QR',
          style: TextStyle(color: c.textPrimary, fontWeight: FontWeight.bold)),
      ),
      body: FutureBuilder<String?>(
        future: _getQRData(),
        builder: (context, snapshot) {
          if (snapshot.connectionState == ConnectionState.waiting) {
            return Center(child: CircularProgressIndicator(color: c.accent));
          }
          if (snapshot.hasError || snapshot.data == null) {
            return Center(
              child: Text('Failed to load connection data',
                style: TextStyle(color: c.textPrimary)),
            );
          }

          return Center(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Container(
                  padding: const EdgeInsets.all(16),
                  // A QR must stay dark-on-light to scan, so this container is
                  // intentionally white in both themes.
                  decoration: BoxDecoration(
                    color: Colors.white,
                    borderRadius: BorderRadius.circular(24),
                    boxShadow: [
                      BoxShadow(
                        color: AppTokens.cyan.withValues(alpha: 0.2),
                        blurRadius: 20,
                        spreadRadius: 5,
                      ),
                    ],
                  ),
                  child: QrImageView(
                    data: snapshot.data!,
                    version: QrVersions.auto,
                    size: 250.0,
                    gapless: false,
                    backgroundColor: Colors.white,
                  ),
                ),
                const SizedBox(height: 32),
                Text(
                  'Scan this QR code with another device\nto sync the channel connection.',
                  textAlign: TextAlign.center,
                  style: TextStyle(color: c.textSecondary, fontSize: 16),
                ),
                const SizedBox(height: 16),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 40),
                  child: Text(
                    snapshot.data!,
                    style: TextStyle(
                      color: c.textMuted,
                      fontSize: 10,
                      fontFamily: 'monospace',
                    ),
                    textAlign: TextAlign.center,
                    maxLines: 2,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
              ],
            ),
          );
        },
      ),
    );
  }
}
