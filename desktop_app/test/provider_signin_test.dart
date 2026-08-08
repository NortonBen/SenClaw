import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/settings/provider_signin_section.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';

/// Records mutating calls. The list providers are overridden directly, so GETs
/// only need to return something well-formed.
class _RecordingApi implements ApiClient {
  final calls = <String>[];

  @override
  void updateConfig(AppConfig config) {}

  @override
  void dispose() {}

  @override
  Future<dynamic> get(String path, {Map<String, dynamic>? query, Duration? timeout}) async {
    calls.add('GET $path');
    return {};
  }

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    calls.add('POST $path ${jsonEncode(body ?? {})}');
    return {'ok': true, 'label': 'stub'};
  }

  @override
  Future<dynamic> put(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> patch(String path, {Object? body}) async => {'ok': true};

  @override
  Future<dynamic> delete(String path, {Object? body}) async {
    calls.add('DELETE $path');
    return {'ok': true};
  }
}

OauthProviderDef _provider({
  String id = 'claude',
  String name = 'Claude Code',
  String flow = 'auth_code_pkce',
  String mark = 'C',
  bool fixedPort = false,
}) =>
    OauthProviderDef.fromJson({
      'id': id,
      'displayName': name,
      'riskNotice': 'Against the vendor terms of service.',
      'brandColor': '#D97757',
      'brandMark': mark,
      'flow': flow,
      'compat': 'anthropic-compatible',
      'requiresFixedPort': fixedPort,
      'models': [
        {'id': 'claude-opus-5', 'name': 'Claude Opus 5'},
      ],
    });

OauthAccount _account({
  String id = 'acct-1',
  String provider = 'claude',
  int? expiresIn = 7200,
  bool expired = false,
  bool hasRefresh = true,
  String? lastError,
}) =>
    OauthAccount.fromJson({
      'id': id,
      'provider': provider,
      'label': 'Claude Code (dev@example.com)',
      'email': 'dev@example.com',
      'expiresIn': expiresIn,
      'expired': expired,
      'hasRefreshToken': hasRefresh,
      'lastError': lastError,
    });

CatalogProvider _preset({
  String id = 'nvidia',
  String name = 'NVIDIA NIM',
  String auth = 'api_key',
  String? placeholder,
}) =>
    CatalogProvider.fromJson({
      'id': id,
      'displayName': name,
      'baseURL': 'https://integrate.api.nvidia.com/v1',
      'adapt': 'openai',
      'auth': auth,
      'signupUrl': auth == 'api_key' ? 'https://build.nvidia.com' : null,
      'note': 'Free developer credits.',
      'brandColor': '#76B900',
      'brandMark': 'NV',
      'urlPlaceholder': placeholder,
      'defaultMaxTokens': 8192,
      'defaultContextLength': 128000,
      'models': [
        {'id': 'deepseek-ai/deepseek-v4-flash', 'name': 'DeepSeek V4 Flash'},
      ],
    });

Future<_RecordingApi> _pump(
  WidgetTester tester, {
  List<OauthProviderDef>? providers,
  List<OauthAccount>? accounts,
  List<CatalogProvider>? catalog,
}) async {
  tester.view.physicalSize = const Size(1400, 1100);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  final api = _RecordingApi();
  await tester.pumpWidget(ProviderScope(
    overrides: [
      apiClientProvider.overrideWithValue(api),
      oauthProvidersProvider
          .overrideWith((ref) async => providers ?? [_provider()]),
      oauthAccountsProvider.overrideWith((ref) async => accounts ?? const []),
      providerCatalogProvider
          .overrideWith((ref) async => catalog ?? [_preset()]),
    ],
    child: MaterialApp(
      theme: AppTheme.dark(),
      home: const Scaffold(body: ProviderSignInSection()),
    ),
  ));
  await tester.pumpAndSettle();
  return api;
}

