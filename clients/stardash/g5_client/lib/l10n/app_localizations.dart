import 'package:flutter/material.dart';

import '../core/vpn/vpn_mode.dart';
import '../core/vpn/windows_vpn_kernel.dart';

/// 应用文案（8 语言）。
class AppLocalizations {
  AppLocalizations(this.locale);

  final Locale locale;

  String get _code {
    if (locale.languageCode == 'zh' && locale.countryCode == 'TW') {
      return 'zh_TW';
    }
    return locale.languageCode;
  }

  String _t(String key) {
    final map = _all[_code] ?? _all['zh']!;
    return map[key] ?? _all['zh']![key] ?? key;
  }

  // ── Tabs & titles ──
  String get tabHome => _t('tabHome');
  String get tabNodes => _t('tabNodes');
  String get tabPlans => _t('tabPlans');
  String get tabProfile => _t('tabProfile');
  String get titleHome => _t('titleHome');
  String get titleNodes => _t('titleNodes');
  String get titlePlans => _t('titlePlans');
  String get titleProfile => _t('titleProfile');
  String get refresh => _t('refresh');
  String get language => _t('language');
  String get theme => _t('theme');
  String get selectTheme => _t('selectTheme');
  String get themeSystem => _t('themeSystem');
  String get themeLight => _t('themeLight');
  String get themeDark => _t('themeDark');
  String get selectLanguage => _t('selectLanguage');

  // ── Home ──
  String get welcomeBack => _t('welcomeBack');
  String get currentNode => _t('currentNode');
  String get auto => _t('auto');
  String get tapSelectNode => _t('tapSelectNode');
  String get connectMode => _t('connectMode');
  String get vpnKernelTitle => _t('vpnKernelTitle');
  String vpnKernelLabel(WindowsVpnKernel kernel) => switch (kernel) {
        WindowsVpnKernel.rsbox => _t('vpnKernelRsbox'),
        WindowsVpnKernel.singbox => _t('vpnKernelSingbox'),
      };
  String vpnKernelDesc(WindowsVpnKernel kernel) => switch (kernel) {
        WindowsVpnKernel.rsbox => _t('vpnKernelDescRsbox'),
        WindowsVpnKernel.singbox => _t('vpnKernelDescSingbox'),
      };
  String get autoConnect => _t('autoConnect');
  String get autoConnectOn => _t('autoConnectOn');
  String get autoConnectOff => _t('autoConnectOff');
  String get autoReconnect => _t('autoReconnect');
  String get autoReconnectOn => _t('autoReconnectOn');
  String get autoReconnectOff => _t('autoReconnectOff');

  // ── VPN status ──
  String get connected => _t('connected');
  String get connecting => _t('connecting');
  String get connectFailed => _t('connectFailed');
  String get tapConnect => _t('tapConnect');
  String get disconnect => _t('disconnect');
  String get connect => _t('connect');
  String get speedTest => _t('speedTest');
  String get testing => _t('testing');
  String get bandwidthTest => _t('bandwidthTest');
  String get bandwidthTesting => _t('bandwidthTesting');
  String downloadSpeedMbps(String speed) =>
      _t('downloadSpeedMbps').replaceAll('{speed}', speed);
  String get timeout => _t('timeout');
  String get testAll => _t('testAll');
  String get retry => _t('retry');
  String nodesCount(int n) => _t('nodesCount').replaceAll('{n}', '$n');
  String connectingTo(String tag) =>
      _t('connectingTo').replaceAll('{tag}', tag);
  String connectFailedMsg(Object e) =>
      _t('connectFailedMsg').replaceAll('{e}', '$e');
  String get disconnectFailed => _t('disconnectFailed');

  // ── Nodes ──
  String get autoSelectNode => _t('autoSelectNode');
  String get autoSelectOn => _t('autoSelectOn');
  String get autoSelectOff => _t('autoSelectOff');
  String autoSelected(String tag) =>
      _t('autoSelected').replaceAll('{tag}', tag);
  String get autoSelectFailed => _t('autoSelectFailed');
  String get noNodes => _t('noNodes');
  String get buyPlanHint => _t('buyPlanHint');
  String get buyPlan => _t('buyPlan');
  String get refreshSubscribe => _t('refreshSubscribe');
  String loadNodesFailed(String msg) =>
      _t('loadNodesFailed').replaceAll('{msg}', msg);

  // ── VPN modes ──
  String get vpnModeSystemProxy => _t('vpnModeSystemProxy');
  String get vpnModeSystemProxyDesc => _t('vpnModeSystemProxyDesc');
  String get vpnModeTun => _t('vpnModeTun');
  String get vpnModeTunDesc => _t('vpnModeTunDesc');
  String get vpnModeTunAndroidDesc => _t('vpnModeTunAndroidDesc');
  String get vpnModeTunIosDesc => _t('vpnModeTunIosDesc');

  String get tunRequiresAdmin => _t('tunRequiresAdmin');
  String get tunRequiresAdminDetail => _t('tunRequiresAdminDetail');
  String get restartAsAdmin => _t('restartAsAdmin');
  String get useSystemProxy => _t('useSystemProxy');
  String get cancel => _t('cancel');
  String get adminRestartFailed => _t('adminRestartFailed');
  String get tunNeedAdminHint => _t('tunNeedAdminHint');

  String vpnModeLabel(VpnModeKind mode) => switch (mode) {
        VpnModeKind.systemProxy => vpnModeSystemProxy,
        VpnModeKind.tun => vpnModeTun,
      };

  String vpnModeDesc(VpnModeKind mode) => switch (mode) {
        VpnModeKind.systemProxy => vpnModeSystemProxyDesc,
        VpnModeKind.tun => vpnModeTunDesc,
      };

  // ── Profile ──
  String get plan => _t('plan');
  String get expire => _t('expire');
  String get balance => _t('balance');
  String get traffic => _t('traffic');
  String get none => _t('none');
  String get accountActive => _t('accountActive');
  String get accountDisabled => _t('accountDisabled');
  String get email => _t('email');
  String get subscribeLink => _t('subscribeLink');
  String get refreshData => _t('refreshData');
  String get logout => _t('logout');
  String loadFailed(String msg) => _t('loadFailed').replaceAll('{msg}', msg);
  String subscribeError(Object e) =>
      _t('subscribeError').replaceAll('{e}', '$e');
  String trafficUsage(double used, double total) => _t('trafficUsage')
      .replaceAll('{used}', used.toStringAsFixed(1))
      .replaceAll('{total}', total.toStringAsFixed(1));

  // ── Auth ──
  String get tagline => _t('tagline');
  String get login => _t('login');
  String get emailLabel => _t('emailLabel');
  String get passwordLabel => _t('passwordLabel');
  String get invalidEmail => _t('invalidEmail');
  String get passwordMin => _t('passwordMin');
  String get invalidChallenge => _t('invalidChallenge');
  String get twoFactor => _t('twoFactor');
  String get twoFactorHint => _t('twoFactorHint');
  String get verificationCode => _t('verificationCode');
  String get confirm => _t('confirm');
  String get backToLogin => _t('backToLogin');
  String get codeLength => _t('codeLength');
  String get verifyFailed => _t('verifyFailed');
  String get register => _t('register');
  String get registerTitle => _t('registerTitle');
  String get confirmPasswordLabel => _t('confirmPasswordLabel');
  String get passwordConfirmMismatch => _t('passwordConfirmMismatch');
  String passwordMinRegister(int min) =>
      _t('passwordMinRegister').replaceAll('{min}', '$min');
  String get inviteCodeLabel => _t('inviteCodeLabel');
  String get inviteCodeRequired => _t('inviteCodeRequired');
  String get emailCodeLabel => _t('emailCodeLabel');
  String get sendEmailCode => _t('sendEmailCode');
  String get emailCodeSent => _t('emailCodeSent');
  String captchaLabel(String question) =>
      _t('captchaLabel').replaceAll('{q}', question);
  String get captchaRequired => _t('captchaRequired');
  String get registerClosed => _t('registerClosed');
  String get noAccountRegister => _t('noAccountRegister');
  String get hasAccountLogin => _t('hasAccountLogin');
  String get registerWebOnly => _t('registerWebOnly');
  String get openWebRegister => _t('openWebRegister');

