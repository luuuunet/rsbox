import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'app.dart';
import 'core/platform/platform_support.dart';
import 'core/vpn/ios_vpn_runtime.dart';
import 'core/vpn/linux_port_helper.dart';
import 'core/vpn/windows_port_helper.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  if (Platform.isWindows) {
    await WindowsPortHelper.prepareForVpnConnect();
  } else if (Platform.isLinux) {
    await LinuxPortHelper.prepareForVpnConnect();
  }
  if (Platform.isAndroid) {
    await PlatformSupport.detectFormFactor();
  }
  if (Platform.isIOS) {
    await IosVpnRuntime.instance.ensureInitialized();
  }
  runApp(const ProviderScope(child: G5ClientApp()));
}
