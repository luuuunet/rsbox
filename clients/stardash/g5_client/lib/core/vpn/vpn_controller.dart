import 'dart:async';
import 'dart:io';

import 'singbox_config_builder.dart';
import 'vpn_mode.dart';
import 'vpn_reconnect_coordinator.dart';
import 'vpn_user_message.dart';
import '../platform/windows_admin.dart';
import 'windows_port_helper.dart';
import 'windows_singbox_runner.dart';
import 'windows_system_proxy.dart';
import 'windows_vpn_kernel.dart';

/// Thrown when TUN mode is selected but the client is not elevated.
const kTunRequiresAdmin = 'TUN_REQUIRES_ADMIN';

enum VpnConnectionState {
  disconnected,
  connecting,
  connected,
  error,
}

class VpnStatus {
  const VpnStatus({
    required this.state,
    this.message,
    this.selectedTag,
    this.mode,
  });

  final VpnConnectionState state;
  final String? message;
  final String? selectedTag;
  final VpnMode? mode;

  bool get isConnected => state == VpnConnectionState.connected;
}

abstract class VpnController {
  Stream<VpnStatus> get statusStream;
  VpnStatus get currentStatus;
  bool get isConnected;

  /// 系统代理模式下的 mixed 入站端口；TUN / 移动端为 null。
  int? get activeMixedPort;

  Future<void> connect({
    required Map<String, dynamic> baseConfig,
    required String selectedTag,
    required VpnMode mode,
    bool allowAutoReconnect = true,
  });

  Future<void> stop();

  void dispose();
}

class DesktopVpnController implements VpnController {
  DesktopVpnController({WindowsVpnKernel kernel = WindowsVpnKernel.rsbox})
      : _runner = WindowsSingboxRunner(kernel: kernel),
        _status = const VpnStatus(state: VpnConnectionState.disconnected);

  final WindowsSingboxRunner _runner;

  WindowsVpnKernel get windowsKernel => _runner.kernel;

  set windowsKernel(WindowsVpnKernel kernel) => _runner.kernel = kernel;

  /// 进程异常退出时由 Provider 注入，走统一 [VpnReconnectCoordinator] 重连。
  Future<void> Function()? onAutoReconnect;

  final _statusController = StreamController<VpnStatus>.broadcast();
  VpnStatus _status;
  VpnMode? _activeMode;
  int? _activeMixedPort;

  // 自动重连配置
  bool _autoReconnect = true;
  int _reconnectAttempts = 0;
  static const _maxReconnectAttempts = 3;
  Timer? _reconnectTimer;
  int _sessionGeneration = 0;
  String? _lastSelectedTag;
  VpnMode? _lastMode;

  @override
  Stream<VpnStatus> get statusStream => _statusController.stream;

  @override
  VpnStatus get currentStatus => _status;

  @override
  bool get isConnected => _status.isConnected;

  @override
  int? get activeMixedPort => _activeMixedPort;

  void _emit(VpnStatus status) {
    _status = status;
    if (!_statusController.isClosed) {
      _statusController.add(status);
    }
  }

  @override
  Future<void> connect({
    required Map<String, dynamic> baseConfig,
    required String selectedTag,
    required VpnMode mode,
    bool allowAutoReconnect = true,
  }) async {
    if (!Platform.isWindows) {
      throw UnsupportedError('当前平台 VPN 尚未实现');
    }

    if (mode == VpnMode.tun) {
      final admin = await WindowsAdmin.isRunningAsAdmin();
      if (!admin) {
        throw StateError(kTunRequiresAdmin);
      }
    }

    // 保存配置用于自动重连
    _lastSelectedTag = selectedTag;
    _lastMode = mode;
    _reconnectAttempts = 0;
    _autoReconnect = allowAutoReconnect;
    _sessionGeneration++;
    _reconnectTimer?.cancel();

    _emit(VpnStatus(
      state: VpnConnectionState.connecting,
      selectedTag: selectedTag,
      mode: mode,
    ));

    try {
      final binDir = await _runner.ensureBinaries();
      await _startSingboxWithPortRetry(
        binDir: binDir,
        baseConfig: baseConfig,
        selectedTag: selectedTag,
        mode: mode,
      );

      if (mode == VpnMode.systemProxy && _activeMixedPort != null) {
        await WindowsSystemProxy.enable('127.0.0.1', _activeMixedPort!);
      }

      _activeMode = mode;
      _emit(VpnStatus(
        state: VpnConnectionState.connected,
        message: '已连接 · $selectedTag',
        selectedTag: selectedTag,
        mode: mode,
      ));
    } catch (e) {
      await _cleanup();
      _emit(VpnStatus(
        state: VpnConnectionState.error,
        message: _userFacingError(e),
        selectedTag: selectedTag,
        mode: mode,
      ));
      rethrow;
    }
  }

