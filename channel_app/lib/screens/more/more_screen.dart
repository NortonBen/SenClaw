import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import '../../models/agent_model.dart';
import '../../services/config_service.dart';
import '../../services/language_service.dart';
import '../../services/relay_manager.dart';
import '../../theme/theme_mode_provider.dart';
import '../../theme/tokens.dart';
import '../../widgets/app_drawer.dart';
import '../../widgets/states.dart';
import '../connection_qr_screen.dart';
import '../welcome_screen.dart';

/// Settings ("Cài đặt") for the REMOTE control app. The remote client has no
/// daemon configuration — only client-side controls: quick stats + connection
/// (re-pair / disconnect), theme, and language. (Wiki / Cognitive / Plugins are
/// reached from the drawer.)
class MoreScreen extends ConsumerWidget {
  const MoreScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final c = context.colors;
    final themeMode = ref.watch(themeModeProvider);
    return Scaffold(
      backgroundColor: c.bg,
      drawer: const AppDrawer(),
      appBar: AppBar(
        backgroundColor: c.surface,
        elevation: 0,
        leading: Builder(
          builder: (ctx) => IconButton(
            icon: Icon(Icons.menu, color: c.textSecondary),
            onPressed: () => Scaffold.of(ctx).openDrawer(),
          ),
        ),
        title: Row(
          children: [
            const Text('Cài đặt'),
            const SizedBox(width: 8),
            AnimatedBuilder(
              animation: RelayManager(),
              builder: (_, _) =>
                  ConnectionDot(connected: RelayManager().connected),
            ),
          ],
        ),
        actions: [
          IconButton(
            tooltip: 'Tải lại',
            icon: Icon(Icons.refresh, color: c.textSecondary),
            onPressed: () => RelayManager().requestAgentList(),
          ),
        ],
      ),
      body: Container(
        decoration: BoxDecoration(color: c.bg),
        child: AnimatedBuilder(
          animation: RelayManager(),
          builder: (context, _) {
            final agents = RelayManager().agents;
            return ListView(
              padding: const EdgeInsets.fromLTRB(12, 12, 12, 24),
              children: [
                _DashboardCards(agents: agents),
                const SizedBox(height: 16),

                // ── Connection / account controls (client-side only) ──────
                _sectionLabel(context, 'Kết nối'),
                const SizedBox(height: 8),
                const _ConnectionCard(),
                const SizedBox(height: 16),

                // ── Appearance ────────────────────────────────────────────
                _sectionLabel(context, 'Giao diện'),
                const SizedBox(height: 8),
                _ThemeModeControl(
                  current: themeMode,
                  onSelected: (m) =>
                      ref.read(themeModeProvider.notifier).set(m),
                ),
                const SizedBox(height: 16),

                // ── Language ──────────────────────────────────────────────
                _sectionLabel(context, 'Ngôn ngữ'),
                const SizedBox(height: 8),
                const _LanguageControl(),
              ],
            );
          },
        ),
      ),
    );
  }

  Widget _sectionLabel(BuildContext context, String t) => Text(t.toUpperCase(),
      style: TextStyle(
          color: context.colors.textMuted,
          fontSize: 11,
          fontWeight: FontWeight.w700,
          letterSpacing: 0.6));
}

/// Connection status + re-pair + disconnect, the only "account" controls a
/// remote control client needs.
class _ConnectionCard extends StatelessWidget {
  const _ConnectionCard();

  Future<void> _confirmDisconnect(BuildContext context) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: Text(t('logout_confirm_title')),
        content: Text(t('logout_confirm_msg')),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx, false),
            child: Text(t('cancel')),
          ),
          TextButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: Text(t('logout'),
                style: const TextStyle(color: AppTokens.danger)),
          ),
        ],
      ),
    );
    if (ok != true || !context.mounted) return;
    await RelayManager().shutdown();
    await ConfigService().clearAll();
    if (!context.mounted) return;
    Navigator.of(context).pushAndRemoveUntil(
      MaterialPageRoute(builder: (_) => const WelcomeScreen()),
      (_) => false,
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return AnimatedBuilder(
      animation: RelayManager(),
      builder: (context, _) {
        final connected = RelayManager().connected;
        return Container(
          decoration: BoxDecoration(
            color: c.surfaceAlt,
            borderRadius: BorderRadius.circular(14),
            border: Border.all(color: c.border),
          ),
          child: Column(
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(14, 12, 14, 12),
                child: Row(
                  children: [
                    ConnectionDot(connected: connected),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Text(
                        connected ? t('connected') : t('disconnected'),
                        style: TextStyle(
                            color: c.textPrimary,
                            fontSize: 14,
                            fontWeight: FontWeight.w600),
                      ),
                    ),
                  ],
                ),
              ),
              Divider(height: 1, color: c.border),
              ListTile(
                leading: const Icon(Icons.qr_code_2, color: AppTokens.cyan),
                title: Text('Ghép lại / Mã QR',
                    style: TextStyle(color: c.textPrimary, fontSize: 14)),
                trailing: Icon(Icons.chevron_right, color: c.textMuted),
                onTap: () => Navigator.of(context).push(MaterialPageRoute(
                    builder: (_) => const ConnectionQRScreen())),
              ),
              Divider(height: 1, color: c.border),
              ListTile(
                leading: const Icon(Icons.logout, color: AppTokens.danger),
                title: Text(t('logout'),
                    style: const TextStyle(
                        color: AppTokens.danger, fontSize: 14)),
                onTap: () => _confirmDisconnect(context),
              ),
            ],
          ),
        );
      },
    );
  }
}

