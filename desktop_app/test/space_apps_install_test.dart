// Màn Apps: ô tìm kiếm lọc lưới launcher, nút cài mở hộp thoại ba đường
// (cửa hàng / ZIP / manifest URL).
//
// Điểm đáng test nhất là lọc bỏ dấu — người Việt gõ "kho" phải ra "Quản lý
// Kho" — và việc tab cửa hàng CHỈ nhận mục registry `kind: "app"` có slug:
// một mục marketplace.json lọt vào đây sẽ cài bằng endpoint sai.

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:senclaw_desktop/core/config/app_config.dart';
import 'package:senclaw_desktop/core/prefs.dart';
import 'package:senclaw_desktop/core/transport/api_client.dart';
import 'package:senclaw_desktop/core/transport/connection.dart';
import 'package:senclaw_desktop/features/space/app_install_dialog.dart';
import 'package:senclaw_desktop/features/space/app_search.dart';
import 'package:senclaw_desktop/features/space/space_screen.dart';
import 'package:senclaw_desktop/theme/app_theme.dart';
import 'package:shared_preferences/shared_preferences.dart';

/// Trả app đã cài + danh mục hub, ghi lại mọi lời gọi.
class _FakeApi implements ApiClient {
  _FakeApi({this.sourcesFail = false});

  final bool sourcesFail;
  final calls = <String>[];

  @override
  void updateConfig(AppConfig config) {}

  @override
  void dispose() {}

  @override
  Future<dynamic> get(String path,
      {Map<String, dynamic>? query, Duration? timeout}) async {
    calls.add('GET $path');
    if (path == '/api/space/apps') {
      return [
        {
          'id': 'warehouse',
          'enabled': true,
          'manifest': {
            'name': 'Quản lý Kho',
            'icon': '📦',
            'description': 'Nhập xuất tồn',
            'integration': {'type': 'iframe', 'url': '/'},
          },
        },
        {
          'id': 'cafe',
          'enabled': true,
          'manifest': {
            'name': 'Cafe',
            'icon': '☕',
            'description': 'Quản lý quán',
            'integration': {'type': 'iframe', 'url': '/'},
          },
        },
      ];
    }
    if (path == '/api/marketplace/sources') {
      if (sourcesFail) {
        throw ApiException(503, 'Marketplace manager not available');
      }
      return {
        'sources': [
          {'id': 'hub', 'name': 'SenClaw Hub', 'enabled': true},
          {'id': 'off', 'name': 'Disabled', 'enabled': false},
        ],
      };
    }
    if (path == '/api/marketplace/sources/hub') {
      return {
        'plugins': [
          {
            'name': 'predict',
            'description': 'Dự đoán trận đấu',
            'version': '1.0.1',
            'kind': 'app',
            'slug': 'senclaw/predict',
            'installed': false,
          },
          {
            'name': 'cafe',
            'description': 'Quản lý quán cafe',
            'version': '1.2.0',
            'kind': 'app',
            'slug': 'senclaw/cafe',
            'installed': true,
            'installedVersion': '1.0.0',
            'updateAvailable': true,
          },
          // Mục git-clone: không slug, không phải Space App — phải bị loại.
          {'name': 'some-plugin', 'description': 'a plugin', 'installed': true},
        ],
      };
    }
    throw ApiException(404, 'unexpected GET $path');
  }

  @override
  Future<dynamic> post(String path, {Object? body}) async {
    calls.add('POST $path ${body ?? ''}'.trim());
    return {
      'id': 'predict',
      'enabled': true,
      'manifest': {'name': 'Siêu Dự Đoán'},
    };
  }

  @override
  Future<dynamic> put(String path, {Object? body}) async => {};

  @override
  Future<dynamic> patch(String path, {Object? body}) async => {};

  @override
  Future<dynamic> delete(String path, {Object? body}) async => {};
}

Future<_FakeApi> _pump(WidgetTester tester, Widget home,
    {_FakeApi? given}) async {
  tester.view.physicalSize = const Size(1400, 1600);
  tester.view.devicePixelRatio = 1.0;
  addTearDown(tester.view.reset);

  SharedPreferences.setMockInitialValues({});
  final prefs = await SharedPreferences.getInstance();
  final api = given ?? _FakeApi();

  await tester.pumpWidget(ProviderScope(
    overrides: [
      apiClientProvider.overrideWithValue(api),
      prefsProvider.overrideWithValue(prefs),
    ],
    child: MaterialApp(theme: AppTheme.light(), home: Scaffold(body: home)),
  ));
  await tester.pumpAndSettle();
  return api;
}

