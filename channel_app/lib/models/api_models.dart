import 'dart:convert';
import 'dart:typed_data';

/// Result of a REST call tunnelled over the relay (API_RESP frame).
class ApiResponse {
  final String requestId;
  final int status;
  final String body;

  /// The response's Content-Type (when the daemon reported one).
  final String? contentType;

  /// When true, [body] is base64-encoded binary (images/fonts/wasm) rather than
  /// UTF-8 text — used by the Space-app webview proxy.
  final bool bodyBase64;

  const ApiResponse({
    required this.requestId,
    required this.status,
    required this.body,
    this.contentType,
    this.bodyBase64 = false,
  });

  bool get isOk => status >= 200 && status < 300;

  /// Raw response bytes, decoding base64 when needed.
  Uint8List get bytes => bodyBase64
      ? base64Decode(body)
      : Uint8List.fromList(utf8.encode(body));

  factory ApiResponse.fromJson(Map<String, dynamic> json) => ApiResponse(
    requestId: (json['requestId'] ?? '').toString(),
    status: (json['status'] as num?)?.toInt() ?? 0,
    body: (json['body'] ?? '').toString(),
    contentType: json['contentType'] as String?,
    bodyBase64: json['bodyBase64'] == true,
  );
}

/// A server-pushed event delivered over the relay (API_EVENT frame).
class ApiEvent {
  final String topic;
  final dynamic data;

  const ApiEvent({required this.topic, this.data});

  factory ApiEvent.fromJson(Map<String, dynamic> json) =>
      ApiEvent(topic: (json['topic'] ?? '').toString(), data: json['data']);
}

/// Thrown when a tunnelled REST call fails (non-2xx, timeout, no transport).
class ApiException implements Exception {
  final int status;
  final String message;

  const ApiException(this.status, this.message);

  @override
  String toString() => 'ApiException($status): $message';
}
