import 'package:flutter/material.dart';
import 'tokens.dart';

/// Builds Ant Design (v5) flavored Material themes. The component styling here
/// mirrors antd defaults — 32px control height, 6px radius, bordered inputs,
/// subtle split borders, the antd font stack — so the redesigned widgets read
/// as Ant Design while staying on the Material widget set.
class AppTheme {
  static ThemeData dark() => _build(Brightness.dark, AppColors.dark);
  static ThemeData light() => _build(Brightness.light, AppColors.light);

  // antd default font stack.
  static const List<String> _fontStack = [
    '-apple-system',
    'BlinkMacSystemFont',
    'SF Pro Text',
    'Segoe UI',
    'Roboto',
    'PingFang SC',
    'Helvetica Neue',
    'Arial',
  ];

  static ThemeData _build(Brightness brightness, AppColors c) {
    final scheme = ColorScheme.fromSeed(
      seedColor: AppTokens.brand,
      brightness: brightness,
    ).copyWith(
      surface: c.surface,
      primary: c.accent,
      error: AppTokens.danger,
    );

    final base = ThemeData(
      useMaterial3: true,
      brightness: brightness,
      colorScheme: scheme,
      scaffoldBackgroundColor: c.bg,
      // Dropdown (DropdownButtonFormField) menus paint on canvasColor — keep it
      // on the container surface so popups read as cards, not the page bg.
      canvasColor: c.surface,
      fontFamilyFallback: _fontStack,
      dividerColor: c.border,
      splashFactory: NoSplash.splashFactory, // antd has no ink ripple
      visualDensity: VisualDensity.compact,
    );

    OutlineInputBorder border(Color color, [double w = 1]) => OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppTokens.rMd),
          borderSide: BorderSide(color: color, width: w),
        );

    // antd v5 type scale: base 14 / lineHeight 1.5714, SM 12 / 1.6667,
    // LG 16, headings 600 (H5 16 / H4 20 / H3 24 / H2 30).
    const antdText = TextTheme(
      bodyLarge: TextStyle(fontSize: 16, height: 1.5),
      bodyMedium: TextStyle(fontSize: 14, height: 1.5714),
      bodySmall: TextStyle(fontSize: 12, height: 1.6667),
      labelLarge: TextStyle(fontSize: 14, height: 1.5714),
      labelMedium: TextStyle(fontSize: 12, height: 1.6667),
      labelSmall: TextStyle(fontSize: 12, height: 1.6667),
      titleSmall:
          TextStyle(fontSize: 14, height: 1.5714, fontWeight: FontWeight.w600),
      titleMedium:
          TextStyle(fontSize: 16, height: 1.5, fontWeight: FontWeight.w600),
      titleLarge:
          TextStyle(fontSize: 20, height: 1.4, fontWeight: FontWeight.w600),
      headlineSmall:
          TextStyle(fontSize: 24, height: 1.35, fontWeight: FontWeight.w600),
      headlineMedium:
          TextStyle(fontSize: 30, height: 1.3, fontWeight: FontWeight.w600),
    );

    return base.copyWith(
      extensions: <ThemeExtension<dynamic>>[c],
      textTheme: antdText.apply(
        bodyColor: c.textPrimary,
        displayColor: c.textPrimary,
        fontFamilyFallback: _fontStack,
      ),
      cardTheme: CardThemeData(
        color: c.surface,
        elevation: 0,
        shape: RoundedRectangleBorder(
          side: BorderSide(color: c.border),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
        ),
        margin: EdgeInsets.zero,
      ),
      iconTheme: IconThemeData(color: c.textSecondary, size: 16),
      dividerTheme: DividerThemeData(color: c.border, thickness: 1, space: 1),
      tooltipTheme: TooltipThemeData(
        decoration: BoxDecoration(
          color: brightness == Brightness.dark
              ? const Color(0xFF1F1F1F)
              : const Color(0xFF000000).withValues(alpha: 0.85),
          borderRadius: BorderRadius.circular(AppTokens.rMd),
        ),
        textStyle: const TextStyle(color: Colors.white, fontSize: 13),
      ),
      // antd bordered input: subtle container fill, 1px split border, 6px
      // radius, accent focus ring, comfortable ~40px control height. Labels
      // float to the accent color; hover lifts the border like antd.
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: c.surfaceAlt,
        hoverColor: Colors.transparent,
        isDense: true,
        contentPadding: const EdgeInsets.symmetric(
            horizontal: AppTokens.s12, vertical: AppTokens.s12),
        hintStyle: TextStyle(color: c.textMuted, fontSize: 14, height: 1.3),
        labelStyle: TextStyle(color: c.textSecondary, fontSize: 14),
        floatingLabelStyle: TextStyle(
            color: c.accent, fontSize: 13, fontWeight: FontWeight.w600),
        helperStyle: TextStyle(color: c.textMuted, fontSize: 11, height: 1.4),
        prefixIconColor: c.textMuted,
        suffixIconColor: c.textMuted,
        border: border(c.borderStrong),
        enabledBorder: border(c.borderStrong),
        focusedBorder: border(c.accent, 1.5),
        errorBorder: border(AppTokens.danger),
        focusedErrorBorder: border(AppTokens.danger, 1.5),
      ),
      // Make selected dropdown items / list rows read on the surface.
      dropdownMenuTheme: DropdownMenuThemeData(
        menuStyle: MenuStyle(
          backgroundColor: WidgetStatePropertyAll(c.surface),
          elevation: const WidgetStatePropertyAll(6),
          shape: WidgetStatePropertyAll(RoundedRectangleBorder(
            side: BorderSide(color: c.border),
            borderRadius: BorderRadius.circular(AppTokens.rLg),
          )),
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          backgroundColor: c.accent,
          foregroundColor: Colors.white,
          minimumSize: const Size(0, AppTokens.controlHeight),
          padding: const EdgeInsets.symmetric(horizontal: AppTokens.s16),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AppTokens.rMd),
          ),
          textStyle: const TextStyle(fontWeight: FontWeight.w500, fontSize: 14),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          foregroundColor: c.textPrimary,
          minimumSize: const Size(0, AppTokens.controlHeight),
          side: BorderSide(color: c.borderStrong),
          padding: const EdgeInsets.symmetric(horizontal: AppTokens.s16),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AppTokens.rMd),
          ),
          textStyle: const TextStyle(fontWeight: FontWeight.w400, fontSize: 14),
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          foregroundColor: c.accent,
          minimumSize: const Size(0, AppTokens.controlHeight),
          padding: const EdgeInsets.symmetric(horizontal: AppTokens.s12),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(AppTokens.rMd),
          ),
          textStyle: const TextStyle(fontSize: 14),
        ),
      ),
      switchTheme: SwitchThemeData(
        thumbColor: const WidgetStatePropertyAll(Colors.white),
        trackColor: WidgetStateProperty.resolveWith(
          (s) => s.contains(WidgetState.selected) ? c.accent : c.borderStrong,
        ),
        trackOutlineColor: const WidgetStatePropertyAll(Colors.transparent),
      ),
      popupMenuTheme: PopupMenuThemeData(
        color: c.surface,
        elevation: 6,
        shape: RoundedRectangleBorder(
          side: BorderSide(color: c.border),
          borderRadius: BorderRadius.circular(AppTokens.rLg),
        ),
        textStyle: TextStyle(color: c.textPrimary, fontSize: 14),
      ),
      dialogTheme: DialogThemeData(
        backgroundColor: c.surface,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppTokens.rLg),
        ),
      ),
      tabBarTheme: TabBarThemeData(
        labelColor: c.accent,
        unselectedLabelColor: c.textSecondary,
        indicatorColor: c.accent,
        dividerColor: c.border,
        labelStyle: const TextStyle(fontWeight: FontWeight.w500, fontSize: 14),
      ),
      scrollbarTheme: ScrollbarThemeData(
        thumbColor: WidgetStatePropertyAll(c.borderStrong),
        thickness: const WidgetStatePropertyAll(6),
        radius: const Radius.circular(AppTokens.rFull),
      ),
    );
  }
}
