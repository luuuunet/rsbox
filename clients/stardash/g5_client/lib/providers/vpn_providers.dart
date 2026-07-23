import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../core/api/api_exception.dart';
import '../core/models/proxy_node.dart';
import '../core/platform/android_libbox_support.dart';
import '../core/platform/linux_admin.dart';
import '../core/platform/windows_admin.dart';
import '../core/vpn/bandwidth_test_live_state.dart';
import '../core/vpn/node_speed_test.dart';
import '../core/vpn/singbox_config_builder.dart';
import '../core/vpn/singbox_config_parser.dart';
import '../core/vpn/singbox_test_route.dart';
import '../core/vpn/vpn_bandwidth_test.dart';
import '../core/vpn/vpn_connection_watchdog.dart';
import '../core/vpn/vpn_reconnect_coordinator.dart';
import '../core/vpn/vpn_controller.dart';
import '../core/vpn/linux_vpn_controller.dart';
import '../core/vpn/vpn_controller_factory.dart';
import '../core/vpn/vpn_mode.dart';
import '../core/vpn/windows_vpn_kernel.dart';
import 'app_providers.dart';

final vpnControllerProvider = Provider<VpnController>((ref) {
  final controller = createVpnController();
  final reconnect = () async {
    await ref.read(vpnConnectProvider.notifier).reconnect();
  };
  if (controller is DesktopVpnController) {
    controller.onAutoReconnect = reconnect;
  } else if (controller is LinuxVpnController) {
    controller.onAutoReconnect = reconnect;
  }
  ref.onDispose(controller.dispose);
  return controller;
});

final vpnStatusProvider = StreamProvider<VpnStatus>((ref) {
  return ref.watch(vpnControllerProvider).statusStream;
});

final vpnModeProvider =
    StateNotifierProvider<VpnModeNotifier, VpnMode>((ref) => VpnModeNotifier());

const _prefVpnMode = 'vpn_mode';

class VpnModeNotifier extends StateNotifier<VpnMode> {
  VpnModeNotifier() : super(_defaultVpnMode) {
    _load();
  }

  static VpnMode get _defaultVpnMode {
    if (Platform.isWindows || Platform.isLinux) return VpnMode.tun;
    if (Platform.isAndroid || Platform.isIOS) return VpnMode.tun;
    return VpnMode.systemProxy;
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    final stored = prefs.getString(_prefVpnMode);
    if (stored == null) return;
    state = switch (stored) {
      'tun' => VpnMode.tun,
      'systemProxy' => VpnMode.systemProxy,
      _ => state,
    };
  }

  Future<void> setMode(VpnMode mode) async {
    state = mode;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString(_prefVpnMode, mode.name);
  }
}

WindowsVpnKernel _windowsKernelFor(Ref ref) =>
    WindowsVpnKernel.effective(ref.read(vpnModeProvider));

final isAdminProvider = FutureProvider<bool>((ref) async {
  if (Platform.isWindows) return WindowsAdmin.isRunningAsAdmin();
  if (Platform.isLinux) return LinuxAdmin.isRoot();
  if (Platform.isAndroid || Platform.isIOS) return true;
  return false;
});

final selectedNodeTagProvider = StateProvider<String?>((ref) => null);

final singboxProfileProvider = FutureProvider<SingboxProfile>((ref) async {
  final token = ref.watch(sessionTokenProvider);
  if (token == null || token.isEmpty) {
    throw const AuthRequiredException();
  }
  final sub = await ref.watch(subscribeInfoProvider.future);
  final url = sub.singbox;
  if (url == null || url.isEmpty) {
    throw StateError('未获取到 sing-box 订阅 URL');
  }
  final result = await ref.read(apiClientProvider).fetchSingboxProfile(url);
  return SingboxConfigParser.parse(result.body);
});

class VpnConnectNotifier extends StateNotifier<AsyncValue<void>> {
  VpnConnectNotifier(this.ref) : super(const AsyncData(null));

  final Ref ref;

  Future<void> connect() async {
    if (ref.read(autoSelectNodeEnabledProvider)) {
      await ref.read(autoSelectNodeServiceProvider).selectBest();
    }
    final tag = ref.read(selectedNodeTagProvider);
    if (tag == null || tag.isEmpty) {
      throw StateError('请先选择节点');
    }
    await _connectTag(tag);
  }