  // ── Plans ──
  String get noPlans => _t('noPlans');
  String get buyNow => _t('buyNow');
  String get trial => _t('trial');
  String get unlimitedTraffic => _t('unlimitedTraffic');
  String get buySuccess => _t('buySuccess');
  String get confirmPay => _t('confirmPay');
  String get paySuccess => _t('paySuccess');
  String get payOpened => _t('payOpened');
  String get noPaymentMethods => _t('noPaymentMethods');
  String savePercent(int p) => _t('savePercent').replaceAll('{p}', '$p');
  String planTraffic(String t) => _t('planTraffic').replaceAll('{t}', t);
  String devicesCount(int n) => _t('devicesCount').replaceAll('{n}', '$n');
  String loadPlansFailed(String msg) =>
      _t('loadPlansFailed').replaceAll('{msg}', msg);
  String orderSummary(String name, String price) => _t('orderSummary')
      .replaceAll('{name}', name)
      .replaceAll('{price}', price);
  String balancePayLabel(String amount) =>
      _t('balancePayLabel').replaceAll('{amount}', amount);
  String balanceInsufficient(String amount) =>
      _t('balanceInsufficient').replaceAll('{amount}', amount);
  String loadPaymentsFailed(Object e) =>
      _t('loadPaymentsFailed').replaceAll('{e}', '$e');
  String balanceYuan(double v) => '¥${v.toStringAsFixed(2)}';

  String get periodMonth => _t('periodMonth');
  String get periodQuarter => _t('periodQuarter');
  String get periodHalfYear => _t('periodHalfYear');
  String get periodYear => _t('periodYear');
  String get periodTwoYear => _t('periodTwoYear');
  String get periodThreeYear => _t('periodThreeYear');
  String get periodOnetime => _t('periodOnetime');
  String get planNameBasic => _t('planNameBasic');
  String get planNameStandard => _t('planNameStandard');
  String get planNamePremium => _t('planNamePremium');
  String get planNamePro => _t('planNamePro');
  String get planNameTrial => _t('planNameTrial');
  String get planContentUnlimited => _t('planContentUnlimited');
  String planTrafficGb(String gb) => _t('planTrafficGb').replaceAll('{gb}', gb);

  static const _all = <String, Map<String, String>>{
    'zh': _zh,
    'zh_TW': _zhTw,
    'en': _en,
    'ja': _ja,
    'ko': _ko,
    'es': _es,
    'fr': _fr,
  };
}

extension AppLocalizationsX on BuildContext {
  AppLocalizations get l10n =>
      AppLocalizationsLocalizations.of(this);
}

class AppLocalizationsLocalizations extends InheritedWidget {
  const AppLocalizationsLocalizations({
    super.key,
    required this.l10n,
    required super.child,
  });

  final AppLocalizations l10n;

  static AppLocalizations of(BuildContext context) {
    final w = context
        .dependOnInheritedWidgetOfExactType<AppLocalizationsLocalizations>();
    assert(w != null, 'AppLocalizationsLocalizations not found');
    return w!.l10n;
  }

  @override
  bool updateShouldNotify(AppLocalizationsLocalizations oldWidget) =>
      oldWidget.l10n.locale != l10n.locale;
}

// ── 简体中文 ──
const _zh = {
  'tabHome': '首页',
  'tabNodes': '节点',
  'tabPlans': '套餐',
  'tabProfile': '我的',
  'titleHome': '控制台',
  'titleNodes': '节点',
  'titlePlans': '套餐',
  'titleProfile': '我的',
  'refresh': '刷新',
  'language': '语言',
  'theme': '主题',
  'selectTheme': '选择主题',
  'themeSystem': '跟随系统',
  'themeLight': '浅色',
  'themeDark': '深色',
  'selectLanguage': '选择语言',
  'welcomeBack': '欢迎回来',
  'currentNode': '当前节点',
  'auto': '自动',
  'tapSelectNode': '点击选择节点',
  'connectMode': '连接模式',
  'vpnKernelTitle': 'VPN 内核',
  'vpnKernelRsbox': 'rsbox',
  'vpnKernelSingbox': 'sing-box',
  'vpnKernelDescRsbox': 'Rust 轻量内核（A/B 测试中）',
  'vpnKernelDescSingbox': 'sing-box 官方内核，YouTube 等兼容性更好',
  'autoConnect': '自动连接',
  'autoConnectOn': '启动应用后自动连接 VPN',
  'autoConnectOff': '需手动点击连接按钮',
  'autoReconnect': '自动重连',
  'autoReconnectOn': '连接异常时自动检测并重连',
  'autoReconnectOff': '连接中断需手动重连',
  'connected': '已连接',
  'connecting': '连接中…',
  'connectFailed': '连接失败',
  'tapConnect': '点击连接',
  'disconnect': '断开',
  'connect': '连接',
  'speedTest': '测速',
  'testing': '测速中…',
  'bandwidthTest': '测网速',
  'bandwidthTesting': '测速中…',
  'downloadSpeedMbps': '下载 {speed}',
  'timeout': '超时',
  'testAll': '全部测速',
  'retry': '重试',
  'nodesCount': '共 {n} 个节点',
  'connectingTo': '正在连接: {tag}',
  'connectFailedMsg': '连接失败: {e}',
  'disconnectFailed': '断开失败',
  'autoSelectNode': '自动选择节点',
  'autoSelectOn': '连接前测速并选用延迟最低的节点',
  'autoSelectOff': '手动选择节点',
  'autoSelected': '已自动选择: {tag}',
  'autoSelectFailed': '未能选择节点',
  'noNodes': '暂无可用节点',
  'buyPlanHint': '请先购买或续费套餐',
  'buyPlan': '购买套餐',
  'refreshSubscribe': '刷新订阅',
  'loadNodesFailed': '加载节点失败: {msg}',
  'vpnModeSystemProxy': '系统代理',
  'vpnModeSystemProxyDesc': '仅修改 Windows 代理设置，部分应用（QUIC/游戏）可能不走代理',
  'vpnModeTun': '全局代理',
  'vpnModeTunDesc': '类似 SSTap，接管全部 TCP/UDP 流量（游戏、浏览器均生效），需管理员 + wintun',
  'vpnModeTunAndroidDesc': '通过 Android VPN 服务全局代理，首次连接需授权',
  'vpnModeTunIosDesc': '通过 iOS Network Extension 全局代理，需真机测试',
  'tunRequiresAdmin': 'TUN 需要管理员权限',
  'tunRequiresAdminDetail': 'TUN 全局模式需要创建虚拟网卡，必须以管理员身份运行客户端。可点击「以管理员重启」，或在开始菜单右键 g5_client 选择「以管理员身份运行」。',
  'restartAsAdmin': '以管理员重启',
  'useSystemProxy': '使用系统代理',
  'cancel': '取消',
  'adminRestartFailed': '无法启动管理员模式，请手动右键以管理员运行',
  'tunNeedAdminHint': '当前非管理员，TUN 不可用',
  'plan': '套餐',
  'expire': '到期',
  'balance': '余额',
  'traffic': '流量',
  'none': '无',
  'accountActive': '账户正常',
  'accountDisabled': '账户已停用',
  'email': '邮箱',
  'subscribeLink': '订阅链接',
  'refreshData': '刷新数据',
  'logout': '退出登录',
  'loadFailed': '加载失败: {msg}',
  'subscribeError': '订阅: {e}',
  'trafficUsage': '{used} / {total} GB',
  'tagline': '安全 · 快速 · 私密',
  'login': '登录',
  'emailLabel': '邮箱',
  'passwordLabel': '密码',
  'invalidEmail': '请输入有效邮箱',
  'passwordMin': '密码至少 6 位',
  'invalidChallenge': '两步验证令牌无效',
  'twoFactor': '两步验证',
  'twoFactorHint': '请输入 Authenticator 中的 6 位验证码',
  'verificationCode': '验证码',
  'confirm': '确认',
  'backToLogin': '返回登录',
  'codeLength': '请输入 6 位验证码',
  'verifyFailed': '验证失败',
  'register': '注册',
  'registerTitle': '创建账号',
  'confirmPasswordLabel': '确认密码',
  'passwordConfirmMismatch': '两次密码不一致',
  'passwordMinRegister': '密码至少 {min} 位',
  'inviteCodeLabel': '邀请码',
  'inviteCodeRequired': '请输入邀请码',
  'emailCodeLabel': '邮箱验证码',
  'sendEmailCode': '发送验证码',
  'emailCodeSent': '验证码已发送，请查收邮件',
  'captchaLabel': '验证码：{q} = ?',
  'captchaRequired': '请输入验证码',
  'registerClosed': '当前未开放注册',
  'noAccountRegister': '没有账号？立即注册',
  'hasAccountLogin': '已有账号？去登录',
  'registerWebOnly': '当前站点启用了图形验证码，请在浏览器中完成注册。',
  'openWebRegister': '打开网页注册',
  'noPlans': '暂无可购套餐',
  'buyNow': '立即购买',
  'trial': '试用',
  'unlimitedTraffic': '不限流量',
  'buySuccess': '购买成功，套餐已生效',
  'confirmPay': '确认支付',
  'paySuccess': '支付成功，套餐已生效',
  'payOpened': '已打开支付页面，完成后将自动确认…',
  'noPaymentMethods': '暂无可用支付渠道',
  'savePercent': '省 {p}%',
  'planTraffic': '流量 {t}',
  'devicesCount': '{n} 设备',
  'loadPlansFailed': '加载套餐失败: {msg}',
  'orderSummary': '{name} · {price}',
  'balancePayLabel': '余额支付（{amount}）',
  'balanceInsufficient': '余额不足（{amount}），请选择其他支付方式',
  'loadPaymentsFailed': '加载支付方式失败: {e}',
  'periodMonth': '月付',
  'periodQuarter': '季付',
  'periodHalfYear': '半年付',
  'periodYear': '年付',
  'periodTwoYear': '两年付',
  'periodThreeYear': '三年付',
  'periodOnetime': '一次性',
  'planNameBasic': '基础套餐',
  'planNameStandard': '标准套餐',
  'planNamePremium': '高级套餐',
  'planNamePro': '专业套餐',
  'planNameTrial': '试用套餐',
  'planContentUnlimited': '不限流量',
  'planTrafficGb': '{gb} GB',
};

