use std::collections::HashMap;

use whisperpilot_lib::cloud_provider::{
    CloudProvider, CloudProviderConfiguration, CloudProviderService, CredentialStore,
};
use whisperpilot_lib::error::Result;

#[derive(Default)]
struct MemoryCredentialStore {
    values: HashMap<CloudProvider, String>,
}

impl CredentialStore for MemoryCredentialStore {
    fn save(&mut self, provider: CloudProvider, api_key: &str) -> Result<()> {
        self.values.insert(provider, api_key.to_string());
        Ok(())
    }

    fn configured(&self, provider: CloudProvider) -> Result<bool> {
        Ok(self.values.contains_key(&provider))
    }

    fn remove(&mut self, provider: CloudProvider) -> Result<()> {
        self.values.remove(&provider);
        Ok(())
    }
}

// WP-106 C-1/C-2, decision table: provider selection persists as a non-secret
// setting, while save/remove affect only the injected secure credential store.
#[test]
fn cloud_provider_configuration_exposes_fixed_models_and_never_serializes_api_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut service = CloudProviderService::new(dir.path(), MemoryCredentialStore::default());
    let api_key = "not-a-real-key";

    let initial = service.configuration().unwrap();
    assert_eq!(initial.selected_provider, CloudProvider::Deepgram);
    assert_eq!(initial.providers.len(), 3);
    assert_eq!(initial.providers[0].model, "Nova-3");
    assert_eq!(initial.providers[1].model, "Universal-3.5 Pro");
    assert_eq!(initial.providers[2].model, "GPT Live Transcribe");
    assert!(!initial.providers[0].configured);

    service.select(CloudProvider::Deepgram).unwrap();
    let after_save = service
        .save_api_key(CloudProvider::Deepgram, api_key)
        .unwrap();
    assert!(after_save.providers[0].configured);
    let serialized = serde_json::to_string(&after_save).unwrap();
    assert!(!serialized.contains(api_key));
    let json: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(json["selected_provider"], "deepgram");
    assert_eq!(json["providers"][1]["id"], "assemblyai");
    assert!(json["providers"][0].get("api_key").is_none());
    let round_trip: CloudProviderConfiguration = serde_json::from_str(&serialized).unwrap();
    assert_eq!(round_trip, after_save);
    assert!(!std::fs::read_to_string(dir.path().join("settings.json"))
        .unwrap()
        .contains(api_key));

    let after_select = service.select(CloudProvider::OpenAi).unwrap();
    assert_eq!(after_select.selected_provider, CloudProvider::OpenAi);
    assert!(after_select.providers[0].configured);

    let after_remove = service.remove_api_key(CloudProvider::Deepgram).unwrap();
    assert!(!after_remove.providers[0].configured);
}

// WP-106 C-2, EP invalid partition: whitespace input is rejected before it
// reaches the credential store, so it cannot create an empty Keychain item.
#[test]
fn cloud_provider_configuration_rejects_blank_api_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut service = CloudProviderService::new(dir.path(), MemoryCredentialStore::default());

    assert!(service
        .save_api_key(CloudProvider::AssemblyAi, "   ")
        .is_err());
    assert!(!service.configuration().unwrap().providers[1].configured);
}
