import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

import '../../core/api/api_exception.dart';
import '../../core/models/app_order.dart';
import '../../core/models/plan.dart';
import '../../providers/app_providers.dart';
import '../../providers/plan_providers.dart';
import '../../providers/vpn_providers.dart';
import '../../l10n/app_localizations.dart';
import '../../l10n/plan_catalog_l10n.dart';
import '../../theme/app_colors.dart';
import '../../widgets/web3/glass_card.dart';

class PlansPage extends ConsumerWidget {
  const PlansPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = context.l10n;
    final plansAsync = ref.watch(plansProvider);
    final user = ref.watch(userProfileProvider).valueOrNull;

    return plansAsync.when(
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
                Text(l10n.loadPlansFailed(msg), textAlign: TextAlign.center),
                const SizedBox(height: 12),
                FilledButton(
                  onPressed: () => ref.invalidate(plansProvider),
                  child: Text(l10n.retry),
                ),
              ],
            ),
          ),
        );
      },
      data: (plans) {
        if (plans.isEmpty) {
          return Center(child: Text(l10n.noPlans));
        }
        return ListView.separated(
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 24),
          itemCount: plans.length,
          separatorBuilder: (_, __) => const SizedBox(height: 12),
          itemBuilder: (context, index) {
            return _PlanCard(
              plan: plans[index],
              userBalance: user?.balance ?? 0,
            );
          },
        );
      },
    );
  }
}

class _PlanCard extends ConsumerStatefulWidget {
  const _PlanCard({
    required this.plan,
    required this.userBalance,
  });

  final Plan plan;
  final double userBalance;

  @override
  ConsumerState<_PlanCard> createState() => _PlanCardState();
}

class _PlanCardState extends ConsumerState<_PlanCard> {
  late String _selectedPeriod;

  @override
  void initState() {
    super.initState();
    _selectedPeriod = widget.plan.periods.isNotEmpty
        ? widget.plan.periods.first.key
        : 'month';
  }

  PlanPeriod? get _currentPeriod {
    for (final p in widget.plan.periods) {
      if (p.key == _selectedPeriod) return p;
    }
    return widget.plan.periods.firstOrNull;
  }

  Future<void> _buy() async {
    final period = _currentPeriod;
    if (period == null) return;

    final purchasing = ref.read(planPurchaseProvider.notifier);
    try {
      final order = await purchasing.purchase(
        planId: widget.plan.id,
        period: period.key,
        userBalance: widget.userBalance,
        preferBalance: false,
      );

      if (!mounted) return;

      if (order.isPaid) {
        ref.invalidate(singboxProfileProvider);
        _showSuccess(context.l10n.buySuccess);
        return;
      }

      await _showPaySheet(order);
    } catch (e) {
      if (!mounted) return;
      final msg = e is ApiException ? e.message : e.toString();
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
    }
  }