// ── 繁體中文 ──
const _zhTw = {
  'tabHome': '首頁',
  'tabNodes': '節點',
  'tabPlans': '套餐',
  'tabProfile': '我的',
  'titleHome': '控制台',
  'titleNodes': '節點',
  'titlePlans': '套餐',
  'titleProfile': '我的',
  'refresh': '刷新',
  'language': '語言',
  'theme': '主題',
  'selectTheme': '選擇主題',
  'themeSystem': '跟隨系統',
  'themeLight': '淺色',
  'themeDark': '深色',
  'selectLanguage': '選擇語言',
  'welcomeBack': '歡迎回來',
  'currentNode': '目前節點',
  'auto': '自動',
  'tapSelectNode': '點擊選擇節點',
  'connectMode': '連線模式',
  'vpnKernelTitle': 'VPN 內核',
  'vpnKernelRsbox': 'rsbox',
  'vpnKernelSingbox': 'sing-box',
  'vpnKernelDescRsbox': 'Rust 輕量內核（A/B 測試中）',
  'vpnKernelDescSingbox': 'sing-box 官方內核，YouTube 等相容性更好',
  'autoConnect': '自動連線',
  'autoConnectOn': '啟動應用後自動連線 VPN',
  'autoConnectOff': '需手動點擊連線按鈕',
  'autoReconnect': '自動重連',
  'autoReconnectOn': '連線異常時自動偵測並重連',
  'autoReconnectOff': '連線中斷需手動重連',
  'connected': '已連線',
  'connecting': '連線中…',
  'connectFailed': '連線失敗',
  'tapConnect': '點擊連線',
  'disconnect': '斷開',
  'connect': '連線',
  'speedTest': '測速',
  'testing': '測速中…',
  'bandwidthTest': '測網速',
  'bandwidthTesting': '測速中…',
  'downloadSpeedMbps': '下載 {speed}',
  'timeout': '逾時',
  'testAll': '全部測速',
  'retry': '重試',
  'nodesCount': '共 {n} 個節點',
  'connectingTo': '正在連線: {tag}',
  'connectFailedMsg': '連線失敗: {e}',
  'disconnectFailed': '斷開失敗',
  'autoSelectNode': '自動選擇節點',
  'autoSelectOn': '連線前測速並選用延遲最低的節點',
  'autoSelectOff': '手動選擇節點',
  'autoSelected': '已自動選擇: {tag}',
  'autoSelectFailed': '未能選擇節點',
  'noNodes': '暫無可用節點',
  'buyPlanHint': '請先購買或續費套餐',
  'buyPlan': '購買套餐',
  'refreshSubscribe': '刷新訂閱',
  'loadNodesFailed': '載入節點失敗: {msg}',
  'vpnModeSystemProxy': '系統代理',
  'vpnModeSystemProxyDesc': '僅修改 Windows 代理設定，部分應用（QUIC/遊戲）可能不走代理',
  'vpnModeTun': '全域代理',
  'vpnModeTunDesc': '類似 SSTap，接管全部 TCP/UDP 流量（遊戲、瀏覽器均生效），需管理員 + wintun',
  'vpnModeTunAndroidDesc': '透過 Android VPN 服務全域代理，首次連線需授權',
  'vpnModeTunIosDesc': '透過 iOS Network Extension 全域代理，需真機測試',
  'tunRequiresAdmin': 'TUN 需要管理員權限',
  'tunRequiresAdminDetail': 'TUN 全域模式需要建立虛擬網卡，必須以管理員身分執行用戶端。可點「以管理員重啟」，或右鍵 g5_client 選「以系統管理員身分執行」。',
  'restartAsAdmin': '以管理員重啟',
  'useSystemProxy': '使用系統代理',
  'cancel': '取消',
  'adminRestartFailed': '無法啟動管理員模式，請手動以管理員執行',
  'tunNeedAdminHint': '目前非管理員，TUN 不可用',
  'plan': '套餐',
  'expire': '到期',
  'balance': '餘額',
  'traffic': '流量',
  'none': '無',
  'accountActive': '帳戶正常',
  'accountDisabled': '帳戶已停用',
  'email': '信箱',
  'subscribeLink': '訂閱連結',
  'refreshData': '刷新資料',
  'logout': '登出',
  'loadFailed': '載入失敗: {msg}',
  'subscribeError': '訂閱: {e}',
  'trafficUsage': '{used} / {total} GB',
  'tagline': '安全 · 快速 · 私密',
  'login': '登入',
  'emailLabel': '信箱',
  'passwordLabel': '密碼',
  'invalidEmail': '請輸入有效信箱',
  'passwordMin': '密碼至少 6 位',
  'invalidChallenge': '兩步驗證令牌無效',
  'twoFactor': '兩步驗證',
  'twoFactorHint': '請輸入 Authenticator 中的 6 位驗證碼',
  'verificationCode': '驗證碼',
  'confirm': '確認',
  'backToLogin': '返回登入',
  'codeLength': '請輸入 6 位驗證碼',
  'verifyFailed': '驗證失敗',
  'register': '註冊',
  'registerTitle': '建立帳號',
  'confirmPasswordLabel': '確認密碼',
  'passwordConfirmMismatch': '兩次密碼不一致',
  'passwordMinRegister': '密碼至少 {min} 位',
  'inviteCodeLabel': '邀請碼',
  'inviteCodeRequired': '請輸入邀請碼',
  'emailCodeLabel': '郵箱驗證碼',
  'sendEmailCode': '發送驗證碼',
  'emailCodeSent': '驗證碼已發送，請查收郵件',
  'captchaLabel': '驗證碼：{q} = ?',
  'captchaRequired': '請輸入驗證碼',
  'registerClosed': '目前未開放註冊',
  'noAccountRegister': '沒有帳號？立即註冊',
  'hasAccountLogin': '已有帳號？去登入',
  'registerWebOnly': '目前站點啟用了圖形驗證碼，請在瀏覽器中完成註冊。',
  'openWebRegister': '開啟網頁註冊',
  'noPlans': '暫無可購套餐',
  'buyNow': '立即購買',
  'trial': '試用',
  'unlimitedTraffic': '不限流量',
  'buySuccess': '購買成功，套餐已生效',
  'confirmPay': '確認支付',
  'paySuccess': '支付成功，套餐已生效',
  'payOpened': '已開啟支付頁面，完成後將自動確認…',
  'noPaymentMethods': '暫無可用支付渠道',
  'savePercent': '省 {p}%',
  'planTraffic': '流量 {t}',
  'devicesCount': '{n} 裝置',
  'loadPlansFailed': '載入套餐失敗: {msg}',
  'orderSummary': '{name} · {price}',
  'balancePayLabel': '餘額支付（{amount}）',
  'balanceInsufficient': '餘額不足（{amount}），請選擇其他支付方式',
  'loadPaymentsFailed': '載入支付方式失敗: {e}',
  'periodMonth': '月付',
  'periodQuarter': '季付',
  'periodHalfYear': '半年付',
  'periodYear': '年付',
  'periodTwoYear': '兩年付',
  'periodThreeYear': '三年付',
  'periodOnetime': '一次性',
  'planNameBasic': '基礎套餐',
  'planNameStandard': '標準套餐',
  'planNamePremium': '高級套餐',
  'planNamePro': '專業套餐',
  'planNameTrial': '試用套餐',
  'planContentUnlimited': '不限流量',
  'planTrafficGb': '{gb} GB',
};

