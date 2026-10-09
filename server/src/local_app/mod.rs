//! Exposes the local desktop application integration.
mod account;
mod byok;
mod ca;
mod process;
mod proxy;
mod settings;

use std::{net::SocketAddr, sync::Arc};

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::{
    store::{Store, TabMode, TabSettings},
    Error, Result,
};

use self::{ca::CaManager, proxy::ProxyRuntime};

pub(crate) fn proxy_host_allowed(host: &str) -> bool {
    proxy::is_cursor_host(host)
}

pub(crate) fn request_uses_local_cursor_token(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(account::is_local_cursor_authorization)
}

#[cfg(test)]
pub(crate) fn local_cursor_authorization() -> String {
    format!("Bearer {}", account::local_token().unwrap())
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaState {
    Missing,
    Untrusted,
    Ready,
    Invalid,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationState {
    Disabled,
    Enabled,
    Degraded,
}

#[derive(Clone, Debug, Serialize)]
pub struct CursorHarnessStatus {
    pub platform: &'static str,
    pub ca: CaState,
    pub configured_models: usize,
    pub enabled_models: usize,
    pub integration: IntegrationState,
    pub settings_applied: bool,
    pub proxy_url: Option<String>,
    pub ca_install_command: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct SetEnabled {
    pub enabled: bool,
}

#[derive(Clone)]
pub struct CursorHarness {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    plugins: Option<crate::plugin::PluginRegistry>,
    ca: CaManager,
    ca_initialization: Mutex<()>,
    configuration: Mutex<()>,
    backend_addr: RwLock<Option<SocketAddr>>,
    tab_mode: Arc<RwLock<TabMode>>,
    proxy: Mutex<ProxyRuntime>,
}

impl CursorHarness {
    pub fn new(store: Store) -> Result<Self> {
        Self::with_plugins(store, None)
    }

    pub(crate) fn with_plugins(
        store: Store,
        plugins: Option<crate::plugin::PluginRegistry>,
    ) -> Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                store,
                plugins,
                ca: CaManager::managed()?,
                ca_initialization: Mutex::new(()),
                configuration: Mutex::new(()),
                backend_addr: RwLock::new(None),
                tab_mode: Arc::new(RwLock::new(TabMode::default())),
                proxy: Mutex::new(ProxyRuntime::default()),
            }),
        })
    }

    pub fn set_backend_addr(&self, addr: SocketAddr) {
        *self.inner.backend_addr.write() = Some(addr);
    }

    pub async fn proxy_port(&self) -> Option<u16> {
        self.inner.proxy.lock().await.port()
    }

    pub async fn cleanup_stale_settings(&self) -> Result<()> {
        settings::clear_stale_managed_settings()
    }

    pub async fn status(&self) -> Result<CursorHarnessStatus> {
        let models = self.inner.store.models().await?;
        let mut configured_models = models.len();
        let mut enabled_models = configured_models;
        if let Some(plugins) = &self.inner.plugins {
            for provider in plugins
                .plugins()
                .await
                .into_iter()
                .flat_map(|plugin| plugin.providers)
            {
                configured_models += provider.models.len();
                if provider.configured {
                    enabled_models += provider.models.iter().filter(|model| model.enabled).count();
                }
            }
        }
        let ca = self.inner.ca.state()?;
        if self.inner.store.cursor_takeover_enabled().await?
            && matches!(ca, CaState::Ready)
            && self.inner.backend_addr.read().is_some()
        {
            self.enable().await?;
        }
        let proxy = self.inner.proxy.lock().await;
        let proxy_url = proxy.url();
        let settings_applied = byok::enabled().await?
            && proxy_url
                .as_deref()
                .map(settings::settings_match)
                .transpose()?
                .unwrap_or(false);
        let integration = match (proxy.running(), settings_applied) {
            (false, false) => IntegrationState::Disabled,
            (true, true) => IntegrationState::Enabled,
            _ => IntegrationState::Degraded,
        };
        Ok(CursorHarnessStatus {
            platform: std::env::consts::OS,
            ca,
            configured_models,
            enabled_models,
            integration,
            settings_applied,
            proxy_url,
            ca_install_command: self.inner.ca.install_command(),
        })
    }

    pub async fn initialize_ca(&self) -> Result<CursorHarnessStatus> {
        let _initialization = self.inner.ca_initialization.lock().await;
        let manager = self.inner.ca.clone();
        tokio::task::spawn_blocking(move || manager.initialize_local())
            .await
            .map_err(|error| Error::Store(format!("CA initialization task failed: {error}")))??;
        self.status().await
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<CursorHarnessStatus> {
        if enabled {
            self.inner.store.set_cursor_takeover_enabled(true).await?;
            self.enable().await?;
        } else {
            self.inner.store.set_cursor_takeover_enabled(false).await?;
            self.disable().await?;
        }
        self.status().await
    }

    pub async fn set_tab_settings(&self, settings: TabSettings) -> Result<TabSettings> {
        let saved = self.inner.store.set_tab_settings(settings).await?;
        *self.inner.tab_mode.write() = saved.mode;
        Ok(saved)
    }

    pub async fn repair_integration(&self) -> Result<CursorHarnessStatus> {
        let configuration = self.inner.configuration.lock().await;
        let was_running = process::terminate_cursor().await?;
        // Same guarantee as disable: whatever the repair does, Cursor comes back.
        let repaired: Result<()> = async {
            settings::clear_stale_managed_settings()?;
            account::inject_if_missing().await?;
            let proxy = self.inner.proxy.lock().await;
            if let Some(url) = proxy.url() {
                apply_cursor_configuration(&url).await?;
            }
            Ok(())
        }
        .await;
        process::reopen_cursor(was_running)?;
        drop(configuration);
        repaired?;
        self.status().await
    }

    async fn enable(&self) -> Result<()> {
        let _configuration = self.inner.configuration.lock().await;
        if !self.inner.store.cursor_takeover_enabled().await? {
            return Ok(());
        }
        if !matches!(self.inner.ca.state()?, CaState::Ready) {
            return Err(Error::Config(
                "initialize and trust the CA before enabling Cursor".into(),
            ));
        }
        let backend_addr = self
            .inner
            .backend_addr
            .read()
            .ok_or_else(|| Error::Config("desktop management server is not ready".into()))?;
        let mut proxy = self.inner.proxy.lock().await;
        let settings_applied = byok::enabled().await?
            && proxy
                .url()
                .as_deref()
                .map(settings::settings_match)
                .transpose()?
                .unwrap_or(false);
        let was_running = if !settings_applied {
            // Cursor caches applicationUser in memory; stop before transactional
            // preference updates so shutdown cannot overwrite the new value.
            process::terminate_cursor().await?
        } else {
            false
        };
        // Cursor is stopped from here on, so every exit path has to restart it: a
        // failure that returns early would otherwise leave the user's editor killed.
        let outcome = self
            .apply_configuration(&mut proxy, backend_addr, settings_applied)
            .await;
        let reopened = process::reopen_cursor(was_running);
        outcome?;
        reopened
    }

    /// Starts the proxy if needed and writes the Cursor-facing configuration.
    async fn apply_configuration(
        &self,
        proxy: &mut ProxyRuntime,
        backend_addr: SocketAddr,
        settings_applied: bool,
    ) -> Result<()> {
        if proxy.running() {
            if !settings_applied {
                if let Some(url) = proxy.url() {
                    apply_cursor_configuration(&url).await?;
                }
            }
            return Ok(());
        }
        let ca = self.inner.ca.load()?;
        let requested_port = self.inner.store.port_settings().await?.proxy_port;
        *self.inner.tab_mode.write() = self.inner.store.tab_settings().await?.mode;
        let (url, actual_port) = proxy
            .start(
                backend_addr,
                ca,
                requested_port,
                self.inner.tab_mode.clone(),
            )
            .await?;
        // A proxy that is up but not configured is worse than one that was never
        // started, so both failures stop it again.
        let configured = match self.inner.store.set_proxy_port(actual_port).await {
            Ok(()) => apply_cursor_configuration(&url).await,
            Err(error) => Err(error),
        };
        if let Err(error) = configured {
            proxy.stop().await;
            return Err(error);
        }
        Ok(())
    }

    pub async fn disable(&self) -> Result<()> {
        let _configuration = self.inner.configuration.lock().await;
        let was_running = match byok::enabled().await {
            Ok(true) => process::terminate_cursor().await?,
            // BYOK was already off, so Cursor needs no restart before the revert.
            Ok(false) => false,
            // The preference is unreadable. Cursor is left alone and the revert still
            // runs; if it also fails, that failure is what the caller hears about.
            Err(error) => {
                tracing::warn!(%error, "cannot read the Cursor BYOK preference before disabling");
                false
            }
        };
        // Every revert step runs even when one fails, and Cursor is restarted either
        // way: a single unreadable restoration record must not leave the app killed,
        // nor leave the injected account behind in Cursor's state database.
        let reverted = revert_cursor_configuration().await;
        self.inner.proxy.lock().await.stop().await;
        let reopened = process::reopen_cursor(was_running);
        reverted?;
        reopened
    }
}

