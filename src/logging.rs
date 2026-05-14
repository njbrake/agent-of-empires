//! Centralized logging configuration + runtime filter control.
//!
//! Single source of truth for env-var resolution, default-filter
//! construction, and the reloadable subscriber handle. Both the main
//! daemon and cockpit runner subprocesses use this module so they
//! agree on what `AOE_LOG_LEVEL=debug` means.
//!
//! The process-global `FilterController` is exposed via free
//! functions (`set_filter`, `set_level`, `current_filter`). Tracing's
//! subscriber is already a process-wide singleton; we mirror that
//! design rather than threading a handle through application state.
//! `Mutex<Option<Arc<_>>>` (over `OnceLock`) so tests can reset.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::reload;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Registry;

/// Top-level tracing target roots. The default filter expands a
/// single level (e.g. "debug") to one directive per root so user-defined
/// targets like `auth.token`, `process.signal`, `git.command` inherit
/// the same level.
pub const DEFAULT_TARGET_ROOTS: &[&str] = &[
    "agent_of_empires",
    "cockpit",
    "terminal",
    "auth",
    "process",
    "update",
    "containers",
    "git",
    "migrations",
    "web",
    // `log` is the meta-target prefix for filter-swap audit events
    // (`log.runtime`). Without this, `log.runtime` would be dropped
    // under any expanded-level filter that has no global default.
    "log",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "trace" => Some(Self::Trace),
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

/// Resolved logging configuration. Pure data; env-touching lives only in
/// `from_env` so the rest of the module is unit-testable without env hacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogConfig {
    pub level: Option<LogLevel>,
    pub acp_trace: bool,
    pub terminal_trace: bool,
}

impl LogConfig {
    pub fn from_env() -> Self {
        let level = std::env::var("AOE_LOG_LEVEL")
            .ok()
            .and_then(|v| LogLevel::parse(&v))
            .or_else(|| {
                if std::env::var("AGENT_OF_EMPIRES_DEBUG").is_ok() {
                    Some(LogLevel::Debug)
                } else {
                    None
                }
            });
        Self {
            level,
            acp_trace: std::env::var("AOE_ACP_TRACE").is_ok(),
            terminal_trace: std::env::var("AOE_TERMINAL_TRACE").is_ok(),
        }
    }

    /// Default for foreground `aoe serve` (info level, no overlays).
    pub fn serve_default() -> Self {
        Self {
            level: Some(LogLevel::Info),
            acp_trace: false,
            terminal_trace: false,
        }
    }

    /// EnvFilter directive string. None when level unset.
    pub fn filter_string(&self) -> Option<String> {
        let level = self.level?;
        let mut s = Self::filter_for_level(level);
        if self.acp_trace {
            s.push_str(
                ",agent_client_protocol=debug,agent_client_protocol::jsonrpc::transport_actor=trace",
            );
        }
        if self.terminal_trace {
            s.push_str(",terminal=trace");
        }
        Some(s)
    }