// ── English ──
const _en = {
  'tabHome': 'Home',
  'tabNodes': 'Nodes',
  'tabPlans': 'Plans',
  'tabProfile': 'Profile',
  'titleHome': 'Dashboard',
  'titleNodes': 'Nodes',
  'titlePlans': 'Plans',
  'titleProfile': 'Profile',
  'refresh': 'Refresh',
  'language': 'Language',
  'theme': 'Theme',
  'selectTheme': 'Choose theme',
  'themeSystem': 'Follow system',
  'themeLight': 'Light',
  'themeDark': 'Dark',
  'selectLanguage': 'Select language',
  'welcomeBack': 'Welcome back',
  'currentNode': 'Current node',
  'auto': 'Auto',
  'tapSelectNode': 'Tap to select node',
  'connectMode': 'Connection mode',
  'vpnKernelTitle': 'VPN core',
  'vpnKernelRsbox': 'rsbox',
  'vpnKernelSingbox': 'sing-box',
  'vpnKernelDescRsbox': 'Lightweight Rust core (A/B testing)',
  'vpnKernelDescSingbox': 'Official sing-box core, better YouTube compatibility',
  'autoConnect': 'Auto connect',
  'autoConnectOn': 'Connect VPN when app starts',
  'autoConnectOff': 'Connect manually with the button',
  'autoReconnect': 'Auto reconnect',
  'autoReconnectOn': 'Detect dead tunnel and reconnect automatically',
  'autoReconnectOff': 'Reconnect manually when disconnected',
  'connected': 'Connected',
  'connecting': 'Connecting…',
  'connectFailed': 'Connection failed',
  'tapConnect': 'Tap to connect',
  'disconnect': 'Disconnect',
  'connect': 'Connect',
  'speedTest': 'Test',
  'testing': 'Testing…',
  'bandwidthTest': 'Speed test',
  'bandwidthTesting': 'Testing…',
  'downloadSpeedMbps': 'Download {speed}',
  'timeout': 'Timeout',
  'testAll': 'Test all',
  'retry': 'Retry',
  'nodesCount': '{n} nodes',
  'connectingTo': 'Connecting: {tag}',
  'connectFailedMsg': 'Connection failed: {e}',
  'disconnectFailed': 'Disconnect failed',
  'autoSelectNode': 'Auto select node',
  'autoSelectOn': 'Pick the lowest-latency node before connecting',
  'autoSelectOff': 'Select node manually',
  'autoSelected': 'Auto selected: {tag}',
  'autoSelectFailed': 'Could not select a node',
  'noNodes': 'No nodes available',
  'buyPlanHint': 'Please purchase or renew a plan',
  'buyPlan': 'Buy plan',
  'refreshSubscribe': 'Refresh subscription',
  'loadNodesFailed': 'Failed to load nodes: {msg}',
  'vpnModeSystemProxy': 'System proxy',
  'vpnModeSystemProxyDesc': 'Windows proxy settings only; some apps (QUIC/games) may bypass',
  'vpnModeTun': 'Global proxy',
  'vpnModeTunDesc': 'Like SSTap: all TCP/UDP traffic (games, browsers); admin + wintun required',
  'vpnModeTunAndroidDesc': 'Routes all traffic via Android VPN; grant permission on first connect',
  'vpnModeTunIosDesc': 'Routes all traffic via iOS Network Extension; test on a physical device',
  'tunRequiresAdmin': 'TUN requires administrator',
  'tunRequiresAdminDetail': 'TUN mode needs a virtual network adapter. Restart as administrator, or right-click g5_client and choose Run as administrator.',
  'restartAsAdmin': 'Restart as admin',
  'useSystemProxy': 'Use system proxy',
  'cancel': 'Cancel',
  'adminRestartFailed': 'Could not elevate. Run as administrator manually.',
  'tunNeedAdminHint': 'Not running as admin — TUN unavailable',
  'plan': 'Plan',
  'expire': 'Expires',
  'balance': 'Balance',
  'traffic': 'Traffic',
  'none': 'None',
  'accountActive': 'Account active',
  'accountDisabled': 'Account disabled',
  'email': 'Email',
  'subscribeLink': 'Subscription URL',
  'refreshData': 'Refresh data',
  'logout': 'Log out',
  'loadFailed': 'Load failed: {msg}',
  'subscribeError': 'Subscription: {e}',
  'trafficUsage': '{used} / {total} GB',
  'tagline': 'Secure · Fast · Private',
  'login': 'Log in',
  'emailLabel': 'Email',
  'passwordLabel': 'Password',
  'invalidEmail': 'Enter a valid email',
  'passwordMin': 'Password must be at least 6 characters',
  'invalidChallenge': 'Invalid 2FA token',
  'twoFactor': 'Two-factor auth',
  'twoFactorHint': 'Enter the 6-digit code from Authenticator',
  'verificationCode': 'Verification code',
  'confirm': 'Confirm',
  'backToLogin': 'Back to login',
  'codeLength': 'Enter a 6-digit code',
  'verifyFailed': 'Verification failed',
  'register': 'Sign up',
  'registerTitle': 'Create account',
  'confirmPasswordLabel': 'Confirm password',
  'passwordConfirmMismatch': 'Passwords do not match',
  'passwordMinRegister': 'Password must be at least {min} characters',
  'inviteCodeLabel': 'Invite code',
  'inviteCodeRequired': 'Invite code is required',
  'emailCodeLabel': 'Email verification code',
  'sendEmailCode': 'Send code',
  'emailCodeSent': 'Verification code sent. Check your email.',
  'captchaLabel': 'Captcha: {q} = ?',
  'captchaRequired': 'Enter the captcha answer',
  'registerClosed': 'Registration is currently closed',
  'noAccountRegister': 'No account? Sign up',
  'hasAccountLogin': 'Already have an account? Log in',
  'registerWebOnly':
      'This site requires web captcha. Please register in your browser.',
  'openWebRegister': 'Open web registration',
  'noPlans': 'No plans available',
  'buyNow': 'Buy now',
  'trial': 'Trial',
  'unlimitedTraffic': 'Unlimited',
  'buySuccess': 'Purchase successful, plan activated',
  'confirmPay': 'Confirm payment',
  'paySuccess': 'Payment successful, plan activated',
  'payOpened': 'Payment page opened, confirming when done…',
  'noPaymentMethods': 'No payment methods available',
  'savePercent': 'Save {p}%',
  'planTraffic': 'Traffic {t}',
  'devicesCount': '{n} devices',
  'loadPlansFailed': 'Failed to load plans: {msg}',
  'orderSummary': '{name} · {price}',
  'balancePayLabel': 'Pay with balance ({amount})',
  'balanceInsufficient': 'Insufficient balance ({amount}), choose another method',
  'loadPaymentsFailed': 'Failed to load payment methods: {e}',
  'periodMonth': 'Monthly',
  'periodQuarter': 'Quarterly',
  'periodHalfYear': 'Semi-annual',
  'periodYear': 'Annual',
  'periodTwoYear': '2-year',
  'periodThreeYear': '3-year',
  'periodOnetime': 'One-time',
  'planNameBasic': 'Basic',
  'planNameStandard': 'Standard',
  'planNamePremium': 'Premium',
  'planNamePro': 'Pro',
  'planNameTrial': 'Trial',
  'planContentUnlimited': 'Unlimited traffic',
  'planTrafficGb': '{gb} GB',
};

