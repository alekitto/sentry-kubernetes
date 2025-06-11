use crate::caching_client::CachingClient;
use crate::config::ConfigMonitor;
use crate::monitor::CLIENTS;
use crate::sentry_event::SentryEvent;
use crate::GlobalConfiguration;
use k8s_openapi::api::core::v1::Event;
use log::debug;
use sentry::transports::DefaultTransportFactory;
use sentry::types::Dsn;
use sentry::{Breadcrumb, Client, Hub, Integration, Level};
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

pub struct EventMonitor<F: Fn(&Hub, &SentryEvent)> {
    client: Arc<CachingClient>,
    configuration: ConfigMonitor,
    event_levels: Vec<Level>,
    sentry_hub: Arc<Hub>,
    sender: F,
}

impl<F: Fn(&Hub, &SentryEvent)> EventMonitor<F> {
    pub fn new(
        configuration: ConfigMonitor,
        global_configuration: &GlobalConfiguration,
        client: Arc<CachingClient>,
        sender: F,
    ) -> Self {
        let dsn = configuration
            .dsn
            .as_deref()
            .and_then(|s| Dsn::from_str(s).ok())
            .or_else(|| global_configuration.dsn.clone());

        let integrations = {
            // default integrations need to be ordered *before* custom integrations,
            // since they also process events in order
            let integrations: Vec<Arc<dyn Integration>> = vec![
                Arc::new(
                    sentry::integrations::contexts::ContextIntegration::default(),
                )
            ];

            integrations
        };

        let mut clients_map = CLIENTS.lock().unwrap();
        let sentry_hub = {
            if !clients_map.contains_key(&dsn) {
                let sentry_client = Client::with_options(sentry::ClientOptions {
                    dsn: dsn.clone(),
                    transport: Some(Arc::new(DefaultTransportFactory)),
                    integrations,
                    environment:configuration
                        .environment
                        .as_deref()
                        .or(global_configuration.environment.as_deref())
                        .map(|e| e.to_string().into()),
                    release: configuration
                        .release
                        .as_deref()
                        .or(global_configuration.release.as_deref())
                        .map(|rel| rel.to_string().into()),
                    ..Default::default()
                });

                let scope = sentry::Scope::default();
                let sentry_hub = Hub::new(Some(Arc::new(sentry_client)), Arc::new(scope));

                clients_map.insert(dsn.clone(), Arc::new(sentry_hub));
            }

            clients_map.get(&dsn).unwrap()
        }
        .clone();

        let levels = if configuration.levels.is_empty() {
            global_configuration.levels.clone()
        } else {
            configuration.levels.clone()
        }
        .into_iter()
        .filter_map(|level| Level::from_str(&level.to_lowercase()).ok())
        .collect();

        Self {
            client,
            configuration,
            event_levels: levels,
            sentry_hub,
            sender,
        }
    }

    pub(super) async fn process(&self, event: Event) {
        let mut matches = false;
        for c in &self.configuration.resources {
            matches = matches || c.event_matches(&event, &self.client).await;
        }

        if !matches {
            return;
        }

        let mut sentry_event = SentryEvent::from(event);
        let mut hostname = sentry_event.source_host.clone();
        if hostname.is_none() && sentry_event.kind.as_deref() == Some("Pod") {
            if let Some(pod) = self.client.get_pod(&sentry_event.name).await {
                hostname = pod.spec.and_then(|p| p.node_name);
            }
        }

        if let Some(hostname) = hostname.as_ref() {
            if let Some(node) = self.client.get_node(hostname).await {
                sentry_event.node_labels = node.metadata.labels.unwrap_or_default();
            }
        }

        if self.event_levels.iter().any(|e| e == &sentry_event.level)
            || sentry_event.level == Level::Error
        {
            sentry_event.source_host = hostname;

            debug!("sending event to sentry");
            (self.sender)(&self.sentry_hub, &sentry_event);
        } else {
            debug!("excluded by event level");
        }

        let mut breadcrumb = Breadcrumb {
            data: {
                let mut map = BTreeMap::new();
                map.insert("name".into(), sentry_event.name.into());
                map.insert("namespace".into(), sentry_event.namespace.into());
                map
            },
            level: sentry_event.level,
            message: sentry_event.message,
            ..Default::default()
        };

        if let Some(timestamp) = sentry_event.creation_timestamp {
            breadcrumb.timestamp = timestamp;
        }

        self.sentry_hub.add_breadcrumb(breadcrumb);
    }
}

