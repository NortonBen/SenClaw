import 'package:flutter/material.dart';
import '../services/relay_service.dart';
import '../services/crypto_service.dart';
import 'welcome_screen.dart';
import '../services/language_service.dart';
import 'connection_qr_screen.dart';
import '../services/config_service.dart';

class ChatScreen extends StatefulWidget {
  const ChatScreen({super.key});

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> {
  final _config = ConfigService();
  RelayService? _relayService;
  final List<String> _messages = [];

  @override
  void initState() {
    super.initState();
    _initRelay();
  }

  Future<void> _initRelay() async {
    final hub = await _config.hubUrl;
    final grpc = await _config.grpcUrl;
    final cid = await _config.channelId;
    final token = await _config.accessToken;
    final key = await _config.encryptionKey;

    // Use grpc_url if available, otherwise fallback to hub_url
    final connectionUrl = (grpc != null && grpc.isNotEmpty) ? grpc : hub;

    if (connectionUrl != null && cid != null && token != null && key != null) {
      _relayService = RelayService(
        hubUrl: connectionUrl,
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
      await _config.clearAll();
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

  final TextEditingController _messageController = TextEditingController();

  Future<void> _sendMessage() async {
    final text = _messageController.text.trim();
    if (text.isEmpty || _relayService == null) return;

    try {
      await _relayService!.sendMessage(text);
      setState(() {
        _messages.add(text); // Local echo
        _messageController.clear();
      });
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to send message: $e')),
        );
      }
    }
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
              controller: _messageController,
              onSubmitted: (_) => _sendMessage(),
              style: const TextStyle(color: Colors.white),
              decoration: InputDecoration(
                hintText: 'Message ...',
                hintStyle: TextStyle(color: Colors.white.withOpacity(0.3)),
                border: InputBorder.none,
              ),
            ),
          ),
          IconButton(
            icon: const Icon(Icons.send, color: Colors.purpleAccent),
            onPressed: _sendMessage,
          ),
        ],
      ),
    );
  }

  @override
  void dispose() {
    _messageController.dispose();
    _relayService?.dispose();
    super.dispose();
  }
}
