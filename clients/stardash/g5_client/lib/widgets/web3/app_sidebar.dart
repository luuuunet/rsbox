import 'package:flutter/material.dart';

import '../../config/app_config.dart';
import '../../l10n/app_localizations.dart';
import '../../theme/app_colors.dart';

enum DashboardSection { overview, nodes }

class AppSidebar extends StatelessWidget {
  const AppSidebar({
    super.key,
    required this.selected,
    required this.onSelected,
    required this.onRefresh,
    required this.onLogout,
    this.email,
  });

  final DashboardSection selected;
  final ValueChanged<DashboardSection> onSelected;
  final VoidCallback onRefresh;
  final VoidCallback onLogout;
  final String? email;

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;

    return Container(
      width: 260,
      decoration: BoxDecoration(
        color: AppColors.bgElevated.withValues(alpha: 0.92),
        border: Border(
          right: BorderSide(color: Colors.black.withValues(alpha: 0.06)),
        ),
        boxShadow: [
          BoxShadow(
            color: Colors.black.withValues(alpha: 0.04),
            blurRadius: 20,
            offset: const Offset(4, 0),
          ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 24, 20, 28),
            child: Row(
              children: [
                Container(
                  width: 40,
                  height: 40,
                  decoration: BoxDecoration(
                    borderRadius: BorderRadius.circular(11),
                    color: AppColors.primary,
                  ),
                  alignment: Alignment.center,
                  child: const Text(
                    'G5',
                    style: TextStyle(
                      color: Colors.white,
                      fontWeight: FontWeight.w700,
                      fontSize: 14,
                    ),
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        AppConfig.appName,
                        style: Theme.of(context).textTheme.titleMedium,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                      ),
                      Text(
                        l10n.tagline,
                        style: Theme.of(context).textTheme.bodySmall,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 8),
          _NavItem(
            icon: Icons.dashboard_rounded,
            label: l10n.titleHome,
            selected: selected == DashboardSection.overview,
            onTap: () => onSelected(DashboardSection.overview),
          ),
          _NavItem(
            icon: Icons.public_rounded,
            label: l10n.titleNodes,
            selected: selected == DashboardSection.nodes,
            onTap: () => onSelected(DashboardSection.nodes),
          ),
          const Spacer(),
          if (email != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(20, 0, 20, 12),
              child: Text(
                email!,
                style: Theme.of(context).textTheme.bodySmall,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
            ),
          Padding(
            padding: const EdgeInsets.fromLTRB(12, 0, 12, 20),
            child: Column(
              children: [
                _NavItem(
                  icon: Icons.refresh_rounded,
                  label: l10n.refreshData,
                  selected: false,
                  onTap: onRefresh,
                ),
                const SizedBox(height: 4),
                _NavItem(
                  icon: Icons.logout_rounded,
                  label: l10n.logout,
                  selected: false,
                  onTap: onLogout,
                  danger: true,
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _NavItem extends StatelessWidget {
  const _NavItem({
    required this.icon,
    required this.label,
    required this.selected,
    required this.onTap,
    this.danger = false,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onTap;
  final bool danger;

  @override
  Widget build(BuildContext context) {
    final color = danger
        ? AppColors.danger
        : selected
            ? AppColors.primary
            : AppColors.textSecondary;

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 2),
      child: Material(
        color: selected
            ? AppColors.primary.withValues(alpha: 0.1)
            : Colors.transparent,
        borderRadius: BorderRadius.circular(12),
        child: InkWell(
          onTap: onTap,
          borderRadius: BorderRadius.circular(12),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
            child: Row(
              children: [
                Icon(icon, size: 20, color: color),
                const SizedBox(width: 12),
                Expanded(
                  child: Text(
                    label,
                    style: TextStyle(
                      color: color,
                      fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
                      fontSize: 15,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
