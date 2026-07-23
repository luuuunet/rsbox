import 'package:dio/dio.dart';

import '../../config/app_config.dart';
import '../models/app_order.dart';
import '../models/auth_result.dart';
import '../models/plan.dart';
import '../models/register_config.dart';
import '../models/subscribe_info.dart';
import '../models/user_profile.dart';
import 'api_exception.dart';

class G5ApiClient {
  G5ApiClient({String? accessToken}) {
    _dio = Dio(
      BaseOptions(
        baseUrl: AppConfig.apiBaseUrl,
        connectTimeout: const Duration(seconds: 20),
        receiveTimeout: const Duration(seconds: 60),
        headers: {
          'Accept': 'application/json',
          'Content-Type': 'application/json',
        },
      ),
    );
    _accessToken = accessToken;
    _dio.interceptors.add(
      InterceptorsWrapper(
        onRequest: (options, handler) {
          final key = AppConfig.appKey;
          if (key.isNotEmpty) {
            options.headers['X-App-Key'] = key;
          }
          if (_accessToken != null && _accessToken!.isNotEmpty) {
            options.headers['Authorization'] = 'Bearer $_accessToken';
          }
          handler.next(options);
        },
      ),
    );
  }

  late final Dio _dio;
  String? _accessToken;

  void setAccessToken(String? token) {
    _accessToken = token;
  }

