import 'package:flutter/material.dart';
import 'pairing_screen.dart';
import '../services/language_service.dart';
import '../services/config_service.dart';

class WelcomeScreen extends StatelessWidget {
  const WelcomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Container(
        width: double.infinity,
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
                          icon: const Icon(Icons.settings, color: Colors.white70),
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
                          color: const Color(0xFF5BBFE8).withOpacity(0.2),
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
                    style: const TextStyle(
                      fontSize: 32,
                      fontWeight: FontWeight.bold,
                      color: Colors.white,
                      letterSpacing: 1.2,
                    ),
                  ),
                  const SizedBox(height: 10),
                  Text(
                    t('welcome_subtitle'),
                    style: TextStyle(
                      fontSize: 16,
                      color: Colors.white.withOpacity(0.7),
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
                        gradient: const LinearGradient(
                          colors: [Color(0xFF5BBFE8), Color(0xFF3AAAD4)],
                        ),
                        boxShadow: [
                          BoxShadow(
                            color: const Color(0xFF5BBFE8).withOpacity(0.3),
                            blurRadius: 25,
                            offset: const Offset(0, 8),
                          ),
                        ],
                      ),
                      child: ElevatedButton(
                        onPressed: () {
                          Navigator.push(
                            context,
                            MaterialPageRoute(builder: (context) => const PairingScreen()),
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
                            color: Colors.white,
                            fontSize: 18,
                            fontWeight: FontWeight.bold,
                          ),
                        ),
                      ),
                    ),
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
    final isSelected = LanguageService().currentLocale.languageCode == code;
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => LanguageService().setLanguage(code),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
        decoration: BoxDecoration(
          color: isSelected ? const Color(0xFF5BBFE8).withOpacity(0.15) : Colors.transparent,
          borderRadius: BorderRadius.circular(20),
          border: Border.all(
            color: isSelected ? const Color(0xFF5BBFE8) : Colors.white.withOpacity(0.15),
            width: 1,
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            color: isSelected ? Colors.white : Colors.white.withOpacity(0.5),
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
    final controller = TextEditingController(text: currentHub ?? 'http://127.0.0.1:18080');

    if (!context.mounted) return;
    
    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          backgroundColor: const Color(0xFF16162E),
          title: Text(t('settings_hub_title'), style: const TextStyle(color: Colors.white)),
          content: TextField(
            controller: controller,
            style: const TextStyle(color: Colors.white),
            decoration: InputDecoration(
              labelText: 'Hub URL',
              labelStyle: TextStyle(color: Colors.white.withOpacity(0.5)),
              enabledBorder: const UnderlineInputBorder(borderSide: BorderSide(color: Colors.white24)),
              focusedBorder: const UnderlineInputBorder(borderSide: BorderSide(color: Color(0xFF5BBFE8))),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: Text(t('cancel'), style: const TextStyle(color: Colors.white54)),
            ),
            TextButton(
              onPressed: () async {
                await config.setHubUrl(controller.text.trim());
                await config.setGrpcUrl(''); // Force recalculation/re-verification
                if (context.mounted) Navigator.pop(context);
              },
              child: Text(t('save'), style: const TextStyle(color: Color(0xFF5BBFE8))),
            ),
          ],
        );
      },
    );
  }
}
