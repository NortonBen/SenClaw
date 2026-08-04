/// Provider Sign-in — connect subscription LLM accounts (OAuth) and add
/// free-tier API-key endpoints.
///
/// Mirrors the web UI's Provider Sign-in page against the same `/api/oauth/*`
/// and `/api/provider-catalog` endpoints. Tokens never reach this layer: the
/// daemon returns only ids, labels and expiry, and the sign-in itself happens
/// in the system browser.
library;

import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/transport/connection.dart';
import '../../theme/tokens.dart';
import 'settings_screen.dart' show SettingsBody;

// ---------------------------------------------------------------- models

class OauthProviderDef {
  OauthProviderDef.fromJson(Map j)
      : id = j['id'] as String? ?? '',
        displayName = j['displayName'] as String? ?? '',
        riskNotice = j['riskNotice'] as String? ?? '',
        brandColor = j['brandColor'] as String? ?? '#888888',
        brandMark = j['brandMark'] as String? ?? '?',
        flow = j['flow'] as String? ?? 'auth_code_pkce',
        compat = j['compat'] as String?,
        requiresFixedPort = j['requiresFixedPort'] as bool? ?? false,
        models = ((j['models'] as List?) ?? [])
            .map((m) => (
                  m['id'] as String? ?? '',
                  m['name'] as String? ?? '',
                ))
            .toList();

  final String id;
  final String displayName;
  final String riskNotice;
  final String brandColor;
  final String brandMark;
  final String flow;
  final String? compat;
  final bool requiresFixedPort;
  final List<(String, String)> models;

  bool get isDeviceFlow => flow == 'device_code';
}

class OauthAccount {
  OauthAccount.fromJson(Map j)
      : id = j['id'] as String? ?? '',
        provider = j['provider'] as String? ?? '',
        label = j['label'] as String? ?? '',
        email = j['email'] as String?,
        expiresIn = (j['expiresIn'] as num?)?.toInt(),
        expired = j['expired'] as bool? ?? false,
        hasRefreshToken = j['hasRefreshToken'] as bool? ?? false,
        lastError = j['lastError'] as String?;

  final String id;
  final String provider;
  final String label;
  final String? email;
  final int? expiresIn;
  final bool expired;
  final bool hasRefreshToken;
  final String? lastError;
}

/// Outcome of a live model probe against a connected account.
class ModelProbe {
  const ModelProbe({required this.ok, required this.latencyMs, this.error});
  final bool ok;
  final int latencyMs;
  final String? error;
}

class CatalogProvider {
  CatalogProvider.fromJson(Map j)
      : id = j['id'] as String? ?? '',
        displayName = j['displayName'] as String? ?? '',
        baseUrl = j['baseURL'] as String? ?? '',
        adapt = j['adapt'] as String? ?? 'openai',
        auth = j['auth'] as String? ?? 'api_key',
        signupUrl = j['signupUrl'] as String?,
        note = j['note'] as String? ?? '',
        brandColor = j['brandColor'] as String? ?? '#888888',
        brandMark = j['brandMark'] as String? ?? '?',
        urlPlaceholder = j['urlPlaceholder'] as String?,
        maxTokens = (j['defaultMaxTokens'] as num?)?.toInt() ?? 8192,
        contextLength = (j['defaultContextLength'] as num?)?.toInt() ?? 128000,
        models = ((j['models'] as List?) ?? [])
            .map((m) => (
                  m['id'] as String? ?? '',
                  m['name'] as String? ?? '',
                ))
            .toList();

  final String id;
  final String displayName;
  final String baseUrl;
  final String adapt;
  final String auth;
  final String? signupUrl;
  final String note;
  final String brandColor;
  final String brandMark;
  final String? urlPlaceholder;
  final int maxTokens;
  final int contextLength;
  final List<(String, String)> models;

  bool get needsKey => auth == 'api_key';
}

// ------------------------------------------------------------- providers

