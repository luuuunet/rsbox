import 'dart:io';

import '../vpn/vpn_mode.dart';
import 'device_form_factor.dart';

enum G5Platform { windows, android, ios, macos, linux, other }

/// Cross-platform capability flags for G5 Client.
class PlatformSupport {
  const PlatformSupport._();

  static G5Platform get current {
    if (Platform.isWindows) return G5Platform.windows;
    if (Platform.isAndroid) return G5Platform.android;
    if (Platform.isIOS) return G5Platform.ios;
    if (Platform.isMacOS) return G5Platform.macos;
    if (Platform.isLinux) return G5Platform.linux;
    return G5Platform.other;
  }

  /// sing-box one-click connect: Windows / Linux / Android / iOS.
  static bool get vpnAvailable =>
      Platform.isWindows ||
      Platform.isLinux ||
      Platform.isAndroid ||
      Platform.isIOS;

  static bool get supportsTunMode =>
      Platform.isWindows ||
      Platform.isLinux ||
      Platform.isAndroid ||
      Platform.isIOS;

  static bool get supportsSystemProxyMode =>
      Platform.isWindows || Platform.isLinux;

  static bool get isTv => _isTv ?? false;
  static bool? _isTv;

  static Future<void> detectFormFactor() async {
    _isTv = await DeviceFormFactor.isTelevision;
  }

  static List<VpnMode> get availableVpnModes {
    if (Platform.isWindows || Platform.isLinux) {
      return const [VpnMode.tun, VpnMode.systemProxy];
    }
    if (Platform.isAndroid || Platform.isIOS) {
      return const [VpnMode.tun];
    }
    return const [];
  }

  static bool get isDesktop =>
      Platform.isWindows || Platform.isMacOS || Platform.isLinux;

  static bool get isMobile => Platform.isAndroid || Platform.isIOS;

  static String get displayName => switch (current) {
        G5Platform.windows => 'Windows',
        G5Platform.android => 'Android',
        G5Platform.ios => 'iOS',
        G5Platform.macos => 'macOS',
        G5Platform.linux => 'Linux',
        G5Platform.other => '当前平台',
      };
}