/// Undoes the three configuration writes, attempting all of them.
///
/// Each write is independent, so one failure must not skip the others: a corrupt
/// settings journal would otherwise leave `useOpenAIKey` set and the account
/// injected, which is a worse state than the error that started it.
async fn revert_cursor_configuration() -> Result<()> {
    let steps = [
        byok::restore().await,
        settings::clear_proxy_settings(),
        account::remove_injected_account_if_present().await,
    ];
    first_failure(steps)
}

/// Reports the first failure after every step has already been attempted.
fn first_failure(steps: [Result<()>; 3]) -> Result<()> {
    let mut first = None;
    for step in steps {
        if let Err(error) = step {
            // The remaining failures are real but only one can be returned; log them so
            // a partial revert is diagnosable.
            tracing::warn!(%error, "a Cursor configuration revert step failed");
            first.get_or_insert(error);
        }
    }
    match first {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

async fn apply_cursor_configuration(proxy_url: &str) -> Result<()> {
    account::inject_if_missing().await?;
    byok::apply().await?;
    settings::write_proxy_settings(proxy_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(label: &str) -> Error {
        Error::Config(label.into())
    }

    #[test]
    fn a_clean_revert_reports_nothing() {
        let clean: [Result<()>; 3] = [Ok(()), Ok(()), Ok(())];
        assert!(first_failure(clean).is_ok());
    }

    /// One bad restoration record must not hide the others: every step is reported so
    /// the caller learns the revert is partial rather than complete.
    #[test]
    fn a_failing_step_does_not_hide_a_later_one() {
        let error =
            first_failure([Ok(()), Err(failure("settings")), Err(failure("account"))]).unwrap_err();
        assert!(matches!(error, Error::Config(ref message) if message == "settings"));
    }

    #[test]
    fn the_first_failure_is_the_one_reported() {
        let error = first_failure([
            Err(failure("byok")),
            Err(failure("settings")),
            Err(failure("account")),
        ])
        .unwrap_err();
        assert!(matches!(error, Error::Config(ref message) if message == "byok"));
    }
}
