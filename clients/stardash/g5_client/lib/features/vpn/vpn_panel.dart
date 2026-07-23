import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/platform/platform_support.dart';
import '../../core/api/api_exception.dart';
import '../../widgets/country_flag_icon.dart';
import '../../core/models/proxy_node.dart';
import '../../core/vpn/bandwidth_format.dart';
import '../../core/vpn/node_speed_test.dart';
import '../../core/vpn/vpn_controller.dart';
import '../../core/vpn/vpn_mode.dart';
import '../../core/vpn/vpn_user_message.dart';
import '../../l10n/app_localizations.dart';
import '../../providers/vpn_providers.dart';
import '../../theme/g5_theme_extension.dart';
import '../../widgets/admin_required_dialog.dart';
import '../../widgets/smooth_bandwidth_progress.dart';
import '../../widgets/web3/glass_card.dart';
import '../../widgets/clay/clay_surface.dart';

class NodesPage extends ConsumerStatefulWidget {
  const NodesPage({super.key, this.onBuyPlan});

  final VoidCallback? onBuyPlan;

  @override
  ConsumerState<NodesPage> createState() => _NodesPageState();
}

class _NodesPageState extends ConsumerState<NodesPage> {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      await ensureSelectedNode(ref);
    });
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final profileAsync = ref.watch(singboxProfileProvider);
    final selected = ref.watch(selectedNodeTagProvider);
    final testing = ref.watch(nodeTestProvider.select(
          (m) => m.values.contains(NodeLatency.testing),
        )) ||
        ref.watch(nodeBandwidthProvider.select(
          (m) => m.values.contains(NodeBandwidth.testing),
        )) ||
        ref.watch(nodeBandwidthProgressProvider.select((m) => m.isNotEmpty));

    return profileAsync.when(
      loading: () => const Center(child: CircularProgressIndicator()),
      error: (e, _) {
        if (e is AuthRequiredException) {
          return const Center(child: CircularProgressIndicator());
        }
        final msg = e is ApiException ? e.message : e.toString();
        return Center(
          child: Padding(
            padding: const EdgeInsets.all(24),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(l10n.loadNodesFailed(msg), textAlign: TextAlign.center),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: () => ref.invalidate(singboxProfileProvider),
                  child: Text(l10n.retry),
                ),
              ],
            ),
          ),
        );
      },
      data: (profile) {
        if (profile.nodes.isEmpty) {
          return Center(
            child: Padding(
              padding: const EdgeInsets.all(24),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    l10n.noNodes,
                    style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w600),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    l10n.buyPlanHint,
                    style: Theme.of(context).textTheme.bodyMedium,
                    textAlign: TextAlign.center,
                  ),
                  if (widget.onBuyPlan != null) ...[
                    const SizedBox(height: 20),
                    FilledButton.icon(
                      onPressed: widget.onBuyPlan,
                      icon: const Icon(Icons.shopping_bag_outlined),
                      label: Text(l10n.buyPlan),
                    ),
                  ],
                ],
              ),
            ),
          );
        }

        return Padding(
          padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _AutoSelectNodeCard(nodes: profile.nodes),
              const SizedBox(height: 10),
              GlassCard(
                padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
                borderRadius: 16,
                child: Row(
                  children: [
                    Text(
                      l10n.nodesCount(profile.nodes.length),
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                    const Spacer(),
                    if (widget.onBuyPlan != null)
                      TextButton.icon(
                        onPressed: widget.onBuyPlan,
                        icon: const Icon(Icons.shopping_bag_outlined, size: 18),
                        label: Text(l10n.buyPlan),
                      ),
                    TextButton.icon(
                      onPressed: testing
                          ? null
                          : () => ref
                              .read(nodeTestProvider.notifier)
                              .testAll(profile.nodes),
                      icon: testing
                          ? const SizedBox(
                              width: 16,
                              height: 16,
                              child: CircularProgressIndicator(strokeWidth: 2),
                            )
                          : const Icon(Icons.speed, size: 18),
                      label: Text(testing ? l10n.testing : l10n.testAll),
                    ),
                    IconButton(
                      tooltip: l10n.refreshSubscribe,
                      onPressed: () => ref.invalidate(singboxProfileProvider),
                      icon: const Icon(Icons.refresh),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 12),
              Expanded(
                child: ListView.separated(
                  itemCount: profile.nodes.length,
                  separatorBuilder: (_, __) => const SizedBox(height: 10),
                  itemBuilder: (context, index) {
                  final node = profile.nodes[index];
                  return _NodeTile(
                    node: node,
                    selected: node.tag == selected,
                    onSelect: () {
                      ref.read(selectedNodeTagProvider.notifier).state =
                          node.tag;
                    },
                    onTest: () =>
                        ref.read(nodeTestProvider.notifier).testOne(node),
                    onConnect: () => _connectNode(context, node.tag),
                    onDisconnect: () => _disconnectNode(context, node.tag),
                  );
                  },
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  Future<void> _connectNode(BuildContext context, String tag) async {
    try {
      final mode = ref.read(vpnModeProvider);
      if (!await ensureAdminForTunMode(context, ref, mode)) return;
      await ref.read(vpnConnectProvider.notifier).connectTo(tag);
    } catch (e) {
      if (!context.mounted) return;
      if (e is StateError && e.message == kTunRequiresAdmin) {
        await showTunAdminRequiredDialog(context, ref);
        return;
      }
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.l10n.connectFailedMsg(VpnUserMessage.fromError(e)))),
      );
    }
  }

  Future<void> _disconnectNode(BuildContext context, String tag) async {
    try {
      await ref.read(vpnConnectProvider.notifier).disconnectIfNode(tag);
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.l10n.disconnectFailed)),
        );
      }
    }
  }
}

