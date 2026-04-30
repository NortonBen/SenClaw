import 'package:flutter/material.dart';
import 'config_service.dart';

class LanguageService extends ChangeNotifier {
  static final LanguageService _instance = LanguageService._internal();
  factory LanguageService() => _instance;
  LanguageService._internal();

  final _config = ConfigService();
  Locale _currentLocale = const Locale('vi');
  Locale get currentLocale => _currentLocale;

  bool get isVietnamese => _currentLocale.languageCode == 'vi';

  Future<void> init() async {
    final langCode = await _config.languageCode;
    _currentLocale = Locale(langCode);
    notifyListeners();
  }

  Future<void> setLanguage(String langCode) async {
    _currentLocale = Locale(langCode);
    await _config.setLanguageCode(langCode);
    notifyListeners();
  }

  String translate(String key) {
    return _translations[_currentLocale.languageCode]?[key] ?? key;
  }

  static final Map<String, Map<String, String>> _translations = {
    'vi': {
      'welcome_title': 'Senclaw Connect',
      'welcome_subtitle': 'Kênh truyền tin mã hóa bảo mật',
      'start_now': 'Bắt đầu ngay',
      'scan_hint': 'Di chuyển camera đến mã QR để quét',
      'step_1': 'Kết nối với Senclaw Agent',
      'step_2': 'Thiết lập kênh truyền mã hóa E2EE',
      'step_3': 'Đồng bộ lịch sử và điều khiển từ xa',
      'connecting': 'Đang kết nối...',
      'connected': 'Đã kết nối',
      'disconnected': 'Đã ngắt kết nối',
      'logout': 'Đăng xuất',
      'logout_confirm_title': 'Xác nhận đăng xuất',
      'logout_confirm_msg': 'Bạn có chắc chắn muốn ngắt kết nối?',
      'cancel': 'Hủy',
      'confirm': 'Xác nhận',
      'settings_hub_title': 'Cài đặt kết nối Hub',
      'save': 'Lưu',
    },
    'en': {
      'welcome_title': 'Senclaw Connect',
      'welcome_subtitle': 'Secure encrypted communication channel',
      'start_now': 'Start Now',
      'scan_hint': 'Move camera to QR code to scan',
      'step_1': 'Connect to Senclaw Agent',
      'step_2': 'Setup E2EE encrypted channel',
      'step_3': 'Sync history and remote control',
      'connecting': 'Connecting...',
      'connected': 'Connected',
      'disconnected': 'Disconnected',
      'logout': 'Logout',
      'logout_confirm_title': 'Confirm Logout',
      'logout_confirm_msg': 'Are you sure you want to disconnect?',
      'cancel': 'Cancel',
      'confirm': 'Confirm',
      'settings_hub_title': 'Hub Connection Settings',
      'save': 'Save',
    }
  };
}

// Global shortcut
String t(String key) => LanguageService().translate(key);
