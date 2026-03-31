//! Per-directory isolation for the web backend.
//!
//! Each project directory gets its own [`DirectoryContext`] with an independent
//! [`SessionManager`], [`EventBus`], and lifecycle. The [`DirectoryRegistry`]
//! manages creation, lookup, and cleanup of these contexts.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{debug, info};

use opendev_history::SessionManager;

use crate::event_bus::{EventBus, GlobalEventBus, now_ms};

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// DirectoryContext
// ---------------------------------------------------------------------------

/// Per-directory isolated context.
///
/// Each project directory served by the web backend gets its own
/// `DirectoryContext` with independent session management and event bus.
pub struct DirectoryContext {
    /// The project directory this context is scoped to.
    working_dir: PathBuf,
    /// Session manager for this directory.
    session_manager: RwLock<SessionManager>,
    /// Event bus scoped to this directory.
    event_bus: EventBus,
    /// When this context was created.
    created_at: Instant,
    /// Last activity timestamp (millis since epoch) -- updated on every access.
    pub(crate) last_activity: AtomicU64,
}

impl DirectoryContext {
    /// Create a new directory context.
    pub fn new(working_dir: PathBuf, session_manager: SessionManager, event_bus: EventBus) -> Self {
        Self {
            working_dir,
            session_manager: RwLock::new(session_manager),
            event_bus,
            created_at: Instant::now(),
            last_activity: AtomicU64::new(now_ms()),
        }
    }

    /// The project directory this context is scoped to.
    pub fn working_dir(&self) -> &Path {
        &self.working_dir
    }

    /// Acquire a read lock on the session manager.
    pub async fn session_manager(&self) -> tokio::sync::RwLockReadGuard<'_, SessionManager> {
        self.session_manager.read().await
    }

    /// Acquire a write lock on the session manager.
    pub async fn session_manager_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, SessionManager> {
        self.session_manager.write().await
    }

    /// The event bus scoped to this directory.
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Record current time as the last activity.
    pub fn touch(&self) {
        self.last_activity.store(now_ms(), Ordering::Relaxed);
    }

    /// Duration since the last activity.
    pub fn idle_duration(&self) -> Duration {
        let last = self.last_activity.load(Ordering::Relaxed);
        let now = now_ms();
        Duration::from_millis(now.saturating_sub(last))
    }

    /// When this context was created.
    pub fn created_at(&self) -> Instant {
        self.created_at
    }
}

impl std::fmt::Debug for DirectoryContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectoryContext")
            .field("working_dir", &self.working_dir)
            .field("idle_ms", &self.idle_duration().as_millis())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// DirectoryRegistry
// ---------------------------------------------------------------------------

/// Registry of per-directory contexts.
///
/// Manages creation, lookup, and lifecycle of [`DirectoryContext`] instances.
/// When accessed, a context is created on-demand and cached. Idle contexts
/// are periodically cleaned up by a background task.
pub struct DirectoryRegistry {
    contexts: RwLock<HashMap<PathBuf, Arc<DirectoryContext>>>,
    /// Default sessions directory (e.g., `~/.opendev/sessions/`).
    sessions_base_dir: PathBuf,
    /// Maximum idle duration before cleanup (default: 30 minutes).
    max_idle: Duration,
    /// Optional global bus that receives events from all directories.
    global_bus: Option<Arc<GlobalEventBus>>,
}

impl DirectoryRegistry {
    /// Create a new registry.
    pub fn new(sessions_base_dir: PathBuf, max_idle: Duration) -> Self {
        Self {
            contexts: RwLock::new(HashMap::new()),
            sessions_base_dir,
            max_idle,
            global_bus: None,
        }
    }

    /// Builder method: attach a global event bus that receives events from all directories.
    pub fn with_global_bus(mut self, bus: Arc<GlobalEventBus>) -> Self {
        self.global_bus = Some(bus);
        self
    }

