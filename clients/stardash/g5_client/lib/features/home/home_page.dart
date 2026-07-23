import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:intl/intl.dart';


import '../../core/api/api_exception.dart';
import '../../core/models/user_profile.dart';
import '../../providers/app_providers.dart';
import '../../providers/vpn_providers.dart';
import '../../theme/g5_theme_extension.dart';
import '../../widgets/country_flag_icon.dart';
import '../../widgets/language_switcher.dart';
import '../../widgets/theme_switcher.dart';
import '../../widgets/web3/glass_card.dart';
import '../../widgets/web3/web3_background.dart';
import '../../widgets/clay/clay_surface.dart';
import '../../l10n/app_localizations.dart';
import '../../l10n/plan_catalog_l10n.dart';
import '../plans/plans_page.dart';
import '../vpn/vpn_panel.dart';

enum _MobileTab { home, nodes, plans, profile }

/// 手机 VPN 客户端布局：底部 Tab + 居中内容区。
class HomePage extends ConsumerStatefulWidget {
  const HomePage({super.key});

  @override
  ConsumerState<HomePage> createState() => _HomePageState();
}

class _HomePageState extends ConsumerState<HomePage> {
  _MobileTab _tab = _MobileTab.home;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      await ensureSelectedNode(ref);
      await tryAutoConnect(ref);
    });
  }

  Future<void> _logout() async {
    try {
      await ref.read(vpnConnectProvider.notifier).disconnect();
    } catch (_) {}
    await ref.read(authNotifierProvider.notifier).logout();
  }

  void _refreshAll() {
    ref.invalidate(userProfileProvider);
    ref.invalidate(subscribeInfoProvider);
    ref.invalidate(singboxProfileProvider);
  }

  @override
  Widget build(BuildContext context) {
    final userAsync = ref.watch(userProfileProvider);

    return Web3Background(
      child: userAsync.when(
        loading: () => const Scaffold(
          backgroundColor: Colors.transparent,
          body: Center(child: CircularProgressIndicator()),
        ),
        error: (e, _) {
          if (e is AuthRequiredException) {
            return const Scaffold(
              backgroundColor: Colors.transparent,
              body: Center(child: CircularProgressIndicator()),
            );
          }
          final msg = e is ApiException ? e.message : e.toString();
          final l10n = context.l10n;
          return Scaffold(
            backgroundColor: Colors.transparent,
            body: Center(
              child: Padding(
                padding: const EdgeInsets.all(24),
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(l10n.loadFailed(msg), textAlign: TextAlign.center),
                    const SizedBox(height: 16),
                    FilledButton(
                      onPressed: _refreshAll,
                      child: Text(l10n.retry),
                    ),
                  ],
                ),
              ),
            ),
          );
        },
        data: (user) => _MobileShell(
          tab: _tab,
          onTabChanged: (t) => setState(() => _tab = t),
          onRefresh: _refreshAll,
          child: switch (_tab) {
            _MobileTab.home => _MobileHomeTab(
                onOpenNodes: () => setState(() => _tab = _MobileTab.nodes),
              ),
            _MobileTab.nodes => NodesPage(
                onBuyPlan: () => setState(() => _tab = _MobileTab.plans),
              ),
            _MobileTab.plans => const PlansPage(),
            _MobileTab.profile => _ProfileTab(
                user: user,
                onRefresh: _refreshAll,
                onLogout: _logout,
              ),
          },
        ),
      ),
    );
  }
}

class _MobileShell extends StatelessWidget {
  const _MobileShell({
    required this.tab,
    required this.onTabChanged,
    required this.onRefresh,
    required this.child,
  });

  final _MobileTab tab;
  final ValueChanged<_MobileTab> onTabChanged;
  final VoidCallback onRefresh;
  final Widget child;

