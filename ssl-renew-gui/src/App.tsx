import React, { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";

type Store = {
  current_domain: string;
  profiles: Record<string, Profile>;
  vendor_configs: Record<string, VendorEntry[]>;
  monitor: MonitorConfig;
  app_settings: AppSettings;
};

type VendorEntry = { alias: string; key: string };
type NotificationScope = {
  step_check_success: boolean;
  step_check_failure: boolean;
  step_order_success: boolean;
  step_order_failure: boolean;
  step_dns_check_success: boolean;
  step_dns_check_failure: boolean;
  step_issue_success: boolean;
  step_issue_failure: boolean;
  step_restart_success: boolean;
  step_restart_failure: boolean;
  monitor_start: boolean;
  monitor_stop: boolean;
  monitor_profile_start: boolean;
  monitor_no_renew_needed: boolean;
  monitor_renew_needed: boolean;
  monitor_manual_dns_skipped: boolean;
  monitor_full_success: boolean;
  monitor_full_failure: boolean;
};
type AppSettings = {
  theme: "light" | "dark" | string;
  language: "zh" | "en" | string;
  toast: {
    enabled: boolean;
    position: string;
    duration_ms: number;
  };
  notification: {
    enabled: boolean;
    channel: string;
    scope: NotificationScope;
    dingtalk: { access_token: string; secret: string };
    telegram: { bot_token: string; chat_id: string };
    feishu: { webhook_url: string; secret: string };
  };
};
type Profile = {
  domain: string;
  email: string;
  renew: { days_before_expiry: number; force?: boolean };
  paths: {
    cert_file: string;
    key_file: string;
    log_file: string;
    max_log_size_mb: number;
    backup_dir: string;
    state_dir: string;
    work_dir: string;
  };
  dns: { provider: string; signer?: { pipe_name: string } };
  nginx: { enabled: boolean; restart_mode?: string; exe_path: string; working_dir: string; kill_image_name: string };
};
type MonitorConfig = {
  enabled: boolean;
  profiles: string[];
  mode: string;
  daily_time: string;
  interval_minutes: number;
  cron_expression: string;
};
type Toast = { id: number; message: string; kind: "success" | "error" | "info" };
type DnsChallenge = {
  domain: string;
  txt_name: string;
  rr_name: string;
  txt_value: string;
};

const steps = ["检查证书", "创建订单", "检测 DNS", "签发并保存证书", "重启 Nginx"];
const stepKeys = ["checkCert", "createOrder", "dnsCheck", "issueCert", "restartNginx"] as const;
const providerOptions = [
  ["manual", "手动DNS"],
  ["aliyun", "阿里云"],
  ["cloudflare", "Cloudflare"],
  ["signer", "签发程序"]
];
const vendorProviderOptions = providerOptions.filter(([value]) => value !== "manual" && value !== "signer");
const appVersion = "0.1.0";
const defaultSettings: AppSettings = {
  theme: "light",
  language: "zh",
  toast: { enabled: true, position: "top-right", duration_ms: 3200 },
  notification: {
    enabled: false,
    channel: "dingtalk",
    scope: {
      step_check_success: false,
      step_check_failure: false,
      step_order_success: false,
      step_order_failure: false,
      step_dns_check_success: false,
      step_dns_check_failure: false,
      step_issue_success: false,
      step_issue_failure: false,
      step_restart_success: false,
      step_restart_failure: false,
      monitor_start: false,
      monitor_stop: false,
      monitor_profile_start: false,
      monitor_no_renew_needed: false,
      monitor_renew_needed: false,
      monitor_manual_dns_skipped: false,
      monitor_full_success: false,
      monitor_full_failure: false
    },
    dingtalk: { access_token: "", secret: "" },
    telegram: { bot_token: "", chat_id: "" },
    feishu: { webhook_url: "", secret: "" }
  }
};

const i18n = {
  zh: {
    loading: "加载中...",
    loadFailed: "加载配置失败",
    profiles: "域名配置",
    addProfile: "新增配置",
    deleteProfile: "删除配置",
    vendorConfig: "厂商配置",
    signerProgram: "签发程序",
    monitor: "启动监控",
    openGithub: "打开项目仓库",
    settings: "设置",
    domain: "域名",
    saveConfig: "保存配置",
    skipCertCheck: "跳过证书检查",
    runAll: "一键运行",
    log: "日志",
    executeStep: "执行当前步骤",
    checkCert: "检查证书",
    createOrder: "创建订单",
    dnsCheck: "检测 DNS",
    issueCert: "签发并保存证书",
    restartNginx: "重启 Nginx",
    certPemPath: "证书 PEM 路径",
    keyPath: "私钥 KEY 路径",
    daysBeforeExpiry: "提前续期天数",
    email: "邮箱",
    dnsProvider: "DNS 厂商",
    useSigner: "使用签发程序代请求厂商",
    signerPipe: "签发程序 Pipe",
    signerTitle: "签发程序管理",
    signerProvider: "签发厂商",
    aliyun: "阿里云",
    signerRootDomain: "根域名",
    signerAllowedDomains: "允许域名（逗号分隔）",
    signerTtl: "固定 TTL",
    signerInit: "初始化签发程序",
    signerInitPassphrase: "初始化加密口令",
    signerUnlockMenu: "签发程序解锁",
    signerStatus: "查看状态",
    signerStatusLabel: "状态",
    aliyunAccessKeyId: "阿里云 AccessKeyId",
    aliyunAccessKeySecret: "阿里云 AccessKeySecret",
    cloudflareToken: "Cloudflare API Token",
    signerUnlockPassphrase: "高安全解锁口令",
    signerUnlock: "解锁签发程序",
    signerLock: "锁定签发程序",
    signerRuntimeStatus: "运行时状态",
    signerAuthorizeTest: "授权测试",
    signerHint: "高安全模式会用口令派生密钥加密 DNS Key，并用 Windows DPAPI 绑定当前用户保护元数据；signer 启动后需要手动解锁，密钥只保留在进程内存。",
    forceRenew: "强制申请证书",
    enabled: "已启用",
    disabled: "已关闭",
    dnsEmptyOrder: "执行创建订单后，这里会显示需要配置的 DNS TXT 名称和值。",
    dnsEmptyCheck: "还没有可显示的 DNS TXT 记录。请先执行“创建订单”。",
    dnsCheckHint: "点击执行后会查询 TXT 记录是否已经生效。没生效可以稍等后重复点击。",
    issueHint: "点击执行后会通知 Let's Encrypt 验证 DNS，签发证书，并立即保存到上面的 PEM/KEY 路径。",
    nginxEnabled: "启用 Nginx 重启",
    nginxRestartMode: "Nginx 重启方式",
    nginxKillStart: "杀进程 + 启进程",
    nginxReload: "nginx -s reload",
    nginxExe: "Nginx exe 路径",
    nginxDir: "Nginx 工作目录",
    cancel: "取消",
    add: "新增",
    close: "关闭",
    saveClose: "保存并关闭",
    delete: "删除",
    confirmDeleteTitle: "删除配置",
    confirmDeleteMessage: "确定删除配置 {domain} 吗？不会删除证书文件。",
    addProfileTitle: "新增域名配置",
    addProfileHelp: "新增后会复制当前配置作为模板，并自动生成证书 PEM/KEY 默认路径。",
    vendorTitle: "厂商配置管理",
    aliasPlaceholder: "别名，例如 AccessKeyId",
    envKeyPlaceholder: "环境变量 key，例如 Ali_Key",
    emptyVendor: "当前厂商没有环境变量要求。",
    monitorConfig: "监控配置",
    monitorFrequency: "监控频率",
    daily: "每天固定时间",
    interval: "每隔多少分钟",
    cron: "Cron 表达式",
    dailyTime: "每天时间",
    intervalMinutes: "间隔分钟",
    stopMonitor: "停止监控",
    saveStart: "保存并启动",
    settingsTitle: "设置",
    theme: "主题",
    toast: "Toast",
    language: "语言",
    logSettings: "日志",
    importExport: "导入导出",
    about: "关于",
    light: "亮白",
    dark: "暗夜",
    toastEnabled: "开启 Toast",
    toastPosition: "弹出位置",
    toastDuration: "持续时间（毫秒）",
    notification: "通知",
    notificationEnabled: "开启通知",
    notificationChannel: "通知渠道",
    dingtalk: "钉钉",
    telegram: "Telegram",
    feishu: "飞书",
    dingtalkAccessToken: "钉钉机器人 token",
    dingtalkSecret: "钉钉加签密钥/密码（可选）",
    telegramBotToken: "Telegram Bot Token",
    telegramChatId: "Telegram chat_id",
    feishuWebhookUrl: "飞书 Webhook 地址",
    feishuSecret: "飞书签名密钥（可选）",
    notificationHint: "这里只保存通知渠道配置，后续可以接到续期成功、失败或监控告警事件上。",
    notificationScope: "通知范围",
    manualStepNotifications: "手动步骤通知",
    monitorNotifications: "监控通知",
    successNotice: "成功通知",
    failureNotice: "失败通知",
    monitorStartNotice: "监控开始",
    monitorStopNotice: "监控停止",
    monitorProfileStartNotice: "开始监控某个配置",
    monitorNoRenewNeededNotice: "检查结果：无需申请",
    monitorRenewNeededNotice: "检查结果：需要申请",
    monitorManualDnsSkippedNotice: "手动 DNS 无法无人值守，已跳过",
    monitorFullSuccessNotice: "步骤 1-5 总运行成功",
    monitorFullFailureNotice: "步骤 1-5 总运行失败",
    notificationScopeHint: "建议至少勾选失败通知和监控总结果通知；成功通知适合你想留完整流水时开启。",
    dingtalkHint: "钉钉自定义机器人使用 webhook 里的 access_token；如果开启加签，还需要填写 secret。",
    telegramHint: "Telegram 发送消息需要 Bot Token 和目标 chat_id。",
    feishuHint: "飞书自定义机器人使用 webhook 地址；如果启用签名校验，再填写 secret。",
    topRight: "右上角",
    topLeft: "左上角",
    bottomRight: "右下角",
    bottomLeft: "左下角",
    languageLabel: "界面语言",
    chinese: "中文",
    english: "English",
    logDir: "日志存储位置",
    logName: "日志文件名",
    logMaxSize: "日志最大大小 MB",
    logCurrentDomain: "日志设置会保存到当前选中的域名配置。",
    exportYaml: "导出 YAML",
    importYaml: "导入 YAML",
    chooseYaml: "选择 YAML 文件",
    importExportHint: "导出会下载当前 profiles.yaml；导入会先选择文件，再确认是否覆盖当前所有配置。",
    importConfirmTitle: "确认导入",
    importConfirmMessage: "确定用 {file} 覆盖当前 profiles.yaml 吗？当前所有域名配置、设置和监控配置都会被替换。",
    importSuccess: "导入完成",
    exportSuccess: "导出完成",
    exportCanceled: "已取消导出",
    saveSettings: "保存设置",
    aboutTitle: "SSL证书自动续期",
    aboutBody: "用于在 Windows 上管理多域名证书配置，支持 Let's Encrypt DNS-01、手动步骤、一键运行和定时监控。Rust/Tauri 版本与 CLI 共用核心逻辑。",
    version: "版本",
    copied: "已复制",
    configSaved: "配置已保存"
  },
  en: {
    loading: "Loading...",
    loadFailed: "Failed to load config",
    profiles: "Domain Profiles",
    addProfile: "Add Profile",
    deleteProfile: "Delete Profile",
    vendorConfig: "Vendors",
    signerProgram: "Signer",
    monitor: "Monitor",
    openGithub: "Open repository",
    settings: "Settings",
    domain: "Domain",
    saveConfig: "Save",
    skipCertCheck: "Skip cert check gate",
    runAll: "Run All",
    log: "Logs",
    executeStep: "Run Current Step",
    checkCert: "Check Certificate",
    createOrder: "Create Order",
    dnsCheck: "Check DNS",
    issueCert: "Issue and Save",
    restartNginx: "Restart Nginx",
    certPemPath: "Certificate PEM path",
    keyPath: "Private KEY path",
    daysBeforeExpiry: "Renew before expiry days",
    email: "Email",
    dnsProvider: "DNS Provider",
    useSigner: "Use signer agent for DNS provider requests",
    signerPipe: "Signer Pipe",
    signerTitle: "Signer Agent",
    signerProvider: "Signer Provider",
    aliyun: "Aliyun",
    signerRootDomain: "Root domain",
    signerAllowedDomains: "Allowed domains (comma separated)",
    signerTtl: "Fixed TTL",
    signerInit: "Initialize Signer",
    signerInitPassphrase: "Initialization encryption passphrase",
    signerUnlockMenu: "Signer Unlock",
    signerStatus: "Check Status",
    signerStatusLabel: "Status",
    aliyunAccessKeyId: "Aliyun AccessKeyId",
    aliyunAccessKeySecret: "Aliyun AccessKeySecret",
    cloudflareToken: "Cloudflare API Token",
    signerUnlockPassphrase: "High-security unlock passphrase",
    signerUnlock: "Unlock Signer",
    signerLock: "Lock Signer",
    signerRuntimeStatus: "Runtime Status",
    signerAuthorizeTest: "Authorize Test",
    signerHint: "High-security mode encrypts DNS keys with a passphrase-derived key and protects metadata with Windows DPAPI. The signer must be unlocked after startup, and keys stay only in process memory.",
    forceRenew: "Force renewal",
    enabled: "Enabled",
    disabled: "Disabled",
    dnsEmptyOrder: "Run Create Order to show DNS TXT name and value here.",
    dnsEmptyCheck: "No DNS TXT record yet. Run Create Order first.",
    dnsCheckHint: "This checks whether the TXT record is visible. You can run it repeatedly.",
    issueHint: "This asks Let's Encrypt to validate DNS, issues the certificate, and saves it to the PEM/KEY paths above.",
    nginxEnabled: "Enable Nginx restart",
    nginxRestartMode: "Nginx restart mode",
    nginxKillStart: "Kill process + start process",
    nginxReload: "nginx -s reload",
    nginxExe: "Nginx exe path",
    nginxDir: "Nginx working directory",
    cancel: "Cancel",
    add: "Add",
    close: "Close",
    saveClose: "Save and Close",
    delete: "Delete",
    confirmDeleteTitle: "Delete Profile",
    confirmDeleteMessage: "Delete profile {domain}? Certificate files will not be removed.",
    addProfileTitle: "Add Domain Profile",
    addProfileHelp: "The new profile copies the current one as a template and creates default PEM/KEY paths.",
    vendorTitle: "Vendor Config",
    aliasPlaceholder: "Alias, e.g. AccessKeyId",
    envKeyPlaceholder: "Env key, e.g. Ali_Key",
    emptyVendor: "No environment variables configured for this vendor.",
    monitorConfig: "Monitor Profiles",
    monitorFrequency: "Schedule",
    daily: "Daily",
    interval: "Interval Minutes",
    cron: "Cron Expression",
    dailyTime: "Daily time",
    intervalMinutes: "Interval minutes",
    stopMonitor: "Stop Monitor",
    saveStart: "Save and Start",
    settingsTitle: "Settings",
    theme: "Theme",
    toast: "Toast",
    language: "Language",
    logSettings: "Logs",
    importExport: "Import/Export",
    about: "About",
    light: "Light",
    dark: "Dark",
    toastEnabled: "Enable Toast",
    toastPosition: "Position",
    toastDuration: "Duration (ms)",
    notification: "Notifications",
    notificationEnabled: "Enable notifications",
    notificationChannel: "Channel",
    dingtalk: "DingTalk",
    telegram: "Telegram",
    feishu: "Feishu",
    dingtalkAccessToken: "DingTalk robot token",
    dingtalkSecret: "DingTalk signing secret/password (optional)",
    telegramBotToken: "Telegram Bot Token",
    telegramChatId: "Telegram chat_id",
    feishuWebhookUrl: "Feishu Webhook URL",
    feishuSecret: "Feishu signing secret (optional)",
    notificationHint: "This only saves notification settings for now. It can later be wired to renewal success, failure, and monitor alerts.",
    notificationScope: "Notification Scope",
    manualStepNotifications: "Manual Step Notifications",
    monitorNotifications: "Monitor Notifications",
    successNotice: "Success",
    failureNotice: "Failure",
    monitorStartNotice: "Monitor started",
    monitorStopNotice: "Monitor stopped",
    monitorProfileStartNotice: "Profile monitor started",
    monitorNoRenewNeededNotice: "Check result: no renewal needed",
    monitorRenewNeededNotice: "Check result: renewal needed",
    monitorManualDnsSkippedNotice: "Manual DNS skipped",
    monitorFullSuccessNotice: "Steps 1-5 finished successfully",
    monitorFullFailureNotice: "Steps 1-5 failed",
    notificationScopeHint: "A practical default is to enable failure notices and monitor summary notices. Success notices are useful when you want a full audit trail.",
    dingtalkHint: "DingTalk custom bots use the webhook access_token. If signing is enabled, also fill in the secret.",
    telegramHint: "Telegram messages require a Bot Token and target chat_id.",
    feishuHint: "Feishu custom bots use a webhook URL. Fill in the secret only when signature verification is enabled.",
    topRight: "Top right",
    topLeft: "Top left",
    bottomRight: "Bottom right",
    bottomLeft: "Bottom left",
    languageLabel: "Display language",
    chinese: "中文",
    english: "English",
    logDir: "Log directory",
    logName: "Log file name",
    logMaxSize: "Max log size MB",
    logCurrentDomain: "Log settings are saved to the currently selected domain profile.",
    exportYaml: "Export YAML",
    importYaml: "Import YAML",
    chooseYaml: "Choose YAML file",
    importExportHint: "Export downloads the current profiles.yaml. Import first lets you choose a file, then confirms whether to overwrite all current settings.",
    importConfirmTitle: "Confirm Import",
    importConfirmMessage: "Overwrite current profiles.yaml with {file}? All domain profiles, settings, and monitor config will be replaced.",
    importSuccess: "Import completed",
    exportSuccess: "Export completed",
    exportCanceled: "Export canceled",
    saveSettings: "Save Settings",
    aboutTitle: "SSL Certificate Auto Renewal",
    aboutBody: "A Windows tool for multi-domain certificate profiles, Let's Encrypt DNS-01 renewal, manual steps, one-click runs, and scheduled monitoring. The Rust/Tauri GUI shares the same core with the CLI.",
    version: "Version",
    copied: "Copied ",
    configSaved: "Config saved"
  }
} as const;

type Lang = keyof typeof i18n;
type I18nKey = keyof typeof i18n.zh;

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value));
}

