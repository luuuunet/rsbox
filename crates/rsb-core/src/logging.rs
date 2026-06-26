// 日志级别配置模块
// 生产环境默认只输出 WARN 和 ERROR

use tracing_subscriber::{fmt, EnvFilter};

pub fn init_logging() {
    // 生产环境：只显示警告和错误
    // 开发环境：通过 RUST_LOG 环境变量控制
    let default_level = if cfg!(debug_assertions) {
        "info"  // 开发模式：info 级别
    } else {
        "warn"  // 生产模式：只显示 warn 和 error
    };

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level));

    fmt()
        .with_env_filter(filter)
        .with_target(false)  // 不显示模块路径
        .with_thread_ids(false)  // 不显示线程 ID
        .with_thread_names(false)  // 不显示线程名
        .compact()  // 紧凑格式
        .init();
}

// 条件编译：生产环境移除 debug/trace 日志
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        tracing::debug!($($arg)*);
    };
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        tracing::trace!($($arg)*);
    };
}

// 保留的日志宏（生产环境也会输出）
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        tracing::info!($($arg)*);
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        tracing::warn!($($arg)*);
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        tracing::error!($($arg)*);
    };
}