  Future<void> _startSingboxWithPortRetry({
    required Directory binDir,
    required Map<String, dynamic> baseConfig,
    required String selectedTag,
    required VpnMode mode,
  }) async {
    final triedPorts = <int>{};
    Object? lastError;

    for (var attempt = 0; attempt < 6; attempt++) {
      int? mixedPort;
      try {
        if (mode == VpnMode.systemProxy) {
          mixedPort = await WindowsPortHelper.findAndReserveMixedPort(
            exclude: triedPorts,
          );
          triedPorts.add(mixedPort);
        }
        _activeMixedPort = mixedPort;
        final kernel = WindowsVpnKernel.effective(mode);
        final configJson = SingboxConfigBuilder.build(
          baseConfig: baseConfig,
          selectedTag: selectedTag,
          mode: mode,
          mixedListenPort: mixedPort ?? SingboxConfigBuilder.mixedPort,
          windowsKernel: kernel,
        );
        _runner.onProcessExit = (_, detail) {
          unawaited(_onSingboxExit(detail));
        };
        await _runner.start(
          binDir: binDir,
          configJson: configJson,
          mixedPort: mixedPort,
          mode: mode,
          kernel: kernel,
        );
        return;
      } catch (e) {
        lastError = e;
        await _cleanup();
        if (!WindowsPortHelper.isPortBindError(e) || attempt >= 5) {
          rethrow;
        }
        await WindowsPortHelper.releaseMixedReservation();
        if (mixedPort != null) {
          await WindowsPortHelper.releasePort(mixedPort, onlySingbox: true);
        }
        await WindowsPortHelper.killTrackedVpnCores();
      }
    }

    throw lastError ?? StateError('sing-box 启动失败');
  }

  Future<void> _onSingboxExit(String detail) async {
    if (_status.state != VpnConnectionState.connected) return;

    if (!_autoReconnect ||
        _reconnectAttempts >= _maxReconnectAttempts ||
        _lastSelectedTag == null ||
        onAutoReconnect == null) {
      await _cleanup();
      _emit(VpnStatus(
        state: VpnConnectionState.error,
        message: detail.isNotEmpty ? detail : 'VPN 已断开，请手动重新连接',
        selectedTag: _status.selectedTag,
        mode: _status.mode,
      ));
      return;
    }

    _reconnectAttempts++;
    await _cleanup();

    final gen = _sessionGeneration;
    _emit(VpnStatus(
      state: VpnConnectionState.connecting,
      message: '连接断开，正在自动重连 ($_reconnectAttempts/$_maxReconnectAttempts)...',
      selectedTag: _lastSelectedTag,
      mode: _lastMode,
    ));

    _reconnectTimer?.cancel();
    _reconnectTimer = Timer(const Duration(seconds: 2), () async {
      if (!_autoReconnect || gen != _sessionGeneration) return;

      final ran = await VpnReconnectCoordinator.run(() async {
        if (!_autoReconnect || gen != _sessionGeneration) return;
        await onAutoReconnect!.call();
      });

      if (!ran && _autoReconnect && gen == _sessionGeneration) {
        _emit(VpnStatus(
          state: VpnConnectionState.error,
          message: detail.isNotEmpty ? detail : 'VPN 已断开，自动重连繁忙，请稍后手动连接',
          selectedTag: _lastSelectedTag,
          mode: _lastMode,
        ));
      }
    });
  }

  static String userFacingError(Object e) => VpnUserMessage.fromError(e);

  static String _userFacingError(Object e) => userFacingError(e);

  @override
  Future<void> stop() async {
    _autoReconnect = false;
    _sessionGeneration++;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    _reconnectAttempts = 0;
    VpnReconnectCoordinator.resetCooldown();
    _lastSelectedTag = null;
    _lastMode = null;

    await _cleanup();
    _activeMode = null;
    _emit(const VpnStatus(state: VpnConnectionState.disconnected));
  }

  Future<void> _cleanup() async {
    if (_activeMode == VpnMode.systemProxy || _activeMode == null) {
      try {
        await WindowsSystemProxy.disable();
      } catch (_) {}
    }
    await WindowsPortHelper.releaseMixedReservation();
    await _runner.stop(mixedPort: _activeMixedPort, mode: _activeMode);
    _activeMixedPort = null;
  }

  @override
  void dispose() {
    _autoReconnect = false;
    _reconnectTimer?.cancel();
    unawaited(_cleanup());
    if (Platform.isWindows) {
      unawaited(WindowsPortHelper.killTrackedVpnCores());
    }
    _statusController.close();
  }
}