  Future<AuthResult> login({
    required String email,
    required String password,
    String? deviceName,
  }) async {
    try {
      final res = await _dio.post<Map<String, dynamic>>(
        '/login',
        data: {
          'email': email,
          'password': password,
          'device_name': deviceName ?? AppConfig.deviceName,
        },
        options: Options(
          validateStatus: (status) => status != null && status < 500,
        ),
      );
      return _parseAuthResponse(res.data ?? {}, statusCode: res.statusCode);
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  Future<AuthResult> verifyTwoFactor({
    required String challengeToken,
    required String code,
    String? deviceName,
  }) async {
    try {
      final res = await _dio.post<Map<String, dynamic>>(
        '/two-factor',
        data: {
          'challenge_token': challengeToken,
          'code': code,
          'device_name': deviceName ?? AppConfig.deviceName,
        },
        options: Options(
          validateStatus: (status) => status != null && status < 500,
        ),
      );
      return _parseAuthResponse(res.data ?? {}, statusCode: res.statusCode);
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  Future<RegisterConfig> fetchRegisterConfig() async {
    try {
      final res = await _dio.get<Map<String, dynamic>>(
        '/register/config',
        options: Options(
          validateStatus: (status) => status != null && status < 500,
        ),
      );
      final data = res.data ?? {};
      if (res.statusCode == 404) {
        throw ApiException(
          '注册接口不可用，请确认面板已部署最新 App API',
          statusCode: 404,
        );
      }
      if (res.statusCode == 403 || data['enabled'] == false) {
        return RegisterConfig.fromJson({...data, 'enabled': false});
      }
      if (res.statusCode != 200 || data['enabled'] != true) {
        throw ApiException(
          data['message'] as String? ?? '无法加载注册配置',
          statusCode: res.statusCode,
        );
      }
      return RegisterConfig.fromJson(data);
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  Future<void> sendRegisterEmailCode(String email) async {
    try {
      final res = await _dio.post<Map<String, dynamic>>(
        '/register/send-code',
        data: {'email': email},
        options: Options(
          validateStatus: (status) => status != null && status < 500,
        ),
      );
      final data = res.data ?? {};
      if (res.statusCode != 200 || data['ok'] != true) {
        throw ApiException(
          data['message'] as String? ?? '发送验证码失败',
          statusCode: res.statusCode,
          retryAfter: data['retry_after'] as int?,
        );
      }
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  Future<AuthResult> register({
    required String email,
    required String password,
    required String passwordConfirmation,
    String? emailCode,
    String? inviteCode,
    String? captchaChallenge,
    String? captchaAnswer,
    String? turnstileToken,
    String? captchaVerification,
    String? deviceName,
  }) async {
    try {
      final res = await _dio.post<Map<String, dynamic>>(
        '/register',
        data: {
          'email': email,
          'password': password,
          'password_confirmation': passwordConfirmation,
          if (emailCode != null && emailCode.isNotEmpty) 'email_code': emailCode,
          if (inviteCode != null && inviteCode.isNotEmpty) 'invite_code': inviteCode,
          if (captchaChallenge != null && captchaChallenge.isNotEmpty)
            'captcha_challenge': captchaChallenge,
          if (captchaAnswer != null && captchaAnswer.isNotEmpty)
            'captcha_answer': captchaAnswer,
          if (turnstileToken != null && turnstileToken.isNotEmpty)
            'turnstile_token': turnstileToken,
          if (captchaVerification != null && captchaVerification.isNotEmpty)
            'captcha_verification': captchaVerification,
          'device_name': deviceName ?? AppConfig.deviceName,
        },
        options: Options(
          validateStatus: (status) => status != null && status < 500,
        ),
      );
      return _parseAuthResponse(res.data ?? {}, statusCode: res.statusCode);
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  AuthResult _parseAuthResponse(
    Map<String, dynamic> data, {
    int? statusCode,
  }) {
    if (data['requires_2fa'] == true) {
      return AuthRequires2Fa(
        challengeToken: data['challenge_token'] as String? ?? '',
      );
    }
    if (data['ok'] == true && data['access_token'] != null) {
      return AuthSuccess(
        accessToken: data['access_token'] as String,
        expiresAt: data['expires_at'] as String?,
        user: UserProfile.fromJson(data['user'] as Map<String, dynamic>),
      );
    }
    if (statusCode == 401) {
      return AuthFailure('邮箱或密码错误');
    }
    if (statusCode == 422) {
      return AuthFailure(data['message'] as String? ?? '请求无效');
    }
    return AuthFailure(data['message'] as String? ?? '登录失败');
  }

  Future<UserProfile> me() async {
    final data = await _getJson('/me');
    return UserProfile.fromJson(data['user'] as Map<String, dynamic>);
  }

  Future<SubscribeInfo> subscribe() async {
    final data = await _getJson('/subscribe');
    return SubscribeInfo.fromJson(data);
  }

  Future<List<Plan>> fetchPlans() async {
    final data = await _getJson('/plans');
    final raw = data['plans'] as List<dynamic>? ?? [];
    return raw.map((e) => Plan.fromJson(e as Map<String, dynamic>)).toList();
  }

  Future<List<PaymentChannel>> fetchPayments() async {
    final data = await _getJson('/payments');
    final raw = data['payments'] as List<dynamic>? ?? [];
    return raw
        .map((e) => PaymentChannel.fromJson(e as Map<String, dynamic>))
        .toList();
  }

  Future<CreatePlanOrderResult> createPlanOrder({
    required int planId,
    required String period,
  }) async {
    try {
      final res = await _dio.post<Map<String, dynamic>>(
        '/orders/plan',
        data: {'plan_id': planId, 'period': period},
        options: Options(
          validateStatus: (status) => status != null && status < 500,
        ),
      );
      final data = res.data ?? {};
      if (res.statusCode == 422 || data['ok'] != true) {
        throw ApiException(
          data['message'] as String? ?? '创建订单失败',
          statusCode: res.statusCode,
        );
      }
      return CreatePlanOrderResult(
        order: AppOrder.fromJson(data['order'] as Map<String, dynamic>),
        paid: data['paid'] as bool? ?? false,
      );
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  Future<PayOrderResult> payOrder({
    required String tradeNo,
    required String method,
    int? paymentId,
  }) async {
    try {
      final res = await _dio.post<Map<String, dynamic>>(
        '/orders/$tradeNo/pay',
        data: {
          'method': method,
          if (paymentId != null) 'payment_id': paymentId,
        },
        options: Options(
          validateStatus: (status) => status != null && status < 500,
        ),
      );
      final data = res.data ?? {};
      if (res.statusCode == 422 || data['ok'] != true) {
        throw ApiException(
          data['message'] as String? ?? '支付失败',
          statusCode: res.statusCode,
        );
      }
      PayAction? action;
      if (data['action'] is Map<String, dynamic>) {
        action = PayAction.fromJson(data['action'] as Map<String, dynamic>);
      }
      return PayOrderResult(
        paid: data['paid'] as bool? ?? false,
        order: AppOrder.fromJson(data['order'] as Map<String, dynamic>),
        action: action,
      );
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  Future<AppOrder> fetchOrder(String tradeNo) async {
    final data = await _getJson('/orders/$tradeNo');
    return AppOrder.fromJson(data['order'] as Map<String, dynamic>);
  }

  /// 拉取 sing-box 订阅 JSON 正文（非 App API，直接 GET 订阅 URL）。
  Future<SubscribeFetchResult> fetchSingboxProfile(String subscribeUrl) async {
    final client = Dio(
      BaseOptions(
        connectTimeout: const Duration(seconds: 20),
        receiveTimeout: const Duration(seconds: 60),
        responseType: ResponseType.plain,
        headers: {'Accept': 'application/json, text/plain, */*'},
      ),
    );
    try {
      final res = await client.get<String>(subscribeUrl);
      final userinfoRaw = res.headers.value('subscription-userinfo') ??
          res.headers.value('Subscription-Userinfo');
      return SubscribeFetchResult(
        body: res.data ?? '',
        userinfo: userinfoRaw != null
            ? SubscriptionUserinfo.parse(userinfoRaw)
            : null,
      );
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  Future<void> logout() async {
    try {
      await _dio.post('/logout');
    } on DioException catch (e) {
      if (e.response?.statusCode != 401) {
        throw _fromDio(e);
      }
    }
  }

  Future<Map<String, dynamic>> _getJson(String path) async {
    try {
      final res = await _dio.get<Map<String, dynamic>>(path);
      return res.data ?? {};
    } on DioException catch (e) {
      throw _fromDio(e);
    }
  }

  ApiException _fromDio(DioException e) {
    final status = e.response?.statusCode;
    final data = e.response?.data;
    String message = e.message ?? '网络错误';
    int? retryAfter;

    if (data is Map<String, dynamic>) {
      message = data['message'] as String? ?? message;
      retryAfter = data['retry_after'] as int?;
    } else if (status == 503) {
      message = 'App API 未开启，请在面板后台启用「自研 App REST API」';
    } else if (status == 401) {
      message = '未授权，请重新登录';
    } else if (status == 429) {
      message = '请求过于频繁，请稍后再试';
    }

    return ApiException(message, statusCode: status, retryAfter: retryAfter);
  }
}

class SubscribeFetchResult {
  SubscribeFetchResult({required this.body, this.userinfo});

  final String body;
  final SubscriptionUserinfo? userinfo;
}