class _AutoSelectNodeCard extends ConsumerWidget {
  const _AutoSelectNodeCard({required this.nodes});

  final List<ProxyNode> nodes;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final c = context.g5;
    final enabled = ref.watch(autoSelectNodeEnabledProvider);
    final testing = ref
        .watch(nodeTestProvider)
        .values
        .contains(NodeLatency.testing);

    return GlassCard(
      padding: const EdgeInsets.fromLTRB(4, 0, 8, 0),
      child: SwitchListTile(
        contentPadding: const EdgeInsets.symmetric(horizontal: 8),
        title: Text(l10n.autoSelectNode),
        subtitle: Text(
          enabled ? l10n.autoSelectOn : l10n.autoSelectOff,
          style: Theme.of(context).textTheme.bodySmall,
        ),
        value: enabled,
        activeTrackColor: c.primary.withValues(alpha: 0.45),
        onChanged: testing
            ? null
            : (value) async {
                await ref
                    .read(autoSelectNodeEnabledProvider.notifier)
                    .setEnabled(value);
                if (value && context.mounted) {
                  await ref
                      .read(autoSelectNodeServiceProvider)
                      .selectBest(forceRetest: true);
                  if (context.mounted) {
                    final tag = ref.read(selectedNodeTagProvider);
                    ScaffoldMessenger.of(context).showSnackBar(
                      SnackBar(
                        content: Text(
                          tag != null
                              ? l10n.autoSelected(tag)
                              : l10n.autoSelectFailed,
                        ),
                      ),
                    );
                  }
                }
              },
        secondary: Icon(
          enabled ? Icons.auto_fix_high_rounded : Icons.touch_app_outlined,
          color: enabled ? c.primary : c.textDim,
        ),
      ),
    );
  }
}

class _NodeTile extends ConsumerWidget {
  const _NodeTile({
    required this.node,
    required this.selected,
    required this.onSelect,
    required this.onTest,
    required this.onConnect,
    required this.onDisconnect,
  });

  final ProxyNode node;
  final bool selected;
  final VoidCallback onSelect;
  final VoidCallback onTest;
  final VoidCallback onConnect;
  final VoidCallback onDisconnect;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final c = context.g5;
    final ms = ref.watch(nodeTestProvider.select((m) => m[node.tag]));
    final bw = ref.watch(nodeBandwidthProvider.select((m) => m[node.tag]));
    final bandwidthProgress = ref.watch(
      nodeBandwidthProgressProvider.select((m) => m[node.tag]),
    );
    final latencyTesting = ms == NodeLatency.testing;
    final bandwidthTesting =
        bw == NodeBandwidth.testing || bandwidthProgress != null;
    final latencyLabel = switch (ms) {
      null => l10n.speedTest,
      NodeLatency.testing => '…',
      NodeLatency.timeout => l10n.timeout,
      _ => '${ms}ms',
    };
    final bandwidthLabel = _formatBandwidthMbps(bw);
    final testLabel = bandwidthLabel.isNotEmpty && ms != null && ms > 0
        ? '$latencyLabel · $bandwidthLabel'
        : latencyLabel;