function safeDomainFilename(domain: string): string {
  return domain.trim().replace(/^\*\./, "wildcard.").replace(/\*/g, "wildcard").replace(/[\\/]/g, "_");
}

function defaultEmail(domain: string): string {
  return `admin@${domain.trim().replace(/^\*\./, "")}`;
}

export default function App() {
  const [store, setStore] = useState<Store | null>(null);
  const [current, setCurrent] = useState("");
  const [step, setStep] = useState(0);
  const [logs, setLogs] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [showVendor, setShowVendor] = useState(false);
  const [showMonitor, setShowMonitor] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showAddProfile, setShowAddProfile] = useState(false);
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [loadError, setLoadError] = useState("");
  const [skipCertCheckGate, setSkipCertCheckGate] = useState(true);
  const [dnsChallenges, setDnsChallenges] = useState<Record<string, DnsChallenge[]>>({});
  const logRef = useRef<HTMLPreElement | null>(null);
  const profile = current && store ? store.profiles[current] : null;
  const settings = { ...defaultSettings, ...(store?.app_settings ?? {}) };
  settings.toast = { ...defaultSettings.toast, ...(store?.app_settings?.toast ?? {}) };
  settings.notification = {
    ...defaultSettings.notification,
    ...(store?.app_settings?.notification ?? {}),
    scope: { ...defaultSettings.notification.scope, ...(store?.app_settings?.notification?.scope ?? {}) },
    dingtalk: { ...defaultSettings.notification.dingtalk, ...(store?.app_settings?.notification?.dingtalk ?? {}) },
    telegram: { ...defaultSettings.notification.telegram, ...(store?.app_settings?.notification?.telegram ?? {}) },
    feishu: { ...defaultSettings.notification.feishu, ...(store?.app_settings?.notification?.feishu ?? {}) }
  };
  const lang = (settings.language === "en" ? "en" : "zh") as Lang;
  const t = (key: I18nKey) => i18n[lang][key] ?? i18n.zh[key];
  const localizedSteps = stepKeys.map((key) => t(key));

  useEffect(() => {
    refresh();
    const unlisten = listen<string>("backend-log", (event) => appendLog(event.payload));
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (logRef.current) {
      logRef.current.scrollTop = logRef.current.scrollHeight;
    }
  }, [logs]);

  async function refresh() {
    try {
      setLoadError("");
      const loaded = await invoke<Store>("load_profiles");
      setStore(loaded);
      setCurrent(loaded.current_domain);
    } catch (error) {
      setLoadError(String(error));
      toast(`${t("loadFailed")}：${String(error)}`, "error");
    }
  }

  function toast(message: string, kind: Toast["kind"] = "info") {
    const toastSettings = { ...defaultSettings.toast, ...(store?.app_settings?.toast ?? {}) };
    if (!toastSettings.enabled) return;
    const id = Date.now() + Math.random();
    setToasts((items) => [...items, { id, message, kind }]);
    window.setTimeout(() => {
      setToasts((items) => items.filter((item) => item.id !== id));
    }, Math.max(800, Number(toastSettings.duration_ms) || defaultSettings.toast.duration_ms));
  }

  function appendLog(line: string) {
    setLogs((items) => [...items.slice(-800), line]);
  }

  function appendLogLines(lines: string[]) {
    setLogs((items) => [...items, ...lines].slice(-800));
  }

  async function openGithub() {
    try {
      await invoke("open_github_profile");
    } catch (error) {
      window.open("https://github.com/zengyufei/ssl-renew", "_blank", "noopener,noreferrer");
    }
  }

  async function copyText(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text);
      toast(`${t("copied")}${label}`, "success");
    } catch (error) {
      toast(`复制失败：${String(error)}`, "error");
    }
  }

  function logStepHeader(index: number) {
    appendLogLines([
      "",
      "======================================================================",
      `开始执行：${index + 1}. ${localizedSteps[index]}`,
      "======================================================================"
    ]);
  }

  function logStepFooter(index: number, ok: boolean, message?: string) {
    appendLogLines([
      ok ? `执行成功：${index + 1}. ${localizedSteps[index]}` : `执行失败：${index + 1}. ${localizedSteps[index]}`,
      ...(message ? [message] : []),
      "======================================================================"
    ]);
  }

  async function save(nextStore = store, preferredCurrent = current) {
    if (!nextStore) return null;
    const normalized = normalizeStoreForSave(nextStore, preferredCurrent);
    await invoke("save_profiles", { store: normalized });
    setStore(normalized);
    setCurrent(normalized.current_domain);
    appendLog(t("configSaved"));
    toast(t("configSaved"), "success");
    return normalized;
  }

  function normalizeStoreForSave(source: Store, preferredCurrent: string): Store {
    const next = clone(source);
    const profiles: Record<string, Profile> = {};
    Object.values(next.profiles).forEach((item) => {
      const domain = item.domain.trim();
      if (!domain) return;
      item.domain = domain;
      profiles[domain] = item;
    });
    next.profiles = profiles;
    const currentProfile = source.profiles[preferredCurrent];
    const preferredDomain = currentProfile?.domain.trim();
    if (preferredDomain && profiles[preferredDomain]) {
      next.current_domain = preferredDomain;
    } else if (!profiles[next.current_domain]) {
      next.current_domain = Object.keys(profiles)[0] ?? "";
    }
    next.monitor.profiles = next.monitor.profiles.filter((domain) => profiles[domain]);
    return next;
  }

  function updateProfile(mutator: (profile: Profile) => void) {
    if (!store || !profile) return;
    const next = clone(store);
    mutator(next.profiles[current]);
    next.current_domain = current;
    setStore(next);
  }

  function addProfile(domain: string) {
    if (!store || !profile) return;
    const trimmed = domain.trim();
    if (!trimmed) {
      toast("请输入域名", "error");
      return;
    }
    if (store.profiles[trimmed]) {
      toast(`${trimmed} 已存在`, "error");
      return;
    }
    const safe = safeDomainFilename(trimmed);
    const next = clone(store);
    const newProfile = clone(profile);
    newProfile.domain = trimmed;
    newProfile.email = defaultEmail(trimmed);
    newProfile.renew.force = false;
    newProfile.dns.provider = "manual";
    newProfile.paths.cert_file = `D:/cert/${safe}.pem`;
    newProfile.paths.key_file = `D:/cert/${safe}.key`;
    next.profiles[trimmed] = newProfile;
    next.current_domain = trimmed;
    setStore(next);
    setCurrent(trimmed);
    setStep(0);
    setShowAddProfile(false);
    appendLog(`已新增配置：${trimmed}`);
    toast(`已新增配置：${trimmed}`, "success");
  }

  function deleteProfile() {
    if (!store) return;
    const domains = Object.keys(store.profiles);
    if (domains.length <= 1) {
      toast("至少需要保留一个域名配置", "error");
      return;
    }
    const next = clone(store);
    delete next.profiles[current];
    next.monitor.profiles = next.monitor.profiles.filter((domain) => domain !== current);
    const nextCurrent = Object.keys(next.profiles)[0] ?? "";
    next.current_domain = nextCurrent;
    setStore(next);
    setCurrent(nextCurrent);
    setStep(0);
    setShowDeleteConfirm(false);
    appendLog(`已删除配置：${current}`);
    toast(`已删除配置：${current}`, "success");
  }

  async function executeStep(index: number, domain: string, strictDns: boolean, activeStore = store) {
    const activeProfile = activeStore?.profiles[domain];
    logStepHeader(index);
    if (index === 0) {
      const status = await invoke<any>("check_certificate_cmd", { domain, force: Boolean(activeProfile?.renew.force) });
      appendLog(status.message);
      return status;
    }
    if (index === 1) {
      appendLog(`强制申请证书：${activeProfile?.renew.force ? "是" : "否"}`);
      const challenges = await invoke<DnsChallenge[]>("create_order_cmd", { domain });
      setDnsChallenges((items) => ({ ...items, [domain]: challenges }));
      challenges.forEach((item) => {
        appendLog(`DNS TXT 名称：${item.txt_name}`);
        appendLog(`DNS TXT 主机记录：${item.rr_name}`);
        appendLog(`DNS TXT 值：${item.txt_value}`);
      });
      return true;
    }
    if (index === 2) {
      const visible = await invoke<boolean>("dns_check_cmd", { domain });
      appendLog(visible ? "DNS TXT 记录已生效" : "DNS TXT 记录尚未生效");
      if (!visible && strictDns) {
        throw new Error("DNS TXT 记录尚未生效，已停止后续步骤");
      }
      return visible;
    }
    if (index === 3) {
      await invoke("issue_cmd", { domain });
      appendLog("证书已签发并保存，DNS TXT 记录已保留");
      return true;
    }
    if (index === 4) {
      await invoke("restart_cmd", { domain });
      appendLog("Nginx 重启步骤已执行");
      return true;
    }
    return true;
  }

  async function runStep(index: number) {
    if (!profile) return;
    setBusy(true);
    try {
      const saved = await save();
      const domain = saved?.current_domain ?? current;
      await executeStep(index, domain, false, saved);
      logStepFooter(index, true);
      toast(`${localizedSteps[index]}执行成功`, "success");
    } catch (error) {
      logStepFooter(index, false, `失败原因：${String(error)}`);
      toast(`执行失败：${String(error)}`, "error");
    } finally {
      setBusy(false);
    }
  }

  async function runAllSteps() {
    if (!profile) return;
    setBusy(true);
    try {
      const saved = await save();
      const domain = saved?.current_domain ?? current;
      setStep(0);
      const status = await executeStep(0, domain, true, saved);
      logStepFooter(0, true);
      if (!skipCertCheckGate && !status.should_renew) {
        appendLog("未勾选“跳过证书检查”，当前证书未达到续期阈值，一键运行已在检查证书步骤结束。");
        appendLog("======================================================================");
        toast("证书暂不需要续期，已停止后续步骤", "info");
        return;
      }
      if (skipCertCheckGate && !status.should_renew) {
        appendLog("已勾选“跳过证书检查”，检查结果不拦截后续步骤。");
      }
      for (let index = 1; index < steps.length; index += 1) {
        setStep(index);
        await executeStep(index, domain, true, saved);
        logStepFooter(index, true);
      }
      toast("一键运行完成", "success");
    } catch (error) {
      appendLogLines([
        `一键运行失败：${String(error)}`,
        "======================================================================"
      ]);
      toast(`一键运行失败：${String(error)}`, "error");
    } finally {
      setBusy(false);
    }
  }

  const domains = useMemo(() => Object.keys(store?.profiles ?? {}), [store]);
  if (loadError) {
    return <div className="app loading">{t("loadFailed")}：{loadError}</div>;
  }
  if (!store || !profile) {
    return <div className="app">{t("loading")}</div>;
  }

  return (
    <div className={`app theme-${settings.theme === "dark" ? "dark" : "light"}`}>
      <aside>
        <h2>{t("profiles")}</h2>
        <div className="profile-list">
          {domains.map((domain) => (
            <button key={domain} className={domain === current ? "active" : ""} onClick={() => setCurrent(domain)}>
              {domain}
            </button>
          ))}
        </div>
        <div className="side-actions">
          <button onClick={() => setShowAddProfile(true)}>{t("addProfile")}</button>
          <button onClick={() => setShowDeleteConfirm(true)}>{t("deleteProfile")}</button>
        </div>
        <div className="bottom-tools">
          <button onClick={() => setShowVendor(true)}>{t("vendorConfig")}</button>
          <button onClick={() => setShowMonitor(true)}>{t("monitor")}</button>
        </div>
        <div className="app-footer">
          <button className="github-link" title={t("openGithub")} onClick={openGithub}>
            <GitHubIcon />
          </button>
          <div className="footer-right">
            <span className="version">v{appVersion}</span>
            <button className="settings-link" title={t("settings")} onClick={() => setShowSettings(true)}>
              <GearIcon />
            </button>
          </div>
        </div>
      </aside>
      <main>
        <div className="topbar">
          <label>{t("domain")}</label>
          <input value={profile.domain} onChange={(e) => updateProfile((p) => (p.domain = e.target.value))} />
          <button onClick={() => save()}>{t("saveConfig")}</button>
          <label className="run-option">
            <input type="checkbox" checked={skipCertCheckGate} onChange={(event) => setSkipCertCheckGate(event.target.checked)} />
            <span>{t("skipCertCheck")}</span>
          </label>
          <button className="primary run-all" disabled={busy} onClick={runAllSteps}>{t("runAll")}</button>
        </div>
        <div className="steps">
          {localizedSteps.map((name, index) => (
            <button key={name} className={step === index ? "active" : ""} onClick={() => setStep(index)}>
              {index + 1}. {name}
            </button>
          ))}
        </div>
        <section className="panel">
          <h1>{step + 1}. {localizedSteps[step]}</h1>
          {step === 0 && <CheckStep profile={profile} update={updateProfile} t={t} />}
          {step === 1 && <OrderStep profile={profile} update={updateProfile} t={t} />}
          {step === 2 && <DnsCheckStep challenges={dnsChallenges[current] ?? []} copy={copyText} t={t} />}
          {step === 3 && <IssueStep profile={profile} update={updateProfile} t={t} />}
          {step === 4 && <RestartStep profile={profile} update={updateProfile} t={t} />}
          <div className="actions">
            <button className="primary" disabled={busy} onClick={() => runStep(step)}>{t("executeStep")}</button>
          </div>
        </section>
        <section className="log">
          <h2>{t("log")}</h2>
          <pre ref={logRef}>{logs.join("\n")}</pre>
        </section>
      </main>
      <ToastHost toasts={toasts} position={settings.toast.position} />
      {showAddProfile && <AddProfileDialog close={() => setShowAddProfile(false)} submit={addProfile} t={t} />}
      {showDeleteConfirm && (
        <ConfirmDialog
          title={t("confirmDeleteTitle")}
          message={t("confirmDeleteMessage").replace("{domain}", current)}
          confirmText={t("delete")}
          close={() => setShowDeleteConfirm(false)}
          confirm={deleteProfile}
          t={t}
        />
      )}
      {showVendor && <VendorDialog store={store} setStore={setStore} close={() => setShowVendor(false)} save={save} t={t} />}
      {showMonitor && <MonitorDialog store={store} setStore={setStore} close={() => setShowMonitor(false)} toast={toast} t={t} />}
      {showSettings && (
        <SettingsDialog
          store={store}
          profile={profile}
          setStore={setStore}
          onImported={(nextStore) => {
            setStore(nextStore);
            setCurrent(nextStore.current_domain);
            setStep(0);
          }}
          close={() => setShowSettings(false)}
          save={save}
          toast={toast}
          t={t}
        />
      )}
    </div>
  );
}

