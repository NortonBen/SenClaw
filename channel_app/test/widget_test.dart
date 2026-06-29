// Minimal smoke test: the app's root widget can be constructed.
//
// A full pumpWidget would require the secure-storage / relay plumbing that
// `SenclawApp` boots, so this just guards that the widget tree compiles and the
// root type is wired up.

import 'package:flutter_test/flutter_test.dart';

import 'package:channel_app/main.dart';

void main() {
  test('SenclawApp can be instantiated', () {
    const app = SenclawApp();
    expect(app, isNotNull);
  });
}
