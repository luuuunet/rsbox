import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../core/platform/platform_support.dart';
import '../features/auth/login_page.dart';
import '../features/auth/register_page.dart';
import '../features/auth/two_factor_page.dart';
import '../features/home/home_page.dart';
import '../features/home/tv_home_page.dart';
import '../providers/app_providers.dart';

final _rootKey = GlobalKey<NavigatorState>();

/// 登录态变化时通知 GoRouter 重新执行 redirect。
class AuthRedirectNotifier extends ChangeNotifier {
  AuthRedirectNotifier(this.ref) {
    ref.listen<String?>(sessionTokenProvider, (_, __) => notifyListeners());
    ref.listen(authBootstrapProvider, (_, __) => notifyListeners());
  }

  final Ref ref;

  bool get loggedIn {
    final session = ref.read(sessionTokenProvider);
    if (session != null && session.isNotEmpty) return true;
    return ref.read(authBootstrapProvider).valueOrNull == true;
  }

  bool get ready => ref.read(authBootstrapProvider).hasValue;
}

final authRedirectNotifierProvider = Provider<AuthRedirectNotifier>((ref) {
  final notifier = AuthRedirectNotifier(ref);
  ref.onDispose(notifier.dispose);
  return notifier;
});

final appRouterProvider = Provider<GoRouter>((ref) {
  final auth = ref.watch(authRedirectNotifierProvider);

  return GoRouter(
    navigatorKey: _rootKey,
    initialLocation: '/login',
    refreshListenable: auth,
    redirect: (context, state) {
      final path = state.matchedLocation;
      if (!auth.ready) return null;

      final loggedIn = auth.loggedIn;

      if (!loggedIn && path != '/login' && path != '/2fa' && path != '/register') {
        return '/login';
      }
      if (loggedIn && (path == '/login' || path == '/2fa' || path == '/register')) {
        return '/home';
      }
      return null;
    },
    routes: [
      GoRoute(
        path: '/login',
        builder: (_, __) => const LoginPage(),
      ),
      GoRoute(
        path: '/register',
        builder: (_, state) => RegisterPage(
          initialInviteCode: state.uri.queryParameters['invite'],
        ),
      ),
      GoRoute(
        path: '/2fa',
        builder: (_, state) {
          final challenge = state.extra as String? ?? '';
          return TwoFactorPage(challengeToken: challenge);
        },
      ),
      GoRoute(
        path: '/home',
        builder: (_, __) =>
            PlatformSupport.isTv ? const TvHomePage() : const HomePage(),
      ),
    ],
  );
});