    /// Look up or create a context for the given working directory.
    ///
    /// On create: instantiates a [`SessionManager`] under `sessions_base_dir`,
    /// creates a new [`EventBus`], and wraps them in a [`DirectoryContext`].
    /// Calls [`DirectoryContext::touch`] on every access.
    pub async fn get_or_create(
        &self,
        working_dir: &Path,
    ) -> Result<Arc<DirectoryContext>, std::io::Error> {
        // Fast path: read lock
        {
            let contexts = self.contexts.read().await;
            if let Some(ctx) = contexts.get(working_dir) {
                ctx.touch();
                return Ok(Arc::clone(ctx));
            }
        }

        // Slow path: write lock, double-check
        let mut contexts = self.contexts.write().await;
        if let Some(ctx) = contexts.get(working_dir) {
            ctx.touch();
            return Ok(Arc::clone(ctx));
        }

        let dir_str = working_dir.to_string_lossy();
        // Strip Windows extended-length path prefix (\\?\) before sanitizing
        let dir_str = dir_str.strip_prefix(r"\\?\").unwrap_or(&dir_str);
        let session_dir = self.sessions_base_dir.join(
            dir_str
                .replace(['/', '\\', ':'], "_")
                .trim_start_matches('_'),
        );
        let session_manager = SessionManager::new(session_dir)?;
        let event_bus = EventBus::new();

        // Subscribe to the bus *before* passing it to DirectoryContext so
        // that no events are missed by the forwarder.
        if let Some(global_bus) = &self.global_bus {
            let mut rx = event_bus.subscribe();
            let global = Arc::clone(global_bus);
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(event) => global.forward(event),
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            debug!(n, "Global bus forwarder lagged, skipping events");
                        }
                    }
                }
            });
        }

        let ctx = Arc::new(DirectoryContext::new(
            working_dir.to_path_buf(),
            session_manager,
            event_bus,
        ));
        info!(?working_dir, "Created new DirectoryContext");

        contexts.insert(working_dir.to_path_buf(), Arc::clone(&ctx));
        Ok(ctx)
    }

    /// Look up a context without creating one.
    pub async fn get(&self, working_dir: &Path) -> Option<Arc<DirectoryContext>> {
        let contexts = self.contexts.read().await;
        contexts.get(working_dir).map(|ctx| {
            ctx.touch();
            Arc::clone(ctx)
        })
    }

    /// Remove and drop a context for the given working directory.
    pub async fn dispose(&self, working_dir: &Path) {
        let mut contexts = self.contexts.write().await;
        if contexts.remove(working_dir).is_some() {
            info!(?working_dir, "Disposed DirectoryContext");
        }
    }

    /// Remove contexts that have been idle longer than `max_idle`.
    ///
    /// Returns the number of contexts removed.
    pub async fn cleanup_idle(&self) -> usize {
        let mut contexts = self.contexts.write().await;
        let before = contexts.len();
        contexts.retain(|path, ctx| {
            let idle = ctx.idle_duration();
            let keep = idle <= self.max_idle;
            if !keep {
                debug!(?path, ?idle, "Removing idle DirectoryContext");
            }
            keep
        });
        let removed = before - contexts.len();
        if removed > 0 {
            info!(removed, "Cleaned up idle directory contexts");
        }
        removed
    }

    /// Number of active (tracked) contexts.
    pub async fn active_count(&self) -> usize {
        self.contexts.read().await.len()
    }

    /// Spawn a background task that runs [`cleanup_idle`](Self::cleanup_idle)
    /// every 5 minutes.
    pub fn spawn_cleanup_task(registry: Arc<DirectoryRegistry>) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));
            loop {
                interval.tick().await;
                registry.cleanup_idle().await;
            }
        })
    }
}

impl std::fmt::Debug for DirectoryRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirectoryRegistry")
            .field("sessions_base_dir", &self.sessions_base_dir)
            .field("max_idle", &self.max_idle)
            .finish()
    }
}