function CheckStep({ profile, update, t }: { profile: Profile; update: (mutator: (profile: Profile) => void) => void; t: (key: I18nKey) => string }) {
  return (
    <div className="form">
      <Field label={t("certPemPath")} value={profile.paths.cert_file} onChange={(v) => update((p) => (p.paths.cert_file = v))} />
      <Field label={t("keyPath")} value={profile.paths.key_file} onChange={(v) => update((p) => (p.paths.key_file = v))} />
      <Field label={t("daysBeforeExpiry")} value={String(profile.renew.days_before_expiry)} onChange={(v) => update((p) => (p.renew.days_before_expiry = Number(v) || 30))} />
    </div>
  );
}

function OrderStep({
  profile,
  update,
  t
}: {
  profile: Profile;
  update: (mutator: (profile: Profile) => void) => void;
  t: (key: I18nKey) => string;
}) {
  const signerEnabled = profile.dns.provider === "signer";
  const signerPipe = profile.dns.signer?.pipe_name || "\\\\.\\pipe\\ssl-renew-signer";
  return (
    <div className="form">
      <Field label={t("email")} value={profile.email} onChange={(v) => update((p) => (p.email = v))} />
      <label>{t("dnsProvider")}</label>
      <select value={profile.dns.provider} onChange={(e) => update((p) => (p.dns.provider = e.target.value))}>
        {providerOptions.map(([value, label]) => <option key={value} value={value}>{label}</option>)}
      </select>
      <label>{t("useSigner")}</label>
      <Switch checked={signerEnabled} onChange={(checked) => update((p) => {
        p.dns.provider = checked ? "signer" : "manual";
        if (!p.dns.signer) p.dns.signer = { pipe_name: "\\\\.\\pipe\\ssl-renew-signer" };
      })} t={t} />
      {signerEnabled && (
        <>
          <Field label={t("signerPipe")} value={signerPipe} onChange={(value) => update((p) => {
            if (!p.dns.signer) p.dns.signer = { pipe_name: "\\\\.\\pipe\\ssl-renew-signer" };
            p.dns.signer.pipe_name = value;
          })} />
        </>
      )}
      <label>{t("forceRenew")}</label>
      <Switch checked={Boolean(profile.renew.force)} onChange={(checked) => update((p) => (p.renew.force = checked))} t={t} />
    </div>
  );
}

