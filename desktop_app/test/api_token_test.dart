import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';

void main() {
  group('AppConfig API token', () {
    test('authHeaders empty without a token', () {
      const cfg = AppConfig(host: '127.0.0.1', uiPort: 18788, wsPort: 18789);
      expect(cfg.authHeaders, isEmpty);
    });

    test('authHeaders empty for an explicitly empty token', () {
      const cfg = AppConfig(
          host: '127.0.0.1', uiPort: 18788, wsPort: 18789, apiToken: '');
      expect(cfg.authHeaders, isEmpty);
    });

    test('authHeaders carries X-SenClaw-Token', () {
      const cfg = AppConfig(
          host: '192.168.1.9', uiPort: 18788, wsPort: 18789, apiToken: 'abc');
      expect(cfg.authHeaders, {'x-senclaw-token': 'abc'});
    });

    test('copyWith sets and keeps apiToken/host/uiPort', () {
      const cfg = AppConfig(host: '127.0.0.1', uiPort: 18788, wsPort: 18789);
      final withToken = cfg.copyWith(apiToken: 'abc', host: '10.0.0.5');
      expect(withToken.apiToken, 'abc');
      expect(withToken.host, '10.0.0.5');
      expect(withToken.uiPort, 18788);
      // Unrelated copyWith calls must not drop the token (the bug this
      // guards: discovery ran copyWith(wsPort/wsToken) after token seeding).
      final afterDiscovery = withToken.copyWith(wsPort: 20000, wsToken: 'ws');
      expect(afterDiscovery.apiToken, 'abc');
      expect(afterDiscovery.host, '10.0.0.5');
    });
  });
}
