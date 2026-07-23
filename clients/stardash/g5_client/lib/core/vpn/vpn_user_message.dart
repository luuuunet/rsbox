import 'vpn_controller.dart';
import 'windows_port_helper.dart';

/// 将连接异常转为用户可读文案。
abstract final class VpnUserMessage {
  static String fromError(Object error) {
    if (error is StateError) {
      if (error.message == kTunRequiresAdmin) {
        return '全局模式需要管理员权限，请以管理员身份运行或改用系统代理。';
      }
      if (error.message == kPortUnavailable) {
        return '找不到可用本地端口，请关闭 Clash 等其它代理软件后重试。';
      }
      final msg = error.message;
      if (msg.isNotEmpty) {
        return msg;
      }
    }
    if (WindowsPortHelper.isPortBindError(error)) {
      return '本地代理端口被占用，已尝试切换仍失败。请关闭 Clash / v2rayN 等代理后重试。';
    }
    final text = error.toString();
    if (text.startsWith('StateError: ')) {
      return text.substring('StateError: '.length);
    }
    if (text.startsWith('UnsupportedError: ')) {
      return text.substring('UnsupportedError: '.length);
    }
    return text;
  }
}
