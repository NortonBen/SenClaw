import 'api_client.dart';

/// Resolves pending agent interactions (tool-permission requests, ask-question
/// batches) over the relay tunnel — parity with the web WS `permission:response`
/// / `question:response`.
class ChatApi {
  final _api = ApiClient();

  Future<void> respondPermission(String requestId, String optionKey) =>
      _api.post('/api/chat/permission/respond',
          body: {'requestId': requestId, 'optionKey': optionKey});

  /// [answers] is `{ "<questionIndex>": optionIndex | [optionIndex, …] }`.
  Future<void> respondQuestion(
    String requestId,
    Map<String, dynamic> answers, {
    Map<String, dynamic>? otherTexts,
  }) =>
      _api.post('/api/chat/question/respond', body: {
        'requestId': requestId,
        'answers': answers,
        if (otherTexts != null) 'otherTexts': otherTexts,
      });

  /// [selected] = 'startEditing' | 'clearContextAndStart' | 'cancelled'.
  Future<void> respondPlan(
    String groupJid,
    String agentId,
    String selected,
  ) =>
      _api.post('/api/chat/plan/respond', body: {
        'groupJid': groupJid,
        'agentId': agentId,
        'selected': selected,
      });
}
