/// Windows 桌面 VPN 内核（rsbox / sing-box A/B 测试）。
import 'vpn_mode.dart';

enum WindowsVpnKernel {
  rsbox,
  singbox;

  String get exeName => switch (this) {
        WindowsVpnKernel.rsbox => 'rsbox.exe',
        WindowsVpnKernel.singbox => 'sing-box.exe',
      };

  String get assetPath => 'assets/binaries/windows/$exeName';

  String get logPrefix => switch (this) {
        WindowsVpnKernel.rsbox => 'rsbox',
        WindowsVpnKernel.singbox => 'sing-box',
      };

  String get displayLabel => switch (this) {
        WindowsVpnKernel.rsbox => 'rsbox',
        WindowsVpnKernel.singbox => 'sing-box',
      };

  /// 系统代理默认 rsbox；TUN 固定 sing-box。
  static const systemProxyDefault = WindowsVpnKernel.rsbox;

  static WindowsVpnKernel effective(VpnMode mode) =>
      forMode(mode, systemProxyDefault);

  /// 全局 TUN 模式固定使用 sing-box（wintun + auto_route），rsbox 仅用于系统代理。
  static WindowsVpnKernel forMode(VpnMode mode, WindowsVpnKernel preferred) {
    if (mode == VpnMode.tun) return WindowsVpnKernel.singbox;
    return preferred;
  }
}
