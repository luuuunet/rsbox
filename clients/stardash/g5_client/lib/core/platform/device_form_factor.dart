import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// Detects TV / leanback devices (Android TV, Fire TV, etc.).
class DeviceFormFactor {
  const DeviceFormFactor._();

  static const _channel = MethodChannel('com.g5panel.g5_client/device');

  static bool? _isTelevision;

  static Future<bool> get isTelevision async {
    if (!Platform.isAndroid) return false;
    if (_isTelevision != null) return _isTelevision!;
    try {
      _isTelevision =
          await _channel.invokeMethod<bool>('isTelevision') ?? false;
    } catch (_) {
      _isTelevision = false;
    }
    return _isTelevision!;
  }

  static bool get isLargeScreen {
    if (kIsWeb) return false;
    final views = PlatformDispatcher.instance.views;
    if (views.isEmpty) return false;
    final size = views.first.physicalSize / views.first.devicePixelRatio;
    return size.shortestSide >= 600;
  }
}