  /// 选中指定节点并连接；已连接同一节点则忽略；已连接其他节点则先断开再连。
  Future<void> connectTo(String tag) async {
    ref.read(selectedNodeTagProvider.notifier).state = tag;
    final status = ref.read(vpnControllerProvider).currentStatus;
    if (status.isConnected && status.selectedTag == tag) {
      return;
    }
    if (status.isConnected) {
      await ref.read(vpnControllerProvider).stop();
    }
    await _connectTag(tag);
  }

  /// 若当前正连接该节点，则断开。
  Future<void> disconnectIfNode(String tag) async {
    final status = ref.read(vpnControllerProvider).currentStatus;
    if (status.isConnected && status.selectedTag == tag) {
      await disconnect();
    }
  }

  Future<void> _connectTag(String tag) async {
    await _connectTagOnce(tag, allowRefreshRetry: true);
  }

  Future<void> _connectTagOnce(String tag, {required bool allowRefreshRetry}) async {
    var profile = await ref.read(singboxProfileProvider.future);
    var resolved = SingboxConfigBuilder.resolveSelectedTag(profile.rawConfig, tag);
    if (resolved != tag) {
      ref.read(selectedNodeTagProvider.notifier).state = resolved;
    }
    final mode = ref.read(vpnModeProvider);
    final allowAutoReconnect = ref.read(vpnAutoReconnectEnabledProvider);
    state = const AsyncLoading();
    try {
      await ref.read(vpnControllerProvider).connect(
            baseConfig: profile.rawConfig,
            selectedTag: resolved,
            mode: mode,
            allowAutoReconnect: allowAutoReconnect,
          );
      final prefs = await SharedPreferences.getInstance();
      await prefs.setString(_prefSelectedNodeTag, resolved);
      state = const AsyncData(null);
    } catch (e, st) {
      if (allowRefreshRetry && _isStaleNodeError(e)) {
        ref.invalidate(singboxProfileProvider);
        try {
          profile = await ref.read(singboxProfileProvider.future);
          resolved = SingboxConfigBuilder.resolveSelectedTag(
            profile.rawConfig,
            resolved,
          );
          ref.read(selectedNodeTagProvider.notifier).state = resolved;
          await ref.read(vpnControllerProvider).connect(
                baseConfig: profile.rawConfig,
                selectedTag: resolved,
                mode: mode,
                allowAutoReconnect: allowAutoReconnect,
              );
          final prefs = await SharedPreferences.getInstance();
          await prefs.setString(_prefSelectedNodeTag, resolved);
          state = const AsyncData(null);
          return;
        } catch (retryError, retrySt) {
          state = AsyncError(retryError, retrySt);
          rethrow;
        }
      }
      state = AsyncError(e, st);
      rethrow;
    }
  }

  bool _isStaleNodeError(Object e) {
    final msg = e.toString();
    return msg.contains('不存在') ||
        msg.contains('outbound not found') ||
        msg.contains('default outbound not found') ||
        msg.contains('无可用节点');
  }

  Future<void> disconnect() async {
    state = const AsyncLoading();
    try {
      await ref.read(vpnControllerProvider).stop();
      ref.read(connectedSpeedTestProvider.notifier).reset();
      state = const AsyncData(null);
    } catch (e, st) {
      state = AsyncError(e, st);
      rethrow;
    }
  }

  /// 保活探测失败时自动重连（不断开用户选择的节点）。
  Future<void> reconnect() async {
    final tag = ref.read(vpnControllerProvider).currentStatus.selectedTag;
    if (tag == null || tag.isEmpty) return;
    if (state.isLoading) return;

    await VpnReconnectCoordinator.run(() async {
      if (state.isLoading) return;
      try {
        await ref.read(vpnControllerProvider).stop();
      } catch (_) {}
      await _connectTagOnce(tag, allowRefreshRetry: true);
    });
  }
}

final vpnConnectProvider =
    StateNotifierProvider<VpnConnectNotifier, AsyncValue<void>>(
  (ref) => VpnConnectNotifier(ref),
);

SingboxTestRoute? vpnReuseTestRoute(Ref ref) {
  final controller = ref.read(vpnControllerProvider);
  final status = controller.currentStatus;
  final tag = status.selectedTag;
  if (!status.isConnected || tag == null || tag.isEmpty) return null;
  return SingboxTestRoute(
    connectedTag: tag,
    proxyPort: controller.activeMixedPort,
  );
}

