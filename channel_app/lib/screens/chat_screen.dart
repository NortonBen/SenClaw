import 'dart:convert';
import 'dart:async';
import 'package:flutter/material.dart';
import '../models/agent_model.dart';
import '../services/relay_service.dart';
import '../services/crypto_service.dart';
import '../services/config_service.dart';
import '../services/language_service.dart';
import '../services/logger_service.dart';
import '../generated/channel_relay.pbenum.dart';
import 'welcome_screen.dart';
import 'connection_qr_screen.dart';
import 'agent_select_screen.dart';

class ChatMessage {
  final String text;
  final bool isFromMe;
  final bool isHistory;
  ChatMessage(this.text, this.isFromMe, {this.isHistory = false});
}

class ChatScreen extends StatefulWidget {
  const ChatScreen({super.key});

  @override
  State<ChatScreen> createState() => _ChatScreenState();
}

class _ChatScreenState extends State<ChatScreen> {
  final _config = ConfigService();
  final _messageController = TextEditingController();
  final _scrollController = ScrollController();

  RelayService? _relay;
  Timer? _loadTimeout;

  final List<ChatMessage> _messages = [];
  bool _isTyping = false;

  List<AgentInfo> _agents = [];
  AgentInfo? _selectedAgent;
  bool _agentLoaded = false;
  bool _historyLoaded = false;

  String _statusText = 'Đang kết nối tới relay…';
  bool _loadTimedOut = false;

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

    final url = (grpc != null && grpc.isNotEmpty) ? grpc : hub;
    if (url == null || cid == null || token == null || key == null) return;

    final encKey = await CryptoService.deriveKey(key);
    Log.i('[Chat] Khởi tạo relay — channel=$cid url=$url');

    _relay = RelayService(
      hubUrl: url,
      channelId: cid,
      senderId: 'mobile-app',
      accessToken: token,
      encryptionKey: encKey,
    );

    _relay!.incomingMessages.listen((text) {
      if (!mounted) return;
      Log.d('[Chat] Tin nhắn mới từ agent: "${text.length > 60 ? text.substring(0, 60) : text}…"');
      setState(() => _messages.add(ChatMessage(text, false)));
      _scrollToBottom();
    });

    _relay!.typingUpdates.listen((typing) {
      if (!mounted) return;
      setState(() => _isTyping = typing);
    });

    _relay!.agentListUpdates.listen(_onAgentList);
    _relay!.historyUpdates.listen(_onHistory);

    _relay!.start();

