import 'package:flutter/material.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';
import '../services/relay_service.dart';
import '../services/crypto_service.dart';
import 'welcome_screen.dart';
import '../services/language_service.dart';
import 'connection_qr_screen.dart';

class ChatScreen extends StatefulWidget {
  const ChatScreen({super.key});

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> {
  final storage = const FlutterSecureStorage();
  RelayService? _relayService;
  final List<String> _messages = [];

  @override
  void initState() {
    super.initState();
    _initRelay();
  }

  Future<void> _initRelay() async {
    final hub = await storage.read(key: 'hub_url');
    final cid = await storage.read(key: 'channel_id');
    final token = await storage.read(key: 'access_token');
    final key = await storage.read(key: 'encryption_key');

    if (hub != null && cid != null && token != null && key != null) {
      _relayService = RelayService(
        hubUrl: hub,
        channelId: cid,
        senderId: 'mobile-app',
        accessToken: token,
        encryptionKey: CryptoService.parseBase64Key(key),
      );
      
      _relayService!.incomingMessages.listen((msg) {
        setState(() {
          _messages.add(msg);
        });
      });

      _relayService!.start();
    }
  }

  Future<void> _confirmDisconnect(BuildContext context) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(t('logout_confirm_title')),
        content: Text(t('logout_confirm_msg')),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: Text(t('cancel'))),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: Text(t('logout'), style: const TextStyle(color: Colors.redAccent)),
          ),
        ],
      ),
    );

    if (confirmed == true) {
      await storage.deleteAll();
      if (!mounted) return;
      Navigator.pushAndRemoveUntil(
        context,
        MaterialPageRoute(builder: (context) => const WelcomeScreen()),
        (route) => false,
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D0D1F),
      appBar: AppBar(
        backgroundColor: Colors.transparent,
        elevation: 0,
        title: const Text('Senclaw Connect', 
          style: TextStyle(color: Colors.white, fontWeight: FontWeight.bold)),
        actions: [
          IconButton(
            icon: const Icon(Icons.logout, color: Colors.redAccent),
            tooltip: 'Disconnect',
            onPressed: () => _confirmDisconnect(context),
          ),
          IconButton(
            icon: const Icon(Icons.qr_code, color: Colors.cyanAccent),
            tooltip: 'Show Connection QR',
            onPressed: () {
              Navigator.push(
                context,
                MaterialPageRoute(builder: (context) => const ConnectionQRScreen()),
              );
            },
          ),
        ],
      ),
      body: Container(
        decoration: const BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [
              Color(0xFF0D0D1F),
              Color(0xFF16162E),
              Color(0xFF0D0D1F),
            ],
          ),
        ),
        child: Column(
          children: [
            Expanded(
              child: ListView.builder(
                padding: const EdgeInsets.all(16),
                itemCount: _messages.length,
                itemBuilder: (context, index) {
                  return _buildMessageBubble(_messages[index]);
                },
              ),
            ),
            _buildInputArea(),
          ],
        ),
      ),
    );
  }

  Widget _buildMessageBubble(String text) {
    return Align(
      alignment: Alignment.centerLeft,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 10),
        decoration: BoxDecoration(
          color: Colors.white.withOpacity(0.1),
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: Colors.white.withOpacity(0.05)),
        ),
        child: Text(text, style: const TextStyle(color: Colors.white)),
      ),
    );
  }

  Widget _buildInputArea() {
    return Container(
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: Colors.black.withOpacity(0.3),
        border: Border(top: BorderSide(color: Colors.white.withOpacity(0.1))),
      ),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              style: const TextStyle(color: Colors.white),
              decoration: InputDecoration(
                hintText: 'Message (E2EE)...',
                hintStyle: TextStyle(color: Colors.white.withOpacity(0.3)),
                border: InputBorder.none,
              ),
            ),
          ),
          IconButton(
            icon: const Icon(Icons.send, color: Colors.purpleAccent),
            onPressed: () {},
          ),
        ],
      ),
    );
  }

  @override
  void dispose() {
    _relayService?.dispose();
    super.dispose();
  }
}
