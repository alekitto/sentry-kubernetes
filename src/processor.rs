use crate::sentry_event::SentryEvent;
use k8s_openapi::api::core::v1::{Event, Node, Pod};
use kube::{Api, Client};
use log::debug;
use sentry::{add_breadcrumb, Breadcrumb, Level};
use std::collections::BTreeMap;

pub struct Processor<F: Fn(&SentryEvent)> {
    event_namespaces: Vec<String>,
    exclude_components: Vec<String>,
    exclude_reasons: Vec<String>,
    exclude_namespaces: Vec<String>,
    event_levels: Vec<String>,
    sender: F,

    pod_api: Api<Pod>,
    nodes_api: Api<Node>,
}

pub struct ProcessorBuilder<F: Fn(&SentryEvent)> {
    event_namespaces: Vec<String>,
    exclude_components: Vec<String>,
    exclude_reasons: Vec<String>,
    exclude_namespaces: Vec<String>,
    event_levels: Vec<String>,
    sender: F,
    client: Client,
}

impl<F: Fn(&SentryEvent)> ProcessorBuilder<F> {
    fn new(client: Client, sender: F) -> Self {
        Self {
            event_namespaces: Default::default(),
            exclude_components: Default::default(),
            exclude_reasons: Default::default(),
            exclude_namespaces: Default::default(),
            event_levels: Default::default(),
            client,
            sender,
        }
    }

    #[must_use]
    pub fn event_namespaces(mut self, include: Vec<String>, exclude: Vec<String>) -> Self {
        self.event_namespaces = include;
        self.exclude_namespaces = exclude;
        self
    }

    #[must_use]
    pub fn event_components(mut self, exclude: Vec<String>) -> Self {
        self.exclude_components = exclude;
        self
    }

    #[must_use]
    pub fn event_reasons(mut self, exclude: Vec<String>) -> Self {
        self.exclude_reasons = exclude;
        self
    }

    #[must_use]
    pub fn event_levels(mut self, levels: Vec<String>) -> Self {
        self.event_levels = levels;
        self
    }
}

impl<F: Fn(&SentryEvent)> From<ProcessorBuilder<F>> for Processor<F> {
    fn from(value: ProcessorBuilder<F>) -> Self {
        Processor::new(
            value.event_namespaces,
            value.exclude_components,
            value.exclude_reasons,
            value.exclude_namespaces,
            value.event_levels,
            value.client,
            value.sender,
        )
    }
}

impl<F: Fn(&SentryEvent)> Processor<F> {
    pub fn builder(client: Client, sender: F) -> ProcessorBuilder<F> {
        ProcessorBuilder::new(client, sender)
    }

    fn new(
        event_namespaces: Vec<String>,
        exclude_components: Vec<String>,
        exclude_reasons: Vec<String>,
        exclude_namespaces: Vec<String>,
        event_levels: Vec<String>,
        client: Client,
        sender: F,
    ) -> Self {
        Self {
            event_namespaces,
            exclude_components,
            exclude_reasons,
            exclude_namespaces,
            event_levels,
            sender,

            pod_api: Api::<Pod>::all(client.clone()),
            nodes_api: Api::<Node>::all(client),
        }
    }

    pub async fn process(&self, event: Event) {
        let mut sentry_event = SentryEvent::from(event);
        let mut hostname = sentry_event.source_host;
        if hostname.is_none() {
            if sentry_event.kind.as_deref() == Some("Pod") {
                if let Ok(pod) = self.pod_api.get(&sentry_event.name).await {
                    hostname = pod.spec.and_then(|p| p.node_name);
                }
            }
        }

        if let Some(hostname) = hostname.as_deref() {
            if let Ok(node) = self.nodes_api.get(hostname).await {
                sentry_event.node_labels = node.metadata.labels.unwrap_or_default();
            }
        }

        if self.exclude_components.contains(&sentry_event.component) {
            debug!("excluded by component filter");
            return;
        }

        if self.exclude_reasons.contains(&sentry_event.reason) {
            debug!("excluded by reason filter");
            return;
        }

        if self.exclude_namespaces.contains(&sentry_event.namespace) {
            debug!("excluded by namespace filter");
            return;
        }

        if !self.event_namespaces.is_empty()
            && !self.event_namespaces.contains(&sentry_event.namespace)
        {
            debug!("event not in monitored namespace");
            return;
        }

        if self
            .event_levels
            .iter()
            .any(|e| e == &sentry_event.level.to_string())
            || sentry_event.level == Level::Error
        {
            sentry_event.source_host = hostname;

            debug!("sending event to sentry");
            (self.sender)(&sentry_event);
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

        add_breadcrumb(breadcrumb);
    }
}

#[cfg(test)]
mod tests {
    use crate::processor::Processor;
    use k8s_openapi::api::core::v1::{Event, EventSource, ObjectReference};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};
    use k8s_openapi::chrono::DateTime;
    use kube::Client;
    use std::sync::atomic::{AtomicBool, Ordering};

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
                cluster_name: None,
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
        let client = Client::try_default().await.unwrap();
        let processor = Processor::new(
            vec![],
            vec![],
            vec![],
            vec![],
            vec!["warning".to_string(), "error".to_string()],
            client,
            |se| {
                assert_eq!(se.type_, "warning".to_string());
                passed.store(true, Ordering::SeqCst);
            },
        );

        processor.process(event).await;
        assert_eq!(passed.load(Ordering::SeqCst), true);
    }
}
