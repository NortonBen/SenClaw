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

import 'package:protobuf/protobuf.dart' as $pb;

class ControlMessage_Type extends $pb.ProtobufEnum {
  static const ControlMessage_Type PING =
      ControlMessage_Type._(0, _omitEnumNames ? '' : 'PING');
  static const ControlMessage_Type PONG =
      ControlMessage_Type._(1, _omitEnumNames ? '' : 'PONG');
  static const ControlMessage_Type ACK =
      ControlMessage_Type._(2, _omitEnumNames ? '' : 'ACK');
  static const ControlMessage_Type TYPING_START =
      ControlMessage_Type._(3, _omitEnumNames ? '' : 'TYPING_START');
  static const ControlMessage_Type TYPING_STOP =
      ControlMessage_Type._(4, _omitEnumNames ? '' : 'TYPING_STOP');
  static const ControlMessage_Type DISCONNECT =
      ControlMessage_Type._(5, _omitEnumNames ? '' : 'DISCONNECT');
  static const ControlMessage_Type AGENT_LIST_REQ =
      ControlMessage_Type._(6, _omitEnumNames ? '' : 'AGENT_LIST_REQ');
  static const ControlMessage_Type AGENT_LIST_RESP =
      ControlMessage_Type._(7, _omitEnumNames ? '' : 'AGENT_LIST_RESP');
  static const ControlMessage_Type AGENT_SELECT =
      ControlMessage_Type._(8, _omitEnumNames ? '' : 'AGENT_SELECT');
  static const ControlMessage_Type HISTORY_REQ =
      ControlMessage_Type._(9, _omitEnumNames ? '' : 'HISTORY_REQ');
  static const ControlMessage_Type HISTORY_RESP =
      ControlMessage_Type._(10, _omitEnumNames ? '' : 'HISTORY_RESP');

  static const $core.List<ControlMessage_Type> values = <ControlMessage_Type>[
    PING, PONG, ACK, TYPING_START, TYPING_STOP, DISCONNECT,
    AGENT_LIST_REQ, AGENT_LIST_RESP, AGENT_SELECT, HISTORY_REQ, HISTORY_RESP,
  ];

  static final $core.List<ControlMessage_Type?> _byValue =
      $pb.ProtobufEnum.$_initByValueList(values, 10);
  static ControlMessage_Type? valueOf($core.int value) =>
      value < 0 || value >= _byValue.length ? null : _byValue[value];

  const ControlMessage_Type._(super.value, super.name);
}

const $core.bool _omitEnumNames =
    $core.bool.fromEnvironment('protobuf.omit_enum_names');
