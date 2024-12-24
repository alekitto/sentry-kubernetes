use crate::caching_client::CachingClient;
use crate::selector::Selectors;
use k8s_openapi::api::core::v1::Event;
use serde::Deserialize;

#[derive(Deserialize, Debug, Default)]
pub struct ConfigResource {
    pub api_version: Option<String>,
    pub kind: Option<String>,
    pub label_selector: Option<Selectors>,
    pub namespace: Option<String>,
}

impl ConfigResource {
    pub async fn event_matches(&self, event: &Event, client: &CachingClient) -> bool {
        if self.api_version.as_deref().is_some_and(|s| {
            s != event
                .involved_object
                .api_version
                .clone()
                .unwrap_or_default()
        }) {
            false
        } else if self
            .kind
            .as_deref()
            .is_some_and(|s| s != event.involved_object.kind.clone().unwrap_or_default())
        {
            false
        } else if self
            .namespace
            .as_deref()
            .is_some_and(|s| s != event.involved_object.namespace.clone().unwrap_or_default())
        {
            false
        } else if let Some(selectors) = self.label_selector.as_ref() {
            let Some(obj) = client.get_obj(&event.involved_object).await else {
                return false;
            };

            selectors.matches(
                obj.metadata
                    .labels
                    .unwrap_or_default()
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            )
        } else {
            true
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct ConfigWatcher {
    pub resources: Vec<ConfigResource>,
    pub dsn: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub levels: Vec<String>,
}

impl ConfigWatcher {
    pub fn all() -> Self {
        Self {
            resources: vec![ConfigResource::default()],
            ..Default::default()
        }
    }
}

impl Default for ConfigWatcher {
    fn default() -> Self {
        Self {
            resources: vec![],
            dsn: None,
            environment: None,
            release: None,
            levels: vec!["ERROR".to_string(), "WARNING".to_string()],
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct SentryConfig {
    pub dsn: Option<String>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub levels: Vec<String>,
    pub watchers: Vec<ConfigWatcher>,
}
