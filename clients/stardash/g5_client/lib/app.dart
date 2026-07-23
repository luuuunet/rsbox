import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'config/app_config.dart';
import 'l10n/app_localizations.dart';
import 'l10n/locale_options.dart';
import 'providers/app_providers.dart';
import 'providers/locale_provider.dart';
import 'providers/theme_provider.dart';
import 'router/app_router.dart';
import 'theme/app_theme.dart';
import 'theme/g5_theme_extension.dart';
import 'widgets/app_lifecycle_guard.dart';
import 'widgets/web3/web3_background.dart';

class G5ClientApp extends ConsumerWidget {
  const G5ClientApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final router = ref.watch(appRouterProvider);
    final boot = ref.watch(authBootstrapProvider);
    final locale = ref.watch(localeProvider);
    final themeMode = ref.watch(themeModeProvider);
    final l10n = AppLocalizations(locale);

    return AppLifecycleGuard(
      child: MaterialApp.router(
      title: AppConfig.appName,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      themeMode: themeMode,
      locale: locale,
      supportedLocales: LocaleOptions.supportedLocales,
      localizationsDelegates: const [
        GlobalMaterialLocalizations.delegate,
        GlobalWidgetsLocalizations.delegate,
        GlobalCupertinoLocalizations.delegate,
      ],
      routerConfig: router,
      debugShowCheckedModeBanner: false,
      builder: (context, child) {
        final wrapped = AppLocalizationsLocalizations(
          l10n: l10n,
          child: child ?? const SizedBox.shrink(),
        );

        if (boot.isLoading) {
          return AppLocalizationsLocalizations(
            l10n: l10n,
            child: Web3Background(
              child: Material(
                color: Colors.transparent,
                child: Center(
                  child: CircularProgressIndicator(
                    color: context.g5.primary.withValues(alpha: 0.9),
                  ),
                ),
              ),
            ),
          );
        }
        return wrapped;
      },
    ),
    );
  }
}
