// This is a generated file - do not edit.
//
// Generated from channel_relay.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:async' as $async;
import 'dart:core' as $core;

import 'package:grpc/service_api.dart' as $grpc;
import 'package:protobuf/protobuf.dart' as $pb;

import 'channel_relay.pb.dart' as $0;

export 'channel_relay.pb.dart';

@$pb.GrpcServiceName('relay.ChannelRelay')
class ChannelRelayClient extends $grpc.Client {
  /// The hostname for this service.
  static const $core.String defaultHost = '';

  /// OAuth scopes needed for the client.
  static const $core.List<$core.String> oauthScopes = [
    '',
  ];

  ChannelRelayClient(super.channel, {super.options, super.interceptors});

  /// Bi-directional stream for real-time interaction between SemaClaw and Channel App
  /// Both SemaClaw (Edge) and Channel App (Client) connect to this stream
  $grpc.ResponseStream<$0.RelayMessage> stream(
    $async.Stream<$0.RelayMessage> request, {
    $grpc.CallOptions? options,
  }) {
    return $createStreamingCall(_$stream, request, options: options);
  }

  // method descriptors

  static final _$stream = $grpc.ClientMethod<$0.RelayMessage, $0.RelayMessage>(
      '/relay.ChannelRelay/Stream',
      ($0.RelayMessage value) => value.writeToBuffer(),
      $0.RelayMessage.fromBuffer);
}

@$pb.GrpcServiceName('relay.ChannelRelay')
abstract class ChannelRelayServiceBase extends $grpc.Service {
  $core.String get $name => 'relay.ChannelRelay';

  ChannelRelayServiceBase() {
    $addMethod($grpc.ServiceMethod<$0.RelayMessage, $0.RelayMessage>(
        'Stream',
        stream,
        true,
        true,
        ($core.List<$core.int> value) => $0.RelayMessage.fromBuffer(value),
        ($0.RelayMessage value) => value.writeToBuffer()));
  }

  $async.Stream<$0.RelayMessage> stream(
      $grpc.ServiceCall call, $async.Stream<$0.RelayMessage> request);
}