    _loadTimeout = Timer(const Duration(seconds: 20), () {
      if (!mounted || _agentLoaded) return;
      Log.w('[Chat] Timeout — chưa nhận được danh sách agent sau 20 giây');
      setState(() {
        _loadTimedOut = true;
        _statusText = 'Không nhận được phản hồi từ server';
      });
    });
  }

  Future<void> _onAgentList(List<AgentInfo> agents) async {
    if (!mounted) return;

    _loadTimeout?.cancel();
    Log.i('[Chat] Nhận danh sách agent: ${agents.length} — ${agents.map((a) => a.name).join(', ')}');

    setState(() {
      _agents = agents;
      _agentLoaded = true;
      _loadTimedOut = false;
      _statusText = agents.isEmpty
          ? 'Không có agent nào được bind với kênh này'
          : 'Đã tải ${agents.length} agent';
    });

    if (agents.isEmpty) return;

    final savedFolder = await _config.selectedAgentFolder;
    if (savedFolder != null) {
      final saved = agents.where((a) => a.folder == savedFolder).firstOrNull;
      if (saved != null) {
        _selectAgent(saved, sendSelect: false);
        return;
      }
    }

    if (agents.length == 1) {
      _selectAgent(agents.first);
    } else if (mounted) {
      final chosen = await AgentSelectScreen.show(
        context,
        agents: agents,
        selected: _selectedAgent,
      );
      if (chosen != null) _selectAgent(chosen);
    }
  }

  void _onHistory(List<HistoryMessage> history) {
    if (!mounted || _historyLoaded) return;
    Log.i('[Chat] Nhận lịch sử: ${history.length} tin cho agent "${_selectedAgent?.name}"');

    final histMsgs = history
        .map((m) => ChatMessage(m.content, m.isFromMe, isHistory: true))
        .toList();

    setState(() {
      _messages.insertAll(0, histMsgs);
      _historyLoaded = true;
    });
    _scrollToBottom();
  }

  void _selectAgent(AgentInfo agent, {bool sendSelect = true}) {
    Log.i('[Chat] Chọn agent: ${agent.name} (folder=${agent.folder})');

    setState(() {
      _selectedAgent = agent;
      _historyLoaded = false;
      _messages.removeWhere((m) => m.isHistory);
      _statusText = 'Đang tải lịch sử cho "${agent.name}"…';
    });

    _config.setSelectedAgentFolder(agent.folder);
    _config.setSelectedAgentName(agent.name);

    if (sendSelect) {
      _relay?.sendControl(
        ControlMessage_Type.AGENT_SELECT,
        jsonEncode({'folder': agent.folder}),
      );
    }
    _relay?.sendControl(ControlMessage_Type.HISTORY_REQ, '{}');
  }

  void _reloadAgentList() {
    Log.i('[Chat] Người dùng yêu cầu tải lại danh sách agent');
    setState(() {
      _agentLoaded = false;
      _statusText = 'Đang tải lại danh sách agent…';
    });
    _relay?.sendControl(ControlMessage_Type.AGENT_LIST_REQ, '{}');
  }

  void _reloadHistory() {
    if (_selectedAgent == null) return;
    Log.i('[Chat] Người dùng yêu cầu tải lại lịch sử cho "${_selectedAgent!.name}"');
    setState(() {
      _historyLoaded = false;
      _messages.removeWhere((m) => m.isHistory);
    });
    _relay?.sendControl(ControlMessage_Type.HISTORY_REQ, '{}');
  }

  void _retryLoad() {
    Log.i('[Chat] Thử lại kết nối');
    setState(() {
      _loadTimedOut = false;
      _agentLoaded = false;
      _statusText = 'Đang kết nối lại…';
    });
    _loadTimeout?.cancel();
    _relay?.dispose();
    _relay = null;
    _initRelay();
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        _scrollController.animateTo(
          _scrollController.position.maxScrollExtent,
          duration: const Duration(milliseconds: 250),
          curve: Curves.easeOut,
        );
      }
    });
  }

  Future<void> _openAgentPicker() async {
    if (_agents.isEmpty) return;
    final chosen = await AgentSelectScreen.show(
      context,
      agents: _agents,
      selected: _selectedAgent,
    );
    if (chosen != null && chosen.folder != _selectedAgent?.folder) {
      _selectAgent(chosen);
    }
  }

  Future<void> _confirmDisconnect() async {
    if (Navigator.canPop(context)) Navigator.pop(context); // close drawer
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: const Color(0xFF16162E),
        title: Text(t('logout_confirm_title'),
            style: const TextStyle(color: Colors.white)),
        content: Text(t('logout_confirm_msg'),
            style: const TextStyle(color: Colors.white70)),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: Text(t('cancel'))),
          TextButton(
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(t('logout'),
                  style: const TextStyle(color: Colors.redAccent))),
        ],
      ),
    );
    if (ok == true) {
      await _config.clearAll();
      if (!mounted) return;
      Navigator.pushAndRemoveUntil(
        context,
        MaterialPageRoute(builder: (_) => const WelcomeScreen()),
        (_) => false,
      );
    }
  }

  Future<void> _send() async {
    final text = _messageController.text.trim();
    if (text.isEmpty || _relay == null) return;
    try {
      await _relay!.sendMessage(text);
      setState(() {
        _messages.add(ChatMessage(text, true));
        _messageController.clear();
      });
      _scrollToBottom();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Lỗi gửi: $e')),
        );
      }
    }
  }

  // ── Build ────────────────────────────────────────────────────────────────

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: const Color(0xFF0D0D1F),
      drawer: _buildDrawer(),
      appBar: _buildAppBar(),
      body: Container(
        decoration: const BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.topLeft,
            end: Alignment.bottomRight,
            colors: [Color(0xFF0D0D1F), Color(0xFF16162E), Color(0xFF0D0D1F)],
          ),
        ),
        child: Column(
          children: [
            if (!_agentLoaded) _buildConnectingBanner(),
            Expanded(child: _buildMessageList()),
            _buildInputArea(),
          ],
        ),
      ),
    );
  }

  Widget _buildDrawer() {
    return Drawer(
      backgroundColor: const Color(0xFF16162E),
      child: SafeArea(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 24, 20, 16),
              child: Row(
                children: [
                  Container(
                    width: 40,
                    height: 40,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: Colors.purpleAccent.withOpacity(0.2),
                    ),
                    child: const Icon(Icons.smart_toy_outlined,
                        color: Colors.purpleAccent, size: 22),
                  ),
                  const SizedBox(width: 12),
                  const Text('SenClaw',
                      style: TextStyle(
                          color: Colors.white,
                          fontSize: 18,
                          fontWeight: FontWeight.bold)),
                ],
              ),
            ),
            Divider(color: Colors.white.withOpacity(0.08)),
            const SizedBox(height: 8),

            // Reload agent list
            _drawerItem(
              icon: Icons.manage_accounts_outlined,
              label: 'Tải lại danh sách agent',
              onTap: () {
                Navigator.pop(context);
                _reloadAgentList();
              },
            ),

            // Reload history
            _drawerItem(
              icon: Icons.history,
              label: 'Tải lại lịch sử chat',
              onTap: _selectedAgent == null
                  ? null
                  : () {
                      Navigator.pop(context);
                      _reloadHistory();
                    },
              disabled: _selectedAgent == null,
            ),

            const Spacer(),
            Divider(color: Colors.white.withOpacity(0.08)),

            // QR code
            _drawerItem(
              icon: Icons.qr_code,
              iconColor: Colors.cyanAccent,
              label: 'Kết nối QR',
              onTap: () {
                Navigator.pop(context);
                Navigator.push(
                  context,
                  MaterialPageRoute(builder: (_) => const ConnectionQRScreen()),
                );
              },
            ),

            // Logout
            _drawerItem(
              icon: Icons.logout,
              iconColor: Colors.redAccent,
              label: t('logout'),
              labelColor: Colors.redAccent,
              onTap: _confirmDisconnect,
            ),

            const SizedBox(height: 12),
          ],
        ),
      ),
    );
  }

  Widget _drawerItem({
    required IconData icon,
    required String label,
    VoidCallback? onTap,
    Color iconColor = Colors.white70,
    Color labelColor = Colors.white70,
    bool disabled = false,
  }) {
    return Opacity(
      opacity: disabled ? 0.35 : 1.0,
      child: ListTile(
        leading: Icon(icon, color: iconColor, size: 22),
        title: Text(label,
            style: TextStyle(color: labelColor, fontSize: 14)),
        onTap: disabled ? null : onTap,
        contentPadding: const EdgeInsets.symmetric(horizontal: 20),
        dense: true,
      ),
    );
  }

  AppBar _buildAppBar() {
    final agentName = _selectedAgent?.name ?? (_agentLoaded ? '—' : '…');

    return AppBar(
      backgroundColor: const Color(0xFF16162E),
      elevation: 0,
      // Hamburger opens drawer
      leading: Builder(
        builder: (ctx) => IconButton(
          icon: const Icon(Icons.menu, color: Colors.white70),
          onPressed: () => Scaffold.of(ctx).openDrawer(),
        ),
      ),
      // Center: agent selector
      title: GestureDetector(
        onTap: _agents.length > 1 ? _openAgentPicker : null,
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            CircleAvatar(
              radius: 15,
              backgroundColor: Colors.purpleAccent.withOpacity(0.2),
              child: Text(
                agentName.isNotEmpty ? agentName[0].toUpperCase() : 'A',
                style: const TextStyle(
                    color: Colors.purpleAccent,
                    fontSize: 13,
                    fontWeight: FontWeight.bold),
              ),
            ),
            const SizedBox(width: 8),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(agentName,
                    style: const TextStyle(
                        color: Colors.white,
                        fontSize: 14,
                        fontWeight: FontWeight.bold)),
                if (_selectedAgent != null)
                  Text(_selectedAgent!.folder,
                      style: const TextStyle(
                          color: Colors.white38, fontSize: 10)),
              ],
            ),
            if (_agents.length > 1) ...[
              const SizedBox(width: 4),
              const Icon(Icons.expand_more, color: Colors.white38, size: 16),
            ],
          ],
        ),
      ),
      centerTitle: true,
      // Reload button
      actions: [
        IconButton(
          icon: const Icon(Icons.refresh, color: Colors.white54),
          tooltip: 'Tải lại',
          onPressed: () {
            _reloadAgentList();
            if (_selectedAgent != null) _reloadHistory();
          },
        ),
      ],
    );
  }

  Widget _buildConnectingBanner() {
    return Container(
      color: (_loadTimedOut ? Colors.redAccent : Colors.cyanAccent)
          .withOpacity(0.07),
      padding: const EdgeInsets.symmetric(vertical: 7, horizontal: 16),
      child: Row(
        children: [
          if (!_loadTimedOut)
            const SizedBox(
              width: 12,
              height: 12,
              child: CircularProgressIndicator(
                  strokeWidth: 2,
                  valueColor:
                      AlwaysStoppedAnimation<Color>(Colors.cyanAccent)),
            )
          else
            const Icon(Icons.warning_amber_rounded,
                color: Colors.orangeAccent, size: 14),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              _statusText,
              style: TextStyle(
                color: _loadTimedOut ? Colors.orangeAccent : Colors.white54,
                fontSize: 12,
              ),
            ),
          ),
          if (_loadTimedOut)
            TextButton(
              onPressed: _retryLoad,
              style: TextButton.styleFrom(
                  padding: const EdgeInsets.symmetric(horizontal: 8),
                  minimumSize: Size.zero),
              child: const Text('Thử lại',
                  style: TextStyle(color: Colors.cyanAccent, fontSize: 12)),
            ),
        ],
      ),
    );
  }

  Widget _buildMessageList() {
    if (!_agentLoaded && _messages.isEmpty) {
      return Center(
        child: _loadTimedOut
            ? _buildTimeoutState()
            : Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  const CircularProgressIndicator(
                      valueColor: AlwaysStoppedAnimation<Color>(
                          Colors.purpleAccent)),
                  const SizedBox(height: 16),
                  Text(_statusText,
                      style: const TextStyle(
                          color: Colors.white38, fontSize: 13),
                      textAlign: TextAlign.center),
                ],
              ),
      );
    }

    if (_agentLoaded && _agents.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.smart_toy_outlined,
                color: Colors.white24, size: 48),
            const SizedBox(height: 12),
            const Text('Không có agent nào được bind với kênh này.',
                style: TextStyle(color: Colors.white38)),
            const SizedBox(height: 6),
            const Text('Vào Web UI → Channels → bind agent cho kênh app này',
                style: TextStyle(color: Colors.white24, fontSize: 12),
                textAlign: TextAlign.center),
            const SizedBox(height: 16),
            OutlinedButton.icon(
              onPressed: _reloadAgentList,
              icon: const Icon(Icons.refresh, color: Colors.purpleAccent, size: 16),
              label: const Text('Tải lại',
                  style: TextStyle(color: Colors.purpleAccent, fontSize: 13)),
              style: OutlinedButton.styleFrom(
                  side: const BorderSide(color: Colors.purpleAccent)),
            ),
          ],
        ),
      );
    }

    final totalCount = _messages.length + (_isTyping ? 1 : 0);
    return ListView.builder(
      controller: _scrollController,
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
      itemCount: totalCount,
      itemBuilder: (ctx, i) {
        if (i == _messages.length) return _buildTypingIndicator();

        final msg = _messages[i];
        final isLastHistory = msg.isHistory &&
            (i + 1 >= _messages.length || !_messages[i + 1].isHistory);

        return Column(
          children: [
            if (isLastHistory) _buildHistorySeparator(),
            _buildBubble(msg),
          ],
        );
      },
    );
  }

  Widget _buildHistorySeparator() {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 12),
      child: Row(
        children: [
          Expanded(child: Divider(color: Colors.white12)),
          const SizedBox(width: 8),
          const Text('Lịch sử',
              style: TextStyle(color: Colors.white24, fontSize: 11)),
          const SizedBox(width: 8),
          Expanded(child: Divider(color: Colors.white12)),
        ],
      ),
    );
  }

  Widget _buildBubble(ChatMessage msg) {
    // user (isFromMe=true) → LEFT (trái)
    // AI  (isFromMe=false) → RIGHT (phải)
    final isUser = msg.isFromMe;
    return Align(
      alignment: isUser ? Alignment.centerLeft : Alignment.centerRight,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        constraints:
            BoxConstraints(maxWidth: MediaQuery.of(context).size.width * 0.75),
        decoration: BoxDecoration(
          color: isUser
              ? Colors.purpleAccent.withOpacity(msg.isHistory ? 0.1 : 0.18)
              : Colors.white.withOpacity(msg.isHistory ? 0.05 : 0.1),
          borderRadius: BorderRadius.only(
            topLeft: const Radius.circular(16),
            topRight: const Radius.circular(16),
            bottomLeft: Radius.circular(isUser ? 0 : 16),
            bottomRight: Radius.circular(isUser ? 16 : 0),
          ),
          border: Border.all(
            color: isUser
                ? Colors.purpleAccent.withOpacity(msg.isHistory ? 0.15 : 0.3)
                : Colors.white.withOpacity(0.06),
          ),
        ),
        child: Text(
          msg.text,
          style: TextStyle(
              color: msg.isHistory ? Colors.white60 : Colors.white,
              fontSize: 14),
        ),
      ),
    );
  }

  Widget _buildTypingIndicator() {
    return Align(
      alignment: Alignment.centerRight,
      child: Container(
        margin: const EdgeInsets.symmetric(vertical: 4),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
        decoration: BoxDecoration(
          color: Colors.white.withOpacity(0.05),
          borderRadius: BorderRadius.circular(16),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            const SizedBox(
              width: 12,
              height: 12,
              child: CircularProgressIndicator(
                  strokeWidth: 2,
                  valueColor:
                      AlwaysStoppedAnimation<Color>(Colors.white54)),
            ),
            const SizedBox(width: 8),
            Text('${_selectedAgent?.name ?? 'Agent'} đang soạn…',
                style: const TextStyle(
                    color: Colors.white54,
                    fontSize: 12,
                    fontStyle: FontStyle.italic)),
          ],
        ),
      ),
    );
  }

  Widget _buildInputArea() {
    final enabled = _selectedAgent != null;
    return Container(
      padding: const EdgeInsets.fromLTRB(16, 8, 8, 16),
      decoration: BoxDecoration(
        color: Colors.black.withOpacity(0.3),
        border: Border(top: BorderSide(color: Colors.white.withOpacity(0.08))),
      ),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: _messageController,
              enabled: enabled,
              onSubmitted: (_) => _send(),
              style: const TextStyle(color: Colors.white),
              decoration: InputDecoration(
                hintText: enabled ? 'Nhắn tin…' : 'Chọn agent để bắt đầu',
                hintStyle: TextStyle(color: Colors.white.withOpacity(0.3)),
                border: InputBorder.none,
              ),
            ),
          ),
          IconButton(
            icon: Icon(Icons.send,
                color: enabled ? Colors.purpleAccent : Colors.white24),
            onPressed: enabled ? _send : null,
          ),
        ],
      ),
    );
  }

  Widget _buildTimeoutState() {
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        const Icon(Icons.wifi_off_rounded, color: Colors.orangeAccent, size: 48),
        const SizedBox(height: 16),
        Text(_statusText,
            style: const TextStyle(color: Colors.white60, fontSize: 13),
            textAlign: TextAlign.center),
        const SizedBox(height: 20),
        OutlinedButton.icon(
          onPressed: _retryLoad,
          icon: const Icon(Icons.refresh, color: Colors.purpleAccent),
          label: const Text('Thử lại',
              style: TextStyle(color: Colors.purpleAccent)),
          style: OutlinedButton.styleFrom(
              side: const BorderSide(color: Colors.purpleAccent)),
        ),
      ],
    );
  }

  @override
  void dispose() {
    _loadTimeout?.cancel();
    _messageController.dispose();
    _scrollController.dispose();
    _relay?.dispose();
    super.dispose();
  }
}