function DnsCheckStep({ challenges, copy, t }: { challenges: DnsChallenge[]; copy: (text: string, label: string) => void; t: (key: I18nKey) => string }) {
  return (
    <>
      <p>{t("dnsCheckHint")}</p>
      <DnsRecords challenges={challenges} copy={copy} emptyText={t("dnsEmptyCheck")} />
    </>
  );
}

function DnsRecords({
  challenges,
  copy,
  emptyText
}: {
  challenges: DnsChallenge[];
  copy: (text: string, label: string) => void;
  emptyText: string;
}) {
  if (challenges.length === 0) {
    return <div className="dns-empty">{emptyText}</div>;
  }
  return (
    <div className="dns-records">
      {challenges.map((item, index) => (
        <div className="dns-card" key={`${item.txt_name}-${index}`}>
          <div className="dns-row">
            <span>类型</span>
            <code>TXT</code>
          </div>
          <div className="dns-row">
            <span>名称</span>
            <code>{item.txt_name}</code>
            <button onClick={() => copy(item.txt_name, "名称")}>复制</button>
          </div>
          <div className="dns-row">
            <span>主机记录</span>
            <code>{item.rr_name}</code>
            <button onClick={() => copy(item.rr_name, "主机记录")}>复制</button>
          </div>
          <div className="dns-row">
            <span>值</span>
            <code>{item.txt_value}</code>
            <button onClick={() => copy(item.txt_value, "值")}>复制</button>
          </div>
        </div>
      ))}
    </div>
  );
}

