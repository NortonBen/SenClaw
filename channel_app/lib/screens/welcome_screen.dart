import 'package:flutter/material.dart';
import 'pairing_screen.dart';
import '../services/language_service.dart';
import '../services/config_service.dart';
import '../theme/tokens.dart';

class WelcomeScreen extends StatelessWidget {
  const WelcomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    return Scaffold(
      body: Container(
        width: double.infinity,
        decoration: BoxDecoration(color: c.bg),
        child: ListenableBuilder(
          listenable: LanguageService(),
          builder: (context, _) {
            return SafeArea(
              child: Column(
                children: [
                  // Language Selector at top right
                  Padding(
                    padding: const EdgeInsets.all(20),
                    child: Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        IconButton(
                          icon: Icon(
                            Icons.settings,
                            color: c.textSecondary,
                          ),
                          onPressed: () => _showSettingsDialog(context),
                        ),
                        Row(
                          children: [
                            _buildLanguageButton(context, 'vi', 'VN'),
                            const SizedBox(width: 10),
                            _buildLanguageButton(context, 'en', 'EN'),
                          ],
                        ),
                      ],
                    ),
                  ),
                  const Spacer(),
                  // Logo placeholder or Icon
                  Container(
                    padding: const EdgeInsets.all(10),
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      boxShadow: [
                        BoxShadow(
                          color: c.accent.withValues(alpha: 0.2),
                          blurRadius: 60,
                          spreadRadius: 5,
                        ),
                      ],
                    ),
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(100),
                      child: Image.asset(
                        'assets/images/logo.png',
                        height: 180,
                        width: 180,
                        fit: BoxFit.contain,
                      ),
                    ),
                  ),
                  const SizedBox(height: 30),
                  Text(
                    t('welcome_title'),
                    style: TextStyle(
                      fontSize: 32,
                      fontWeight: FontWeight.bold,
                      color: c.textPrimary,
                      letterSpacing: 1.2,
                    ),
                  ),
                  const SizedBox(height: 10),
                  Text(
                    t('welcome_subtitle'),
                    style: TextStyle(
                      fontSize: 16,
                      color: c.textSecondary,
                    ),
                  ),
                  const Spacer(),
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 40),
                    child: Container(
                      width: double.infinity,
                      height: 56,
                      decoration: BoxDecoration(
                        borderRadius: BorderRadius.circular(20),
                        color: c.accent,
                        boxShadow: [
                          BoxShadow(
                            color: c.accent.withValues(alpha: 0.3),
                            blurRadius: 25,
                            offset: const Offset(0, 8),
                          ),
                        ],
                      ),
                      child: ElevatedButton(
                        onPressed: () {
                          Navigator.push(
                            context,
                            MaterialPageRoute(
                              builder: (context) => const PairingScreen(),
                            ),
                          );
                        },
                        style: ElevatedButton.styleFrom(
                          backgroundColor: Colors.transparent,
                          shadowColor: Colors.transparent,
                          shape: RoundedRectangleBorder(
                            borderRadius: BorderRadius.circular(16),
                          ),
                        ),
                        child: Text(
                          t('start_now'),
                          style: const TextStyle(
                            color: Color(0xFF0A1A22),
                            fontSize: 18,
                            fontWeight: FontWeight.bold,
                          ),
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(height: 14),
                  FutureBuilder<String?>(
                    future: ConfigService().hubUrl,
                    builder: (context, snapshot) {
                      final hub = (snapshot.data ?? '').trim();
                      final displayHub = hub.isEmpty
                          ? 'https://senclaw-hub.bacnd.com'
                          : hub;
                      return TextButton.icon(
                        onPressed: () => _showSettingsDialog(context),
                        icon: Icon(
                          Icons.link,
                          color: c.textSecondary,
                          size: 18,
                        ),
                        label: Text(
                          'Hub URL: $displayHub',
                          style: TextStyle(
                            color: c.textSecondary,
                            fontSize: 12,
                          ),
                          overflow: TextOverflow.ellipsis,
                        ),
                      );
                    },
                  ),
                  const SizedBox(height: 40),
                ],
              ),
            );
          },
        ),
      ),
    );
  }

  Widget _buildLanguageButton(BuildContext context, String code, String label) {
    final c = context.colors;
    final isSelected = LanguageService().currentLocale.languageCode == code;
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => LanguageService().setLanguage(code),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: isSelected ? c.accentSoft : Colors.transparent,
          borderRadius: BorderRadius.circular(20),
          border: Border.all(
            color: isSelected ? c.accent : c.border,
            width: 1,
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            color: isSelected ? c.accent : c.textMuted,
            fontSize: 12,
            fontWeight: isSelected ? FontWeight.bold : FontWeight.normal,
          ),
        ),
      ),
    );
  }

  Future<void> _showSettingsDialog(BuildContext context) async {
    final config = ConfigService();
    String? currentHub = await config.hubUrl;
    final controller = TextEditingController(
      text: currentHub ?? 'https://senclaw-hub.bacnd.com',
    );

    if (!context.mounted) return;

    showDialog(
      context: context,
      builder: (context) {
        final c = context.colors;
        return AlertDialog(
          backgroundColor: c.surface,
          title: Text(
            t('settings_hub_title'),
            style: TextStyle(color: c.textPrimary),
          ),
          content: TextField(
            controller: controller,
            style: TextStyle(color: c.textPrimary),
            decoration: InputDecoration(
              labelText: 'Hub URL',
              labelStyle: TextStyle(color: c.textMuted),
              enabledBorder: UnderlineInputBorder(
                borderSide: BorderSide(color: c.border),
              ),
              focusedBorder: UnderlineInputBorder(
                borderSide: BorderSide(color: c.accent),
              ),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: Text(
                t('cancel'),
                style: TextStyle(color: c.textMuted),
              ),
            ),
            TextButton(
              onPressed: () async {
                await config.setHubUrl(controller.text.trim());
                await config.setRelayUrl(
                  '',
                ); // Force recalculation/re-verification
                if (context.mounted) Navigator.pop(context);
              },
              child: Text(
                t('save'),
                style: TextStyle(color: c.accent),
              ),
            ),
          ],
        );
      },
    );
  }
}
