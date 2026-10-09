//! Reading and writing the persisted console settings.

use crate::store::{
    CommitSettings, DesktopSettings, PortSettings, ProxySettings, ProxySettingsInput,
    StatisticsStorage, TabSettings, TokenPricingSettings,
};

use super::{ControlService, Result};

impl ControlService {
    pub async fn ports(&self) -> Result<PortSettings> {
        self.store.port_settings().await
    }

    pub async fn set_ports(&self, settings: PortSettings) -> Result<PortSettings> {
        self.store.set_port_settings(settings).await?;
        Ok(settings)
    }

    pub async fn statistics_storage(&self) -> Result<StatisticsStorage> {
        self.store.statistics_storage().await
    }

    pub async fn clear_statistics_storage(&self) -> Result<StatisticsStorage> {
        self.store.clear_statistics_storage().await
    }

    pub async fn clear_all_statistics_storage(&self) -> Result<StatisticsStorage> {
        self.store.clear_all_statistics_storage().await
    }

    pub async fn proxy_settings(&self) -> Result<ProxySettings> {
        self.store.proxy_settings().await
    }

    pub async fn set_proxy_settings(&self, settings: ProxySettingsInput) -> Result<ProxySettings> {
        if settings.mode.is_custom() {
            let local_proxy_port = match self.cursor_harness.proxy_port().await {
                Some(port) => port,
                None => self.store.port_settings().await?.proxy_port,
            };
            crate::network::reject_self_proxy(&settings.address, local_proxy_port)?;
        }
        let settings = self.store.set_proxy_settings(settings).await?;
        self.clients.invalidate().await;
        self.plugins.invalidate_clients().await;
        Ok(settings)
    }

    pub async fn tab_settings(&self) -> Result<TabSettings> {
        self.store.tab_settings().await
    }

    pub async fn set_tab_settings(&self, settings: TabSettings) -> Result<TabSettings> {
        self.cursor_harness.set_tab_settings(settings).await
    }

    pub async fn desktop_settings(&self) -> Result<DesktopSettings> {
        self.store.desktop_settings().await
    }

    pub async fn set_desktop_settings(&self, settings: DesktopSettings) -> Result<()> {
        self.store.set_desktop_settings(settings).await
    }

    pub async fn commit_settings(&self) -> Result<CommitSettings> {
        self.store.commit_settings().await
    }

    pub async fn cmdk_settings(&self) -> Result<crate::store::CmdKSettings> {
        self.store.cmdk_settings().await
    }

    pub async fn set_cmdk_settings(
        &self,
        settings: crate::store::CmdKSettings,
    ) -> Result<crate::store::CmdKSettings> {
        self.store.set_cmdk_settings(settings).await
    }

    pub async fn set_commit_settings(&self, settings: CommitSettings) -> Result<CommitSettings> {
        self.store.set_commit_settings(settings).await
    }

    pub async fn pricing_settings(&self) -> Result<TokenPricingSettings> {
        self.store.pricing_settings().await
    }

    pub async fn set_pricing_settings(
        &self,
        settings: TokenPricingSettings,
    ) -> Result<TokenPricingSettings> {
        self.store.set_pricing_settings(settings).await
    }
}