final oauthProvidersProvider =
    FutureProvider<List<OauthProviderDef>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/oauth/providers');
  final list = (r is Map ? r['providers'] as List? : null) ?? const [];
  return list.map((e) => OauthProviderDef.fromJson(e as Map)).toList();
});

final oauthAccountsProvider = FutureProvider<List<OauthAccount>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/oauth/accounts');
  final list = (r is Map ? r['accounts'] as List? : null) ?? const [];
  return list.map((e) => OauthAccount.fromJson(e as Map)).toList();
});

final providerCatalogProvider =
    FutureProvider<List<CatalogProvider>>((ref) async {
  final r = await ref.read(apiClientProvider).get('/api/provider-catalog');
  final list = (r is Map ? r['providers'] as List? : null) ?? const [];
  return list.map((e) => CatalogProvider.fromJson(e as Map)).toList();
});

/// `#RRGGBB` → [Color]. Falls back to grey on anything unparseable so a bad
/// registry entry can't crash the settings screen.
Color _brandColor(String hex) {
  final cleaned = hex.replaceFirst('#', '');
  final value = int.tryParse(cleaned, radix: 16);
  if (value == null || cleaned.length != 6) return const Color(0xFF888888);
  return Color(0xFF000000 | value);
}

// ------------------------------------------------------------ small bits

/// Brand badge: a monogram on the vendor's colour.
///
/// A monogram rather than a traced logo — an inaccurate redraw of someone's
/// trademark reads worse than a clean initial, and the app ships no vendor art.
class ProviderLogo extends StatelessWidget {
  const ProviderLogo({
    super.key,
    required this.color,
    required this.mark,
    this.size = 34,
  });

  final String color;
  final String mark;
  final double size;

  @override
  Widget build(BuildContext context) {
    final c = _brandColor(color);
    return Container(
      width: size,
      height: size,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: c.withValues(alpha: 0.14),
        borderRadius: BorderRadius.circular(size * 0.28),
        border: Border.all(color: c.withValues(alpha: 0.35)),
      ),
      child: Text(
        mark,
        style: TextStyle(
          color: c,
          fontWeight: FontWeight.w700,
          fontSize: mark.length > 1 ? size * 0.36 : size * 0.46,
          height: 1,
        ),
      ),
    );
  }
}

class _Pill extends StatelessWidget {
  const _Pill(this.text, {this.color});
  final String text;
  final Color? color;

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final tint = color ?? c.textMuted;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
      decoration: BoxDecoration(
        color: tint.withValues(alpha: 0.12),
        borderRadius: BorderRadius.circular(AppTokens.rSm),
      ),
      child: Text(text, style: TextStyle(color: tint, fontSize: 11)),
    );
  }
}

/// Remaining-life pill for a token.
Widget _expiryPill(BuildContext context, OauthAccount a) {
  if (a.expired) return const _Pill('Expired', color: Colors.redAccent);
  final secs = a.expiresIn;
  if (secs == null) return const _Pill('No expiry');
  final label = secs > 86400
      ? '${secs ~/ 86400}d'
      : secs > 3600
          ? '${secs ~/ 3600}h'
          : '${(secs ~/ 60).clamp(1, 59)}m';
  return _Pill('$label left',
      color: secs < 600 ? Colors.orangeAccent : Colors.greenAccent);
}

// ------------------------------------------------------------ the section

class ProviderSignInSection extends ConsumerStatefulWidget {
  const ProviderSignInSection({super.key});

  @override
  ConsumerState<ProviderSignInSection> createState() =>
      _ProviderSignInSectionState();
}

class _ProviderSignInSectionState extends ConsumerState<ProviderSignInSection> {
  String? _connecting;
  Timer? _poll;
  bool _riskExpanded = false;

  @override
  void dispose() {
    _poll?.cancel();
    super.dispose();
  }

  void _reload() {
    ref.invalidate(oauthProvidersProvider);
    ref.invalidate(oauthAccountsProvider);
    ref.invalidate(providerCatalogProvider);
  }

