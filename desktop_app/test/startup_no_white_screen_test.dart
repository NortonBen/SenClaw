import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/daemon/daemon_supervisor.dart';
import 'package:senclaw_desktop/core/daemon/port_tools.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';

/// The bug these guard: 0.5.0/0.5.1 could sit on an empty white window with no
/// text, no spinner and no way out. Every path below is one of the ways that
/// happened — each now ends in an error the user can read and act on.
void main() {
  group('a request always ends', () {
    late HttpServer silent;

    setUp(() async {
      // Accepts the connection, reads the request, and then says nothing —
      // exactly what a wedged daemon looks like from the client side.
      silent = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      silent.listen((_) {});
    });

    tearDown(() => silent.close(force: true));

    test('a socket that never answers times out instead of hanging', () async {
      final api = ApiClient(AppConfig.fromEnvironment().copyWith(
        host: '127.0.0.1',
        uiPort: silent.port,
      ));
      await expectLater(
        api.get('/api/config', timeout: const Duration(milliseconds: 300)),
        throwsA(isA<ApiTimeout>()),
      );
      api.dispose();
    });
  });

  group('adopting a port', () {
    test('a listener that does not answer is refused, not adopted', () async {
      // A raw socket server: it accepts, so the old TCP-only check called it a
      // healthy daemon and the app waited on it forever.
      final squatter = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      squatter.listen((_) {});

      final sup = DaemonSupervisor(
        uiPort: squatter.port,
        adoptProbeBudget: const Duration(milliseconds: 600),
      );
      await sup.start();

      expect(sup.phase, DaemonPhase.crashed);
      expect(sup.lastError, contains('does not answer'));
      expect(sup.isUp, isFalse, reason: 'a silent port is not a live daemon');

      await squatter.close();
      sup.dispose();
    });

    test('any HTTP reply counts as alive — even 401 from a gated daemon',
        () async {
      final gated = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      gated.listen((req) {
        req.response.statusCode = HttpStatus.unauthorized;
        req.response.close();
      });

      final sup = DaemonSupervisor(
        uiPort: gated.port,
        adoptProbeBudget: const Duration(seconds: 2),
      );
      await sup.start();

      expect(sup.phase, DaemonPhase.adopted);
      await gated.close(force: true);
      sup.dispose();
    });
  });

  group('bind host', () {
    test('defaults to loopback and reports public only when it is', () {
      expect(DaemonSupervisor().bindHost, '127.0.0.1');
      expect(DaemonSupervisor().isPublicBind, isFalse);
      expect(
        DaemonSupervisor(bindHost: DaemonSupervisor.kPublicBindHost)
            .isPublicBind,
        isTrue,
      );
    });

    test('an empty value falls back to loopback rather than binding the world',
        () {
      final sup = DaemonSupervisor()..bindHost = '   ';
      expect(sup.bindHost, '127.0.0.1');
      sup.dispose();
    });
  });

  group('netstat parsing (Windows kill-port)', () {
    const sample = '''
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    127.0.0.1:18788        0.0.0.0:0              LISTENING       4242
  TCP    127.0.0.1:52001        127.0.0.1:18788        ESTABLISHED     9999
  TCP    [::]:18789             [::]:0                 LISTENING       4242
''';

    test('finds the listening owner', () {
      expect(PortTools.parseNetstatPid(sample, 18788), 4242);
      expect(PortTools.parseNetstatPid(sample, 18789), 4242,
          reason: 'IPv6 rows must parse too');
    });

    test('never returns a client that merely connects to the port', () {
      // 52001 is the ESTABLISHED row's local port; 18788 appears as its REMOTE
      // port. Matching that row would kill the wrong process.
      expect(PortTools.parseNetstatPid(sample, 52001), isNull);
    });

    test('an unused port has no owner', () {
      expect(PortTools.parseNetstatPid(sample, 9), isNull);
    });
  });
}