    final vpnBusy = ref.watch(vpnConnectProvider).isLoading;
    final status = ref.watch(vpnStatusProvider).valueOrNull ??
        ref.read(vpnControllerProvider).currentStatus;
    final connectedHere =
        status.isConnected && status.selectedTag == node.tag;
    final connectingHere =
        vpnBusy && ref.watch(selectedNodeTagProvider) == node.tag;

    return GlassCard(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 14),
      borderRadius: 18,
      depth: selected || connectedHere ? 7 : 5,
      glowColor: selected || connectedHere ? c.primary : null,
      inset: false,
      onTap: onSelect,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              CountryFlagIcon(countryCode: node.countryCode, size: 36),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Flexible(
                          child: Text(
                            node.tag,
                            style: Theme.of(context).textTheme.titleMedium,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        if (selected)
                          Padding(
                            padding: const EdgeInsets.only(left: 6),
                            child: Icon(
                              Icons.check_circle_rounded,
                              size: 18,
                              color: c.primary,
                            ),
                          ),
                      ],
                    ),
                    if (node.countryName.isNotEmpty) ...[
                      const SizedBox(height: 2),
                      Text(
                        node.countryName,
                        style: Theme.of(context).textTheme.bodySmall,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ],
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              if (bandwidthTesting)
                Expanded(
                  child: SmoothBandwidthProgress(
                    progress: bandwidthProgress?.progress ?? 0,
                    liveRateMbps: bandwidthProgress?.liveRateMbps,
                    label: ms != null && ms > 0
                        ? latencyLabel
                        : l10n.bandwidthTesting,
                  ),
                )
              else if (latencyTesting)
                const SizedBox(
                  width: 28,
                  height: 28,
                  child: Padding(
                    padding: EdgeInsets.all(5),
                    child: CircularProgressIndicator(strokeWidth: 2),
                  ),
                )
              else
                TextButton.icon(
                  onPressed: onTest,
                  icon: const Icon(Icons.speed, size: 16),
                  label: Text(testLabel),
                  style: TextButton.styleFrom(
                    visualDensity: VisualDensity.compact,
                    padding: const EdgeInsets.symmetric(horizontal: 8),
                  ),
                ),
              const Spacer(),
              if (connectingHere)
                const SizedBox(
                  width: 32,
                  height: 32,
                  child: CircularProgressIndicator(strokeWidth: 2),
                )
              else if (connectedHere)
                FilledButton.tonal(
                  onPressed: vpnBusy ? null : onDisconnect,
                  style: FilledButton.styleFrom(
                    backgroundColor: c.danger.withValues(alpha: 0.12),
                    foregroundColor: c.danger,
                    visualDensity: VisualDensity.compact,
                  ),
                  child: Text(l10n.disconnect),
                )
              else
                FilledButton(
                  onPressed: vpnBusy ? null : onConnect,
                  style: FilledButton.styleFrom(
                    visualDensity: VisualDensity.compact,
                    padding: const EdgeInsets.symmetric(horizontal: 20),
                  ),
                  child: Text(l10n.connect),
                ),
            ],
          ),
        ],
      ),
    );
  }
}

/// 手机首页大圆钮连接区。
class MobileConnectHero extends ConsumerStatefulWidget {
  const MobileConnectHero({super.key, this.compact = false});

  final bool compact;

  @override
  ConsumerState<MobileConnectHero> createState() => _MobileConnectHeroState();
}

class _MobileConnectHeroState extends ConsumerState<MobileConnectHero> {
  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final c = context.g5;
    final vpnAsync = ref.watch(vpnConnectProvider);
    final status = ref.watch(vpnStatusProvider).valueOrNull ??
        ref.read(vpnControllerProvider).currentStatus;
    final connected = status.isConnected;
    final busy = vpnAsync.isLoading;
    final speedState = ref.watch(connectedSpeedTestProvider);
    final speedTesting = speedState.isRunning;
    final speedRate = speedState.resultRateMbps;

    final ringColor = switch (status.state) {
      VpnConnectionState.connected => c.success,
      VpnConnectionState.connecting => c.warning,
      VpnConnectionState.error => c.danger,
      VpnConnectionState.disconnected => c.primary,
    };

