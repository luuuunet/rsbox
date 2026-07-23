import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../l10n/app_localizations.dart';
import '../l10n/locale_options.dart';
import '../providers/locale_provider.dart';
import '../theme/g5_theme_extension.dart';

/// 语言切换按钮（AppBar 或设置页）。
class LanguageSwitcherButton extends ConsumerWidget {
  const LanguageSwitcherButton({super.key, this.iconOnly = true});

  final bool iconOnly;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;

    if (iconOnly) {
      return IconButton(
        tooltip: l10n.language,
        onPressed: () => showLanguagePicker(context, ref),
        icon: const Icon(Icons.translate_rounded),
      );
    }

    return TextButton.icon(
      onPressed: () => showLanguagePicker(context, ref),
      icon: const Icon(Icons.translate_rounded, size: 20),
      label: Text(l10n.language),
    );
  }
}

Future<void> showLanguagePicker(BuildContext context, WidgetRef ref) async {
  final l10n = context.l10n;
  final current = ref.read(localeProvider);
  final c = context.g5;

  await showModalBottomSheet<void>(
    context: context,
    backgroundColor: c.bgElevated,
    shape: const RoundedRectangleBorder(
      borderRadius: BorderRadius.vertical(top: Radius.circular(16)),
    ),
    builder: (ctx) {
      return SafeArea(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const SizedBox(height: 12),
            Text(
              l10n.selectLanguage,
              style: Theme.of(ctx).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            ...LocaleOptions.all.map((option) {
              final selected = option.locale.languageCode ==
                      current.languageCode &&
                  option.locale.countryCode == current.countryCode;
              return ListTile(
                leading: Icon(
                  selected
                      ? Icons.check_circle_rounded
                      : Icons.circle_outlined,
                  color: selected ? c.primary : c.textDim,
                ),
                title: Text(option.nativeName),
                onTap: () async {
                  await ref
                      .read(localeProvider.notifier)
                      .setLocaleCode(option.code);
                  if (ctx.mounted) Navigator.pop(ctx);
                },
              );
            }),
            const SizedBox(height: 8),
          ],
        ),
      );
    },
  );
}