    /// Expand a level to one directive per target root.
    pub fn filter_for_level(level: LogLevel) -> String {
        let lvl = level.as_str();
        DEFAULT_TARGET_ROOTS
            .iter()
            .map(|t| format!("{t}={lvl}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

pub enum SubscriberTarget {
    File(PathBuf),
    Stdout,
}

pub struct InitResult {
    pub controller: Option<Arc<FilterController>>,
    pub warning: Option<String>,
}

pub struct FilterController {
    inner: reload::Handle<EnvFilter, Registry>,
    current: Mutex<String>,
}

impl FilterController {
    pub fn current(&self) -> String {
        self.current.lock().unwrap().clone()
    }

    pub fn set_filter(&self, directive: &str) -> Result<SwapResult, LogFilterError> {
        let directive = directive.trim();
        if directive.is_empty() {
            return Err(LogFilterError::Invalid("empty filter".into()));
        }
        // Bare global levels would enable debug for hyper/rustls/tower etc.
        // Use set_level when you mean "everything we own at this level".
        if LogLevel::parse(directive).is_some() {
            return Err(LogFilterError::BareGlobalLevel);
        }
        let filter = EnvFilter::builder()
            .with_regex(false)
            .parse(directive)
            .map_err(|e| LogFilterError::Invalid(e.to_string()))?;
        self.swap(filter, directive.to_string())
    }

    pub fn set_level(&self, level: LogLevel) -> Result<SwapResult, LogFilterError> {
        let directive = LogConfig::filter_for_level(level);
        let filter = EnvFilter::builder()
            .with_regex(false)
            .parse(&directive)
            .map_err(|e| LogFilterError::Invalid(e.to_string()))?;
        self.swap(filter, directive)
    }

    fn swap(&self, filter: EnvFilter, directive: String) -> Result<SwapResult, LogFilterError> {
        let previous = self.current();
        self.inner
            .modify(|f| *f = filter)
            .map_err(|e| LogFilterError::Invalid(e.to_string()))?;
        *self.current.lock().unwrap() = directive.clone();
        Ok(SwapResult {
            previous,
            current: directive,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SwapResult {
    pub previous: String,
    pub current: String,
}

#[derive(Debug)]
pub enum LogFilterError {
    Invalid(String),
    BareGlobalLevel,
    Unavailable,
}

impl std::fmt::Display for LogFilterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(msg) => write!(f, "invalid filter: {msg}"),
            Self::BareGlobalLevel => write!(
                f,
                "bare global level not accepted in filter mode; use set_level instead"
            ),
            Self::Unavailable => write!(f, "log subscriber not initialized in reloadable mode"),
        }
    }
}

impl std::error::Error for LogFilterError {}

pub fn init_subscriber(target: SubscriberTarget, filter: String) -> InitResult {
    let parsed = match EnvFilter::builder().with_regex(false).parse(&filter) {
        Ok(f) => f,
        Err(e) => {
            return InitResult {
                controller: None,
                warning: Some(format!("invalid initial filter {filter:?}: {e}")),
            };
        }
    };
    let (reload_layer, handle) = reload::Layer::new(parsed);

    let install_result = match target {
        SubscriberTarget::File(path) => match open_log_file(&path) {
            Ok(file) => {
                let writer = std::sync::Mutex::new(file);
                let fmt_layer = tracing_subscriber::fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false);
                Registry::default()
                    .with(reload_layer)
                    .with(fmt_layer)
                    .try_init()
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(format!("open log file {}: {e}", path.display())),
        },
        SubscriberTarget::Stdout => {
            let fmt_layer = tracing_subscriber::fmt::layer().with_ansi(false);
            Registry::default()
                .with(reload_layer)
                .with(fmt_layer)
                .try_init()
                .map_err(|e| e.to_string())
        }
    };

    match install_result {
        Ok(()) => {
            let controller = Arc::new(FilterController {
                inner: handle,
                current: Mutex::new(filter),
            });
            InitResult {
                controller: Some(controller),
                warning: None,
            }
        }
        Err(msg) => InitResult {
            controller: None,
            warning: Some(msg),
        },
    }
}

fn open_log_file(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    Ok(f)
}

static CONTROLLER: Mutex<Option<Arc<FilterController>>> = Mutex::new(None);

pub fn install_controller(c: Arc<FilterController>) {
    *CONTROLLER.lock().unwrap() = Some(c);
}

pub fn controller() -> Option<Arc<FilterController>> {
    CONTROLLER.lock().unwrap().clone()
}

pub fn current_filter() -> Option<String> {
    controller().map(|c| c.current())
}

pub fn set_filter(directive: &str) -> Result<SwapResult, LogFilterError> {
    controller()
        .ok_or(LogFilterError::Unavailable)?
        .set_filter(directive)
}

pub fn set_level(level: LogLevel) -> Result<SwapResult, LogFilterError> {
    controller()
        .ok_or(LogFilterError::Unavailable)?
        .set_level(level)
}

/// Path of the shared runtime-filter file inside `app_dir`. Daemon writes
/// here on every successful swap; cockpit runner subprocesses watch it
/// with `notify` and apply the same filter to their own subscribers.
pub fn runtime_filter_path(app_dir: &std::path::Path) -> std::path::PathBuf {
    app_dir.join("runtime_filter")
}

/// Atomically persist a filter directive to `<app_dir>/runtime_filter`.
/// Write-and-rename so concurrent readers never see a half-written file.
/// Owner-only permissions match the other `serve.*` artifacts.
pub fn persist_runtime_filter(directive: &str, app_dir: &std::path::Path) {
    if let Err(e) = std::fs::create_dir_all(app_dir) {
        tracing::warn!(target: "log.runtime", error = %e, "could not create app dir for runtime_filter");
        return;
    }
    let path = runtime_filter_path(app_dir);
    let tmp = app_dir.join("runtime_filter.tmp");
    if let Err(e) = std::fs::write(&tmp, directive) {
        tracing::warn!(target: "log.runtime", error = %e, "could not write runtime_filter.tmp");
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        tracing::warn!(target: "log.runtime", error = %e, "could not rename runtime_filter");
    }
}

/// Background task: watch `<app_dir>/runtime_filter` and apply changes
/// to this process's `FilterController`. Used by the cockpit runner so
/// the daemon's `aoe log-level` propagates to runners without restart.
///
/// notify watches the parent directory (the file may not exist yet);
/// the task filters events to those touching our target file.
pub async fn watch_runtime_filter(app_dir: std::path::PathBuf) {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc;

    let target = runtime_filter_path(&app_dir);

    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = match notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    }) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(target: "log.runtime", error = %e, "notify init failed; live propagation disabled");
            return;
        }
    };
    if let Err(e) = watcher.watch(&app_dir, RecursiveMode::NonRecursive) {
        tracing::warn!(target: "log.runtime", error = %e, dir = %app_dir.display(), "notify watch failed");
        return;
    }

    // Apply once at startup if the file is already there.
    apply_filter_file(&target);

    loop {
        let evt = match tokio::task::block_in_place(|| rx.recv()) {
            Ok(e) => e,
            Err(_) => return,
        };
        let Ok(evt) = evt else { continue };
        if evt.paths.iter().any(|p| p == &target) {
            apply_filter_file(&target);
        }
    }
}

fn apply_filter_file(path: &std::path::Path) {
    let directive = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return,
    };
    let directive = directive.trim();
    if directive.is_empty() {
        return;
    }
    match set_filter(directive) {
        Ok(swap) => tracing::info!(
            target: "log.runtime",
            previous = %swap.previous,
            current = %swap.current,
            source = "file-watch",
            "runner filter swapped"
        ),
        Err(e) => tracing::warn!(
            target: "log.runtime",
            error = %e,
            directive = %directive,
            "runner filter swap failed"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Env-touching tests must serialize.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn log_level_parse_accepts_known() {
        assert_eq!(LogLevel::parse("debug"), Some(LogLevel::Debug));
        assert_eq!(LogLevel::parse("INFO"), Some(LogLevel::Info));
        assert_eq!(LogLevel::parse("warning"), Some(LogLevel::Warn));
        assert_eq!(LogLevel::parse("trace "), Some(LogLevel::Trace));
        assert_eq!(LogLevel::parse("bogus"), None);
    }

    #[test]
    fn filter_for_level_expands_all_roots() {
        let s = LogConfig::filter_for_level(LogLevel::Debug);
        for root in DEFAULT_TARGET_ROOTS {
            assert!(
                s.contains(&format!("{root}=debug")),
                "missing {root} in {s}"
            );
        }
    }

    #[test]
    fn filter_string_overlay_acp() {
        let cfg = LogConfig {
            level: Some(LogLevel::Info),
            acp_trace: true,
            terminal_trace: false,
        };
        let s = cfg.filter_string().unwrap();
        assert!(s.contains("agent_client_protocol=debug"));
        assert!(s.contains("transport_actor=trace"));
    }

    #[test]
    fn filter_string_overlay_terminal() {
        let cfg = LogConfig {
            level: Some(LogLevel::Info),
            acp_trace: false,
            terminal_trace: true,
        };
        let s = cfg.filter_string().unwrap();
        assert!(s.ends_with(",terminal=trace"));
    }

    #[test]
    fn filter_string_none_when_level_unset() {
        let cfg = LogConfig {
            level: None,
            acp_trace: false,
            terminal_trace: false,
        };
        assert!(cfg.filter_string().is_none());
    }

    #[test]
    fn from_env_no_vars() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("AOE_LOG_LEVEL");
        std::env::remove_var("AGENT_OF_EMPIRES_DEBUG");
        std::env::remove_var("AOE_ACP_TRACE");
        std::env::remove_var("AOE_TERMINAL_TRACE");
        let cfg = LogConfig::from_env();
        assert_eq!(cfg.level, None);
        assert!(!cfg.acp_trace);
        assert!(!cfg.terminal_trace);
    }

    #[test]
    fn from_env_aoe_log_level() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("AOE_LOG_LEVEL", "trace");
        std::env::remove_var("AGENT_OF_EMPIRES_DEBUG");
        let cfg = LogConfig::from_env();
        std::env::remove_var("AOE_LOG_LEVEL");
        assert_eq!(cfg.level, Some(LogLevel::Trace));
    }

