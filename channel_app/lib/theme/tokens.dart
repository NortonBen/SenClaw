import 'package:flutter/material.dart';

/// SenClaw Connect — Ant Design (v5) flavored design tokens.
///
/// Ported from the desktop app so the mobile channel client shares the same
/// visual language: `colorPrimary: #5BBFE8`, antd dark base `#0D0D1F`, antd
/// radii / control heights / neutral palette, plus a matching light theme. It's
/// a theme layer over Material — every existing widget inherits the Ant Design
/// look through `Theme.of(context)` + the [AppColors] extension.
class AppTokens {
  AppTokens._();

  // ── Brand / accent (antd ConfigProvider tokens from the web app) ────────
  static const Color brand = Color(0xFF5BBFE8); // colorPrimary
  static const Color brandAlt = Color(0xFF722ED1); // antd purple-6
  static const Color cyan = Color(0xFF13C2C2); // antd cyan-6
  static const Color success = Color(0xFF52C41A); // antd green-6
  static const Color warning = Color(0xFFFAAD14); // antd gold-6
  static const Color danger = Color(0xFFFF4D4F); // antd red-5

  // ── Spacing scale (antd 8px base, 4px steps) ────────────────────────────
  static const double s2 = 2;
  static const double s4 = 4;
  static const double s6 = 6;
  static const double s8 = 8;
  static const double s12 = 12;
  static const double s16 = 16;
  static const double s20 = 20;
  static const double s24 = 24;
  static const double s32 = 32;
  static const double s48 = 48;

  // ── Radius (antd: borderRadiusSM 4 / base 6 / LG 8) ─────────────────────
  static const double rSm = 4;
  static const double rMd = 6;
  static const double rLg = 8;
  static const double rXl = 12;
  static const double rFull = 999;

  // ── Control sizing (antd controlHeight = 32; touch targets a bit taller) ─
  static const double controlHeight = 36;

  // antd default font stack (no bundled font; resolved in AppTheme fallback).
  static const String fontMono = 'SFMono-Regular';
}

/// Resolved per-brightness color roles, mapped to antd v5 neutral tokens.
@immutable
class AppColors extends ThemeExtension<AppColors> {
  final Color bg;
  final Color surface;
  final Color surfaceAlt;
  final Color sidebar;
  final Color border;
  final Color borderStrong;
  final Color textPrimary;
  final Color textSecondary;
  final Color textMuted;
  final Color accent;
  final Color accentSoft;
  final Color bubbleUser;
  final Color bubbleAgent;

  const AppColors({
    required this.bg,
    required this.surface,
    required this.surfaceAlt,
    required this.sidebar,
    required this.border,
    required this.borderStrong,
    required this.textPrimary,
    required this.textSecondary,
    required this.textMuted,
    required this.accent,
    required this.accentSoft,
    required this.bubbleUser,
    required this.bubbleAgent,
  });

  // Dark = antd darkAlgorithm with the web's custom bg (#0D0D1F) + 85/65/45%
  // white text and ~5% white borders/containers.
  static const AppColors dark = AppColors(
    bg: Color(0xFF0D0D1F),
    surface: Color(0xFF17182B),
    surfaceAlt: Color(0xFF20223A),
    sidebar: Color(0xFF0A0A17),
    border: Color(0x14FFFFFF), // ~8% white
    borderStrong: Color(0x2EFFFFFF),
    textPrimary: Color(0xD9FFFFFF), // 85%
    textSecondary: Color(0xA6FFFFFF), // 65%
    textMuted: Color(0x73FFFFFF), // 45%
    accent: AppTokens.brand,
    accentSoft: Color(0x265BBFE8),
    bubbleUser: Color(0xFF15324A),
    bubbleAgent: Color(0xFF17182B),
  );

  // Light = antd defaultAlgorithm: bgBase #F0F2F5, container #fff, split #f0f0f0.
  static const AppColors light = AppColors(
    bg: Color(0xFFF0F2F5),
    surface: Color(0xFFFFFFFF),
    surfaceAlt: Color(0xFFFAFAFA),
    sidebar: Color(0xFFFFFFFF),
    border: Color(0xFFF0F0F0),
    borderStrong: Color(0xFFD9D9D9),
    textPrimary: Color(0xE6000000), // 88%
    textSecondary: Color(0xA6000000), // 65%
    textMuted: Color(0x73000000), // 45%
    accent: AppTokens.brand,
    accentSoft: Color(0x1A5BBFE8),
    bubbleUser: Color(0xFFE6F4FB),
    bubbleAgent: Color(0xFFFFFFFF),
  );

  @override
  AppColors copyWith({
    Color? bg,
    Color? surface,
    Color? surfaceAlt,
    Color? sidebar,
    Color? border,
    Color? borderStrong,
    Color? textPrimary,
    Color? textSecondary,
    Color? textMuted,
    Color? accent,
    Color? accentSoft,
    Color? bubbleUser,
    Color? bubbleAgent,
  }) {
    return AppColors(
      bg: bg ?? this.bg,
      surface: surface ?? this.surface,
      surfaceAlt: surfaceAlt ?? this.surfaceAlt,
      sidebar: sidebar ?? this.sidebar,
      border: border ?? this.border,
      borderStrong: borderStrong ?? this.borderStrong,
      textPrimary: textPrimary ?? this.textPrimary,
      textSecondary: textSecondary ?? this.textSecondary,
      textMuted: textMuted ?? this.textMuted,
      accent: accent ?? this.accent,
      accentSoft: accentSoft ?? this.accentSoft,
      bubbleUser: bubbleUser ?? this.bubbleUser,
      bubbleAgent: bubbleAgent ?? this.bubbleAgent,
    );
  }

  @override
  AppColors lerp(ThemeExtension<AppColors>? other, double t) {
    if (other is! AppColors) return this;
    return AppColors(
      bg: Color.lerp(bg, other.bg, t)!,
      surface: Color.lerp(surface, other.surface, t)!,
      surfaceAlt: Color.lerp(surfaceAlt, other.surfaceAlt, t)!,
      sidebar: Color.lerp(sidebar, other.sidebar, t)!,
      border: Color.lerp(border, other.border, t)!,
      borderStrong: Color.lerp(borderStrong, other.borderStrong, t)!,
      textPrimary: Color.lerp(textPrimary, other.textPrimary, t)!,
      textSecondary: Color.lerp(textSecondary, other.textSecondary, t)!,
      textMuted: Color.lerp(textMuted, other.textMuted, t)!,
      accent: Color.lerp(accent, other.accent, t)!,
      accentSoft: Color.lerp(accentSoft, other.accentSoft, t)!,
      bubbleUser: Color.lerp(bubbleUser, other.bubbleUser, t)!,
      bubbleAgent: Color.lerp(bubbleAgent, other.bubbleAgent, t)!,
    );
  }
}

/// Convenience accessor: `context.colors.surface`.
extension AppColorsX on BuildContext {
  AppColors get colors =>
      Theme.of(this).extension<AppColors>() ?? AppColors.dark;
}
