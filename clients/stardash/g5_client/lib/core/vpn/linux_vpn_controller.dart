import 'dart:async';
import 'dart:io';

import '../platform/linux_admin.dart';
import 'linux_port_helper.dart';
import 'linux_singbox_runner.dart';
import 'linux_system_proxy.dart';
import 'singbox_config_builder.dart';
import 'vpn_controller.dart';
import 'vpn_mode.dart';
import 'vpn_reconnect_coordinator.dart';
import 'vpn_user_message.dart';

/// Linux 桌面 VPN：sing-box 子进程 + gsettings 系统代理 / TUN（需 root）。
class LinuxVpnController implements VpnController {
  LinuxVpnController()
      : _status = const VpnStatus(state: VpnConnectionState.disconnected);

  final _runner = LinuxSingboxRunner();
  final _statusController = StreamController<VpnStatus>.broadcast();
  VpnStatus _status;
  VpnMode? _activeMode;
  int? _activeMixedPort;

  Future<void> Function()? onAutoReconnect;

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
    if (!Platform.isLinux) {
      throw UnsupportedError('LinuxVpnController 仅支持 Linux');
    }

    if (mode == VpnMode.tun) {
      final root = await LinuxAdmin.isRoot();
      if (!root) {
        throw StateError(kTunRequiresAdmin);
      }
    }

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
        try {
          await LinuxSystemProxy.enable('127.0.0.1', _activeMixedPort!);
        } catch (_) {
          // 无 gsettings 时仍可手动配置浏览器代理
        }
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
        message: VpnUserMessage.fromError(e),
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
          mixedPort = await LinuxPortHelper.findAndReserveMixedPort(
            exclude: triedPorts,
          );
          triedPorts.add(mixedPort);
        }
        _activeMixedPort = mixedPort;
        final configJson = SingboxConfigBuilder.build(
          baseConfig: baseConfig,
          selectedTag: selectedTag,
          mode: mode,
          mixedListenPort: mixedPort ?? SingboxConfigBuilder.mixedPort,
        );
        _runner.onProcessExit = (_, detail) {
          unawaited(_onSingboxExit(detail));
        };
        await _runner.start(
          binDir: binDir,
          configJson: configJson,
          mixedPort: mixedPort,
        );
        return;
      } catch (e) {
        lastError = e;
        await _cleanup();
        if (!LinuxPortHelper.isPortBindError(e) || attempt >= 5) {
          rethrow;
        }
        await LinuxPortHelper.releaseMixedReservation();
        if (mixedPort != null) {
          await LinuxPortHelper.releasePort(mixedPort, onlySingbox: true);
        }
        await LinuxPortHelper.killTrackedVpnCores();
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
        message: detail.isNotEmpty ? detail : 'sing-box 已意外退出，请重新连接',
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
        await LinuxSystemProxy.disable();
      } catch (_) {}
    }
    await LinuxPortHelper.releaseMixedReservation();
    await _runner.stop(mixedPort: _activeMixedPort);
    _activeMixedPort = null;
  }

  @override
  void dispose() {
    _autoReconnect = false;
    _reconnectTimer?.cancel();
    unawaited(_cleanup());
    if (Platform.isLinux) {
      unawaited(LinuxPortHelper.killTrackedVpnCores());
    }
    _statusController.close();
  }
}