  void _toast(String msg) {
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  /// Start a sign-in, open the vendor page in the system browser, then poll
  /// the daemon until the flow resolves.
  Future<void> _connect(OauthProviderDef p) async {
    setState(() => _connecting = p.id);
    try {
      final r = await ref.read(apiClientProvider).post('/api/oauth/${p.id}/start');
      final data = r as Map;
      final url = data['authorizeUrl'] as String? ?? '';
      final flowId = data['flowId'] as String? ?? '';

      if (url.isNotEmpty) {
        await launchUrl(Uri.parse(url), mode: LaunchMode.externalApplication);
      }

      if (data['kind'] == 'device_code') {
        // The user has to type this code on the vendor's page; keep it on
        // screen until the poll resolves.
        _showDeviceDialog(p, data['userCode'] as String? ?? '', url);
      } else {
        _toast('Finish the sign-in in your browser.');
      }

      _poll?.cancel();
      _poll = Timer.periodic(const Duration(seconds: 2), (t) async {
        try {
          final s = await ref
              .read(apiClientProvider)
              .get('/api/oauth/flows/$flowId') as Map;
          if (s['status'] == 'completed') {
            t.cancel();
            _dismissDeviceDialog();
            if (mounted) setState(() => _connecting = null);
            _toast('Connected ${s['label'] ?? p.displayName}');
            ref.invalidate(oauthAccountsProvider);
          } else if (s['status'] == 'failed') {
            t.cancel();
            _dismissDeviceDialog();
            if (mounted) setState(() => _connecting = null);
            _toast(s['error'] as String? ?? 'Sign-in failed');
          }
        } catch (_) {
          // Daemon restarting or momentarily unreachable — keep polling.
        }
      });
    } catch (e) {
      if (mounted) setState(() => _connecting = null);
      _toast('$e');
    }
  }

  bool _deviceDialogOpen = false;

  void _dismissDeviceDialog() {
    if (_deviceDialogOpen && mounted) {
      Navigator.of(context, rootNavigator: true).pop();
      _deviceDialogOpen = false;
    }
  }

  void _showDeviceDialog(OauthProviderDef p, String code, String url) {
    _deviceDialogOpen = true;
    showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (ctx) {
        final c = ctx.colors;
        return AlertDialog(
          backgroundColor: c.surface,
          title: Row(children: [
            ProviderLogo(color: p.brandColor, mark: p.brandMark, size: 26),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Text('Connect ${p.displayName}',
                  style: TextStyle(color: c.textPrimary, fontSize: 15)),
            ),
          ]),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text('Enter this code at $url',
                  style: TextStyle(color: c.textSecondary, fontSize: 13)),
              const SizedBox(height: AppTokens.s16),
              Container(
                padding: const EdgeInsets.symmetric(vertical: AppTokens.s16),
                decoration: BoxDecoration(
                  color: c.bg,
                  borderRadius: BorderRadius.circular(AppTokens.rMd),
                ),
                child: Text(
                  code,
                  textAlign: TextAlign.center,
                  style: TextStyle(
                    color: c.textPrimary,
                    fontSize: 28,
                    fontWeight: FontWeight.w700,
                    letterSpacing: 6,
                    fontFamily: 'monospace',
                  ),
                ),
              ),
              const SizedBox(height: AppTokens.s12),
              Row(mainAxisAlignment: MainAxisAlignment.center, children: [
                TextButton.icon(
                  icon: const Icon(Icons.copy, size: 15),
                  label: const Text('Copy code'),
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: code));
                    _toast('Code copied');
                  },
                ),
                TextButton.icon(
                  icon: const Icon(Icons.open_in_new, size: 15),
                  label: const Text('Open page'),
                  onPressed: () => launchUrl(Uri.parse(url),
                      mode: LaunchMode.externalApplication),
                ),
              ]),
              const SizedBox(height: AppTokens.s8),
              Text(
                'Waiting for approval — this closes on its own.',
                textAlign: TextAlign.center,
                style: TextStyle(color: c.textMuted, fontSize: 12),
              ),
            ],
          ),
          actions: [
            TextButton(
              onPressed: () {
                // Closing only stops watching; the daemon's poll task ends on
                // its own when the device code expires.
                _poll?.cancel();
                _deviceDialogOpen = false;
                Navigator.of(ctx).pop();
                if (mounted) setState(() => _connecting = null);
              },
              child: const Text('Cancel'),
            ),
          ],
        );
      },
    ).then((_) => _deviceDialogOpen = false);
  }

  Future<void> _refreshAccount(OauthAccount a) async {
    try {
      await ref
          .read(apiClientProvider)
          .post('/api/oauth/accounts/${a.id}/refresh');
      _toast('Token refreshed');
      ref.invalidate(oauthAccountsProvider);
    } catch (e) {
      _toast('$e');
    }
  }

  Future<void> _disconnect(OauthAccount a) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        backgroundColor: ctx.colors.surface,
        title: Text('Disconnect ${a.label}?',
            style: TextStyle(color: ctx.colors.textPrimary, fontSize: 15)),
        content: Text(
          'SenClaw forgets the stored tokens. Any model bound to this account '
          'stops working until you connect again.',
          style: TextStyle(color: ctx.colors.textSecondary, fontSize: 13),
        ),
        actions: [
          TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('Cancel')),
          TextButton(
            onPressed: () => Navigator.pop(ctx, true),
            child: const Text('Disconnect',
                style: TextStyle(color: Colors.redAccent)),
          ),
        ],
      ),
    );
    if (ok != true) return;
    try {
      await ref.read(apiClientProvider).delete('/api/oauth/accounts/${a.id}');
      _toast('Disconnected');
      ref.invalidate(oauthAccountsProvider);
    } catch (e) {
      _toast('$e');
    }
  }

  /// Probe one model through the real adapter.
  Future<ModelProbe> _testModel(String accountId, String model) async {
    try {
      final r = await ref.read(apiClientProvider).post('/api/oauth/test-model',
          body: {'accountId': accountId, 'modelName': model}) as Map;
      return ModelProbe(
        ok: r['ok'] == true,
        latencyMs: (r['latencyMs'] as num?)?.toInt() ?? 0,
        error: r['error'] as String?,
      );
    } catch (e) {
      return ModelProbe(ok: false, latencyMs: 0, error: '$e');
    }
  }

  /// Bind an account to a new model entry via `/api/oauth/bind`.
  ///
  /// Each suggested model carries a probe button: entitlement is per-account
  /// and invisible until tried, so a provider can advertise a dozen models and
  /// serve three. Probing runs the same code path a real chat would.
  Future<void> _useAsModel(OauthProviderDef p, OauthAccount a) async {
    final controller = TextEditingController(
        text: p.models.isNotEmpty ? p.models.first.$1 : '');
    final probes = <String, ModelProbe>{};
    var busy = false;

    final model = await showDialog<String>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setLocal) {
          final c = ctx.colors;

          Future<void> probe(String id) async {
            setLocal(() => busy = true);
            final result = await _testModel(a.id, id);
            setLocal(() {
              probes[id] = result;
              busy = false;
            });
          }

          return AlertDialog(
            backgroundColor: c.surface,
            title: Text('Use ${a.label} as a model',
                style: TextStyle(color: c.textPrimary, fontSize: 15)),
            content: SizedBox(
              width: 430,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    'Creates a model entry backed by this account. No token is '
                    'written into config.json — only a reference to the account.',
                    style: TextStyle(color: c.textMuted, fontSize: 12),
                  ),
                  const SizedBox(height: AppTokens.s12),
                  if (p.models.isNotEmpty)
                    ConstrainedBox(
                      constraints: const BoxConstraints(maxHeight: 220),
                      child: SingleChildScrollView(
                        child: Column(children: [
                          for (final (id, name) in p.models)
                            InkWell(
                              onTap: () => setLocal(() => controller.text = id),
                              child: Padding(
                                padding: const EdgeInsets.symmetric(
                                    vertical: 4, horizontal: 6),
                                child: Row(children: [
                                  if (probes[id] == null)
                                    Icon(Icons.circle_outlined,
                                        size: 13, color: c.textMuted)
                                  else if (probes[id]!.ok)
                                    const Icon(Icons.check_circle,
                                        size: 13, color: Colors.greenAccent)
                                  else
                                    const Icon(Icons.cancel,
                                        size: 13, color: Colors.redAccent),
                                  const SizedBox(width: AppTokens.s8),
                                  Expanded(
                                    child: Text(name,
                                        overflow: TextOverflow.ellipsis,
                                        style: TextStyle(
                                            color: c.textSecondary,
                                            fontSize: 12)),
                                  ),
                                  if (probes[id] != null)
                                    Tooltip(
                                      message: probes[id]!.ok
                                          ? '${probes[id]!.latencyMs} ms'
                                          : (probes[id]!.error ?? 'unavailable'),
                                      child: Text(
                                        probes[id]!.ok
                                            ? '${probes[id]!.latencyMs} ms'
                                            : 'unavailable',
                                        style: TextStyle(
                                          fontSize: 11,
                                          color: probes[id]!.ok
                                              ? c.textMuted
                                              : Colors.redAccent,
                                        ),
                                      ),
                                    ),
                                  IconButton(
                                    tooltip: 'Test this model',
                                    visualDensity: VisualDensity.compact,
                                    constraints: const BoxConstraints(),
                                    padding: const EdgeInsets.only(left: 8),
                                    icon: Icon(Icons.science_outlined,
                                        size: 14, color: c.textSecondary),
                                    onPressed: busy ? null : () => probe(id),
                                  ),
                                ]),
                              ),
                            ),
                        ]),
                      ),
                    ),
                  const SizedBox(height: AppTokens.s12),
                  TextField(
                    controller: controller,
                    style: TextStyle(color: c.textPrimary, fontSize: 13),
                    decoration: const InputDecoration(
                      labelText: 'Model id',
                      isDense: true,
                      border: OutlineInputBorder(),
                    ),
                  ),
                ],
              ),
            ),
            actions: [
              if (p.models.length > 1)
                TextButton(
                  // Sequential: these are real completions against a
                  // subscription, and a burst is the quickest way to trip a
                  // rate limit.
                  onPressed: busy
                      ? null
                      : () async {
                          for (final (id, _) in p.models) {
                            await probe(id);
                          }
                        },
                  child: Text('Test all ${p.models.length}'),
                ),
              TextButton(
                  onPressed: () => Navigator.pop(ctx),
                  child: const Text('Cancel')),
              TextButton(
                onPressed: () => Navigator.pop(ctx, controller.text.trim()),
                child: const Text('Add model'),
              ),
            ],
          );
        },
      ),
    );
    if (model == null || model.isEmpty) return;
    try {
      final r = await ref.read(apiClientProvider).post('/api/oauth/bind',
          body: {'accountId': a.id, 'modelName': model}) as Map;
      _toast('Added "${r['label'] ?? model}"');
    } catch (e) {
      _toast('$e');
    }
  }

  /// Add a free-tier preset as a plain API-key model config.
  Future<void> _addPreset(CatalogProvider p) async {
    final keyCtl = TextEditingController();
    final urlCtl = TextEditingController();
    final modelCtl = TextEditingController(
        text: p.models.isNotEmpty ? p.models.first.$1 : '');

    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) {
        final c = ctx.colors;
        return AlertDialog(
          backgroundColor: c.surface,
          title: Row(children: [
            ProviderLogo(color: p.brandColor, mark: p.brandMark, size: 26),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Text('Add ${p.displayName}',
                  style: TextStyle(color: c.textPrimary, fontSize: 15)),
            ),
          ]),
          content: SizedBox(
            width: 380,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (p.urlPlaceholder != null) ...[
                  TextField(
                    controller: urlCtl,
                    style: TextStyle(color: c.textPrimary, fontSize: 13),
                    decoration: InputDecoration(
                      labelText: p.urlPlaceholder,
                      isDense: true,
                      border: const OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: AppTokens.s12),
                ],
                if (p.needsKey) ...[
                  TextField(
                    controller: keyCtl,
                    obscureText: true,
                    style: TextStyle(color: c.textPrimary, fontSize: 13),
                    decoration: const InputDecoration(
                      labelText: 'API key',
                      isDense: true,
                      border: OutlineInputBorder(),
                    ),
                  ),
                  const SizedBox(height: AppTokens.s12),
                ],
                if (p.models.isNotEmpty)
                  Wrap(
                    spacing: AppTokens.s8,
                    runSpacing: AppTokens.s8,
                    children: [
                      for (final (id, name) in p.models)
                        ActionChip(
                          label:
                              Text(name, style: const TextStyle(fontSize: 12)),
                          onPressed: () => modelCtl.text = id,
                        ),
                    ],
                  ),
                const SizedBox(height: AppTokens.s12),
                TextField(
                  controller: modelCtl,
                  style: TextStyle(color: c.textPrimary, fontSize: 13),
                  decoration: const InputDecoration(
                    labelText: 'Model id',
                    isDense: true,
                    border: OutlineInputBorder(),
                  ),
                ),
              ],
            ),
          ),
          actions: [
            TextButton(
                onPressed: () => Navigator.pop(ctx, false),
                child: const Text('Cancel')),
            TextButton(
                onPressed: () => Navigator.pop(ctx, true),
                child: const Text('Add model')),
          ],
        );
      },
    );
    if (ok != true || modelCtl.text.trim().isEmpty) return;

    if (p.urlPlaceholder != null && urlCtl.text.trim().isEmpty) {
      _toast('${p.urlPlaceholder} is required');
      return;
    }

    final baseUrl = p.urlPlaceholder != null
        ? p.baseUrl.replaceAll('{${p.urlPlaceholder}}', urlCtl.text.trim())
        : p.baseUrl;

    try {
      await ref.read(apiClientProvider).post('/api/llm-config', body: {
        'label': '${p.displayName} — ${modelCtl.text.trim()}',
        'provider': p.id,
        'baseURL': baseUrl,
        'apiKey': keyCtl.text.trim(),
        'modelName': modelCtl.text.trim(),
        'adapt': p.adapt,
        'maxTokens': p.maxTokens,
        'contextLength': p.contextLength,
      });
      _toast('Added ${p.displayName}');
    } catch (e) {
      _toast('$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = context.colors;
    final providers = ref.watch(oauthProvidersProvider);
    final accounts = ref.watch(oauthAccountsProvider).valueOrNull ?? const [];
    final catalog = ref.watch(providerCatalogProvider);

    return SettingsBody(
      title: 'Provider Sign-in',
      onRefresh: _reload,
      children: [
        _riskBanner(c),
        const SizedBox(height: AppTokens.s16),

        _heading(c, 'Subscription accounts'),
        const SizedBox(height: AppTokens.s8),
        providers.when(
          loading: () => const Center(
              child: Padding(
                  padding: EdgeInsets.all(AppTokens.s24),
                  child: CircularProgressIndicator(strokeWidth: 2))),
          error: (e, _) => Text('Could not load providers: $e',
              style: TextStyle(color: c.textMuted, fontSize: 12)),
          data: (list) => Wrap(
            spacing: AppTokens.s12,
            runSpacing: AppTokens.s12,
            children: [
              for (final p in list)
                SizedBox(
                  width: 268,
                  child: _providerCard(
                    c,
                    p,
                    accounts.where((a) => a.provider == p.id).toList(),
                  ),
                ),
            ],
          ),
        ),

        const SizedBox(height: AppTokens.s24),
        _heading(c, 'Free-tier providers'),
        Padding(
          padding: const EdgeInsets.only(top: 4, bottom: AppTokens.s8),
          child: Text(
            'Ready-made endpoints with a free allowance. Each needs its own '
            'API key unless marked otherwise.',
            style: TextStyle(color: c.textMuted, fontSize: 12),
          ),
        ),
        catalog.when(
          loading: () => const SizedBox.shrink(),
          error: (e, _) => Text('Could not load the catalog: $e',
              style: TextStyle(color: c.textMuted, fontSize: 12)),
          data: (list) => Wrap(
            spacing: AppTokens.s12,
            runSpacing: AppTokens.s12,
            children: [
              for (final p in list)
                SizedBox(width: 320, child: _presetCard(c, p)),
            ],
          ),
        ),
      ],
    );
  }

  Widget _heading(dynamic c, String text) => Text(
        text,
        style: TextStyle(
            color: c.textPrimary, fontSize: 14, fontWeight: FontWeight.w700),
      );

  Widget _riskBanner(dynamic c) {
    return Container(
      decoration: BoxDecoration(
        color: Colors.orange.withValues(alpha: 0.08),
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: Colors.orange.withValues(alpha: 0.3)),
      ),
      child: Theme(
        data: Theme.of(context).copyWith(dividerColor: Colors.transparent),
        child: ExpansionTile(
          initiallyExpanded: _riskExpanded,
          onExpansionChanged: (v) => setState(() => _riskExpanded = v),
          tilePadding: const EdgeInsets.symmetric(horizontal: AppTokens.s12),
          leading: const Icon(Icons.warning_amber_rounded,
              size: 18, color: Colors.orange),
          title: const Text(
            "Subscription sign-in is against the vendors' terms of service",
            style: TextStyle(color: Colors.orange, fontSize: 13),
          ),
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(AppTokens.s12, 0,
                  AppTokens.s12, AppTokens.s12),
              child: Text(
                'Subscription credentials are licensed for each vendor\'s own '
                'clients. Using them from SenClaw can get the account suspended, '
                'and the vendors detect it. SenClaw identifies itself honestly '
                'rather than imitating the vendor client, so a provider that '
                'blocks third-party access returns a clear error instead of '
                'failing silently. For anything you depend on, use an API key.',
                style: TextStyle(color: c.textSecondary, fontSize: 12),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _providerCard(dynamic c, OauthProviderDef p, List<OauthAccount> mine) {
    final busy = _connecting == p.id;
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(
          color: mine.isNotEmpty
              ? _brandColor(p.brandColor).withValues(alpha: 0.4)
              : c.border,
        ),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
            ProviderLogo(color: p.brandColor, mark: p.brandMark, size: 38),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(children: [
                    Flexible(
                      child: Text(p.displayName,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontSize: 13,
                              fontWeight: FontWeight.w600)),
                    ),
                    if (mine.isNotEmpty) ...[
                      const SizedBox(width: 4),
                      const Icon(Icons.check_circle,
                          size: 13, color: Colors.greenAccent),
                    ],
                  ]),
                  Text(
                    p.isDeviceFlow ? 'Device code' : 'Browser redirect',
                    style: TextStyle(color: c.textMuted, fontSize: 11),
                  ),
                ],
              ),
            ),
            Tooltip(
              message: p.riskNotice,
              child: const Icon(Icons.warning_amber_rounded,
                  size: 14, color: Colors.orange),
            ),
          ]),
          for (final a in mine) ...[
            const SizedBox(height: AppTokens.s8),
            Container(
              padding: const EdgeInsets.symmetric(
                  horizontal: AppTokens.s8, vertical: 4),
              decoration: BoxDecoration(
                color: c.bg,
                borderRadius: BorderRadius.circular(AppTokens.rSm),
              ),
              // Two lines rather than one: an email plus an expiry pill plus
              // three actions does not fit a 268pt card on one row.
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(children: [
                    Expanded(
                      child: Text(a.email ?? a.label,
                          overflow: TextOverflow.ellipsis,
                          style:
                              TextStyle(color: c.textSecondary, fontSize: 11)),
                    ),
                    const SizedBox(width: 6),
                    _expiryPill(context, a),
                  ]),
                  Row(mainAxisAlignment: MainAxisAlignment.end, children: [
                    IconButton(
                      tooltip: 'Use as model',
                      visualDensity: VisualDensity.compact,
                      constraints: const BoxConstraints(),
                      padding: const EdgeInsets.only(left: 8),
                      icon: Icon(Icons.add_circle_outline,
                          size: 15, color: c.textSecondary),
                      onPressed: () => _useAsModel(p, a),
                    ),
                    IconButton(
                      tooltip: a.hasRefreshToken
                          ? 'Refresh token'
                          : 'No refresh token — reconnect by hand',
                      visualDensity: VisualDensity.compact,
                      constraints: const BoxConstraints(),
                      padding: const EdgeInsets.only(left: 8),
                      icon: Icon(Icons.refresh,
                          size: 15,
                          color: a.hasRefreshToken
                              ? c.textSecondary
                              : c.textMuted),
                      onPressed:
                          a.hasRefreshToken ? () => _refreshAccount(a) : null,
                    ),
                    IconButton(
                      tooltip: 'Disconnect',
                      visualDensity: VisualDensity.compact,
                      constraints: const BoxConstraints(),
                      padding: const EdgeInsets.only(left: 8),
                      icon: const Icon(Icons.link_off,
                          size: 15, color: Colors.redAccent),
                      onPressed: () => _disconnect(a),
                    ),
                  ]),
                ],
              ),
            ),
            if (a.lastError != null)
              Padding(
                padding: const EdgeInsets.only(top: 4),
                child: Text(a.lastError!,
                    style: const TextStyle(
                        color: Colors.redAccent, fontSize: 11)),
              ),
          ],
          const SizedBox(height: AppTokens.s8),
          SizedBox(
            width: double.infinity,
            child: OutlinedButton.icon(
              icon: busy
                  ? const SizedBox(
                      width: 12,
                      height: 12,
                      child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.link, size: 15),
              label: Text(mine.isEmpty ? 'Connect' : 'Add another',
                  style: const TextStyle(fontSize: 12)),
              onPressed: busy ? null : () => _connect(p),
            ),
          ),
          if (p.requiresFixedPort)
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: Text('Needs port 1455 free.',
                  style: TextStyle(color: c.textMuted, fontSize: 11)),
            ),
        ],
      ),
    );
  }

  Widget _presetCard(dynamic c, CatalogProvider p) {
    return Container(
      padding: const EdgeInsets.all(AppTokens.s12),
      decoration: BoxDecoration(
        color: c.surface,
        borderRadius: BorderRadius.circular(AppTokens.rMd),
        border: Border.all(color: c.border),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(crossAxisAlignment: CrossAxisAlignment.start, children: [
            ProviderLogo(color: p.brandColor, mark: p.brandMark, size: 32),
            const SizedBox(width: AppTokens.s8),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(children: [
                    Flexible(
                      child: Text(p.displayName,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                              color: c.textPrimary,
                              fontSize: 13,
                              fontWeight: FontWeight.w600)),
                    ),
                    if (!p.needsKey) ...[
                      const SizedBox(width: 6),
                      const _Pill('No key', color: Colors.greenAccent),
                    ],
                    if (p.urlPlaceholder != null) ...[
                      const SizedBox(width: 6),
                      _Pill('needs ${p.urlPlaceholder}',
                          color: Colors.lightBlueAccent),
                    ],
                  ]),
                  Text(p.note,
                      style: TextStyle(color: c.textMuted, fontSize: 11)),
                ],
              ),
            ),
          ]),
          const SizedBox(height: AppTokens.s8),
          Row(mainAxisAlignment: MainAxisAlignment.end, children: [
            if (p.signupUrl != null)
              TextButton.icon(
                icon: const Icon(Icons.open_in_new, size: 14),
                label: const Text('Get key', style: TextStyle(fontSize: 12)),
                onPressed: () => launchUrl(Uri.parse(p.signupUrl!),
                    mode: LaunchMode.externalApplication),
              ),
            const SizedBox(width: AppTokens.s8),
            OutlinedButton.icon(
              icon: const Icon(Icons.add, size: 14),
              label: const Text('Add', style: TextStyle(fontSize: 12)),
              onPressed: () => _addPreset(p),
            ),
          ]),
        ],
      ),
    );
  }
}
