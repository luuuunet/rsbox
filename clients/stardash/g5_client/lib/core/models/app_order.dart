class AppOrder {
  AppOrder({
    required this.tradeNo,
    required this.type,
    required this.status,
    required this.statusLabel,
    this.period,
    required this.totalCents,
    required this.discountCents,
    required this.totalYuan,
    this.planId,
    this.planName,
    this.paymentMethod,
    this.paidAt,
    this.createdAt,
  });

  factory AppOrder.fromJson(Map<String, dynamic> json) {
    final plan = json['plan'] as Map<String, dynamic>?;
    return AppOrder(
      tradeNo: json['trade_no'] as String,
      type: json['type'] as String? ?? '',
      status: json['status'] as String? ?? 'pending',
      statusLabel: json['status_label'] as String? ?? '',
      period: json['period'] as String?,
      totalCents: json['total_cents'] as int? ?? 0,
      discountCents: json['discount_cents'] as int? ?? 0,
      totalYuan: (json['total_yuan'] as num?)?.toDouble() ?? 0,
      planId: plan?['id'] as int?,
      planName: plan?['name'] as String?,
      paymentMethod: json['payment_method'] as String?,
      paidAt: json['paid_at'] as String?,
      createdAt: json['created_at'] as String?,
    );
  }

  final String tradeNo;
  final String type;
  final String status;
  final String statusLabel;
  final String? period;
  final int totalCents;
  final int discountCents;
  final double totalYuan;
  final int? planId;
  final String? planName;
  final String? paymentMethod;
  final String? paidAt;
  final String? createdAt;

  bool get isPaid => status == 'paid';
  bool get isPending => status == 'pending';
}

class PaymentChannel {
  PaymentChannel({
    required this.id,
    required this.name,
    required this.driver,
  });

  factory PaymentChannel.fromJson(Map<String, dynamic> json) {
    return PaymentChannel(
      id: json['id'] as int,
      name: json['name'] as String,
      driver: json['driver'] as String? ?? '',
    );
  }

  final int id;
  final String name;
  final String driver;
}

class PayAction {
  PayAction({required this.type, this.url, this.content});

  factory PayAction.fromJson(Map<String, dynamic> json) {
    return PayAction(
      type: json['type'] as String? ?? 'unknown',
      url: json['url'] as String?,
      content: json['content'] as String?,
    );
  }

  final String type;
  final String? url;
  final String? content;
}

class CreatePlanOrderResult {
  CreatePlanOrderResult({required this.order, required this.paid});

  final AppOrder order;
  final bool paid;
}

class PayOrderResult {
  PayOrderResult({
    required this.paid,
    required this.order,
    this.action,
  });

  final bool paid;
  final AppOrder order;
  final PayAction? action;
}
