use crate::caching_client::CachingClient;
use crate::selector::Selectors;
use crate::{Args, GlobalConfiguration};
use clap::Parser;
use config::{Config, Environment, File};
use k8s_openapi::api::core::v1::Event;
use log::{LevelFilter, warn};
use sentry::types::Dsn;
use serde::Deserialize;
use simple_logger::SimpleLogger;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Deserialize, Debug, Default)]
pub struct ConfigResource {
    #[serde(default)]
    pub api_version: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub label_selector: Option<Selectors>,
    #[serde(default)]
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
        }) || self
            .kind
            .as_deref()
            .is_some_and(|s| s != event.involved_object.kind.clone().unwrap_or_default())
            || self
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
pub struct ConfigMonitor {
    #[serde(default)]
    pub resources: Vec<ConfigResource>,
    #[serde(default)]
    pub dsn: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub levels: Vec<String>,
}

impl ConfigMonitor {
    pub fn all() -> Self {
        Self {
            resources: vec![ConfigResource::default()],
            ..Default::default()
        }
    }
}

impl Default for ConfigMonitor {
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

#[derive(Deserialize, Debug, Default)]
pub struct SentryConfig {
    #[serde(default)]
    pub dsn: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub release: Option<String>,
    #[serde(default)]
    pub levels: Vec<String>,
    #[serde(default)]
    pub monitors: Vec<ConfigMonitor>,
    #[serde(default)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub historical: bool,
}

pub fn parse_config() -> anyhow::Result<(GlobalConfiguration, Vec<ConfigMonitor>)> {
    let args = Args::parse();
    let log_level = LevelFilter::from_str(&args.log_level).unwrap_or(LevelFilter::Warn);

    let config: SentryConfig = Config::builder()
        .add_source(
            Environment::with_prefix("SENTRY")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("levels"),
        )
        .add_source(File::from(PathBuf::from(args.config)))
        .set_default("log_level", Some(args.log_level))?
        .set_default("historical", true)?
        .build()?
        .try_deserialize()?;

    let log_level = config
        .log_level
        .and_then(|l| match LevelFilter::from_str(&l) {
            Ok(l) => Some(l),
            Err(e) => {
                eprintln!("Unable to parse log level: {}", e);
                None
            }
        })
        .unwrap_or(log_level);

    SimpleLogger::new().with_level(log_level).init()?;

    let global_dsn = config.dsn.and_then(|dsn| match Dsn::from_str(&dsn) {
        Ok(t) => Some(t),
        Err(e) => {
            warn!(r#"Global DSN is invalid ({}), ignoring"#, e.to_string());
            None
        }
    });

    let global_levels = config.levels;
    let global_config = GlobalConfiguration {
        dsn: global_dsn,
        release: config.release,
        environment: config.environment,
        historical: config.historical,
        levels: if global_levels.is_empty() {
            vec!["ERROR".to_string(), "WARNING".to_string()]
        } else {
            global_levels
        },
    };

    let monitors_config = config.monitors;
    let monitors_config = if monitors_config.is_empty() {
        vec![ConfigMonitor::all()]
    } else {
        monitors_config
    };

    Ok((global_config, monitors_config))
}
#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{Event, ObjectReference};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::{Client, Config};

    fn build_client() -> CachingClient {
        let client =
            Client::try_from(Config::new("https://localhost:6443/".try_into().unwrap())).unwrap();
        CachingClient::new(client)
    }

    fn base_event() -> Event {
        Event {
            involved_object: ObjectReference {
                api_version: Some("v1".to_string()),
                kind: Some("Pod".to_string()),
                name: Some("pod-1".to_string()),
                namespace: Some("default".to_string()),
                ..Default::default()
            },
            metadata: ObjectMeta::default(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn event_matches_success() {
        let client = build_client();
        let event = base_event();
        let resource = ConfigResource {
            api_version: Some("v1".to_string()),
            kind: Some("Pod".to_string()),
            namespace: Some("default".to_string()),
            ..Default::default()
        };

        assert!(resource.event_matches(&event, &client).await);
    }

    #[tokio::test]
    async fn event_matches_wrong_namespace() {
        let client = build_client();
        let mut event = base_event();
        event.involved_object.namespace = Some("other".to_string());
        let resource = ConfigResource {
            namespace: Some("default".to_string()),
            kind: Some("Pod".to_string()),
            api_version: None,
            label_selector: None,
        };

        assert!(!resource.event_matches(&event, &client).await);
    }

    #[tokio::test]
    async fn event_matches_wrong_kind() {
        let client = build_client();
        let event = base_event();
        let resource = ConfigResource {
            kind: Some("Node".to_string()),
            namespace: Some("default".to_string()),
            api_version: None,
            label_selector: None,
        };

        assert!(!resource.event_matches(&event, &client).await);
    }
}