class NodeTestNotifier extends StateNotifier<Map<String, int>> {
  NodeTestNotifier(this.ref) : super(const {});

  final Ref ref;
  bool _running = false;

  bool get isRunning => _running;

  Future<void> testAll(List<ProxyNode> nodes) async {
    if (_running || nodes.isEmpty) return;
    _running = true;
    state = {for (final n in nodes) n.tag: NodeLatency.testing};
    try {
      final baseConfig = await _loadBaseConfig();
      final accumulated = <String, int>{};
      await NodeSpeedTest.measureAll(
        nodes,
        baseConfig: baseConfig,
        windowsKernel: Platform.isWindows ? _windowsKernelFor(ref) : null,
        onProgress: (done, total, tag, ms) {
          accumulated[tag] = ms;
          state = {
            for (final n in nodes)
              n.tag: accumulated[n.tag] ?? NodeLatency.testing,
          };
        },
      );
    } finally {
      _running = false;
    }
  }

  Future<void> testOne(ProxyNode node) async {
    if (_running) return;
    _running = true;
    try {
      state = {...state, node.tag: NodeLatency.testing};
      ref.read(nodeBandwidthProvider.notifier).markTesting(node.tag);
      ref.read(nodeBandwidthProgressProvider.notifier).start(node.tag);
      final baseConfig = await _loadBaseConfig();
      final reuseRoute = vpnReuseTestRoute(ref);
      final kernel =
          Platform.isWindows ? _windowsKernelFor(ref) : null;
      final ms = await NodeSpeedTest.measureLatency(
        node,
        baseConfig: baseConfig,
        reuseRoute: reuseRoute,
        windowsKernel: kernel,
      );
      state = {...state, node.tag: ms};
      final mbps = await NodeSpeedTest.measureBandwidth(
        node,
        baseConfig: baseConfig,
        reuseRoute: reuseRoute,
        windowsKernel: kernel,
        onProgress: (snap) => ref
            .read(nodeBandwidthProgressProvider.notifier)
            .update(node.tag, snap),
      );
      ref.read(nodeBandwidthProgressProvider.notifier).clear(node.tag);
      ref.read(nodeBandwidthProvider.notifier).setResult(node.tag, mbps);
    } finally {
      _running = false;
    }
  }

  Future<Map<String, dynamic>?> _loadBaseConfig() async {
    try {
      return (await ref.read(singboxProfileProvider.future)).rawConfig;
    } catch (_) {
      return null;
    }
  }
}

final nodeTestProvider =
    StateNotifierProvider<NodeTestNotifier, Map<String, int>>(
  (ref) => NodeTestNotifier(ref),
);

class NodeBandwidthNotifier extends StateNotifier<Map<String, double>> {
  NodeBandwidthNotifier() : super(const {});

  void markTesting(String tag) {
    state = {...state, tag: NodeBandwidth.testing};
  }

  void setResult(String tag, double? mbps) {
    state = {
      ...state,
      tag: mbps ?? NodeBandwidth.failed,
    };
  }
}

final nodeBandwidthProvider =
    StateNotifierProvider<NodeBandwidthNotifier, Map<String, double>>(
  (ref) => NodeBandwidthNotifier(),
);

class NodeBandwidthProgressNotifier
    extends StateNotifier<Map<String, BandwidthTestLiveState>> {
  NodeBandwidthProgressNotifier() : super(const {});

  void start(String tag) {
    state = {
      ...state,
      tag: const BandwidthTestLiveState(progress: 0),
    };
  }

  void update(String tag, BandwidthTestSnapshot snap) {
    state = {
      ...state,
      tag: BandwidthTestLiveState(
        progress: snap.progress.clamp(0.0, 1.0),
        liveRateMbps: snap.downloadRateMbps,
      ),
    };
  }

  void clear(String tag) {
    if (!state.containsKey(tag)) return;
    final next = Map<String, BandwidthTestLiveState>.from(state)..remove(tag);
    state = next;
  }
}

final nodeBandwidthProgressProvider = StateNotifierProvider<
    NodeBandwidthProgressNotifier, Map<String, BandwidthTestLiveState>>(
  (ref) => NodeBandwidthProgressNotifier(),
);

class ConnectedSpeedTestState {
  const ConnectedSpeedTestState({
    this.isRunning = false,
    this.progress = 0,
    this.liveRateMbps,
    this.resultRateMbps,
  });

