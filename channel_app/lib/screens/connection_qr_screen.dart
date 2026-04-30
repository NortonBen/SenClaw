import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import 'package:qr_flutter/qr_flutter.dart';

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
        if (token != null) 'token': token,
      },
    );
    return uri.toString();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D0D1F),
      appBar: AppBar(
        backgroundColor: Colors.transparent,
        elevation: 0,
        leading: IconButton(
          icon: const Icon(Icons.arrow_back, color: Colors.white),
          onPressed: () => Navigator.pop(context),
        ),
        title: const Text('Connection QR', 
          style: TextStyle(color: Colors.white, fontWeight: FontWeight.bold)),
      ),
      body: FutureBuilder<String?>(
        future: _getQRData(),
        builder: (context, snapshot) {
          if (snapshot.connectionState == ConnectionState.waiting) {
            return const Center(child: CircularProgressIndicator(color: Colors.cyanAccent));
          }
          if (snapshot.hasError || snapshot.data == null) {
            return const Center(
              child: Text('Failed to load connection data', 
                style: TextStyle(color: Colors.white)),
            );
          }

          return Center(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Container(
                  padding: const EdgeInsets.all(16),
                  decoration: BoxDecoration(
                    color: Colors.white,
                    borderRadius: BorderRadius.circular(24),
                    boxShadow: [
                      BoxShadow(
                        color: Colors.cyanAccent.withOpacity(0.2),
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
                const Text(
                  'Scan this QR code with another device\nto sync the channel connection.',
                  textAlign: TextAlign.center,
                  style: TextStyle(color: Colors.white70, fontSize: 16),
                ),
                const SizedBox(height: 16),
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 40),
                  child: Text(
                    snapshot.data!,
                    style: TextStyle(
                      color: Colors.white.withOpacity(0.3),
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