// ── 日本語 ──
const _ja = {
  'tabHome': 'ホーム',
  'tabNodes': 'ノード',
  'tabPlans': 'プラン',
  'tabProfile': 'マイページ',
  'titleHome': 'ダッシュボード',
  'titleNodes': 'ノード',
  'titlePlans': 'プラン',
  'titleProfile': 'マイページ',
  'refresh': '更新',
  'language': '言語',
  'theme': 'テーマ',
  'selectTheme': 'テーマを選択',
  'themeSystem': 'システムに従う',
  'themeLight': 'ライト',
  'themeDark': 'ダーク',
  'selectLanguage': '言語を選択',
  'welcomeBack': 'おかえりなさい',
  'currentNode': '現在のノード',
  'auto': '自動',
  'tapSelectNode': 'タップしてノードを選択',
  'connectMode': '接続モード',
  'vpnKernelTitle': 'VPN コア',
  'vpnKernelRsbox': 'rsbox',
  'vpnKernelSingbox': 'sing-box',
  'vpnKernelDescRsbox': 'Rust 軽量コア（A/B テスト中）',
  'vpnKernelDescSingbox': '公式 sing-box コア、YouTube 互換性が高い',
  'autoConnect': '自動接続',
  'autoConnectOn': 'アプリ起動時に VPN に自動接続',
  'autoConnectOff': 'ボタンで手動接続',
  'autoReconnect': '自動再接続',
  'autoReconnectOn': '接続異常時に自動検出して再接続',
  'autoReconnectOff': '切断時は手動で再接続',
  'connected': '接続済み',
  'connecting': '接続中…',
  'connectFailed': '接続失敗',
  'tapConnect': 'タップして接続',
  'disconnect': '切断',
  'connect': '接続',
  'speedTest': '速度',
  'testing': '測定中…',
  'bandwidthTest': '速度テスト',
  'bandwidthTesting': '測定中…',
  'downloadSpeedMbps': 'ダウンロード {speed}',
  'timeout': 'タイムアウト',
  'testAll': 'すべて測定',
  'retry': '再試行',
  'nodesCount': 'ノード {n} 件',
  'connectingTo': '接続中: {tag}',
  'connectFailedMsg': '接続失敗: {e}',
  'disconnectFailed': '切断失敗',
  'autoSelectNode': 'ノード自動選択',
  'autoSelectOn': '接続前に最速ノードを自動選択',
  'autoSelectOff': '手動でノードを選択',
  'autoSelected': '自動選択: {tag}',
  'autoSelectFailed': 'ノードを選択できませんでした',
  'noNodes': '利用可能なノードがありません',
  'buyPlanHint': 'プランを購入または更新してください',
  'buyPlan': 'プラン購入',
  'refreshSubscribe': '購読を更新',
  'loadNodesFailed': 'ノード読み込み失敗: {msg}',
  'vpnModeSystemProxy': 'システムプロキシ',
  'vpnModeSystemProxyDesc': 'HTTP/SOCKS ローカルプロキシ、ポート自動、管理者不要',
  'vpnModeTun': 'TUN グローバル',
  'vpnModeTunDesc': '全トラフィック、管理者 + wintun.dll が必要',
  'tunRequiresAdmin': 'TUN は管理者権限が必要',
  'tunRequiresAdminDetail': 'TUN モードは仮想 NIC が必要です。「管理者として再起動」するか、g5_client を右クリックして管理者実行してください。',
  'restartAsAdmin': '管理者で再起動',
  'useSystemProxy': 'システムプロキシを使用',
  'cancel': 'キャンセル',
  'adminRestartFailed': '昇格に失敗しました。手動で管理者実行してください',
  'tunNeedAdminHint': '管理者ではないため TUN 不可',
  'plan': 'プラン',
  'expire': '有効期限',
  'balance': '残高',
  'traffic': '流量',
  'none': 'なし',
  'accountActive': 'アカウント正常',
  'accountDisabled': 'アカウント停止中',
  'email': 'メール',
  'subscribeLink': '購読 URL',
  'refreshData': 'データ更新',
  'logout': 'ログアウト',
  'loadFailed': '読み込み失敗: {msg}',
  'subscribeError': '購読: {e}',
  'trafficUsage': '{used} / {total} GB',
  'tagline': '安全 · 高速 · プライベート',
  'login': 'ログイン',
  'emailLabel': 'メール',
  'passwordLabel': 'パスワード',
  'invalidEmail': '有効なメールを入力',
  'passwordMin': 'パスワードは6文字以上',
  'invalidChallenge': '2FA トークンが無効',
  'twoFactor': '二段階認証',
  'twoFactorHint': 'Authenticator の6桁コードを入力',
  'verificationCode': '認証コード',
  'confirm': '確認',
  'backToLogin': 'ログインに戻る',
  'codeLength': '6桁のコードを入力',
  'verifyFailed': '認証失敗',
  'register': '登録',
  'registerTitle': 'アカウント作成',
  'confirmPasswordLabel': 'パスワード確認',
  'passwordConfirmMismatch': 'パスワードが一致しません',
  'passwordMinRegister': 'パスワードは {min} 文字以上',
  'inviteCodeLabel': '招待コード',
  'inviteCodeRequired': '招待コードを入力してください',
  'emailCodeLabel': 'メール認証コード',
  'sendEmailCode': 'コードを送信',
  'emailCodeSent': '認証コードを送信しました。メールを確認してください。',
  'captchaLabel': '認証: {q} = ?',
  'captchaRequired': '認証コードを入力してください',
  'registerClosed': '現在登録を受け付けていません',
  'noAccountRegister': 'アカウントをお持ちでない方は登録',
  'hasAccountLogin': 'アカウントをお持ちの方はログイン',
  'registerWebOnly': 'Web キャプチャが必要です。ブラウザで登録してください。',
  'openWebRegister': 'Web 登録を開く',
  'noPlans': '購入可能なプランがありません',
  'buyNow': '今すぐ購入',
  'trial': 'トライアル',
  'unlimitedTraffic': '無制限',
  'buySuccess': '購入成功、プランが有効になりました',
  'confirmPay': '支払い確認',
  'paySuccess': '支払い成功、プランが有効になりました',
  'payOpened': '支払いページを開きました…',
  'noPaymentMethods': '利用可能な支払い方法がありません',
  'savePercent': '{p}% お得',
  'planTraffic': '流量 {t}',
  'devicesCount': '{n} デバイス',
  'loadPlansFailed': 'プラン読み込み失敗: {msg}',
  'orderSummary': '{name} · {price}',
  'balancePayLabel': '残高で支払い（{amount}）',
  'balanceInsufficient': '残高不足（{amount}）',
  'loadPaymentsFailed': '支払い方法読み込み失敗: {e}',
  'periodMonth': '月払い',
  'periodQuarter': '四半期払い',
  'periodHalfYear': '半年払い',
  'periodYear': '年払い',
  'periodTwoYear': '2年払い',
  'periodThreeYear': '3年払い',
  'periodOnetime': '一括',
  'planNameBasic': 'ベーシック',
  'planNameStandard': 'スタンダード',
  'planNamePremium': 'プレミアム',
  'planNamePro': 'プロ',
  'planNameTrial': 'トライアル',
  'planContentUnlimited': '流量無制限',
  'planTrafficGb': '{gb} GB',
};

