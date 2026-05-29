import 'package:flutter/material.dart';

/// Shared palette so every migrated feature matches the original chat UI
/// (dark, purple-accented) without re-declaring colors per screen.
class AppColors {
  static const bg = Color(0xFF0D0D1F);
  static const surface = Color(0xFF16162E);
  static const accent = Colors.purpleAccent;
  static const cyan = Colors.cyanAccent;
  static const danger = Colors.redAccent;

  static const cardBorder = Color(0x14FFFFFF); // white @ ~8%

  static const gradient = LinearGradient(
    begin: Alignment.topLeft,
    end: Alignment.bottomRight,
    colors: [Color(0xFF0D0D1F), Color(0xFF16162E), Color(0xFF0D0D1F)],
  );

  static BoxDecoration get pageDecoration =>
      const BoxDecoration(gradient: gradient);
}