/// Segmented control for picking the app theme mode (system / light / dark).
class _ThemeModeControl extends StatelessWidget {
  final ThemeMode current;
  final ValueChanged<ThemeMode> onSelected;
  const _ThemeModeControl({required this.current, required this.onSelected});

  @override
  Widget build(BuildContext context) {
    const options = [
      (ThemeMode.system, Icons.brightness_auto, 'Hệ thống'),
      (ThemeMode.light, Icons.light_mode, 'Sáng'),
      (ThemeMode.dark, Icons.dark_mode, 'Tối'),
    ];
    return Row(
      children: [
        for (final o in options) ...[
          Expanded(
            child: _Segment(
              icon: o.$2,
              label: o.$3,
              selected: current == o.$1,
              onTap: () => onSelected(o.$1),
            ),
          ),
          if (o.$1 != options.last.$1) const SizedBox(width: 8),
        ],
      ],
    );
  }
}

/// Vietnamese / English picker, backed by [LanguageService].
class _LanguageControl extends StatelessWidget {
  const _LanguageControl();

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: LanguageService(),
      builder: (context, _) {
        final code = LanguageService().currentLocale.languageCode;
        return Row(
          children: [
            Expanded(
              child: _Segment(
                icon: Icons.flag_outlined,
                label: 'Tiếng Việt',
                selected: code == 'vi',
                onTap: () => LanguageService().setLanguage('vi'),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: _Segment(
                icon: Icons.language,
                label: 'English',
                selected: code == 'en',
                onTap: () => LanguageService().setLanguage('en'),
              ),
            ),
          ],
        );
      },
    );
  }
}

/// Shared pill-segment used by the theme + language controls.
class _Segment extends StatelessWidget {
  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;
  const _Segment({
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return InkWell(
      borderRadius: BorderRadius.circular(12),
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 12),
        decoration: BoxDecoration(
          color: selected ? c.accentSoft : c.surfaceAlt,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(color: selected ? c.accent : c.border),
        ),
        child: Column(
          children: [
            Icon(icon, size: 20, color: selected ? c.accent : c.textSecondary),
            const SizedBox(height: 6),
            Text(label,
                style: TextStyle(
                    color: selected ? c.accent : c.textSecondary,
                    fontSize: 12,
                    fontWeight: selected ? FontWeight.w600 : FontWeight.w500)),
          ],
        ),
      ),
    );
  }
}

class _DashboardCards extends StatelessWidget {
  final List<AgentInfo> agents;
  const _DashboardCards({required this.agents});

  @override
  Widget build(BuildContext context) {
    final total = agents.length;
    return Row(
      children: [
        _card(context, Icons.person_outline, '$total', 'Profile'),
        const SizedBox(width: 10),
        _card(
          context,
          Icons.wifi_tethering,
          RelayManager().connected ? 'Online' : 'Offline',
          'Kết nối',
        ),
      ],
    );
  }

  Widget _card(
      BuildContext context, IconData icon, String value, String label) {
    final c = context.colors;
    return Expanded(
      child: Container(
        padding: const EdgeInsets.symmetric(vertical: 16),
        decoration: BoxDecoration(
          color: c.surfaceAlt,
          borderRadius: BorderRadius.circular(14),
          border: Border.all(color: c.border),
        ),
        child: Column(
          children: [
            Icon(icon, color: AppTokens.cyan, size: 22),
            const SizedBox(height: 8),
            Text(value,
                style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 18,
                    fontWeight: FontWeight.bold)),
            Text(label, style: TextStyle(color: c.textMuted, fontSize: 11)),
          ],
        ),
      ),
    );
  }
}