// ── 한국어 ──
const _ko = {
  'tabHome': '홈',
  'tabNodes': '노드',
  'tabPlans': '요금제',
  'tabProfile': '내 정보',
  'titleHome': '대시보드',
  'titleNodes': '노드',
  'titlePlans': '요금제',
  'titleProfile': '내 정보',
  'refresh': '새로고침',
  'language': '언어',
  'theme': '테마',
  'selectTheme': '테마 선택',
  'themeSystem': '시스템 따르기',
  'themeLight': '라이트',
  'themeDark': '다크',
  'selectLanguage': '언어 선택',
  'welcomeBack': '다시 오신 것을 환영합니다',
  'currentNode': '현재 노드',
  'auto': '자동',
  'tapSelectNode': '탭하여 노드 선택',
  'connectMode': '연결 모드',
  'vpnKernelTitle': 'VPN 코어',
  'vpnKernelRsbox': 'rsbox',
  'vpnKernelSingbox': 'sing-box',
  'vpnKernelDescRsbox': 'Rust 경량 코어 (A/B 테스트)',
  'vpnKernelDescSingbox': '공식 sing-box 코어, YouTube 호환성 우수',
  'autoConnect': '자동 연결',
  'autoConnectOn': '앱 시작 시 VPN 자동 연결',
  'autoConnectOff': '버튼으로 수동 연결',
  'autoReconnect': '자동 재연결',
  'autoReconnectOn': '연결 이상 시 자동 감지 후 재연결',
  'autoReconnectOff': '끊기면 수동으로 재연결',
  'connected': '연결됨',
  'connecting': '연결 중…',
  'connectFailed': '연결 실패',
  'tapConnect': '탭하여 연결',
  'disconnect': '연결 해제',
  'connect': '연결',
  'speedTest': '속도',
  'testing': '측정 중…',
  'bandwidthTest': '속도 테스트',
  'bandwidthTesting': '측정 중…',
  'downloadSpeedMbps': '다운로드 {speed}',
  'timeout': '시간 초과',
  'testAll': '전체 측정',
  'retry': '재시도',
  'nodesCount': '노드 {n}개',
  'connectingTo': '연결 중: {tag}',
  'connectFailedMsg': '연결 실패: {e}',
  'disconnectFailed': '연결 해제 실패',
  'autoSelectNode': '노드 자동 선택',
  'autoSelectOn': '연결 전 최저 지연 노드 자동 선택',
  'autoSelectOff': '수동으로 노드 선택',
  'autoSelected': '자동 선택: {tag}',
  'autoSelectFailed': '노드를 선택할 수 없습니다',
  'noNodes': '사용 가능한 노드 없음',
  'buyPlanHint': '요금제를 구매하거나 갱신하세요',
  'buyPlan': '요금제 구매',
  'refreshSubscribe': '구독 새로고침',
  'loadNodesFailed': '노드 로드 실패: {msg}',
  'vpnModeSystemProxy': '시스템 프록시',
  'vpnModeSystemProxyDesc': 'HTTP/SOCKS 로컬 프록시, 포트 자동, 관리자 불필요',
  'vpnModeTun': 'TUN 전역',
  'vpnModeTunDesc': '전체 트래픽, 관리자 + wintun.dll 필요',
  'tunRequiresAdmin': 'TUN은 관리자 권한 필요',
  'tunRequiresAdminDetail': 'TUN 모드는 가상 NIC가 필요합니다. 「관리자로 재시작」하거나 g5_client를 관리자 권한으로 실행하세요.',
  'restartAsAdmin': '관리자로 재시작',
  'useSystemProxy': '시스템 프록시 사용',
  'cancel': '취소',
  'adminRestartFailed': '권한 상승 실패. 수동으로 관리자 실행하세요',
  'tunNeedAdminHint': '관리자 아님 — TUN 사용 불가',
  'plan': '요금제',
  'expire': '만료',
  'balance': '잔액',
  'traffic': '트래픽',
  'none': '없음',
  'accountActive': '계정 정상',
  'accountDisabled': '계정 비활성',
  'email': '이메일',
  'subscribeLink': '구독 URL',
  'refreshData': '데이터 새로고침',
  'logout': '로그아웃',
  'loadFailed': '로드 실패: {msg}',
  'subscribeError': '구독: {e}',
  'trafficUsage': '{used} / {total} GB',
  'tagline': '안전 · 빠름 · 프라이빗',
  'login': '로그인',
  'emailLabel': '이메일',
  'passwordLabel': '비밀번호',
  'invalidEmail': '유효한 이메일을 입력하세요',
  'passwordMin': '비밀번호는 6자 이상',
  'invalidChallenge': '2FA 토큰이 유효하지 않음',
  'twoFactor': '2단계 인증',
  'twoFactorHint': 'Authenticator 6자리 코드 입력',
  'verificationCode': '인증 코드',
  'confirm': '확인',
  'backToLogin': '로그인으로',
  'codeLength': '6자리 코드 입력',
  'verifyFailed': '인증 실패',
  'register': '회원가입',
  'registerTitle': '계정 만들기',
  'confirmPasswordLabel': '비밀번호 확인',
  'passwordConfirmMismatch': '비밀번호가 일치하지 않습니다',
  'passwordMinRegister': '비밀번호는 최소 {min}자',
  'inviteCodeLabel': '초대 코드',
  'inviteCodeRequired': '초대 코드를 입력하세요',
  'emailCodeLabel': '이메일 인증 코드',
  'sendEmailCode': '코드 보내기',
  'emailCodeSent': '인증 코드를 보냈습니다. 이메일을 확인하세요.',
  'captchaLabel': '캡차: {q} = ?',
  'captchaRequired': '캡차 답을 입력하세요',
  'registerClosed': '현재 가입이 중단되었습니다',
  'noAccountRegister': '계정이 없으신가요? 가입하기',
  'hasAccountLogin': '이미 계정이 있으신가요? 로그인',
  'registerWebOnly': '웹 캡차가 필요합니다. 브라우저에서 가입해 주세요.',
  'openWebRegister': '웹 가입 열기',
  'noPlans': '구매 가능한 요금제 없음',
  'buyNow': '지금 구매',
  'trial': '체험',
  'unlimitedTraffic': '무제한',
  'buySuccess': '구매 성공, 요금제 활성화',
  'confirmPay': '결제 확인',
  'paySuccess': '결제 성공, 요금제 활성화',
  'payOpened': '결제 페이지가 열렸습니다…',
  'noPaymentMethods': '사용 가능한 결제 수단 없음',
  'savePercent': '{p}% 할인',
  'planTraffic': '트래픽 {t}',
  'devicesCount': '기기 {n}대',
  'loadPlansFailed': '요금제 로드 실패: {msg}',
  'orderSummary': '{name} · {price}',
  'balancePayLabel': '잔액 결제 ({amount})',
  'balanceInsufficient': '잔액 부족 ({amount})',
  'loadPaymentsFailed': '결제 수단 로드 실패: {e}',
  'periodMonth': '월간',
  'periodQuarter': '분기',
  'periodHalfYear': '반기',
  'periodYear': '연간',
  'periodTwoYear': '2년',
  'periodThreeYear': '3년',
  'periodOnetime': '일회성',
  'planNameBasic': '베이직',
  'planNameStandard': '스탠다드',
  'planNamePremium': '프리미엄',
  'planNamePro': '프로',
  'planNameTrial': '체험',
  'planContentUnlimited': '무제한 트래픽',
  'planTrafficGb': '{gb} GB',
};