  final bool isRunning;
  final double progress;
  final double? liveRateMbps;
  final double? resultRateMbps;

  static const idle = ConnectedSpeedTestState();
}

class ConnectedSpeedTestNotifier extends StateNotifier<ConnectedSpeedTestState> {
  ConnectedSpeedTestNotifier(this.ref) : super(ConnectedSpeedTestState.idle);

  final Ref ref;

  Future<void> run() async {
    final controller = ref.read(vpnControllerProvider);
    if (!controller.isConnected || state.isRunning) return;

    state = const ConnectedSpeedTestState(isRunning: true, progress: 0);
    try {
      final status = controller.currentStatus;
      final proxyPort = status.mode == VpnMode.systemProxy
          ? controller.activeMixedPort
          : null;
      final rate = await VpnBandwidthTest.measure(
        proxyPort: proxyPort,
        onProgress: (snap) {
          state = ConnectedSpeedTestState(
            isRunning: !snap.isComplete,
            progress: snap.progress,
            liveRateMbps:
                snap.isComplete ? null : snap.downloadRateMbps,
            resultRateMbps:
                snap.isComplete ? snap.downloadRateMbps : null,
          );
        },
      );
      state = ConnectedSpeedTestState(resultRateMbps: rate);
    } catch (_) {
      state = ConnectedSpeedTestState.idle;
    }
  }

  void reset() {
    state = ConnectedSpeedTestState.idle;
  }
}

final connectedSpeedTestProvider =
    StateNotifierProvider<ConnectedSpeedTestNotifier, ConnectedSpeedTestState>(
  (ref) => ConnectedSpeedTestNotifier(ref),
);

const _prefAutoSelectNode = 'auto_select_node';
const _prefAutoConnect = 'auto_connect';
const _prefAutoReconnect = 'vpn_auto_reconnect';
const _prefSelectedNodeTag = 'selected_node_tag';

class AutoSelectNodeNotifier extends StateNotifier<bool> {
  AutoSelectNodeNotifier() : super(false) {
    _load();
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    state = prefs.getBool(_prefAutoSelectNode) ?? false;
  }

  Future<void> setEnabled(bool enabled) async {
    state = enabled;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool(_prefAutoSelectNode, enabled);
  }
}

final autoSelectNodeEnabledProvider =
    StateNotifierProvider<AutoSelectNodeNotifier, bool>(
  (ref) => AutoSelectNodeNotifier(),
);

class AutoConnectNotifier extends StateNotifier<bool> {
  AutoConnectNotifier() : super(false) {
    _load();
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    state = prefs.getBool(_prefAutoConnect) ?? false;
  }

  Future<void> setEnabled(bool enabled) async {
    state = enabled;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool(_prefAutoConnect, enabled);
  }
}

final autoConnectEnabledProvider =
    StateNotifierProvider<AutoConnectNotifier, bool>(
  (ref) => AutoConnectNotifier(),
);

class VpnAutoReconnectNotifier extends StateNotifier<bool> {
  VpnAutoReconnectNotifier() : super(true) {
    _load();
  }

  Future<void> _load() async {
    final prefs = await SharedPreferences.getInstance();
    state = prefs.getBool(_prefAutoReconnect) ?? true;
  }

  Future<void> setEnabled(bool enabled) async {
    state = enabled;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool(_prefAutoReconnect, enabled);
  }
}

final vpnAutoReconnectEnabledProvider =
    StateNotifierProvider<VpnAutoReconnectNotifier, bool>(
  (ref) => VpnAutoReconnectNotifier(),
);

/// 连接期间每 45s 探测隧道；连续 2 次失败则重连（30s 冷却）。
final vpnConnectionWatchdogProvider = Provider<VpnConnectionWatchdog>((ref) {
  final watchdog = VpnConnectionWatchdog(
    isEnabled: () => ref.read(vpnAutoReconnectEnabledProvider),
    isConnected: () =>
        ref.read(vpnControllerProvider).currentStatus.isConnected,
    proxyPort: () {
      final status = ref.read(vpnControllerProvider).currentStatus;
      if (status.mode == VpnMode.systemProxy) {
        return ref.read(vpnControllerProvider).activeMixedPort;
      }
      return null;
    },
    onReconnect: () => ref.read(vpnConnectProvider.notifier).reconnect(),
  );
  ref.onDispose(watchdog.dispose);

  ref.listen<AsyncValue<VpnStatus>>(vpnStatusProvider, (_, next) {
    watchdog.bindConnected(next.valueOrNull?.isConnected ?? false);
  });

  ref.listen<bool>(vpnAutoReconnectEnabledProvider, (_, enabled) {
    watchdog.bindEnabled(enabled);
  });

  watchdog.bindConnected(ref.read(vpnControllerProvider).currentStatus.isConnected);

  return watchdog;
});