    Future<void> toggle() async {
      try {
        if (connected) {
          await ref.read(vpnConnectProvider.notifier).disconnect();
        } else {
          if (ref.read(selectedNodeTagProvider)?.isEmpty ?? true) {
            await ensureSelectedNode(ref);
          }
          final mode = ref.read(vpnModeProvider);
          if (!await ensureAdminForTunMode(context, ref, mode)) return;
          await ref.read(vpnConnectProvider.notifier).connect();
        }
      } catch (e) {
        if (!context.mounted) return;
        if (e is StateError && e.message == kTunRequiresAdmin) {
          await showTunAdminRequiredDialog(context, ref);
          return;
        }
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(l10n.connectFailedMsg(VpnUserMessage.fromError(e)))),
        );
      }
    }

    final statusText = switch (status.state) {
      VpnConnectionState.connected => l10n.connected,
      VpnConnectionState.connecting => l10n.connecting,
      VpnConnectionState.error => status.message ?? l10n.connectFailed,
      VpnConnectionState.disconnected => l10n.tapConnect,
    };

    final compact = widget.compact;
    final outer = compact ? 148.0 : 168.0;
    final midOuter = compact ? 132.0 : 148.0;
    final midInner = compact ? 110.0 : 124.0;
    final inner = compact ? 82.0 : 92.0;
    final iconSize = compact ? 36.0 : 40.0;

    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        Material(
          color: Colors.transparent,
          child: InkWell(
            onTap: busy ? null : toggle,
            customBorder: const CircleBorder(),
            child: SizedBox(
              width: outer,
              height: outer,
              child: Stack(
                alignment: Alignment.center,
                children: [
                  ClaySurface(
                    circle: true,
                    width: midOuter,
                    height: midOuter,
                    depth: 6,
                    style: ClayStyle.convex,
                    child: const SizedBox.expand(),
                  ),
                  ClaySurface(
                    circle: true,
                    width: midInner,
                    height: midInner,
                    style: ClayStyle.concave,
                    child: const SizedBox.expand(),
                  ),
                  ClaySurface(
                    circle: true,
                    width: inner,
                    height: inner,
                    depth: connected ? 3 : 6,
                    style: connected ? ClayStyle.concave : ClayStyle.convex,
                    accent: ringColor,
                    child: Center(
                      child: busy
                          ? SizedBox(
                              width: 28,
                              height: 28,
                              child: CircularProgressIndicator(
                                strokeWidth: 2.5,
                                color: ringColor,
                              ),
                            )
                          : Icon(
                              connected
                                  ? Icons.shield_rounded
                                  : Icons.power_settings_new_rounded,
                              size: iconSize,
                              color: ringColor,
                            ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
        SizedBox(height: compact ? 8 : 10),
        Text(
          statusText,
          style: Theme.of(context).textTheme.titleMedium?.copyWith(
                letterSpacing: -0.2,
              ),
        ),
        if (connected && status.selectedTag != null) ...[
          const SizedBox(height: 4),
          Text(
            status.selectedTag!,
            style: Theme.of(context).textTheme.bodyMedium,
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 8),
          if (speedTesting)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 24),
              child: SmoothBandwidthProgress(
                progress: speedState.progress,
                liveRateMbps: speedState.liveRateMbps,
                label: l10n.bandwidthTesting,
              ),
            )
          else
            TextButton.icon(
              onPressed: busy
                  ? null
                  : () => ref.read(connectedSpeedTestProvider.notifier).run(),
              icon: const Icon(Icons.download_rounded, size: 18),
              label: Text(
                speedRate != null && speedRate > 0
                    ? l10n.downloadSpeedMbps(
                        BandwidthFormat.fromDownloadRate(speedRate),
                      )
                    : l10n.bandwidthTest,
              ),
            ),
        ],
      ],
    );
  }
}

/// 手机首页模式切换。
class MobileModePanel extends ConsumerWidget {
  const MobileModePanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final c = context.g5;
    final mode = ref.watch(vpnModeProvider);
    final isAdmin = ref.watch(isAdminProvider).valueOrNull;
    final autoConnect = ref.watch(autoConnectEnabledProvider);
    final autoReconnect = ref.watch(vpnAutoReconnectEnabledProvider);
    final connected = ref.watch(vpnStatusProvider).valueOrNull?.isConnected ??
        ref.read(vpnControllerProvider).currentStatus.isConnected;
    final connecting = ref.watch(vpnConnectProvider).isLoading;

    final modes = PlatformSupport.availableVpnModes;
    final showModePicker = modes.length > 1;

    return GlassCard(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          if (showModePicker) ...[
            Text(l10n.connectMode, style: Theme.of(context).textTheme.titleSmall),
            const SizedBox(height: 8),
            ClaySurface(
              style: ClayStyle.concave,
              borderRadius: 12,
              padding: const EdgeInsets.all(3),
              child: SegmentedButton<VpnMode>(
                segments: modes
                    .map(
                      (m) => ButtonSegment(
                        value: m,
                        label: Text(l10n.vpnModeLabel(m.kind)),
                        icon: Icon(
                          m == VpnMode.systemProxy
                              ? Icons.settings_ethernet
                              : Icons.vpn_lock,
                        ),
                      ),
                    )
                    .toList(),
                selected: {mode},
                onSelectionChanged: connected
                    ? null
                    : (value) async {
                        final newMode = value.first;
                        if (newMode == mode) return;
                        if (!await ensureAdminForTunMode(context, ref, newMode)) {
                          return;
                        }
                        ref.read(vpnModeProvider.notifier).setMode(newMode);
                      },
              ),
            ),
            const SizedBox(height: 8),
            Text(
              _vpnModeDescription(l10n, mode),
              style: Theme.of(context).textTheme.bodySmall,
            ),
            if (mode == VpnMode.tun &&
                PlatformSupport.supportsTunMode &&
                (Platform.isWindows || Platform.isLinux) &&
                isAdmin == false) ...[
              const SizedBox(height: 6),
              Text(
                l10n.tunNeedAdminHint,
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: c.warning,
                    ),
              ),
            ],
            const Divider(height: 20),
          ] else if (Platform.isAndroid || Platform.isIOS) ...[
            Text(l10n.connectMode, style: Theme.of(context).textTheme.titleSmall),
            const SizedBox(height: 6),
            Text(
              Platform.isIOS ? l10n.vpnModeTunIosDesc : l10n.vpnModeTunAndroidDesc,
              style: Theme.of(context).textTheme.bodySmall,
            ),
            const Divider(height: 20),
          ],
          // 系统代理 → rsbox；全局 TUN → sing-box（自动）
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            dense: true,
            title: Text(l10n.autoConnect),
            subtitle: Text(
              autoConnect ? l10n.autoConnectOn : l10n.autoConnectOff,
              style: Theme.of(context).textTheme.bodySmall,
            ),
            value: autoConnect,
            activeTrackColor: c.primary.withValues(alpha: 0.45),
            onChanged: connecting
                ? null
                : (value) async {
                    await ref
                        .read(autoConnectEnabledProvider.notifier)
                        .setEnabled(value);
                    if (value && context.mounted) {
                      final status =
                          ref.read(vpnControllerProvider).currentStatus;
                      if (!status.isConnected &&
                          status.state != VpnConnectionState.connecting) {
                        try {
                          final mode = ref.read(vpnModeProvider);
                          if (!await ensureAdminForTunMode(context, ref, mode)) {
                            return;
                          }
                          await ref.read(vpnConnectProvider.notifier).connect();
                        } catch (e) {
                          if (!context.mounted) return;
                          if (e is StateError && e.message == kTunRequiresAdmin) {
                            await showTunAdminRequiredDialog(context, ref);
                            return;
                          }
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                              content: Text(context.l10n.connectFailedMsg(VpnUserMessage.fromError(e))),
                            ),
                          );
                        }
                      }
                    }
                  },
            secondary: Icon(
              autoConnect ? Icons.bolt_rounded : Icons.bolt_outlined,
              color: autoConnect ? c.primary : c.textDim,
            ),
          ),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            dense: true,
            title: Text(l10n.autoReconnect),
            subtitle: Text(
              autoReconnect ? l10n.autoReconnectOn : l10n.autoReconnectOff,
              style: Theme.of(context).textTheme.bodySmall,
            ),
            value: autoReconnect,
            activeTrackColor: c.primary.withValues(alpha: 0.45),
            onChanged: connecting
                ? null
                : (value) => ref
                    .read(vpnAutoReconnectEnabledProvider.notifier)
                    .setEnabled(value),
            secondary: Icon(
              autoReconnect ? Icons.sync_rounded : Icons.sync_disabled_rounded,
              color: autoReconnect ? c.primary : c.textDim,
            ),
          ),
        ],
      ),
    );
  }
}

String _vpnModeDescription(AppLocalizations l10n, VpnMode mode) {
  if (Platform.isIOS && mode == VpnMode.tun) {
    return l10n.vpnModeTunIosDesc;
  }
  if (Platform.isAndroid && mode == VpnMode.tun) {
    return l10n.vpnModeTunAndroidDesc;
  }
  return l10n.vpnModeDesc(mode.kind);
}

String _formatBandwidthMbps(double? mbps) {
  if (mbps == null) return '';
  if (mbps == NodeBandwidth.testing) return '';
  if (mbps == NodeBandwidth.failed) return '';
  return BandwidthFormat.fromDownloadRate(mbps);
}