// ── Español ──
const _es = {
  'tabHome': 'Inicio',
  'tabNodes': 'Nodos',
  'tabPlans': 'Planes',
  'tabProfile': 'Perfil',
  'titleHome': 'Panel',
  'titleNodes': 'Nodos',
  'titlePlans': 'Planes',
  'titleProfile': 'Perfil',
  'refresh': 'Actualizar',
  'language': 'Idioma',
  'theme': 'Tema',
  'selectTheme': 'Elegir tema',
  'themeSystem': 'Seguir sistema',
  'themeLight': 'Claro',
  'themeDark': 'Oscuro',
  'selectLanguage': 'Seleccionar idioma',
  'welcomeBack': 'Bienvenido de nuevo',
  'currentNode': 'Nodo actual',
  'auto': 'Auto',
  'tapSelectNode': 'Toca para elegir nodo',
  'connectMode': 'Modo de conexión',
  'vpnKernelTitle': 'Núcleo VPN',
  'vpnKernelRsbox': 'rsbox',
  'vpnKernelSingbox': 'sing-box',
  'vpnKernelDescRsbox': 'Núcleo Rust ligero (prueba A/B)',
  'vpnKernelDescSingbox': 'Núcleo sing-box oficial, mejor compatibilidad con YouTube',
  'autoConnect': 'Conexión automática',
  'autoConnectOn': 'Conectar VPN al iniciar la app',
  'autoConnectOff': 'Conectar manualmente con el botón',
  'autoReconnect': 'Reconexión automática',
  'autoReconnectOn': 'Detectar túnel caído y reconectar solo',
  'autoReconnectOff': 'Reconectar manualmente si se corta',
  'connected': 'Conectado',
  'connecting': 'Conectando…',
  'connectFailed': 'Error de conexión',
  'tapConnect': 'Toca para conectar',
  'disconnect': 'Desconectar',
  'connect': 'Conectar',
  'speedTest': 'Velocidad',
  'testing': 'Probando…',
  'bandwidthTest': 'Prueba de velocidad',
  'bandwidthTesting': 'Probando…',
  'downloadSpeedMbps': 'Descarga {speed}',
  'timeout': 'Tiempo agotado',
  'testAll': 'Probar todo',
  'retry': 'Reintentar',
  'nodesCount': '{n} nodos',
  'connectingTo': 'Conectando: {tag}',
  'connectFailedMsg': 'Error de conexión: {e}',
  'disconnectFailed': 'Error al desconectar',
  'autoSelectNode': 'Selección automática',
  'autoSelectOn': 'Elige el nodo de menor latencia antes de conectar',
  'autoSelectOff': 'Seleccionar nodo manualmente',
  'autoSelected': 'Seleccionado: {tag}',
  'autoSelectFailed': 'No se pudo seleccionar un nodo',
  'noNodes': 'No hay nodos disponibles',
  'buyPlanHint': 'Compra o renueva un plan',
  'buyPlan': 'Comprar plan',
  'refreshSubscribe': 'Actualizar suscripción',
  'loadNodesFailed': 'Error al cargar nodos: {msg}',
  'vpnModeSystemProxy': 'Proxy del sistema',
  'vpnModeSystemProxyDesc': 'HTTP/SOCKS proxy local, puerto auto, sin admin',
  'vpnModeTun': 'TUN global',
  'vpnModeTunDesc': 'Todo el tráfico, requiere admin + wintun.dll',
  'tunRequiresAdmin': 'TUN requiere administrador',
  'tunRequiresAdminDetail': 'TUN necesita un adaptador virtual. Reinicia como administrador o ejecuta g5_client con clic derecho → Ejecutar como administrador.',
  'restartAsAdmin': 'Reiniciar como admin',
  'useSystemProxy': 'Usar proxy del sistema',
  'cancel': 'Cancelar',
  'adminRestartFailed': 'No se pudo elevar. Ejecuta como administrador manualmente.',
  'tunNeedAdminHint': 'Sin admin — TUN no disponible',
  'plan': 'Plan',
  'expire': 'Vence',
  'balance': 'Saldo',
  'traffic': 'Tráfico',
  'none': 'Ninguno',
  'accountActive': 'Cuenta activa',
  'accountDisabled': 'Cuenta desactivada',
  'email': 'Correo',
  'subscribeLink': 'URL de suscripción',
  'refreshData': 'Actualizar datos',
  'logout': 'Cerrar sesión',
  'loadFailed': 'Error al cargar: {msg}',
  'subscribeError': 'Suscripción: {e}',
  'trafficUsage': '{used} / {total} GB',
  'tagline': 'Seguro · Rápido · Privado',
  'login': 'Iniciar sesión',
  'emailLabel': 'Correo',
  'passwordLabel': 'Contraseña',
  'invalidEmail': 'Introduce un correo válido',
  'passwordMin': 'Mínimo 6 caracteres',
  'invalidChallenge': 'Token 2FA inválido',
  'twoFactor': 'Autenticación en dos pasos',
  'twoFactorHint': 'Introduce el código de 6 dígitos',
  'verificationCode': 'Código',
  'confirm': 'Confirmar',
  'backToLogin': 'Volver al inicio',
  'codeLength': 'Introduce 6 dígitos',
  'verifyFailed': 'Verificación fallida',
  'register': 'Registrarse',
  'registerTitle': 'Crear cuenta',
  'confirmPasswordLabel': 'Confirmar contraseña',
  'passwordConfirmMismatch': 'Las contraseñas no coinciden',
  'passwordMinRegister': 'La contraseña debe tener al menos {min} caracteres',
  'inviteCodeLabel': 'Código de invitación',
  'inviteCodeRequired': 'Se requiere código de invitación',
  'emailCodeLabel': 'Código de verificación de email',
  'sendEmailCode': 'Enviar código',
  'emailCodeSent': 'Código enviado. Revisa tu correo.',
  'captchaLabel': 'Captcha: {q} = ?',
  'captchaRequired': 'Introduce la respuesta del captcha',
  'registerClosed': 'El registro está cerrado',
  'noAccountRegister': '¿No tienes cuenta? Regístrate',
  'hasAccountLogin': '¿Ya tienes cuenta? Inicia sesión',
  'registerWebOnly':
      'Este sitio requiere captcha web. Regístrate en el navegador.',
  'openWebRegister': 'Abrir registro web',
  'noPlans': 'No hay planes disponibles',
  'buyNow': 'Comprar ahora',
  'trial': 'Prueba',
  'unlimitedTraffic': 'Ilimitado',
  'buySuccess': 'Compra exitosa, plan activado',
  'confirmPay': 'Confirmar pago',
  'paySuccess': 'Pago exitoso, plan activado',
  'payOpened': 'Página de pago abierta…',
  'noPaymentMethods': 'Sin métodos de pago',
  'savePercent': 'Ahorra {p}%',
  'planTraffic': 'Tráfico {t}',
  'devicesCount': '{n} dispositivos',
  'loadPlansFailed': 'Error al cargar planes: {msg}',
  'orderSummary': '{name} · {price}',
  'balancePayLabel': 'Pagar con saldo ({amount})',
  'balanceInsufficient': 'Saldo insuficiente ({amount})',
  'loadPaymentsFailed': 'Error al cargar pagos: {e}',
  'periodMonth': 'Mensual',
  'periodQuarter': 'Trimestral',
  'periodHalfYear': 'Semestral',
  'periodYear': 'Anual',
  'periodTwoYear': '2 años',
  'periodThreeYear': '3 años',
  'periodOnetime': 'Pago único',
  'planNameBasic': 'Básico',
  'planNameStandard': 'Estándar',
  'planNamePremium': 'Premium',
  'planNamePro': 'Pro',
  'planNameTrial': 'Prueba',
  'planContentUnlimited': 'Tráfico ilimitado',
  'planTrafficGb': '{gb} GB',
};

