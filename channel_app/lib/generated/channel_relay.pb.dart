// This is a generated file - do not edit.
//
// Generated from channel_relay.proto.

// @dart = 3.3

// ignore_for_file: annotate_overrides, camel_case_types, comment_references
// ignore_for_file: constant_identifier_names
// ignore_for_file: curly_braces_in_flow_control_structures
// ignore_for_file: deprecated_member_use_from_same_package, library_prefixes
// ignore_for_file: non_constant_identifier_names, prefer_relative_imports

import 'dart:core' as $core;

import 'package:fixnum/fixnum.dart' as $fixnum;
import 'package:protobuf/protobuf.dart' as $pb;

import 'channel_relay.pbenum.dart';

export 'package:protobuf/protobuf.dart' show GeneratedMessageGenericExtensions;

export 'channel_relay.pbenum.dart';

enum RelayMessage_Payload { encryptedData, control, notSet }

class RelayMessage extends $pb.GeneratedMessage {
  factory RelayMessage({
    $core.String? channelId,
    $core.String? senderId,
    $fixnum.Int64? timestamp,
    $core.String? messageId,
    EncryptedData? encryptedData,
    ControlMessage? control,
  }) {
    final result = create();
    if (channelId != null) result.channelId = channelId;
    if (senderId != null) result.senderId = senderId;
    if (timestamp != null) result.timestamp = timestamp;
    if (messageId != null) result.messageId = messageId;
    if (encryptedData != null) result.encryptedData = encryptedData;
    if (control != null) result.control = control;
    return result;
  }

  RelayMessage._();

  factory RelayMessage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory RelayMessage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static const $core.Map<$core.int, RelayMessage_Payload>
      _RelayMessage_PayloadByTag = {
    5: RelayMessage_Payload.encryptedData,
    6: RelayMessage_Payload.control,
    0: RelayMessage_Payload.notSet
  };
  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'RelayMessage',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'relay'),
      createEmptyInstance: create)
    ..oo(0, [5, 6])
    ..aOS(1, _omitFieldNames ? '' : 'channelId')
    ..aOS(2, _omitFieldNames ? '' : 'senderId')
    ..aInt64(3, _omitFieldNames ? '' : 'timestamp')
    ..aOS(4, _omitFieldNames ? '' : 'messageId')
    ..aOM<EncryptedData>(5, _omitFieldNames ? '' : 'encryptedData',
        subBuilder: EncryptedData.create)
    ..aOM<ControlMessage>(6, _omitFieldNames ? '' : 'control',
        subBuilder: ControlMessage.create)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RelayMessage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  RelayMessage copyWith(void Function(RelayMessage) updates) =>
      super.copyWith((message) => updates(message as RelayMessage))
          as RelayMessage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static RelayMessage create() => RelayMessage._();
  @$core.override
  RelayMessage createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static RelayMessage getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<RelayMessage>(create);
  static RelayMessage? _defaultInstance;

  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  RelayMessage_Payload whichPayload() =>
      _RelayMessage_PayloadByTag[$_whichOneof(0)]!;
  @$pb.TagNumber(5)
  @$pb.TagNumber(6)
  void clearPayload() => $_clearField($_whichOneof(0));

  /// Unique ID for the connection session
  @$pb.TagNumber(1)
  $core.String get channelId => $_getSZ(0);
  @$pb.TagNumber(1)
  set channelId($core.String value) => $_setString(0, value);
  @$pb.TagNumber(1)
  $core.bool hasChannelId() => $_has(0);
  @$pb.TagNumber(1)
  void clearChannelId() => $_clearField(1);

  /// ID of the sender (Agent UUID or App Instance ID)
  @$pb.TagNumber(2)
  $core.String get senderId => $_getSZ(1);
  @$pb.TagNumber(2)
  set senderId($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasSenderId() => $_has(1);
  @$pb.TagNumber(2)
  void clearSenderId() => $_clearField(2);

  /// Unix timestamp in milliseconds
  @$pb.TagNumber(3)
  $fixnum.Int64 get timestamp => $_getI64(2);
  @$pb.TagNumber(3)
  set timestamp($fixnum.Int64 value) => $_setInt64(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTimestamp() => $_has(2);
  @$pb.TagNumber(3)
  void clearTimestamp() => $_clearField(3);

  /// Unique message ID for ack/tracking
  @$pb.TagNumber(4)
  $core.String get messageId => $_getSZ(3);
  @$pb.TagNumber(4)
  set messageId($core.String value) => $_setString(3, value);
  @$pb.TagNumber(4)
  $core.bool hasMessageId() => $_has(3);
  @$pb.TagNumber(4)
  void clearMessageId() => $_clearField(4);

  @$pb.TagNumber(5)
  EncryptedData get encryptedData => $_getN(4);
  @$pb.TagNumber(5)
  set encryptedData(EncryptedData value) => $_setField(5, value);
  @$pb.TagNumber(5)
  $core.bool hasEncryptedData() => $_has(4);
  @$pb.TagNumber(5)
  void clearEncryptedData() => $_clearField(5);
  @$pb.TagNumber(5)
  EncryptedData ensureEncryptedData() => $_ensure(4);

  @$pb.TagNumber(6)
  ControlMessage get control => $_getN(5);
  @$pb.TagNumber(6)
  set control(ControlMessage value) => $_setField(6, value);
  @$pb.TagNumber(6)
  $core.bool hasControl() => $_has(5);
  @$pb.TagNumber(6)
  void clearControl() => $_clearField(6);
  @$pb.TagNumber(6)
  ControlMessage ensureControl() => $_ensure(5);
}

class EncryptedData extends $pb.GeneratedMessage {
  factory EncryptedData({
    $core.List<$core.int>? nonce,
    $core.List<$core.int>? ciphertext,
    $core.List<$core.int>? tag,
  }) {
    final result = create();
    if (nonce != null) result.nonce = nonce;
    if (ciphertext != null) result.ciphertext = ciphertext;
    if (tag != null) result.tag = tag;
    return result;
  }

  EncryptedData._();

  factory EncryptedData.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory EncryptedData.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'EncryptedData',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'relay'),
      createEmptyInstance: create)
    ..a<$core.List<$core.int>>(
        1, _omitFieldNames ? '' : 'nonce', $pb.PbFieldType.OY)
    ..a<$core.List<$core.int>>(
        2, _omitFieldNames ? '' : 'ciphertext', $pb.PbFieldType.OY)
    ..a<$core.List<$core.int>>(
        3, _omitFieldNames ? '' : 'tag', $pb.PbFieldType.OY)
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EncryptedData clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  EncryptedData copyWith(void Function(EncryptedData) updates) =>
      super.copyWith((message) => updates(message as EncryptedData))
          as EncryptedData;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static EncryptedData create() => EncryptedData._();
  @$core.override
  EncryptedData createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static EncryptedData getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<EncryptedData>(create);
  static EncryptedData? _defaultInstance;

  /// Initialization vector / nonce
  @$pb.TagNumber(1)
  $core.List<$core.int> get nonce => $_getN(0);
  @$pb.TagNumber(1)
  set nonce($core.List<$core.int> value) => $_setBytes(0, value);
  @$pb.TagNumber(1)
  $core.bool hasNonce() => $_has(0);
  @$pb.TagNumber(1)
  void clearNonce() => $_clearField(1);

  /// AES-GCM-256 encrypted payload (JSON string)
  @$pb.TagNumber(2)
  $core.List<$core.int> get ciphertext => $_getN(1);
  @$pb.TagNumber(2)
  set ciphertext($core.List<$core.int> value) => $_setBytes(1, value);
  @$pb.TagNumber(2)
  $core.bool hasCiphertext() => $_has(1);
  @$pb.TagNumber(2)
  void clearCiphertext() => $_clearField(2);

  /// Authentication tag
  @$pb.TagNumber(3)
  $core.List<$core.int> get tag => $_getN(2);
  @$pb.TagNumber(3)
  set tag($core.List<$core.int> value) => $_setBytes(2, value);
  @$pb.TagNumber(3)
  $core.bool hasTag() => $_has(2);
  @$pb.TagNumber(3)
  void clearTag() => $_clearField(3);
}

