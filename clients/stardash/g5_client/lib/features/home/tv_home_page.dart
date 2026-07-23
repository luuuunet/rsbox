import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/api/api_exception.dart';
import '../../core/models/user_profile.dart';
import '../../providers/app_providers.dart';
import '../../providers/vpn_providers.dart';
import '../../theme/g5_theme_extension.dart';
import '../../widgets/language_switcher.dart';
import '../../widgets/theme_switcher.dart';
import '../../widgets/web3/app_sidebar.dart';
import '../../widgets/web3/web3_background.dart';
import '../../l10n/app_localizations.dart';
import '../vpn/vpn_panel.dart';

/// Android TV / 电视盒子布局：左侧导航 + 大屏内容，支持遥控器方向键。
class TvHomePage extends ConsumerStatefulWidget {
  const TvHomePage({super.key});

  @override
  ConsumerState<TvHomePage> createState() => _TvHomePageState();
}

class _TvHomePageState extends ConsumerState<TvHomePage> {
  DashboardSection _section = DashboardSection.overview;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      await ensureSelectedNode(ref);
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
        data: (user) => Scaffold(
          backgroundColor: Colors.transparent,
          body: Row(
            children: [
              AppSidebar(
                selected: _section,
                email: user.email,
                onSelected: (s) => setState(() => _section = s),
                onRefresh: _refreshAll,
                onLogout: _logout,
              ),
              Expanded(
                child: Column(
                  children: [
                    Padding(
                      padding: const EdgeInsets.fromLTRB(24, 16, 24, 0),
                      child: Row(
                        children: [
                          Text(
                            _section == DashboardSection.overview
                                ? context.l10n.titleHome
                                : context.l10n.titleNodes,
                            style: Theme.of(context).textTheme.headlineSmall,
                          ),
                          const Spacer(),
                          const ThemeSwitcherButton(),
                          const LanguageSwitcherButton(),
                        ],
                      ),
                    ),
                    Expanded(
                      child: FocusTraversalGroup(
                        policy: OrderedTraversalPolicy(),
                        child: Padding(
                          padding: const EdgeInsets.all(24),
                          child: _section == DashboardSection.overview
                              ? _TvOverviewPane(user: user)
                              : const NodesPage(),
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _TvOverviewPane extends ConsumerWidget {
  const _TvOverviewPane({required this.user});

  final UserProfile user;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final c = context.g5;

    return ListView(
      children: [
        Text(
          user.email,
          style: Theme.of(context).textTheme.titleMedium,
        ),
        const SizedBox(height: 8),
        Text(
          '${l10n.plan}: ${user.planName ?? '-'} · ${l10n.expire}: ${user.expireAt ?? '-'}',
          style: Theme.of(context).textTheme.bodyMedium?.copyWith(color: c.textDim),
        ),
        const SizedBox(height: 24),
        const MobileConnectHero(),
        const SizedBox(height: 16),
        const MobileModePanel(),
      ],
    );
  }
}