// ── Français ──
const _fr = {
  'tabHome': 'Accueil',
  'tabNodes': 'Nœuds',
  'tabPlans': 'Forfaits',
  'tabProfile': 'Profil',
  'titleHome': 'Tableau de bord',
  'titleNodes': 'Nœuds',
  'titlePlans': 'Forfaits',
  'titleProfile': 'Profil',
  'refresh': 'Actualiser',
  'language': 'Langue',
  'theme': 'Thème',
  'selectTheme': 'Choisir le thème',
  'themeSystem': 'Suivre le système',
  'themeLight': 'Clair',
  'themeDark': 'Sombre',
  'selectLanguage': 'Choisir la langue',
  'welcomeBack': 'Bon retour',
  'currentNode': 'Nœud actuel',
  'auto': 'Auto',
  'tapSelectNode': 'Appuyez pour choisir un nœud',
  'connectMode': 'Mode de connexion',
  'vpnKernelTitle': 'Noyau VPN',
  'vpnKernelRsbox': 'rsbox',
  'vpnKernelSingbox': 'sing-box',
  'vpnKernelDescRsbox': 'Noyau Rust léger (test A/B)',
  'vpnKernelDescSingbox': 'Noyau sing-box officiel, meilleure compatibilité YouTube',
  'autoConnect': 'Connexion auto',
  'autoConnectOn': 'Connexion VPN au démarrage',
  'autoConnectOff': 'Connexion manuelle via le bouton',
  'autoReconnect': 'Reconnexion auto',
  'autoReconnectOn': 'Détecter une coupure et reconnecter automatiquement',
  'autoReconnectOff': 'Reconnecter manuellement en cas de coupure',
  'connected': 'Connecté',
  'connecting': 'Connexion…',
  'connectFailed': 'Échec de connexion',
  'tapConnect': 'Appuyez pour connecter',
  'disconnect': 'Déconnecter',
  'connect': 'Connecter',
  'speedTest': 'Vitesse',
  'testing': 'Test…',
  'bandwidthTest': 'Test de débit',
  'bandwidthTesting': 'Test…',
  'downloadSpeedMbps': 'Téléchargement {speed}',
  'timeout': 'Délai dépassé',
  'testAll': 'Tout tester',
  'retry': 'Réessayer',
  'nodesCount': '{n} nœuds',
  'connectingTo': 'Connexion : {tag}',
  'connectFailedMsg': 'Échec : {e}',
  'disconnectFailed': 'Échec de déconnexion',
  'autoSelectNode': 'Sélection auto',
  'autoSelectOn': 'Choisir le nœud le plus rapide avant connexion',
  'autoSelectOff': 'Sélection manuelle',
  'autoSelected': 'Sélectionné : {tag}',
  'autoSelectFailed': 'Impossible de sélectionner',
  'noNodes': 'Aucun nœud disponible',
  'buyPlanHint': 'Achetez ou renouvelez un forfait',
  'buyPlan': 'Acheter un forfait',
  'refreshSubscribe': 'Actualiser l\'abonnement',
  'loadNodesFailed': 'Échec du chargement : {msg}',
  'vpnModeSystemProxy': 'Proxy système',
  'vpnModeSystemProxyDesc': 'HTTP/SOCKS local, port auto, sans admin',
  'vpnModeTun': 'TUN global',
  'vpnModeTunDesc': 'Tout le trafic, admin + wintun.dll requis',
  'tunRequiresAdmin': 'TUN nécessite les droits admin',
  'tunRequiresAdminDetail': 'Le mode TUN crée une carte réseau virtuelle. Redémarrez en admin ou clic droit sur g5_client → Exécuter en tant qu\'administrateur.',
  'restartAsAdmin': 'Redémarrer en admin',
  'useSystemProxy': 'Utiliser le proxy système',
  'cancel': 'Annuler',
  'adminRestartFailed': 'Élévation impossible. Lancez en admin manuellement.',
  'tunNeedAdminHint': 'Pas admin — TUN indisponible',
  'plan': 'Forfait',
  'expire': 'Expire',
  'balance': 'Solde',
  'traffic': 'Trafic',
  'none': 'Aucun',
  'accountActive': 'Compte actif',
  'accountDisabled': 'Compte désactivé',
  'email': 'E-mail',
  'subscribeLink': 'URL d\'abonnement',
  'refreshData': 'Actualiser les données',
  'logout': 'Déconnexion',
  'loadFailed': 'Échec du chargement : {msg}',
  'subscribeError': 'Abonnement : {e}',
  'trafficUsage': '{used} / {total} GB',
  'tagline': 'Sécurisé · Rapide · Privé',
  'login': 'Connexion',
  'emailLabel': 'E-mail',
  'passwordLabel': 'Mot de passe',
  'invalidEmail': 'E-mail invalide',
  'passwordMin': '6 caractères minimum',
  'invalidChallenge': 'Jeton 2FA invalide',
  'twoFactor': 'Authentification à deux facteurs',
  'twoFactorHint': 'Entrez le code à 6 chiffres',
  'verificationCode': 'Code',
  'confirm': 'Confirmer',
  'backToLogin': 'Retour connexion',
  'codeLength': 'Entrez 6 chiffres',
  'verifyFailed': 'Échec de vérification',
  'register': 'Inscription',
  'registerTitle': 'Créer un compte',
  'confirmPasswordLabel': 'Confirmer le mot de passe',
  'passwordConfirmMismatch': 'Les mots de passe ne correspondent pas',
  'passwordMinRegister': 'Le mot de passe doit contenir au moins {min} caractères',
  'inviteCodeLabel': 'Code d\'invitation',
  'inviteCodeRequired': 'Code d\'invitation requis',
  'emailCodeLabel': 'Code de vérification e-mail',
  'sendEmailCode': 'Envoyer le code',
  'emailCodeSent': 'Code envoyé. Vérifiez votre e-mail.',
  'captchaLabel': 'Captcha : {q} = ?',
  'captchaRequired': 'Entrez la réponse du captcha',
  'registerClosed': 'Les inscriptions sont fermées',
  'noAccountRegister': 'Pas de compte ? Inscrivez-vous',
  'hasAccountLogin': 'Déjà un compte ? Connectez-vous',
  'registerWebOnly':
      'Ce site nécessite un captcha web. Inscrivez-vous dans le navigateur.',
  'openWebRegister': 'Ouvrir l\'inscription web',
  'noPlans': 'Aucun forfait disponible',
  'buyNow': 'Acheter',
  'trial': 'Essai',
  'unlimitedTraffic': 'Illimité',
  'buySuccess': 'Achat réussi, forfait activé',
  'confirmPay': 'Confirmer le paiement',
  'paySuccess': 'Paiement réussi, forfait activé',
  'payOpened': 'Page de paiement ouverte…',
  'noPaymentMethods': 'Aucun moyen de paiement',
  'savePercent': 'Économisez {p}%',
  'planTraffic': 'Trafic {t}',
  'devicesCount': '{n} appareils',
  'loadPlansFailed': 'Échec du chargement : {msg}',
  'orderSummary': '{name} · {price}',
  'balancePayLabel': 'Payer avec le solde ({amount})',
  'balanceInsufficient': 'Solde insuffisant ({amount})',
  'loadPaymentsFailed': 'Échec chargement paiements : {e}',
  'periodMonth': 'Mensuel',
  'periodQuarter': 'Trimestriel',
  'periodHalfYear': 'Semestriel',
  'periodYear': 'Annuel',
  'periodTwoYear': '2 ans',
  'periodThreeYear': '3 ans',
  'periodOnetime': 'Paiement unique',
  'planNameBasic': 'Basique',
  'planNameStandard': 'Standard',
  'planNamePremium': 'Premium',
  'planNamePro': 'Pro',
  'planNameTrial': 'Essai',
  'planContentUnlimited': 'Trafic illimité',
  'planTrafficGb': '{gb} GB',
};