#[cfg(test)]
mod tests {
    use crate::caching_client::CachingClient;
    use crate::config::{ConfigMonitor, ConfigResource};
    use crate::monitor::event::EventMonitor;
    use crate::GlobalConfiguration;
    use k8s_openapi::api::core::v1::{Event, EventSource, ObjectReference};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
    use k8s_openapi::chrono::DateTime;
    use kube::{Client, Config};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn generate_event() -> Event {
        Event {
            action: None,
            count: Some(2),
            event_time: None,
            first_timestamp: Some(Time(
                DateTime::parse_from_rfc3339("2023-04-08T22:27:40Z")
                    .unwrap()
                    .into(),
            )),
            involved_object: ObjectReference {
                api_version: Some("v1".to_string()),
                field_path: Some("spec.containers{coredns}".to_string()),
                kind: Some("Pod".to_string()),
                name: Some("coredns-bbbc4b766-fv96b".to_string()),
                namespace: Some("kube-system".to_string()),
                resource_version: Some("355929156".to_string()),
                uid: Some("f4f1a725-a5e8-4cdb-8a6f-cd02917a9056".to_string()),
            },
            last_timestamp: Some(Time(
                DateTime::parse_from_rfc3339("2023-04-08T22:28:03Z")
                    .unwrap()
                    .into(),
            )),
            message: Some("Error: ImagePullBackOff".to_string()),
            metadata: ObjectMeta {
                annotations: None,
                creation_timestamp: Some(Time(
                    DateTime::parse_from_rfc3339("2023-04-08T22:27:40Z")
                        .unwrap()
                        .into(),
                )),
                deletion_grace_period_seconds: None,
                deletion_timestamp: None,
                finalizers: None,
                generate_name: None,
                generation: None,
                labels: None,
                managed_fields: None,
                name: Some("coredns-bbbc4b766-fv96b.17541619a910bfcd".to_string()),
                namespace: Some("kube-system".to_string()),
                owner_references: None,
                resource_version: Some("355929325".to_string()),
                self_link: None,
                uid: Some("bd42879f-7761-4fa0-b802-dfcf8502c44e".to_string()),
            },
            reason: Some("Failed".to_string()),
            related: None,
            reporting_component: Some("".to_string()),
            reporting_instance: Some("".to_string()),
            series: None,
            source: Some(EventSource {
                component: Some("kubelet".to_string()),
                host: None,
            }),
            type_: Some("Warning".to_string()),
        }
    }

    #[tokio::test]
    pub async fn test_processor_should_send_event() {
        let event = generate_event();
        let passed = AtomicBool::new(false);
        let client =
            Client::try_from(Config::new("https://localhost:6443/".try_into().unwrap())).unwrap();

        let processor = EventMonitor::new(
            ConfigMonitor {
                resources: vec![ConfigResource {
                    api_version: None,
                    kind: Some("Pod".to_string()),
                    label_selector: None,
                    namespace: Some("kube-system".to_string()),
                }],
                ..Default::default()
            },
            &GlobalConfiguration {
                dsn: None,
                environment: None,
                release: None,
                historical: true,
                levels: vec![],
            },
            Arc::new(CachingClient::new(client)),
            |_, se| {
                assert_eq!(se.type_, "warning".to_string());
                passed.store(true, Ordering::SeqCst);
            },
        );

        processor.process(event).await;
        assert_eq!(passed.load(Ordering::SeqCst), true);
    }
}