void main() {
  group('foldSearch', () {
    test('bỏ dấu tiếng Việt và hạ chữ thường', () {
      expect(foldSearch('Quản lý Kho'), 'quan ly kho');
      expect(foldSearch('Siêu Dự Đoán'), 'sieu du doan');
      // đ là chữ cái riêng, không phải d + dấu.
      expect(foldSearch('Đồng hồ'), 'dong ho');
    });

    test('khớp không dấu, từ khoá rỗng khớp tất cả', () {
      expect(searchMatches(['Quản lý Kho', 'nhập xuất'], 'kho'), isTrue);
      expect(searchMatches(['Siêu Dự Đoán'], 'du doan'), isTrue);
      expect(searchMatches(['Cafe'], ''), isTrue);
      expect(searchMatches(['Cafe'], 'kho'), isFalse);
    });
  });

  group('SpaceAppsScreen', () {
    testWidgets('ô tìm kiếm lọc lưới app, gõ không dấu vẫn ra', (tester) async {
      await _pump(tester, const SpaceAppsScreen());
      expect(find.text('Quản lý Kho'), findsOneWidget);
      expect(find.text('Cafe'), findsOneWidget);

      await tester.enterText(find.byType(TextField).first, 'kho');
      await tester.pumpAndSettle();

      expect(find.text('Quản lý Kho'), findsOneWidget);
      expect(find.text('Cafe'), findsNothing);
    });

    testWidgets('từ khoá không khớp báo rõ, không phải "chưa cài app nào"',
        (tester) async {
      await _pump(tester, const SpaceAppsScreen());
      await tester.enterText(find.byType(TextField).first, 'zzz');
      await tester.pumpAndSettle();

      expect(find.text('No app matches that search'), findsOneWidget);
      expect(find.text('No apps installed'), findsNothing);
    });

    testWidgets('nút cài mở hộp thoại cài app', (tester) async {
      await _pump(tester, const SpaceAppsScreen());
      await tester.tap(find.text('Install app'));
      await tester.pumpAndSettle();

      expect(find.text('Install a new app'), findsOneWidget);
      expect(find.text('Store'), findsOneWidget);
      expect(find.text('ZIP file'), findsOneWidget);
    });
  });

  group('AppInstallDialog', () {
    testWidgets('cửa hàng chỉ liệt kê mục app có slug', (tester) async {
      final api = await _pump(tester, const AppInstallDialog());

      expect(api.calls, contains('GET /api/marketplace/sources'));
      expect(api.calls, contains('GET /api/marketplace/sources/hub'));
      // Nguồn đang tắt không được đọc.
      expect(api.calls, isNot(contains('GET /api/marketplace/sources/off')));

      expect(find.text('predict'), findsOneWidget);
      expect(find.text('cafe'), findsOneWidget);
      expect(find.text('some-plugin'), findsNothing);
    });

    testWidgets('bản đã cài có bản mới thì mời cập nhật, không phải "đã cài"',
        (tester) async {
      await _pump(tester, const AppInstallDialog());
      expect(find.text('Update'), findsOneWidget); // cafe: 1.0.0 → 1.2.0
      expect(find.text('using 1.0.0'), findsOneWidget);
      expect(find.text('Installed'), findsNothing);
    });

    testWidgets('cài theo slug qua endpoint registry', (tester) async {
      final api = await _pump(tester, const AppInstallDialog());
      await tester.tap(find.text('Install'));
      await tester.pumpAndSettle();

      expect(
        api.calls
            .where((c) => c.startsWith('POST /api/marketplace/hub/install')),
        isNotEmpty,
      );
      expect(api.calls.last, contains('senclaw/predict'));
    });

    testWidgets('tìm trong cửa hàng lọc theo tên lẫn mô tả', (tester) async {
      await _pump(tester, const AppInstallDialog());
      // Ô đầu tiên là ô tìm của tab cửa hàng.
      await tester.enterText(find.byType(TextField).first, 'du doan');
      await tester.pumpAndSettle();

      expect(find.text('predict'), findsOneWidget);
      expect(find.text('cafe'), findsNothing);
    });

    testWidgets('marketplace hỏng thì báo lỗi, không treo màn trắng',
        (tester) async {
      await _pump(tester, const AppInstallDialog(),
          given: _FakeApi(sourcesFail: true));

      expect(find.textContaining('Could not read the store catalog'),
          findsOneWidget);
    });
  });
}
