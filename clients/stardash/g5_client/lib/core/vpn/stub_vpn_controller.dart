import 'dart:async';

import '../platform/platform_support.dart';
import 'vpn_controller.dart';
import 'vpn_mode.dart';

/// Placeholder VPN backend for platforms where sing-box integration is pending.
class StubVpnController implements VpnController {
  StubVpnController()
      : _status = const VpnStatus(state: VpnConnectionState.disconnected);

  final _statusController = StreamController<VpnStatus>.broadcast();
  VpnStatus _status;

  @override
  Stream<VpnStatus> get statusStream => _statusController.stream;

  @override
  VpnStatus get currentStatus => _status;

  @override
  bool get isConnected => _status.isConnected;

  @override
  int? get activeMixedPort => null;

  @override
  Future<void> connect({
    required Map<String, dynamic> baseConfig,
    required String selectedTag,
    required VpnMode mode,
    bool allowAutoReconnect = true,
  }) async {
    throw UnsupportedError(
      '${PlatformSupport.displayName} 平台 VPN 功能开发中。'
      '当前可使用：登录、套餐、节点列表与订阅预览。',
    );
  }

  @override
  Future<void> stop() async {
    _status = const VpnStatus(state: VpnConnectionState.disconnected);
    if (!_statusController.isClosed) {
      _statusController.add(_status);
    }
  }

  @override
  void dispose() {
    _statusController.close();
  }
}
