import 'dart:io';

import 'android_vpn_controller.dart';
import 'ios_vpn_controller.dart';
import 'linux_vpn_controller.dart';
import 'stub_vpn_controller.dart';
import 'vpn_controller.dart';

VpnController createVpnController() {
  if (Platform.isWindows) {
    return DesktopVpnController();
  }
  if (Platform.isLinux) {
    return LinuxVpnController();
  }
  if (Platform.isAndroid) {
    return AndroidVpnController();
  }
  if (Platform.isIOS) {
    return IosVpnController();
  }
  return StubVpnController();
}