function IssueStep({ profile, update, t }: { profile: Profile; update: (mutator: (profile: Profile) => void) => void; t: (key: I18nKey) => string }) {
  return (
    <div className="form">
      <Field label={t("certPemPath")} value={profile.paths.cert_file} onChange={(v) => update((p) => (p.paths.cert_file = v))} />
      <Field label={t("keyPath")} value={profile.paths.key_file} onChange={(v) => update((p) => (p.paths.key_file = v))} />
      <div className="form-note">
        {t("issueHint")}
      </div>
    </div>
  );
}

function RestartStep({ profile, update, t }: { profile: Profile; update: (mutator: (profile: Profile) => void) => void; t: (key: I18nKey) => string }) {
  return (
    <div className="form">
      <label>{t("nginxEnabled")}</label>
      <Switch checked={profile.nginx.enabled} onChange={(checked) => update((p) => (p.nginx.enabled = checked))} t={t} />
      <label>{t("nginxRestartMode")}</label>
      <select value={profile.nginx.restart_mode || "kill_start"} onChange={(event) => update((p) => (p.nginx.restart_mode = event.target.value))}>
        <option value="kill_start">{t("nginxKillStart")}</option>
        <option value="reload">{t("nginxReload")}</option>
      </select>
      <Field label={t("nginxExe")} value={profile.nginx.exe_path} onChange={(v) => update((p) => (p.nginx.exe_path = v))} />
      <Field label={t("nginxDir")} value={profile.nginx.working_dir} onChange={(v) => update((p) => (p.nginx.working_dir = v))} />
    </div>
  );
}

function Field({ label, value, onChange, type = "text" }: { label: string; value: string; onChange: (value: string) => void; type?: string }) {
  return (
    <>
      <label>{label}</label>
      <input type={type} value={value} onChange={(event) => onChange(event.target.value)} />
    </>
  );
}

function Switch({ checked, onChange, t }: { checked: boolean; onChange: (value: boolean) => void; t: (key: I18nKey) => string }) {
  return (
    <button type="button" className={`switch ${checked ? "on" : ""}`} onClick={() => onChange(!checked)}>
      <span />
      <strong>{checked ? t("enabled") : t("disabled")}</strong>
    </button>
  );
}

function Modal({
  title,
  children,
  footer,
  close,
  size = "normal"
}: {
  title: string;
  children: React.ReactNode;
  footer: React.ReactNode;
  close: () => void;
  size?: "small" | "normal";
}) {
  return (
    <div className="modal" onMouseDown={close}>
      <div className={`dialog ${size === "small" ? "small" : ""}`} onMouseDown={(event) => event.stopPropagation()}>
        <div className="dialog-title">
          <h2>{title}</h2>
          <button className="icon-button" onClick={close}>×</button>
        </div>
        <div className="dialog-body">{children}</div>
        <div className="dialog-footer">{footer}</div>
      </div>
    </div>
  );
}

function AddProfileDialog({ close, submit, t }: { close: () => void; submit: (domain: string) => void; t: (key: I18nKey) => string }) {
  const [domain, setDomain] = useState("");
  return (
    <Modal
      title={t("addProfileTitle")}
      close={close}
      size="small"
      footer={<><button onClick={close}>{t("cancel")}</button><button className="primary" onClick={() => submit(domain)}>{t("add")}</button></>}
    >
      <div className="dialog-form">
        <label>{t("domain")}</label>
        <input autoFocus placeholder="*.example.com" value={domain} onChange={(event) => setDomain(event.target.value)} />
        <p>{t("addProfileHelp")}</p>
      </div>
    </Modal>
  );
}

function ConfirmDialog({ title, message, confirmText, close, confirm, t }: { title: string; message: string; confirmText: string; close: () => void; confirm: () => void; t: (key: I18nKey) => string }) {
  return (
    <Modal
      title={title}
      close={close}
      size="small"
      footer={<><button onClick={close}>{t("cancel")}</button><button className="danger" onClick={confirm}>{confirmText}</button></>}
    >
      <p className="confirm-message">{message}</p>
    </Modal>
  );
}

function VendorDialog({ store, setStore, close, save, t }: { store: Store; setStore: (s: Store) => void; close: () => void; save: (s?: Store) => Promise<Store | null>; t: (key: I18nKey) => string }) {
  const [provider, setProvider] = useState("aliyun");
  const [alias, setAlias] = useState("");
  const [key, setKey] = useState("");
  const entries = store.vendor_configs[provider] ?? [];
  function add() {
    if (!alias.trim() || !key.trim()) return;
    const next = clone(store);
    next.vendor_configs[provider] = [...(next.vendor_configs[provider] ?? []), { alias: alias.trim(), key: key.trim() }];
    setStore(next);
    setAlias("");
    setKey("");
  }
  function remove(index: number) {
    const next = clone(store);
    next.vendor_configs[provider].splice(index, 1);
    setStore(next);
  }
  return (
    <Modal
      title={t("vendorTitle")}
      close={close}
      footer={<><button onClick={close}>{t("close")}</button><button className="primary" onClick={() => save(store).then(() => close())}>{t("saveClose")}</button></>}
    >
      <div className="vendor-layout">
        <div className="vendor-list">
          {vendorProviderOptions.map(([value, label]) => (
            <button key={value} className={provider === value ? "active" : ""} onClick={() => setProvider(value)}>
              {label}
            </button>
          ))}
        </div>
        <div className="vendor-panel">
          <div className="env-list">
            {entries.length === 0 && <p className="empty">{t("emptyVendor")}</p>}
            {entries.map((entry, index) => (
              <div className="env-row" key={`${entry.alias}-${entry.key}`}>
                <span>{entry.alias}</span>
                <code>{entry.key}</code>
                <button onClick={() => remove(index)}>删除</button>
              </div>
            ))}
          </div>
          <div className="inline-form">
            <input placeholder={t("aliasPlaceholder")} value={alias} onChange={(event) => setAlias(event.target.value)} />
            <input placeholder={t("envKeyPlaceholder")} value={key} onChange={(event) => setKey(event.target.value)} />
            <button onClick={add}>{t("add")}</button>
          </div>
        </div>
      </div>
    </Modal>
  );
}

