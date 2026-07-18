/// Global shortcut for tray screen capture — the Cmd+Shift+4 equivalent.
library;

import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:hotkey_manager/hotkey_manager.dart';

import '../../core/prefs.dart';

const kCaptureHotkeyPrefKey = 'senclaw:capture-hotkey';

/// Ctrl+Shift+4: macOS's own region capture one modifier over, so it reads as
/// the same gesture, and — unlike Cmd+Shift+4 — the system doesn't already own
/// it. A hotkey macOS has claimed can't be registered; ours has to be free.
HotKey defaultCaptureHotKey() => HotKey(
      key: PhysicalKeyboardKey.digit4,
      modifiers: const [HotKeyModifier.control, HotKeyModifier.shift],
      scope: HotKeyScope.system, // fires even when SenClaw isn't focused
    );

class CaptureHotkeyState {
  final HotKey hotKey;

  /// Set when the OS refused to register [hotKey] — almost always because
  /// something else (usually macOS itself) already owns the combo. The shortcut
  /// is dead until the user picks another, so this has to surface in Settings.
  final String? error;

  const CaptureHotkeyState(this.hotKey, {this.error});
}

class CaptureHotkeyNotifier extends StateNotifier<CaptureHotkeyState> {
  CaptureHotkeyNotifier(this._prefs)
      : super(CaptureHotkeyState(_load(_prefs)));

  final Prefs _prefs;
  HotKeyHandler? _handler;

  static HotKey _load(Prefs p) {
    final raw = p.string(kCaptureHotkeyPrefKey, '');
    if (raw.isEmpty) return defaultCaptureHotKey();
    try {
      return HotKey.fromJson(jsonDecode(raw) as Map<String, dynamic>);
    } catch (_) {
      // A shortcut saved by an older build (or hand-edited prefs) shouldn't
      // cost the user their capture shortcut — fall back rather than throw.
      return defaultCaptureHotKey();
    }
  }

  /// Bind the action the shortcut triggers and register it. Call once at app
  /// start; [update] re-registers with the same handler afterwards.
  Future<void> bind(HotKeyHandler handler) async {
    _handler = handler;
    await _register(state.hotKey);
  }

  /// Swap in a new shortcut: unregister the old, persist, register the new.
  Future<void> update(HotKey next) async {
    try {
      await hotKeyManager.unregister(state.hotKey);
    } catch (_) {/* wasn't registered — nothing to undo */}
    await _prefs.setString(kCaptureHotkeyPrefKey, jsonEncode(next.toJson()));
    state = CaptureHotkeyState(next);
    await _register(next);
  }

  Future<void> resetToDefault() => update(defaultCaptureHotKey());

  /// Drop the OS registration while the Settings recorder is open. Without
  /// this, pressing the CURRENT combo to re-record it just fires a capture —
  /// a system-scope hotkey is swallowed by macOS before Flutter sees the key.
  /// [resume] puts it back if the user cancels.
  Future<void> suspend() async {
    try {
      await hotKeyManager.unregister(state.hotKey);
    } catch (_) {/* not registered — nothing to drop */}
  }

  Future<void> resume() => _register(state.hotKey);

  Future<void> _register(HotKey hk) async {
    final handler = _handler;
    if (handler == null) return; // not bound yet; bind() will register.
    try {
      await hotKeyManager.register(hk, keyDownHandler: handler);
      if (mounted) state = CaptureHotkeyState(hk);
    } catch (e) {
      if (kDebugMode) debugPrint('capture hotkey register failed: $e');
      if (mounted) {
        state = CaptureHotkeyState(hk, error: _explain(e));
      }
    }
  }

  String _explain(Object e) {
    final s = e.toString();
    // macOS refuses duplicates outright; the raw PlatformException says nothing
    // a user could act on, so name the likely cause and the way out.
    if (s.contains('already') || s.contains('duplicate') || s.contains('-9878')) {
      return 'Tổ hợp này đã bị hệ thống hoặc ứng dụng khác chiếm. Chọn tổ hợp khác.';
    }
    return 'Không đăng ký được phím tắt: $s';
  }
}

final captureHotkeyProvider =
    StateNotifierProvider<CaptureHotkeyNotifier, CaptureHotkeyState>(
  (ref) => CaptureHotkeyNotifier(ref.watch(prefsHelperProvider)),
);

/// Whether [hk] is a combo worth registering.
///
/// Rejects two things `HotKeyRecorder` happily emits mid-keystroke: a bare
/// modifier (you're still building the combo), and a modifier-less key —
/// registering plain `4` system-wide would eat the digit in every app you type
/// in. The modifier set is read off [HotKeyModifier] rather than hand-listed,
/// so it can't drift from what the package itself treats as a modifier.
bool isUsableHotKey(HotKey hk) {
  final isBareModifier =
      HotKeyModifier.values.any((m) => m.physicalKeys.contains(hk.key));
  if (isBareModifier) return false;
  return (hk.modifiers ?? []).isNotEmpty;
}

/// Human-readable form for Settings, e.g. `⌃ ⇧ 4`.
String hotKeyLabel(HotKey hk) {
  const symbols = {
    HotKeyModifier.control: '⌃',
    HotKeyModifier.shift: '⇧',
    HotKeyModifier.alt: '⌥',
    HotKeyModifier.meta: '⌘',
    HotKeyModifier.capsLock: '⇪',
    HotKeyModifier.fn: 'fn',
  };
  final mods = (hk.modifiers ?? []).map((m) => symbols[m] ?? '?').join(' ');
  final key = hk.logicalKey.keyLabel;
  return mods.isEmpty ? key : '$mods $key';
}