    #[test]
    fn from_env_legacy_debug_flag() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("AOE_LOG_LEVEL");
        std::env::set_var("AGENT_OF_EMPIRES_DEBUG", "1");
        let cfg = LogConfig::from_env();
        std::env::remove_var("AGENT_OF_EMPIRES_DEBUG");
        assert_eq!(cfg.level, Some(LogLevel::Debug));
    }

    fn with_test_controller<F>(initial: &str, f: F)
    where
        F: FnOnce(&FilterController),
    {
        let filter = EnvFilter::builder()
            .with_regex(false)
            .parse(initial)
            .unwrap();
        let (layer, handle) = reload::Layer::<EnvFilter, Registry>::new(filter);
        let subscriber = Registry::default().with(layer);
        let c = FilterController {
            inner: handle,
            current: Mutex::new(initial.to_string()),
        };
        tracing::subscriber::with_default(subscriber, || f(&c));
    }

    #[test]
    fn controller_swap_returns_previous() {
        with_test_controller("info", |c| {
            let r = c.set_level(LogLevel::Debug).unwrap();
            assert_eq!(r.previous, "info");
            assert!(r.current.contains("agent_of_empires=debug"));
            assert_eq!(c.current(), r.current);
        });
    }

    #[test]
    fn controller_rejects_bare_global_level() {
        with_test_controller("info", |c| {
            let err = c.set_filter("debug").unwrap_err();
            assert!(matches!(err, LogFilterError::BareGlobalLevel));
        });
    }

    #[test]
    fn controller_accepts_targeted_filter() {
        with_test_controller("info", |c| {
            c.set_filter("cockpit.acp=trace,info").unwrap();
            assert_eq!(c.current(), "cockpit.acp=trace,info");
        });
    }

    #[test]
    fn controller_rejects_empty_filter() {
        with_test_controller("info", |c| {
            assert!(matches!(
                c.set_filter("   ").unwrap_err(),
                LogFilterError::Invalid(_)
            ));
        });
    }

    #[test]
    fn controller_rejects_invalid_level() {
        with_test_controller("info", |c| {
            // Unknown level name; EnvFilter rejects.
            assert!(matches!(
                c.set_filter("cockpit=notalevel").unwrap_err(),
                LogFilterError::Invalid(_)
            ));
        });
    }
}
