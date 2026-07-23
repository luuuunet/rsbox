import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../l10n/app_localizations.dart';
import '../providers/theme_provider.dart';
import '../theme/g5_theme_extension.dart';

/// 主题切换按钮（AppBar）。
class ThemeSwitcherButton extends ConsumerWidget {
  const ThemeSwitcherButton({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
  final pref = ref.watch(themePreferenceProvider);
    final icon = switch (pref) {
      ThemePreference.light => Icons.light_mode_rounded,
      ThemePreference.dark => Icons.dark_mode_rounded,
      ThemePreference.system => Icons.brightness_auto_rounded,
    };

    return IconButton(
      tooltip: l10n.theme,
      onPressed: () => showThemePicker(context, ref),
      icon: Icon(icon),
    );
  }
}

Future<void> showThemePicker(BuildContext context, WidgetRef ref) async {
  final l10n = context.l10n;
  final current = ref.read(themePreferenceProvider);
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
              l10n.selectTheme,
              style: Theme.of(ctx).textTheme.titleMedium,
            ),
            const SizedBox(height: 8),
            _ThemeTile(
              icon: Icons.brightness_auto_rounded,
              label: l10n.themeSystem,
              selected: current == ThemePreference.system,
              onTap: () => _select(ctx, ref, ThemePreference.system),
            ),
            _ThemeTile(
              icon: Icons.light_mode_rounded,
              label: l10n.themeLight,
              selected: current == ThemePreference.light,
              onTap: () => _select(ctx, ref, ThemePreference.light),
            ),
            _ThemeTile(
              icon: Icons.dark_mode_rounded,
              label: l10n.themeDark,
              selected: current == ThemePreference.dark,
              onTap: () => _select(ctx, ref, ThemePreference.dark),
            ),
            const SizedBox(height: 8),
          ],
        ),
      );
    },
  );
}

Future<void> _select(
  BuildContext ctx,
  WidgetRef ref,
  ThemePreference value,
) async {
  await ref.read(themePreferenceProvider.notifier).setPreference(value);
  if (ctx.mounted) Navigator.pop(ctx);
}

class _ThemeTile extends StatelessWidget {
  const _ThemeTile({
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final c = context.g5;
    return ListTile(
      leading: Icon(
        selected ? Icons.check_circle_rounded : icon,
        color: selected ? c.primary : c.textDim,
      ),
      title: Text(label),
      onTap: onTap,
    );
  }
}