  Future<void> _showPaySheet(AppOrder order) async {
    await showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      backgroundColor: AppColors.bgElevated,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(16)),
      ),
      builder: (ctx) => _PaySheet(order: order, balance: widget.userBalance),
    );
  }

  void _showSuccess(String msg) {
    ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final period = _currentPeriod;
    final purchasing = ref.watch(planPurchaseProvider).isLoading;

    return GlassCard(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  l10n.localizePlanName(widget.plan.name),
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ),
              if (widget.plan.isTrial)
                Container(
                  padding:
                      const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                  decoration: BoxDecoration(
                    color: AppColors.warning.withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(6),
                  ),
                  child: Text(
                    l10n.trial,
                    style: TextStyle(
                      color: AppColors.warning,
                      fontSize: 12,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 8),
          Text(
            '${l10n.planTraffic(l10n.formatPlanTraffic(widget.plan.transferEnable))}'
            '${widget.plan.deviceLimit != null ? ' · ${l10n.devicesCount(widget.plan.deviceLimit!)}' : ''}',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          if (widget.plan.content != null &&
              widget.plan.content!.trim().isNotEmpty) ...[
            const SizedBox(height: 8),
            Text(
              l10n.localizePlanContent(widget.plan.content),
              style: Theme.of(context).textTheme.bodyMedium,
            ),
          ],
          if (widget.plan.periods.length > 1) ...[
            const SizedBox(height: 12),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: widget.plan.periods.map((p) {
                final selected = p.key == _selectedPeriod;
                return ChoiceChip(
                  label: Text(l10n.localizePeriod(p.key, p.label)),
                  selected: selected,
                  onSelected: (_) => setState(() => _selectedPeriod = p.key),
                  selectedColor: AppColors.primary.withValues(alpha: 0.15),
                  labelStyle: TextStyle(
                    color: selected ? AppColors.primary : AppColors.textSecondary,
                    fontWeight: selected ? FontWeight.w600 : FontWeight.normal,
                  ),
                );
              }).toList(),
            ),
          ],
          const SizedBox(height: 16),
          Row(
            children: [
              Text(
                period != null ? '¥${period.amountYuan.toStringAsFixed(2)}' : '—',
                style: Theme.of(context).textTheme.headlineMedium?.copyWith(
                      color: AppColors.primary,
                    ),
              ),
              if (period?.savePercent != null) ...[
                const SizedBox(width: 8),
                Text(
                  l10n.savePercent(period!.savePercent!),
                  style: const TextStyle(
                    color: AppColors.success,
                    fontWeight: FontWeight.w600,
                    fontSize: 13,
                  ),
                ),
              ],
              const Spacer(),
              FilledButton(
                onPressed: purchasing || period == null ? null : _buy,
                style: FilledButton.styleFrom(
                  minimumSize: const Size(96, 40),
                ),
                child: purchasing
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(
                          strokeWidth: 2,
                          color: Colors.white,
                        ),
                      )
                    : Text(l10n.buyNow),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _PaySheet extends ConsumerStatefulWidget {
  const _PaySheet({required this.order, required this.balance});

  final AppOrder order;
  final double balance;

  @override
  ConsumerState<_PaySheet> createState() => _PaySheetState();
}

class _PaySheetState extends ConsumerState<_PaySheet> {
  bool _busy = false;

  Future<void> _payBalance() async {
    setState(() => _busy = true);
    try {
      final result = await ref
          .read(planPurchaseProvider.notifier)
          .payWithBalance(widget.order.tradeNo);
      if (!mounted) return;
      if (result.paid) {
        ref.invalidate(singboxProfileProvider);
        Navigator.pop(context);
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.l10n.paySuccess)),
        );
      }
    } catch (e) {
      if (!mounted) return;
      final msg = e is ApiException ? e.message : e.toString();
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _payGateway(PaymentChannel channel) async {
    setState(() => _busy = true);
    try {
      final result = await ref.read(planPurchaseProvider.notifier).payWithGateway(
            tradeNo: widget.order.tradeNo,
            paymentId: channel.id,
          );
      if (!mounted) return;

      final action = result.action;
      if (action?.type == 'redirect' && action?.url != null) {
        final uri = Uri.tryParse(action!.url!);
        if (uri != null && await canLaunchUrl(uri)) {
          await launchUrl(uri, mode: LaunchMode.externalApplication);
        }
      }

      if (!mounted) return;
      Navigator.pop(context);
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text(context.l10n.payOpened)),
      );

      try {
        await ref
            .read(planPurchaseProvider.notifier)
            .pollOrderUntilPaid(widget.order.tradeNo);
        if (!mounted) return;
        ref.invalidate(singboxProfileProvider);
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(context.l10n.paySuccess)),
        );
      } catch (e) {
        if (!mounted) return;
        final msg = e is ApiException ? e.message : e.toString();
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
      }
    } catch (e) {
      if (!mounted) return;
      final msg = e is ApiException ? e.message : e.toString();
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(msg)));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = context.l10n;
    final paymentsAsync = ref.watch(paymentsProvider);
    final canBalance = widget.balance >= widget.order.totalYuan;

    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 12, 20, 24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Center(
              child: Container(
                width: 36,
                height: 4,
                decoration: BoxDecoration(
                  color: AppColors.separator,
                  borderRadius: BorderRadius.circular(2),
                ),
              ),
            ),
            const SizedBox(height: 16),
            Text(
              l10n.confirmPay,
              style: Theme.of(context).textTheme.titleLarge,
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 8),
            Text(
              l10n.orderSummary(
                l10n.localizePlanName(widget.order.planName),
                l10n.balanceYuan(widget.order.totalYuan),
              ),
              style: Theme.of(context).textTheme.bodyMedium,
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 20),
            if (canBalance)
              FilledButton.icon(
                onPressed: _busy ? null : _payBalance,
                icon: const Icon(Icons.account_balance_wallet_outlined),
                label: Text(l10n.balancePayLabel(l10n.balanceYuan(widget.balance))),
              ),
            if (!canBalance)
              Padding(
                padding: const EdgeInsets.only(bottom: 8),
                child: Text(
                  l10n.balanceInsufficient(l10n.balanceYuan(widget.balance)),
                  style: Theme.of(context).textTheme.bodySmall,
                  textAlign: TextAlign.center,
                ),
              ),
            const SizedBox(height: 8),
            paymentsAsync.when(
              loading: () => const Center(child: CircularProgressIndicator()),
              error: (e, _) => Text(l10n.loadPaymentsFailed(e)),
              data: (channels) {
                if (channels.isEmpty) {
                  return Text(l10n.noPaymentMethods, textAlign: TextAlign.center);
                }
                return Column(
                  children: channels.map((c) {
                    return Padding(
                      padding: const EdgeInsets.only(bottom: 8),
                      child: OutlinedButton(
                        onPressed: _busy ? null : () => _payGateway(c),
                        child: Text(c.name),
                      ),
                    );
                  }).toList(),
                );
              },
            ),
          ],
        ),
      ),
    );
  }
}
