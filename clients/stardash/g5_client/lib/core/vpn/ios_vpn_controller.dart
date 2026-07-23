import 'dart:async';
import 'dart:convert';

import 'package:singbox_mm/singbox_mm.dart' as sb;

import 'ios_vpn_runtime.dart';
import 'singbox_config_builder.dart';
import 'vpn_controller.dart';
import 'vpn_mode.dart';

/// iOS VPN via singbox_mm + PacketTunnel + Libbox.
class IosVpnController implements VpnController {
  IosVpnController() {
    _status = const VpnStatus(state: VpnConnectionState.disconnected);
    _stateSub = IosVpnRuntime.instance.vpn.stateStream.listen(_onRuntimeState);
  }

  final _runtime = IosVpnRuntime.instance;
  final _statusController = StreamController<VpnStatus>.broadcast();
  StreamSubscription<sb.VpnConnectionState>? _stateSub;

  VpnStatus _status = const VpnStatus(state: VpnConnectionState.disconnected);
  String? _pendingTag;
  VpnMode? _pendingMode;

  @override
  Stream<VpnStatus> get statusStream => _statusController.stream;

  @override
  VpnStatus get currentStatus => _status;

  @override
  bool get isConnected => _status.isConnected;

  @override
  int? get activeMixedPort => null;

  void _emit(VpnStatus status) {
    _status = status;
    if (!_statusController.isClosed) {
      _statusController.add(status);
    }
  }

  void _onRuntimeState(sb.VpnConnectionState state) {
    switch (state) {
      case sb.VpnConnectionState.connected:
        _emit(
          VpnStatus(
            state: VpnConnectionState.connected,
            message: _pendingTag != null ? '已连接 · $_pendingTag' : '已连接',
            selectedTag: _pendingTag,
            mode: _pendingMode ?? VpnMode.tun,
          ),
        );
      case sb.VpnConnectionState.connecting:
      case sb.VpnConnectionState.preparing:
        _emit(
          VpnStatus(
            state: VpnConnectionState.connecting,
            selectedTag: _pendingTag,
            mode: _pendingMode,
          ),
        );
      case sb.VpnConnectionState.disconnecting:
        _emit(
          VpnStatus(
            state: VpnConnectionState.connecting,
            message: '断开中…',
            selectedTag: _pendingTag,
            mode: _pendingMode,
          ),
        );
      case sb.VpnConnectionState.error:
        _emit(
          VpnStatus(
            state: VpnConnectionState.error,
            message: '连接失败',
            selectedTag: _pendingTag,
            mode: _pendingMode,
          ),
        );
      case sb.VpnConnectionState.disconnected:
        _emit(const VpnStatus(state: VpnConnectionState.disconnected));
    }
  }

  @override
  Future<void> connect({
    required Map<String, dynamic> baseConfig,
    required String selectedTag,
    required VpnMode mode,
    bool allowAutoReconnect = true,
  }) async {
    final effectiveMode = VpnMode.tun;
    _pendingTag = selectedTag;
    _pendingMode = effectiveMode;

    _emit(
      VpnStatus(
        state: VpnConnectionState.connecting,
        selectedTag: selectedTag,
        mode: effectiveMode,
      ),
    );

    try {
      await _runtime.ensureInitialized();

      if (!await _runtime.ensureVpnPermission()) {
        throw StateError('需要 VPN 权限，请在系统设置中允许');
      }

      final configJson = SingboxConfigBuilder.build(
        baseConfig: baseConfig,
        selectedTag: selectedTag,
        mode: effectiveMode,
        forAndroid: true,
      );

      final configMap = _parseConfigJson(configJson);
      await _runtime.vpn.setRawConfig(
        configMap.map((k, v) => MapEntry(k, v as Object?)),
      );
      await _runtime.vpn.start();

      final runtimeState = await _runtime.vpn.getState();
      if (runtimeState == sb.VpnConnectionState.error) {
        final err = await _runtime.vpn.getLastError();
        throw StateError(err ?? 'sing-box 启动失败');
      }

      if (runtimeState == sb.VpnConnectionState.connected) {
        _emit(
          VpnStatus(
            state: VpnConnectionState.connected,
            message: '已连接 · $selectedTag',
            selectedTag: selectedTag,
            mode: effectiveMode,
          ),
        );
      }
    } catch (e) {
      await _safeStop();
      _emit(
        VpnStatus(
          state: VpnConnectionState.error,
          message: _userFacingError(e),
          selectedTag: selectedTag,
          mode: effectiveMode,
        ),
      );
      rethrow;
    }
  }

  Map<String, dynamic> _parseConfigJson(String configJson) {
    try {
      final decoded = jsonDecode(configJson);
      if (decoded is Map<String, dynamic>) {
        return decoded;
      }
      throw StateError('Invalid config format');
    } catch (e) {
      throw StateError('配置解析失败: $e');
    }
  }

  static String _userFacingError(Object e) {
    final text = e.toString();
    if (text.contains('PERMISSION_DENIED') || text.contains('VPN permission')) {
      return '需要 VPN 权限，请在系统设置中允许';
    }
    if (text.contains('Libbox') || text.contains('PacketTunnel')) {
      return 'iOS VPN 未完整配置，请确认 sing-box 库已正确集成';
    }
    return text.startsWith('StateError: ')
        ? text.substring('StateError: '.length)
        : text;
  }

  @override
  Future<void> stop() async {
    await _safeStop();
    _pendingTag = null;
    _pendingMode = null;
    _emit(const VpnStatus(state: VpnConnectionState.disconnected));
  }

  Future<void> _safeStop() async {
    try {
      await _runtime.vpn.stop();
    } catch (_) {}
  }

  @override
  void dispose() {
    _stateSub?.cancel();
    _statusController.close();
  }
}
