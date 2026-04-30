// This is a generated file - do not edit.
//
// Generated from channel_relay.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports
// ignore_for_file: unused_import

import 'dart:convert' as $convert;
import 'dart:core' as $core;
import 'dart:typed_data' as $typed_data;

@$core.Deprecated('Use relayMessageDescriptor instead')
const RelayMessage$json = {
  '1': 'RelayMessage',
  '2': [
    {'1': 'channel_id', '3': 1, '4': 1, '5': 9, '10': 'channelId'},
    {'1': 'sender_id', '3': 2, '4': 1, '5': 9, '10': 'senderId'},
    {'1': 'timestamp', '3': 3, '4': 1, '5': 3, '10': 'timestamp'},
    {'1': 'message_id', '3': 4, '4': 1, '5': 9, '10': 'messageId'},
    {
      '1': 'encrypted_data',
      '3': 5,
      '4': 1,
      '5': 11,
      '6': '.relay.EncryptedData',
      '9': 0,
      '10': 'encryptedData'
    },
    {
      '1': 'control',
      '3': 6,
      '4': 1,
      '5': 11,
      '6': '.relay.ControlMessage',
      '9': 0,
      '10': 'control'
    },
  ],
  '8': [
    {'1': 'payload'},
  ],
};

/// Descriptor for `RelayMessage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List relayMessageDescriptor = $convert.base64Decode(
    'CgxSZWxheU1lc3NhZ2USHQoKY2hhbm5lbF9pZBgBIAEoCVIJY2hhbm5lbElkEhsKCXNlbmRlcl'
    '9pZBgCIAEoCVIIc2VuZGVySWQSHAoJdGltZXN0YW1wGAMgASgDUgl0aW1lc3RhbXASHQoKbWVz'
    'c2FnZV9pZBgEIAEoCVIJbWVzc2FnZUlkEj0KDmVuY3J5cHRlZF9kYXRhGAUgASgLMhQucmVsYX'
    'kuRW5jcnlwdGVkRGF0YUgAUg1lbmNyeXB0ZWREYXRhEjEKB2NvbnRyb2wYBiABKAsyFS5yZWxh'
    'eS5Db250cm9sTWVzc2FnZUgAUgdjb250cm9sQgkKB3BheWxvYWQ=');

@$core.Deprecated('Use encryptedDataDescriptor instead')
const EncryptedData$json = {
  '1': 'EncryptedData',
  '2': [
    {'1': 'nonce', '3': 1, '4': 1, '5': 12, '10': 'nonce'},
    {'1': 'ciphertext', '3': 2, '4': 1, '5': 12, '10': 'ciphertext'},
    {'1': 'tag', '3': 3, '4': 1, '5': 12, '10': 'tag'},
  ],
};

/// Descriptor for `EncryptedData`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List encryptedDataDescriptor = $convert.base64Decode(
    'Cg1FbmNyeXB0ZWREYXRhEhQKBW5vbmNlGAEgASgMUgVub25jZRIeCgpjaXBoZXJ0ZXh0GAIgAS'
    'gMUgpjaXBoZXJ0ZXh0EhAKA3RhZxgDIAEoDFIDdGFn');

@$core.Deprecated('Use controlMessageDescriptor instead')
const ControlMessage$json = {
  '1': 'ControlMessage',
  '2': [
    {
      '1': 'type',
      '3': 1,
      '4': 1,
      '5': 14,
      '6': '.relay.ControlMessage.Type',
      '10': 'type'
    },
    {'1': 'metadata', '3': 2, '4': 1, '5': 9, '10': 'metadata'},
  ],
  '4': [ControlMessage_Type$json],
};

@$core.Deprecated('Use controlMessageDescriptor instead')
const ControlMessage_Type$json = {
  '1': 'Type',
  '2': [
    {'1': 'PING', '2': 0},
    {'1': 'PONG', '2': 1},
    {'1': 'ACK', '2': 2},
    {'1': 'TYPING_START', '2': 3},
    {'1': 'TYPING_STOP', '2': 4},
    {'1': 'DISCONNECT', '2': 5},
  ],
};

/// Descriptor for `ControlMessage`. Decode as a `google.protobuf.DescriptorProto`.
final $typed_data.Uint8List controlMessageDescriptor = $convert.base64Decode(
    'Cg5Db250cm9sTWVzc2FnZRIuCgR0eXBlGAEgASgOMhoucmVsYXkuQ29udHJvbE1lc3NhZ2UuVH'
    'lwZVIEdHlwZRIaCghtZXRhZGF0YRgCIAEoCVIIbWV0YWRhdGEiVgoEVHlwZRIICgRQSU5HEAAS'
    'CAoEUE9ORxABEgcKA0FDSxACEhAKDFRZUElOR19TVEFSVBADEg8KC1RZUElOR19TVE9QEAQSDg'
    'oKRElTQ09OTkVDVBAF');