function SignerPanel({ toast, t }: { toast: (message: string, kind?: Toast["kind"]) => void; t: (key: I18nKey) => string }) {
  const [provider, setProvider] = useState("aliyun");
  const [rootDomain, setRootDomain] = useState("");
  const [allowedDomains, setAllowedDomains] = useState("");
  const [ttl, setTtl] = useState("");
  const [pipeName, setPipeName] = useState("\\\\.\\pipe\\ssl-renew-signer");
  const [aliyunKeyId, setAliyunKeyId] = useState("");
  const [aliyunKeySecret, setAliyunKeySecret] = useState("");
  const [cloudflareToken, setCloudflareToken] = useState("");
  const [initPassphrase, setInitPassphrase] = useState("");
  const [status, setStatus] = useState("");

  async function initSigner() {
    const message = await invoke<string>("init_signer_cmd", {
      request: {
        provider,
        root_domain: rootDomain,
        allowed_domains: allowedDomains.split(",").map((item) => item.trim()).filter(Boolean),
        ttl: ttl ? Number(ttl) : null,
        pipe_name: pipeName,
        protection_mode: "passphrase_dpapi_v1",
        unlock_passphrase: initPassphrase,
        aliyun_access_key_id: provider === "aliyun" ? aliyunKeyId : null,
        aliyun_access_key_secret: provider === "aliyun" ? aliyunKeySecret : null,
        aliyun_endpoint: null,
        cloudflare_token: provider === "cloudflare" ? cloudflareToken : null,
        cloudflare_endpoint: null
      }
    });
    setAliyunKeyId("");
    setAliyunKeySecret("");
    setCloudflareToken("");
    setInitPassphrase("");
    setStatus(message);
    toast(message, "success");
  }

  async function checkStatus() {
    const message = await invoke<string>("signer_status_cmd");
    setStatus(message);
    toast(message, "info");
  }

  return (
    <div className="settings-section signer-section">
      <div className="form signer-form">
        <label>{t("signerProvider")}</label>
        <select value={provider} onChange={(event) => setProvider(event.target.value)}>
          <option value="aliyun">{t("aliyun")}</option>
          <option value="cloudflare">Cloudflare</option>
        </select>
        <Field label={t("signerRootDomain")} value={rootDomain} onChange={setRootDomain} />
        <Field label={t("signerAllowedDomains")} value={allowedDomains} onChange={setAllowedDomains} />
        <Field label={t("signerTtl")} value={ttl} onChange={setTtl} />
        <Field label={t("signerPipe")} value={pipeName} onChange={setPipeName} />
        {provider === "aliyun" && (
          <>
            <Field label={t("aliyunAccessKeyId")} value={aliyunKeyId} type="password" onChange={setAliyunKeyId} />
            <Field label={t("aliyunAccessKeySecret")} value={aliyunKeySecret} type="password" onChange={setAliyunKeySecret} />
          </>
        )}
        {provider === "cloudflare" && (
          <Field label={t("cloudflareToken")} value={cloudflareToken} type="password" onChange={setCloudflareToken} />
        )}
        <Field label={t("signerInitPassphrase")} value={initPassphrase} type="password" onChange={setInitPassphrase} />
        <p className="form-note">{t("signerHint")}</p>
        {status && (
          <>
            <label>{t("signerStatusLabel")}</label>
            <code>{status}</code>
          </>
        )}
      </div>
      <div className="settings-action-row">
        <button onClick={checkStatus}>{t("signerStatus")}</button>
        <button className="primary" onClick={initSigner}>{t("signerInit")}</button>
      </div>
    </div>
  );
}

function SignerUnlockPanel({ toast, t }: { toast: (message: string, kind?: Toast["kind"]) => void; t: (key: I18nKey) => string }) {
  const [pipeName, setPipeName] = useState("\\\\.\\pipe\\ssl-renew-signer");
  const [unlockPassphrase, setUnlockPassphrase] = useState("");
  const [status, setStatus] = useState("");

  async function runtimeStatus() {
    const message = await invoke<string>("signer_runtime_status_cmd", { pipeName });
    setStatus(message);
    toast(message, "info");
  }

  async function unlockSigner() {
    const message = await invoke<string>("unlock_signer_cmd", { pipeName, passphrase: unlockPassphrase });
    setUnlockPassphrase("");
    setStatus(message);
    toast(message, "success");
  }

  async function lockSigner() {
    const message = await invoke<string>("lock_signer_cmd", { pipeName });
    setStatus(message);
    toast(message, "success");
  }

  async function authorizeTest() {
    const message = await invoke<string>("signer_authorize_test_cmd", { pipeName });
    setStatus(message);
    toast(message, "success");
  }

  return (
    <div className="settings-section signer-section">
      <div className="form signer-form">
        <Field label={t("signerPipe")} value={pipeName} onChange={setPipeName} />
        <Field label={t("signerUnlockPassphrase")} value={unlockPassphrase} type="password" onChange={setUnlockPassphrase} />
        <p className="form-note">{t("signerHint")}</p>
        {status && (
          <>
            <label>{t("signerStatusLabel")}</label>
            <code>{status}</code>
          </>
        )}
      </div>
      <div className="settings-action-row">
        <button onClick={runtimeStatus}>{t("signerRuntimeStatus")}</button>
        <button onClick={authorizeTest}>{t("signerAuthorizeTest")}</button>
        <button onClick={lockSigner}>{t("signerLock")}</button>
        <button className="primary" onClick={unlockSigner}>{t("signerUnlock")}</button>
      </div>
    </div>
  );
}

function MonitorDialog({ store, setStore, close, toast, t }: { store: Store; setStore: (s: Store) => void; close: () => void; toast: (message: string, kind?: Toast["kind"]) => void; t: (key: I18nKey) => string }) {
  const monitor = store.monitor;
  function update(mutator: (monitor: MonitorConfig) => void) {
    const next = clone(store);
    mutator(next.monitor);
    setStore(next);
  }
  async function start() {
    await invoke("save_profiles", { store });
    await invoke("start_monitor_cmd");
    toast("监控已启动", "success");
    close();
  }
  async function stop() {
    await invoke("stop_monitor_cmd");
    toast("监控已停止", "info");
    close();
  }
  return (
    <Modal
      title={t("monitor")}
      close={close}
      footer={<><button onClick={close}>{t("close")}</button><button onClick={stop}>{t("stopMonitor")}</button><button className="primary" onClick={start}>{t("saveStart")}</button></>}
    >
      <div className="monitor-grid">
        <section>
          <h3>{t("monitorConfig")}</h3>
          <div className="monitor-profile-list">
            {Object.keys(store.profiles).map((domain) => (
              <label className="check-card" key={domain}>
                <input
                  type="checkbox"
                  checked={monitor.profiles.includes(domain)}
                  onChange={(e) => update((m) => {
                    m.profiles = e.target.checked ? [...m.profiles, domain] : m.profiles.filter((item) => item !== domain);
                  })}
                />
                <span>{domain}</span>
              </label>
            ))}
          </div>
        </section>
        <section>
          <h3>{t("monitorFrequency")}</h3>
          <div className="radio-group">
            <RadioCard label={t("daily")} value="daily" current={monitor.mode} update={(value) => update((m) => (m.mode = value))} />
            <RadioCard label={t("interval")} value="interval" current={monitor.mode} update={(value) => update((m) => (m.mode = value))} />
            <RadioCard label={t("cron")} value="cron" current={monitor.mode} update={(value) => update((m) => (m.mode = value))} />
          </div>
          <div className="frequency-box">
            {monitor.mode === "daily" && <Field label={t("dailyTime")} value={monitor.daily_time} onChange={(v) => update((m) => (m.daily_time = v))} />}
            {monitor.mode === "interval" && <Field label={t("intervalMinutes")} value={String(monitor.interval_minutes)} onChange={(v) => update((m) => (m.interval_minutes = Number(v) || 1440))} />}
            {monitor.mode === "cron" && <Field label={t("cron")} value={monitor.cron_expression} onChange={(v) => update((m) => (m.cron_expression = v))} />}
          </div>
        </section>
      </div>
    </Modal>
  );
}