class ControlMessage extends $pb.GeneratedMessage {
  factory ControlMessage({
    ControlMessage_Type? type,
    $core.String? metadata,
  }) {
    final result = create();
    if (type != null) result.type = type;
    if (metadata != null) result.metadata = metadata;
    return result;
  }

  ControlMessage._();

  factory ControlMessage.fromBuffer($core.List<$core.int> data,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromBuffer(data, registry);
  factory ControlMessage.fromJson($core.String json,
          [$pb.ExtensionRegistry registry = $pb.ExtensionRegistry.EMPTY]) =>
      create()..mergeFromJson(json, registry);

  static final $pb.BuilderInfo _i = $pb.BuilderInfo(
      _omitMessageNames ? '' : 'ControlMessage',
      package: const $pb.PackageName(_omitMessageNames ? '' : 'relay'),
      createEmptyInstance: create)
    ..aE<ControlMessage_Type>(1, _omitFieldNames ? '' : 'type',
        enumValues: ControlMessage_Type.values)
    ..aOS(2, _omitFieldNames ? '' : 'metadata')
    ..hasRequiredFields = false;

  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ControlMessage clone() => deepCopy();
  @$core.Deprecated('See https://github.com/google/protobuf.dart/issues/998.')
  ControlMessage copyWith(void Function(ControlMessage) updates) =>
      super.copyWith((message) => updates(message as ControlMessage))
          as ControlMessage;

  @$core.override
  $pb.BuilderInfo get info_ => _i;

  @$core.pragma('dart2js:noInline')
  static ControlMessage create() => ControlMessage._();
  @$core.override
  ControlMessage createEmptyInstance() => create();
  @$core.pragma('dart2js:noInline')
  static ControlMessage getDefault() => _defaultInstance ??=
      $pb.GeneratedMessage.$_defaultFor<ControlMessage>(create);
  static ControlMessage? _defaultInstance;

  @$pb.TagNumber(1)
  ControlMessage_Type get type => $_getN(0);
  @$pb.TagNumber(1)
  set type(ControlMessage_Type value) => $_setField(1, value);
  @$pb.TagNumber(1)
  $core.bool hasType() => $_has(0);
  @$pb.TagNumber(1)
  void clearType() => $_clearField(1);

  @$pb.TagNumber(2)
  $core.String get metadata => $_getSZ(1);
  @$pb.TagNumber(2)
  set metadata($core.String value) => $_setString(1, value);
  @$pb.TagNumber(2)
  $core.bool hasMetadata() => $_has(1);
  @$pb.TagNumber(2)
  void clearMetadata() => $_clearField(2);
}

const $core.bool _omitFieldNames =
    $core.bool.fromEnvironment('protobuf.omit_field_names');
const $core.bool _omitMessageNames =
    $core.bool.fromEnvironment('protobuf.omit_message_names');
