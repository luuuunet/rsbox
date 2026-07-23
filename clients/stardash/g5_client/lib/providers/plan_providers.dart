import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../core/api/api_exception.dart';
import '../core/models/app_order.dart';
import '../core/models/plan.dart';
import 'app_providers.dart';

final plansProvider = FutureProvider<List<Plan>>((ref) async {
  final token = ref.watch(sessionTokenProvider);
  if (token == null || token.isEmpty) {
    throw const AuthRequiredException();
  }
  return ref.read(apiClientProvider).fetchPlans();
});

final paymentsProvider = FutureProvider<List<PaymentChannel>>((ref) async {
  final token = ref.watch(sessionTokenProvider);
  if (token == null || token.isEmpty) {
    throw const AuthRequiredException();
  }
  return ref.read(apiClientProvider).fetchPayments();
});

class PlanPurchaseNotifier extends StateNotifier<AsyncValue<void>> {
  PlanPurchaseNotifier(this.ref) : super(const AsyncData(null));

  final Ref ref;

  Future<AppOrder> purchase({
    required int planId,
    required String period,
    required double userBalance,
    bool preferBalance = true,
  }) async {
    state = const AsyncLoading();
    try {
      final client = ref.read(apiClientProvider);
      final created = await client.createPlanOrder(
        planId: planId,
        period: period,
      );

      if (created.paid || created.order.isPaid) {
        _refreshAfterPay();
        state = const AsyncData(null);
        return created.order;
      }

      final order = created.order;
      if (preferBalance && userBalance >= order.totalYuan) {
        final paid = await client.payOrder(
          tradeNo: order.tradeNo,
          method: 'balance',
        );
        if (paid.paid) {
          _refreshAfterPay();
          state = const AsyncData(null);
          return paid.order;
        }
      }

      state = const AsyncData(null);
      return order;
    } catch (e, st) {
      state = AsyncError(e, st);
      rethrow;
    }
  }

  Future<PayOrderResult> payWithBalance(String tradeNo) async {
    state = const AsyncLoading();
    try {
      final result = await ref.read(apiClientProvider).payOrder(
            tradeNo: tradeNo,
            method: 'balance',
          );
      if (result.paid) {
        _refreshAfterPay();
      }
      state = const AsyncData(null);
      return result;
    } catch (e, st) {
      state = AsyncError(e, st);
      rethrow;
    }
  }

  Future<PayOrderResult> payWithGateway({
    required String tradeNo,
    required int paymentId,
  }) async {
    state = const AsyncLoading();
    try {
      final result = await ref.read(apiClientProvider).payOrder(
            tradeNo: tradeNo,
            method: 'gateway',
            paymentId: paymentId,
          );
      state = const AsyncData(null);
      return result;
    } catch (e, st) {
      state = AsyncError(e, st);
      rethrow;
    }
  }

  Future<AppOrder> pollOrderUntilPaid(
    String tradeNo, {
    int maxAttempts = 60,
  }) async {
    final client = ref.read(apiClientProvider);
    for (var i = 0; i < maxAttempts; i++) {
      await Future<void>.delayed(const Duration(seconds: 2));
      final order = await client.fetchOrder(tradeNo);
      if (order.isPaid) {
        _refreshAfterPay();
        return order;
      }
    }
    throw ApiException('支付超时，请稍后在「我的」刷新查看');
  }

  void _refreshAfterPay() {
    ref.invalidate(userProfileProvider);
    ref.invalidate(subscribeInfoProvider);
  }
}

final planPurchaseProvider =
    StateNotifierProvider<PlanPurchaseNotifier, AsyncValue<void>>(
  (ref) => PlanPurchaseNotifier(ref),
);