function SettingsDialog({
  store,
  profile,
  setStore,
  onImported,
  close,
  save,
  toast,
  t
}: {
  store: Store;
  profile: Profile;
  setStore: (s: Store) => void;
  onImported: (store: Store) => void;
  close: () => void;
  save: (s?: Store) => Promise<Store | null>;
  toast: (message: string, kind?: Toast["kind"]) => void;
  t: (key: I18nKey) => string;
}) {
  const [active, setActive] = useState<"theme" | "toast" | "notification" | "signer" | "signerUnlock" | "language" | "logs" | "importExport" | "about">("theme");
  const [pendingImport, setPendingImport] = useState<{ name: string; text: string } | null>(null);
  const [importExportMessage, setImportExportMessage] = useState("");
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const settings = { ...defaultSettings, ...(store.app_settings ?? {}) };
  settings.toast = { ...defaultSettings.toast, ...(store.app_settings?.toast ?? {}) };
  settings.notification = {
    ...defaultSettings.notification,
    ...(store.app_settings?.notification ?? {}),
    scope: { ...defaultSettings.notification.scope, ...(store.app_settings?.notification?.scope ?? {}) },
    dingtalk: { ...defaultSettings.notification.dingtalk, ...(store.app_settings?.notification?.dingtalk ?? {}) },
    telegram: { ...defaultSettings.notification.telegram, ...(store.app_settings?.notification?.telegram ?? {}) },
    feishu: { ...defaultSettings.notification.feishu, ...(store.app_settings?.notification?.feishu ?? {}) }
  };
  const logParts = splitLogPath(profile.paths.log_file || "./logs/ssl-renew.log");

  function updateStore(mutator: (next: Store) => void) {
    const next = clone(store);
    next.app_settings = { ...defaultSettings, ...(next.app_settings ?? {}) };
    next.app_settings.toast = { ...defaultSettings.toast, ...(next.app_settings.toast ?? {}) };
    next.app_settings.notification = {
      ...defaultSettings.notification,
      ...(next.app_settings.notification ?? {}),
      scope: { ...defaultSettings.notification.scope, ...(next.app_settings.notification?.scope ?? {}) },
      dingtalk: { ...defaultSettings.notification.dingtalk, ...(next.app_settings.notification?.dingtalk ?? {}) },
      telegram: { ...defaultSettings.notification.telegram, ...(next.app_settings.notification?.telegram ?? {}) },
      feishu: { ...defaultSettings.notification.feishu, ...(next.app_settings.notification?.feishu ?? {}) }
    };
    mutator(next);
    setStore(next);
  }

  function updateCurrentProfile(mutator: (profile: Profile) => void) {
    updateStore((next) => {
      const currentProfile = next.profiles[next.current_domain] ?? Object.values(next.profiles).find((item) => item.domain === profile.domain);
      if (currentProfile) mutator(currentProfile);
    });
  }

  function setLogFile(dir: string, name: string) {
    const cleanDir = dir.trim() || "./logs";
    const cleanName = name.trim() || "ssl-renew.log";
    const needsSlash = !/[\\/]$/.test(cleanDir);
    const slash = cleanDir.includes("\\") && !cleanDir.includes("/") ? "\\" : "/";
    updateCurrentProfile((p) => (p.paths.log_file = `${cleanDir}${needsSlash ? slash : ""}${cleanName}`));
  }

  const menus: Array<[typeof active, I18nKey]> = [
    ["theme", "theme"],
    ["toast", "toast"],
    ["notification", "notification"],
    ["signer", "signerProgram"],
    ["signerUnlock", "signerUnlockMenu"],
    ["language", "language"],
    ["logs", "logSettings"],
    ["importExport", "importExport"],
    ["about", "about"]
  ];
  const stepNoticeRows: Array<[I18nKey, keyof NotificationScope, keyof NotificationScope]> = [
    ["checkCert", "step_check_success", "step_check_failure"],
    ["createOrder", "step_order_success", "step_order_failure"],
    ["dnsCheck", "step_dns_check_success", "step_dns_check_failure"],
    ["issueCert", "step_issue_success", "step_issue_failure"],
    ["restartNginx", "step_restart_success", "step_restart_failure"]
  ];
  const monitorNoticeRows: Array<[I18nKey, keyof NotificationScope]> = [
    ["monitorStartNotice", "monitor_start"],
    ["monitorStopNotice", "monitor_stop"],
    ["monitorProfileStartNotice", "monitor_profile_start"],
    ["monitorNoRenewNeededNotice", "monitor_no_renew_needed"],
    ["monitorRenewNeededNotice", "monitor_renew_needed"],
    ["monitorManualDnsSkippedNotice", "monitor_manual_dns_skipped"],
    ["monitorFullSuccessNotice", "monitor_full_success"],
    ["monitorFullFailureNotice", "monitor_full_failure"]
  ];

  function updateScope(key: keyof NotificationScope, checked: boolean) {
    updateStore((next) => (next.app_settings.notification.scope[key] = checked));
  }

  async function exportYaml() {
    await save(store);
    const path = await invoke<string | null>("export_profiles_yaml_to_file");
    setImportExportMessage(path ? `${t("exportSuccess")}：${path}` : t("exportCanceled"));
  }

  function chooseImportFile() {
    fileInputRef.current?.click();
  }

  function readImportFile(file: File | undefined) {
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      setPendingImport({ name: file.name, text: String(reader.result ?? "") });
    };
    reader.readAsText(file, "utf-8");
  }

  async function confirmImport() {
    if (!pendingImport) return;
    const imported = await invoke<Store>("import_profiles_yaml", { text: pendingImport.text });
    onImported(imported);
    setImportExportMessage(t("importSuccess"));
    setPendingImport(null);
    close();
  }

  return (
    <Modal
      title={t("settingsTitle")}
      close={close}
      footer={<><button onClick={close}>{t("close")}</button><button className="primary" onClick={() => save(store).then(() => close())}>{t("saveSettings")}</button></>}
    >
      <div className="settings-layout">
        <div className="settings-menu">
          {menus.map(([value, label]) => (
            <button key={value} className={active === value ? "active" : ""} onClick={() => setActive(value)}>
              {t(label)}
            </button>
          ))}
        </div>
        <div className="settings-panel">
          {active === "theme" && (
            <div className="settings-section">
              <h3>{t("theme")}</h3>
              <div className="segmented">
                <button className={settings.theme !== "dark" ? "active" : ""} onClick={() => updateStore((next) => (next.app_settings.theme = "light"))}>{t("light")}</button>
                <button className={settings.theme === "dark" ? "active" : ""} onClick={() => updateStore((next) => (next.app_settings.theme = "dark"))}>{t("dark")}</button>
              </div>
            </div>
          )}
          {active === "toast" && (
            <div className="settings-section form">
              <label>{t("toastEnabled")}</label>
              <Switch checked={settings.toast.enabled} onChange={(checked) => updateStore((next) => (next.app_settings.toast.enabled = checked))} t={t} />
              <label>{t("toastPosition")}</label>
              <select value={settings.toast.position} onChange={(event) => updateStore((next) => (next.app_settings.toast.position = event.target.value))}>
                <option value="top-right">{t("topRight")}</option>
                <option value="top-left">{t("topLeft")}</option>
                <option value="bottom-right">{t("bottomRight")}</option>
                <option value="bottom-left">{t("bottomLeft")}</option>
              </select>
              <Field label={t("toastDuration")} value={String(settings.toast.duration_ms)} onChange={(value) => updateStore((next) => (next.app_settings.toast.duration_ms = Number(value) || 3200))} />
            </div>
          )}
          {active === "notification" && (
            <div className="settings-section notification-section">
              <div className="form">
                <label>{t("notificationEnabled")}</label>
                <Switch checked={settings.notification.enabled} onChange={(checked) => updateStore((next) => (next.app_settings.notification.enabled = checked))} t={t} />
                <label>{t("notificationChannel")}</label>
                <select value={settings.notification.channel} onChange={(event) => updateStore((next) => (next.app_settings.notification.channel = event.target.value))}>
                  <option value="dingtalk">{t("dingtalk")}</option>
                  <option value="telegram">{t("telegram")}</option>
                  <option value="feishu">{t("feishu")}</option>
                </select>
              </div>
              {settings.notification.channel === "dingtalk" && (
                <div className="form notification-fields">
                  <Field label={t("dingtalkAccessToken")} value={settings.notification.dingtalk.access_token} type="password" onChange={(value) => updateStore((next) => (next.app_settings.notification.dingtalk.access_token = value))} />
                  <Field label={t("dingtalkSecret")} value={settings.notification.dingtalk.secret} type="password" onChange={(value) => updateStore((next) => (next.app_settings.notification.dingtalk.secret = value))} />
                  <p className="form-note">{t("dingtalkHint")}</p>
                </div>
              )}
              {settings.notification.channel === "telegram" && (
                <div className="form notification-fields">
                  <Field label={t("telegramBotToken")} value={settings.notification.telegram.bot_token} type="password" onChange={(value) => updateStore((next) => (next.app_settings.notification.telegram.bot_token = value))} />
                  <Field label={t("telegramChatId")} value={settings.notification.telegram.chat_id} onChange={(value) => updateStore((next) => (next.app_settings.notification.telegram.chat_id = value))} />
                  <p className="form-note">{t("telegramHint")}</p>
                </div>
              )}
              {settings.notification.channel === "feishu" && (
                <div className="form notification-fields">
                  <Field label={t("feishuWebhookUrl")} value={settings.notification.feishu.webhook_url} onChange={(value) => updateStore((next) => (next.app_settings.notification.feishu.webhook_url = value))} />
                  <Field label={t("feishuSecret")} value={settings.notification.feishu.secret} type="password" onChange={(value) => updateStore((next) => (next.app_settings.notification.feishu.secret = value))} />
                  <p className="form-note">{t("feishuHint")}</p>
                </div>
              )}
              <div className="notification-scope">
                <h3>{t("notificationScope")}</h3>
                <section className="scope-panel">
                  <div className="scope-title">{t("manualStepNotifications")}</div>
                  <div className="scope-table">
                    <div />
                    <strong>{t("successNotice")}</strong>
                    <strong>{t("failureNotice")}</strong>
                    {stepNoticeRows.map(([label, successKey, failureKey]) => (
                      <React.Fragment key={label}>
                        <span>{t(label)}</span>
                        <ScopeCheckbox checked={settings.notification.scope[successKey]} onChange={(checked) => updateScope(successKey, checked)} />
                        <ScopeCheckbox checked={settings.notification.scope[failureKey]} onChange={(checked) => updateScope(failureKey, checked)} />
                      </React.Fragment>
                    ))}
                  </div>
                </section>
                <section className="scope-panel">
                  <div className="scope-title">{t("monitorNotifications")}</div>
                  <div className="scope-grid">
                    {monitorNoticeRows.map(([label, key]) => (
                      <label className="scope-card" key={key}>
                        <input type="checkbox" checked={settings.notification.scope[key]} onChange={(event) => updateScope(key, event.target.checked)} />
                        <span>{t(label)}</span>
                      </label>
                    ))}
                  </div>
                </section>
              </div>
              <p className="settings-hint">{t("notificationScopeHint")}</p>
              <p className="settings-hint">{t("notificationHint")}</p>
            </div>
          )}
          {active === "signer" && (
            <SignerPanel toast={toast} t={t} />
          )}
          {active === "signerUnlock" && (
            <SignerUnlockPanel toast={toast} t={t} />
          )}
          {active === "language" && (
            <div className="settings-section form">
              <label>{t("languageLabel")}</label>
              <select value={settings.language === "en" ? "en" : "zh"} onChange={(event) => updateStore((next) => (next.app_settings.language = event.target.value))}>
                <option value="zh">{t("chinese")}</option>
                <option value="en">{t("english")}</option>
              </select>
            </div>
          )}
          {active === "logs" && (
            <div className="settings-section form">
              <Field label={t("logDir")} value={logParts.dir} onChange={(value) => setLogFile(value, logParts.name)} />
              <Field label={t("logName")} value={logParts.name} onChange={(value) => setLogFile(logParts.dir, value)} />
              <Field label={t("logMaxSize")} value={String(profile.paths.max_log_size_mb)} onChange={(value) => updateCurrentProfile((p) => (p.paths.max_log_size_mb = Number(value) || 10))} />
              <p className="form-note">{t("logCurrentDomain")}</p>
            </div>
          )}
          {active === "importExport" && (
            <div className="settings-section import-export-section">
              <p className="settings-hint">{t("importExportHint")}</p>
              <div className="import-export-actions">
                <button className="primary" onClick={exportYaml}>{t("exportYaml")}</button>
                <button onClick={chooseImportFile}>{t("chooseYaml")}</button>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".yaml,.yml,application/x-yaml,text/yaml,text/plain"
                  hidden
                  onChange={(event) => {
                    readImportFile(event.target.files?.[0]);
                    event.target.value = "";
                  }}
                />
              </div>
              {importExportMessage && <p className="settings-hint">{importExportMessage}</p>}
            </div>
          )}
          {active === "about" && (
            <div className="settings-section about-panel">
              <h3>{t("aboutTitle")}</h3>
              <p>{t("aboutBody")}</p>
              <dl>
                <dt>{t("version")}</dt>
                <dd>v{appVersion}</dd>
                <dt>GitHub</dt>
                <dd>github.com/zengyufei/ssl-renew</dd>
                <dt>ACME</dt>
                <dd>Let's Encrypt production / DNS-01</dd>
              </dl>
            </div>
          )}
        </div>
      </div>
      {pendingImport && (
        <ConfirmDialog
          title={t("importConfirmTitle")}
          message={t("importConfirmMessage").replace("{file}", pendingImport.name)}
          confirmText={t("importYaml")}
          close={() => setPendingImport(null)}
          confirm={confirmImport}
          t={t}
        />
      )}
    </Modal>
  );
}

