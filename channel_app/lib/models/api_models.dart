/// Result of a REST call tunnelled over the relay (API_RESP frame).
class ApiResponse {
  final String requestId;
  final int status;
  final String body;

  const ApiResponse({
    required this.requestId,
    required this.status,
    required this.body,
  });

  bool get isOk => status >= 200 && status < 300;

  factory ApiResponse.fromJson(Map<String, dynamic> json) => ApiResponse(
    requestId: (json['requestId'] ?? '').toString(),
    status: (json['status'] as num?)?.toInt() ?? 0,
    body: (json['body'] ?? '').toString(),
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
