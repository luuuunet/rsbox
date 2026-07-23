import 'dart:io';

import 'package:flutter/services.dart';

/// Whether the current Android ABI has a bundled libbox native library.
abstract final class AndroidLibboxSupport {
  static const _channel = MethodChannel('com.g5panel.g5_client/device');

  static bool? _cached;

  static Future<bool> isAvailable() async {
    if (!Platform.isAndroid) return false;
    if (_cached != null) return _cached!;
    try {
      _cached = await _channel.invokeMethod<bool>('libboxNativeAvailable') ?? false;
    } catch (_) {
      _cached = false;
    }
    return _cached!;
  }
}