function splitLogPath(value: string) {
  const normalized = value.trim() || "./logs/ssl-renew.log";
  const slashIndex = Math.max(normalized.lastIndexOf("/"), normalized.lastIndexOf("\\"));
  if (slashIndex < 0) return { dir: "./logs", name: normalized || "ssl-renew.log" };
  return {
    dir: normalized.slice(0, slashIndex) || "./logs",
    name: normalized.slice(slashIndex + 1) || "ssl-renew.log"
  };
}

function ScopeCheckbox({ checked, onChange }: { checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <label className="scope-check">
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
    </label>
  );
}

function RadioCard({ label, value, current, update }: { label: string; value: string; current: string; update: (value: string) => void }) {
  return (
    <label className={`radio-card ${current === value ? "active" : ""}`}>
      <input type="radio" checked={current === value} onChange={() => update(value)} />
      <span>{label}</span>
    </label>
  );
}

function ToastHost({ toasts, position }: { toasts: Toast[]; position: string }) {
  return (
    <div className={`toast-host ${position}`}>
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast ${toast.kind}`}>{toast.message}</div>
      ))}
    </div>
  );
}

function GitHubIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" width="18" height="18" fill="currentColor">
      <path d="M12 .5C5.65.5.5 5.65.5 12c0 5.1 3.29 9.42 7.86 10.95.58.1.79-.25.79-.56v-2.15c-3.2.69-3.87-1.36-3.87-1.36-.53-1.34-1.29-1.7-1.29-1.7-1.05-.72.08-.71.08-.71 1.16.08 1.77 1.2 1.77 1.2 1.03 1.76 2.7 1.25 3.36.96.1-.75.4-1.25.73-1.54-2.55-.29-5.23-1.28-5.23-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.04 0 0 .97-.31 3.17 1.18A10.9 10.9 0 0 1 12 6.07c.98 0 1.96.13 2.88.39 2.2-1.49 3.16-1.18 3.16-1.18.63 1.58.24 2.75.12 3.04.74.81 1.19 1.83 1.19 3.09 0 4.41-2.69 5.38-5.25 5.67.42.36.79 1.07.79 2.16v3.15c0 .31.21.67.8.56A11.52 11.52 0 0 0 23.5 12C23.5 5.65 18.35.5 12 .5Z" />
    </svg>
  );
}

function GearIcon() {
  return (
    <svg aria-hidden="true" viewBox="-1 -1 26 26" width="20" height="20" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round">
      <path d="M12 15.5A3.5 3.5 0 1 0 12 8a3.5 3.5 0 0 0 0 7.5Z" />
      <path d="M19.4 15a1.8 1.8 0 0 0 .36 1.98l.06.06a2.1 2.1 0 1 1-2.97 2.97l-.06-.06a1.8 1.8 0 0 0-1.98-.36 1.8 1.8 0 0 0-1.09 1.65V21.4a2.1 2.1 0 1 1-4.2 0v-.09a1.8 1.8 0 0 0-1.18-1.66 1.8 1.8 0 0 0-1.98.36l-.06.06a2.1 2.1 0 1 1-2.97-2.97l.06-.06A1.8 1.8 0 0 0 3 15.06 1.8 1.8 0 0 0 1.35 14H1.2a2.1 2.1 0 1 1 0-4.2h.09A1.8 1.8 0 0 0 2.95 8.6a1.8 1.8 0 0 0-.36-1.98l-.06-.06A2.1 2.1 0 1 1 5.5 3.59l.06.06a1.8 1.8 0 0 0 1.98.36A1.8 1.8 0 0 0 8.6 2.36V2.2a2.1 2.1 0 1 1 4.2 0v.09a1.8 1.8 0 0 0 1.18 1.66 1.8 1.8 0 0 0 1.98-.36l.06-.06a2.1 2.1 0 1 1 2.97 2.97l-.06.06a1.8 1.8 0 0 0-.36 1.98 1.8 1.8 0 0 0 1.65 1.09h.16a2.1 2.1 0 1 1 0 4.2h-.09A1.8 1.8 0 0 0 19.4 15Z" />
    </svg>
  );
}
