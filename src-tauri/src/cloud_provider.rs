//! Non-secret Cloud provider configuration and the Keychain credential boundary.

use crate::error::{AppError, Result};
use crate::settings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const KEYCHAIN_SERVICE: &str = "com.whisperpilot.cloud-api-keys";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CloudProvider {
    #[serde(rename = "deepgram")]
    Deepgram,
    #[serde(rename = "assemblyai")]
    AssemblyAi,
    #[serde(rename = "openai")]
    OpenAi,
}

impl CloudProvider {
    pub const ALL: [Self; 3] = [Self::Deepgram, Self::AssemblyAi, Self::OpenAi];

    pub fn id(self) -> &'static str {
        match self {
            Self::Deepgram => "deepgram",
            Self::AssemblyAi => "assemblyai",
            Self::OpenAi => "openai",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Deepgram => "Deepgram",
            Self::AssemblyAi => "AssemblyAI",
            Self::OpenAi => "OpenAI",
        }
    }

    fn model(self) -> &'static str {
        match self {
            Self::Deepgram => "Nova-3",
            Self::AssemblyAi => "Universal-3.5 Pro",
            Self::OpenAi => "GPT Live Transcribe",
        }
    }

    /// Fixed provider API identifier used only by the Rust transport. It is
    /// distinct from the human-readable model label returned to the UI.
    pub fn transport_model(self) -> &'static str {
        match self {
            Self::Deepgram => "nova-3",
            Self::AssemblyAi => "universal-3-5-pro",
            Self::OpenAi => "gpt-live-transcribe",
        }
    }
}

impl TryFrom<&str> for CloudProvider {
    type Error = AppError;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "deepgram" => Ok(Self::Deepgram),
            "assemblyai" => Ok(Self::AssemblyAi),
            "openai" => Ok(Self::OpenAi),
            _ => Err(AppError::InvalidSetting(
                "unknown cloud provider".to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudProviderStatus {
    pub id: CloudProvider,
    pub name: String,
    pub model: String,
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudProviderConfiguration {
    pub selected_provider: CloudProvider,
    pub providers: Vec<CloudProviderStatus>,
}

/// A secret-store boundary intentionally exposes only save/remove/configured
/// operations. No UI-facing code may read an API key through this trait.
pub trait CredentialStore {
    fn save(&mut self, provider: CloudProvider, api_key: &str) -> Result<()>;
    fn configured(&self, provider: CloudProvider) -> Result<bool>;
    fn remove(&mut self, provider: CloudProvider) -> Result<()>;
}

pub struct CloudProviderService<S> {
    app_support_dir: PathBuf,
    credentials: S,
}

impl<S: CredentialStore> CloudProviderService<S> {
    pub fn new(app_support_dir: &Path, credentials: S) -> Self {
        Self {
            app_support_dir: app_support_dir.to_path_buf(),
            credentials,
        }
    }

    pub fn configuration(&self) -> Result<CloudProviderConfiguration> {
        let selected_provider = CloudProvider::try_from(
            settings::get_settings(&self.app_support_dir)
                .cloud_provider
                .as_str(),
        )?;
        let providers = CloudProvider::ALL
            .into_iter()
            .map(|provider| {
                Ok(CloudProviderStatus {
                    id: provider,
                    name: provider.name().to_string(),
                    model: provider.model().to_string(),
                    configured: self.credentials.configured(provider)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(CloudProviderConfiguration {
            selected_provider,
            providers,
        })
    }

    pub fn select(&mut self, provider: CloudProvider) -> Result<CloudProviderConfiguration> {
        settings::set_setting(&self.app_support_dir, "cloud_provider", provider.id())?;
        self.configuration()
    }

    pub fn save_api_key(
        &mut self,
        provider: CloudProvider,
        api_key: &str,
    ) -> Result<CloudProviderConfiguration> {
        if api_key.trim().is_empty() {
            return Err(AppError::InvalidSetting(
                "API key must not be empty".to_string(),
            ));
        }
        self.credentials.save(provider, api_key)?;
        self.configuration()
    }

    pub fn remove_api_key(
        &mut self,
        provider: CloudProvider,
    ) -> Result<CloudProviderConfiguration> {
        self.credentials.remove(provider)?;
        self.configuration()
    }
}

pub struct KeychainCredentialStore;

#[cfg(target_os = "macos")]
fn keychain_error() -> AppError {
    AppError::InvalidSetting("Unable to access macOS Keychain.".to_string())
}

#[cfg(target_os = "macos")]
impl KeychainCredentialStore {
    fn entry(provider: CloudProvider) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYCHAIN_SERVICE, provider.id()).map_err(|_| keychain_error())
    }

    /// Internal runtime-only credential read. This is deliberately not part
    /// of the UI-facing `CredentialStore` trait: keys may be used to create
    /// an authenticated transport but must never be returned over IPC.
    pub(crate) fn load_for_transport(provider: CloudProvider) -> Result<String> {
        match Self::entry(provider)?.get_password() {
            Ok(api_key) if !api_key.trim().is_empty() => Ok(api_key),
            Ok(_) | Err(keyring::Error::NoEntry) => Err(AppError::InvalidSetting(
                "Configure this Cloud provider's API key in Settings before starting.".to_string(),
            )),
            Err(_) => Err(keychain_error()),
        }
    }
}

#[cfg(target_os = "macos")]
impl CredentialStore for KeychainCredentialStore {
    fn save(&mut self, provider: CloudProvider, api_key: &str) -> Result<()> {
        Self::entry(provider)?
            .set_password(api_key)
            .map_err(|_| keychain_error())
    }

    fn configured(&self, provider: CloudProvider) -> Result<bool> {
        match Self::entry(provider)?.get_password() {
            Ok(_) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(_) => Err(keychain_error()),
        }
    }

    fn remove(&mut self, provider: CloudProvider) -> Result<()> {
        match Self::entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(keychain_error()),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl CredentialStore for KeychainCredentialStore {
    fn save(&mut self, _provider: CloudProvider, _api_key: &str) -> Result<()> {
        Err(AppError::InvalidSetting(
            "macOS Keychain is unavailable on this platform".to_string(),
        ))
    }

    fn configured(&self, _provider: CloudProvider) -> Result<bool> {
        Err(AppError::InvalidSetting(
            "macOS Keychain is unavailable on this platform".to_string(),
        ))
    }

    fn remove(&mut self, _provider: CloudProvider) -> Result<()> {
        Err(AppError::InvalidSetting(
            "macOS Keychain is unavailable on this platform".to_string(),
        ))
    }
}