void main() {
  testWidgets('renders a card per sign-in provider', (tester) async {
    await _pump(tester, providers: [
      _provider(),
      _provider(id: 'codex', name: 'OpenAI Codex', mark: 'OA'),
      _provider(
          id: 'grok', name: 'Grok CLI', flow: 'device_code', mark: 'X'),
    ]);

    expect(find.text('Claude Code'), findsOneWidget);
    expect(find.text('OpenAI Codex'), findsOneWidget);
    expect(find.text('Grok CLI'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Connect'), findsNWidgets(3));
  });

  testWidgets('labels the grant type per provider', (tester) async {
    await _pump(tester, providers: [
      _provider(),
      _provider(id: 'qwen', name: 'Qwen Code', flow: 'device_code', mark: 'Q'),
    ]);

    expect(find.text('Browser redirect'), findsOneWidget);
    expect(find.text('Device code'), findsOneWidget);
  });

  testWidgets('surfaces the fixed-port requirement only where it applies',
      (tester) async {
    await _pump(tester, providers: [
      _provider(id: 'codex', name: 'OpenAI Codex', fixedPort: true),
      _provider(),
    ]);
    expect(find.text('Needs port 1455 free.'), findsOneWidget);
  });

  testWidgets('the terms-of-service warning is always present', (tester) async {
    await _pump(tester);
    expect(
      find.textContaining("against the vendors' terms of service"),
      findsOneWidget,
    );
  });

  testWidgets('a connected account shows its email and expiry', (tester) async {
    await _pump(tester, accounts: [_account()]);

    expect(find.text('dev@example.com'), findsOneWidget);
    expect(find.text('2h left'), findsOneWidget);
    // With an account attached the button offers a second sign-in.
    expect(find.widgetWithText(OutlinedButton, 'Add another'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Connect'), findsNothing);
  });

  testWidgets('an expired token is called out', (tester) async {
    await _pump(tester,
        accounts: [_account(expiresIn: -10, expired: true)]);
    expect(find.text('Expired'), findsOneWidget);
  });

  testWidgets('an account with no expiry is labelled, not blank',
      (tester) async {
    await _pump(tester, accounts: [_account(expiresIn: null)]);
    expect(find.text('No expiry'), findsOneWidget);
  });

  testWidgets('a refresh failure is shown rather than swallowed',
      (tester) async {
    await _pump(tester,
        accounts: [_account(lastError: 'sign in again — refresh rejected')]);
    expect(
      find.textContaining('refresh rejected'),
      findsOneWidget,
    );
  });

  testWidgets('refresh is disabled when the provider issued no refresh token',
      (tester) async {
    await _pump(tester, accounts: [_account(hasRefresh: false)]);

    // SettingsBody's own reload button also uses Icons.refresh, so scope the
    // finder to the one carrying the account tooltip.
    final button = tester.widget<IconButton>(
      find.byWidgetPredicate((w) =>
          w is IconButton && (w.tooltip ?? '').startsWith('No refresh token')),
    );
    expect(button.onPressed, isNull, reason: 'refresh must be disabled');
  });

  testWidgets('refresh is enabled when a refresh token exists', (tester) async {
    final api = await _pump(tester, accounts: [_account()]);

    await tester.tap(find.byWidgetPredicate(
        (w) => w is IconButton && w.tooltip == 'Refresh token'));
    await tester.pumpAndSettle();

    expect(
      api.calls,
      contains('POST /api/oauth/accounts/acct-1/refresh {}'),
    );
  });

  testWidgets('disconnect asks before removing the account', (tester) async {
    final api = await _pump(tester, accounts: [_account()]);

    await tester.tap(find.byIcon(Icons.link_off));
    await tester.pumpAndSettle();
    expect(find.textContaining('Disconnect Claude Code'), findsOneWidget);

    // Cancelling must not call the API.
    await tester.tap(find.widgetWithText(TextButton, 'Cancel'));
    await tester.pumpAndSettle();
    expect(api.calls.where((c) => c.startsWith('DELETE')), isEmpty);

    // Confirming does.
    await tester.tap(find.byIcon(Icons.link_off));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Disconnect'));
    await tester.pumpAndSettle();
    expect(
      api.calls,
      contains('DELETE /api/oauth/accounts/acct-1'),
    );
  });

  testWidgets('connecting posts to the provider start endpoint',
      (tester) async {
    final api = await _pump(tester);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Connect'));
    await tester.pump();

    expect(
      api.calls.any((c) => c.startsWith('POST /api/oauth/claude/start')),
      isTrue,
      reason: 'calls were: ${api.calls}',
    );
  });

  testWidgets('binding an account posts the chosen model', (tester) async {
    final api = await _pump(tester, accounts: [_account()]);

    await tester.tap(find.byIcon(Icons.add_circle_outline));
    await tester.pumpAndSettle();
    expect(find.textContaining('as a model'), findsOneWidget);

    await tester.tap(find.widgetWithText(TextButton, 'Add model'));
    await tester.pumpAndSettle();

    final bind = api.calls.firstWhere((c) => c.startsWith('POST /api/oauth/bind'),
        orElse: () => '');
    expect(bind, isNotEmpty, reason: 'calls were: ${api.calls}');
    expect(bind, contains('acct-1'));
    expect(bind, contains('claude-opus-5'));
  });

  testWidgets('probing a model posts to the test endpoint', (tester) async {
    final api = await _pump(tester, accounts: [_account()]);

    await tester.tap(find.byIcon(Icons.add_circle_outline));
    await tester.pumpAndSettle();

    // Each suggested model carries its own probe button.
    await tester.tap(find.byIcon(Icons.science_outlined).first);
    await tester.pumpAndSettle();

    final probe = api.calls.firstWhere(
        (c) => c.startsWith('POST /api/oauth/test-model'),
        orElse: () => '');
    expect(probe, isNotEmpty, reason: 'calls were: ${api.calls}');
    expect(probe, contains('acct-1'));
    expect(probe, contains('claude-opus-5'));
  });

  testWidgets('a passing probe marks the model usable', (tester) async {
    await _pump(tester, accounts: [_account()]);

    await tester.tap(find.byIcon(Icons.add_circle_outline));
    await tester.pumpAndSettle();
    // Nothing probed yet: the row shows a neutral marker.
    expect(find.byIcon(Icons.circle_outlined), findsWidgets);

    await tester.tap(find.byIcon(Icons.science_outlined).first);
    await tester.pumpAndSettle();

    // The stub API answers {ok: true}, so the row flips to a tick. Scope to
    // the dialog: the provider card behind it carries its own "connected" tick.
    expect(
      find.descendant(
        of: find.byType(AlertDialog),
        matching: find.byIcon(Icons.check_circle),
      ),
      findsOneWidget,
    );
  });

  testWidgets('the bind dialog offers a bulk probe when there are several models',
      (tester) async {
    await _pump(tester, accounts: [_account()], providers: [
      OauthProviderDef.fromJson({
        'id': 'claude',
        'displayName': 'Claude Code',
        'riskNotice': 'risk',
        'brandColor': '#D97757',
        'brandMark': 'C',
        'flow': 'auth_code_pkce',
        'models': [
          {'id': 'a', 'name': 'Model A'},
          {'id': 'b', 'name': 'Model B'},
        ],
      }),
    ]);

    await tester.tap(find.byIcon(Icons.add_circle_outline));
    await tester.pumpAndSettle();
    expect(find.text('Test all 2'), findsOneWidget);
  });

  testWidgets('free-tier presets render with their note and actions',
      (tester) async {
    await _pump(tester, catalog: [_preset()]);

    expect(find.text('NVIDIA NIM'), findsOneWidget);
    expect(find.text('Free developer credits.'), findsOneWidget);
    expect(find.widgetWithText(TextButton, 'Get key'), findsOneWidget);
    expect(find.widgetWithText(OutlinedButton, 'Add'), findsOneWidget);
  });

  testWidgets('a no-key preset is badged and hides the key link',
      (tester) async {
    await _pump(tester, catalog: [
      _preset(id: 'mimo-free', name: 'Xiaomi MiMo', auth: 'none'),
    ]);

    expect(find.text('No key'), findsOneWidget);
    expect(find.widgetWithText(TextButton, 'Get key'), findsNothing);
  });

  testWidgets('a preset needing a URL value advertises it', (tester) async {
    await _pump(tester, catalog: [
      _preset(
          id: 'cloudflare-ai',
          name: 'Cloudflare Workers AI',
          placeholder: 'accountId'),
    ]);
    expect(find.text('needs accountId'), findsOneWidget);
  });

  testWidgets('adding a preset posts an llm-config with its base URL',
      (tester) async {
    final api = await _pump(tester, catalog: [_preset()]);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Add'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Add model'));
    await tester.pumpAndSettle();

    final call = api.calls.firstWhere(
        (c) => c.startsWith('POST /api/llm-config'),
        orElse: () => '');
    expect(call, isNotEmpty, reason: 'calls were: ${api.calls}');
    expect(call, contains('integrate.api.nvidia.com'));
    expect(call, contains('deepseek-ai/deepseek-v4-flash'));
  });

  testWidgets('a placeholder preset refuses to save without the value',
      (tester) async {
    final api = await _pump(tester, catalog: [
      _preset(
          id: 'cloudflare-ai',
          name: 'Cloudflare Workers AI',
          placeholder: 'accountId'),
    ]);

    await tester.tap(find.widgetWithText(OutlinedButton, 'Add'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(TextButton, 'Add model'));
    await tester.pumpAndSettle();

    expect(api.calls.any((c) => c.startsWith('POST /api/llm-config')), isFalse);
    expect(find.textContaining('accountId is required'), findsOneWidget);
  });

  testWidgets('a malformed brand colour does not crash the badge',
      (tester) async {
    await tester.pumpWidget(MaterialApp(
      theme: AppTheme.dark(),
      home: const Scaffold(
        body: Row(children: [
          ProviderLogo(color: 'not-a-colour', mark: 'X'),
          ProviderLogo(color: '#GGGGGG', mark: 'Y'),
          ProviderLogo(color: '', mark: 'Z'),
        ]),
      ),
    ));
    await tester.pumpAndSettle();
    expect(find.byType(ProviderLogo), findsNWidgets(3));
  });
}