  String _title(AppLocalizations l10n) => switch (tab) {
        _MobileTab.home => l10n.titleHome,
        _MobileTab.nodes => l10n.titleNodes,
        _MobileTab.plans => l10n.titlePlans,
        _MobileTab.profile => l10n.titleProfile,
      };

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final c = context.g5;
    return Scaffold(
      backgroundColor: Colors.transparent,
      appBar: AppBar(
        toolbarHeight: 44,
        title: tab == _MobileTab.home
            ? null
            : Text(_title(l10n)),
        centerTitle: false,
        actions: [
          const ThemeSwitcherButton(),
          const LanguageSwitcherButton(),
          IconButton(
            tooltip: l10n.refresh,
            onPressed: onRefresh,
            icon: Icon(
              Icons.refresh_rounded,
              color: c.textSecondary.withValues(alpha: 0.9),
            ),
          ),
        ],
      ),
      body: child,
      bottomNavigationBar: Padding(
        padding: const EdgeInsets.fromLTRB(16, 0, 16, 10),
        child: ClaySurface(
          borderRadius: 20,
          depth: 5,
          padding: const EdgeInsets.symmetric(vertical: 2),
          child: NavigationBar(
            selectedIndex: tab.index,
            onDestinationSelected: (i) => onTabChanged(_MobileTab.values[i]),
            destinations: [
              NavigationDestination(
                icon: const Icon(Icons.grid_view_rounded),
                selectedIcon: const Icon(Icons.grid_view_rounded),
                label: l10n.tabHome,
              ),
              NavigationDestination(
                icon: const Icon(Icons.language_rounded),
                selectedIcon: const Icon(Icons.language_rounded),
                label: l10n.tabNodes,
              ),
              NavigationDestination(
                icon: const Icon(Icons.auto_awesome_outlined),
                selectedIcon: const Icon(Icons.auto_awesome_rounded),
                label: l10n.tabPlans,
              ),
              NavigationDestination(
                icon: const Icon(Icons.person_outline_rounded),
                selectedIcon: const Icon(Icons.person_rounded),
                label: l10n.tabProfile,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _MobileHomeTab extends ConsumerWidget {
  const _MobileHomeTab({
    required this.onOpenNodes,
  });

  final VoidCallback onOpenNodes;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final isDesktop =
        Platform.isWindows || Platform.isLinux || Platform.isMacOS;

    if (isDesktop) {
      return Padding(
        padding: const EdgeInsets.fromLTRB(16, 4, 16, 12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            const Flexible(
              flex: 5,
              child: Center(child: MobileConnectHero(compact: true)),
            ),
            _CurrentNodeCard(onTap: onOpenNodes),
            const SizedBox(height: 10),
            const MobileModePanel(),
          ],
        ),
      );
    }

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          flex: 2,
          child: Center(child: MobileConnectHero()),
        ),
        Flexible(
          flex: 3,
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                _CurrentNodeCard(onTap: onOpenNodes),
                const SizedBox(height: 10),
                const MobileModePanel(),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _CurrentNodeCard extends ConsumerWidget {
  const _CurrentNodeCard({required this.onTap});

  final VoidCallback onTap;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final c = context.g5;
    final selected = ref.watch(selectedNodeTagProvider);
    final autoSelect = ref.watch(autoSelectNodeEnabledProvider);
    final profile = ref.watch(singboxProfileProvider).valueOrNull;
    final node = selected == null
        ? null
        : profile?.nodes.where((n) => n.tag == selected).firstOrNull;

    return GlassCard(
      onTap: onTap,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Row(
        children: [
          ClaySurface(
            style: ClayStyle.convex,
            borderRadius: 14,
            depth: 4,
            width: 44,
            height: 44,
            child: Center(
              child: CountryFlagIcon(countryCode: node?.countryCode, size: 28),
            ),
          ),
          const SizedBox(width: 14),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  l10n.currentNode,
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                if (autoSelect)
                  Padding(
                    padding: const EdgeInsets.only(top: 2),
                    child: Text(
                      l10n.auto,
                      style: Theme.of(context).textTheme.bodySmall?.copyWith(
                            color: c.primary,
                            fontWeight: FontWeight.w600,
                          ),
                    ),
                  ),
                if (!autoSelect) const SizedBox(height: 2),
                Text(
                  node?.tag ?? selected ?? l10n.tapSelectNode,
                  style: Theme.of(context).textTheme.titleMedium,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                if (node != null)
                  Text(
                    node.countryName,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: c.primary,
                        ),
                  ),
              ],
            ),
          ),
          Icon(Icons.chevron_right_rounded, color: c.textDim),
        ],
      ),
    );
  }
}

class _TrafficCard extends StatelessWidget {
  const _TrafficCard({required this.user});

  final UserProfile user;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final c = context.g5;
    final expire = user.expireAt != null
        ? DateTime.tryParse(user.expireAt!)
        : null;
    final expireStr = expire != null
        ? DateFormat('yyyy-MM-dd').format(expire.toLocal())
        : '—';

    return GlassCard(
      padding: const EdgeInsets.all(2),
      child: Column(
        children: [
          _InfoRow(
            label: l10n.plan,
            value: (user.planName != null && user.planName!.trim().isNotEmpty)
                ? l10n.localizePlanName(user.planName)
                : l10n.none,
          ),
          const Divider(height: 1, indent: 16, endIndent: 16),
          _InfoRow(label: l10n.expire, value: expireStr),
          const Divider(height: 1, indent: 16, endIndent: 16),
          _InfoRow(
            label: l10n.balance,
            value: l10n.balanceYuan(user.balance),
          ),
          if (!user.unlimited) ...[
            const Divider(height: 1, indent: 16, endIndent: 16),
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 10, 16, 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(l10n.traffic, style: Theme.of(context).textTheme.bodyMedium),
                      Text(
                        l10n.trafficUsage(user.usedGb, user.totalGb),
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(6),
                    child: LinearProgressIndicator(
                      value: user.usagePercent,
                      minHeight: 5,
                      backgroundColor: c.surfaceHigh,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _InfoRow extends StatelessWidget {
  const _InfoRow({required this.label, required this.value});

  final String label;
  final String value;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 9),
      child: Row(
        children: [
          Text(label, style: Theme.of(context).textTheme.bodyMedium),
          const Spacer(),
          Text(
            value,
            style: Theme.of(context).textTheme.titleMedium?.copyWith(
                  fontWeight: FontWeight.w500,
                ),
          ),
        ],
      ),
    );
  }
}

class _ProfileTab extends ConsumerWidget {
  const _ProfileTab({
    required this.user,
    required this.onRefresh,
    required this.onLogout,
  });

  final UserProfile user;
  final VoidCallback onRefresh;
  final VoidCallback onLogout;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final c = context.g5;

    return ListView(
      padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
      children: [
        _TrafficCard(user: user),
        const SizedBox(height: 12),
        GlassCard(
          padding: EdgeInsets.zero,
          child: ListTile(
            leading: const Icon(Icons.email_outlined),
            title: Text(user.email),
            subtitle: Text(
              user.active ? l10n.accountActive : l10n.accountDisabled,
              style: TextStyle(
                color: user.active ? c.success : c.danger,
                fontWeight: FontWeight.w500,
              ),
            ),
          ),
        ),
        const SizedBox(height: 12),
        GlassCard(
          padding: EdgeInsets.zero,
          child: Column(
            children: [
              ListTile(
                leading: Icon(Icons.refresh_rounded, color: c.primary),
                title: Text(l10n.refreshData),
                trailing: const Icon(Icons.chevron_right_rounded),
                onTap: onRefresh,
              ),
              const Divider(height: 1, indent: 56),
              ListTile(
                leading: Icon(Icons.logout_rounded, color: c.danger),
                title: Text(l10n.logout, style: TextStyle(color: c.danger)),
                onTap: onLogout,
              ),
            ],
          ),
        ),
      ],
    );
  }
}