class AutoSelectNodeService {
  AutoSelectNodeService(this.ref);

  final Ref ref;

  Future<String?> pickBestTag({bool forceRetest = false}) async {
    final profile = await ref.read(singboxProfileProvider.future);
    final nodeList = profile.nodes;
    if (nodeList.isEmpty) return null;

    var latencies = ref.read(nodeTestProvider);
    final hasValid = latencies.entries.any(
      (e) => nodeList.any((n) => n.tag == e.key) && e.value > 0,
    );

    if (forceRetest || !hasValid) {
      await ref.read(nodeTestProvider.notifier).testAll(nodeList);
      latencies = ref.read(nodeTestProvider);
    }

    String? bestTag;
    var bestMs = 999999;
    for (final node in nodeList) {
      final ms = latencies[node.tag];
      if (ms != null && ms > 0 && ms < bestMs) {
        bestMs = ms;
        bestTag = node.tag;
      }
    }

    return bestTag ?? nodeList.first.tag;
  }

  Future<void> selectBest({bool forceRetest = false}) async {
    if (!ref.read(autoSelectNodeEnabledProvider)) return;

    try {
      final tag = await pickBestTag(forceRetest: forceRetest);
      if (tag == null || tag.isEmpty) return;

      ref.read(selectedNodeTagProvider.notifier).state = tag;
      final prefs = await SharedPreferences.getInstance();
      await prefs.setString(_prefSelectedNodeTag, tag);
    } catch (_) {}
  }
}

final autoSelectNodeServiceProvider = Provider<AutoSelectNodeService>(
  (ref) => AutoSelectNodeService(ref),
);

Future<void> restoreSelectedNode(WidgetRef ref) async {
  if (ref.read(autoSelectNodeEnabledProvider)) return;
  final prefs = await SharedPreferences.getInstance();
  final saved = prefs.getString(_prefSelectedNodeTag);
  if (saved != null && saved.isNotEmpty) {
    ref.read(selectedNodeTagProvider.notifier).state = saved;
  }
}

Future<void> ensureSelectedNode(WidgetRef ref) async {
  try {
    final profile = await ref.read(singboxProfileProvider.future);
    if (profile.nodes.isEmpty) return;

    if (ref.read(autoSelectNodeEnabledProvider)) {
      await ref.read(autoSelectNodeServiceProvider).selectBest();
      return;
    }

    await restoreSelectedNode(ref);
    var selected = ref.read(selectedNodeTagProvider);
    final nodeTags = profile.nodes.map((n) => n.tag).toSet();
    if (selected != null && !nodeTags.contains(selected)) {
      try {
        selected = SingboxConfigBuilder.resolveSelectedTag(
          profile.rawConfig,
          selected,
        );
        ref.read(selectedNodeTagProvider.notifier).state = selected;
        final prefs = await SharedPreferences.getInstance();
        await prefs.setString(_prefSelectedNodeTag, selected);
      } catch (_) {
        selected = null;
        ref.read(selectedNodeTagProvider.notifier).state = null;
      }
    }
    if (selected == null) {
      ref.read(selectedNodeTagProvider.notifier).state =
          profile.nodes.first.tag;
    }
  } catch (_) {}
}

/// 开启自动连接时，启动后尝试连接 VPN。
Future<void> tryAutoConnect(WidgetRef ref) async {
  if (!ref.read(autoConnectEnabledProvider)) return;

  final status = ref.read(vpnControllerProvider).currentStatus;
  if (status.isConnected ||
      status.state == VpnConnectionState.connecting) {
    return;
  }

  // x86_64 模拟器需额外 libbox.so；自动连接触发 native 初始化，缺少时会闪退。
  if (Platform.isAndroid && !await AndroidLibboxSupport.isAvailable()) {
    return;
  }

  try {
    await ref.read(vpnConnectProvider.notifier).connect();
  } catch (_) {}
}
