use crate::sentry_event::SentryEvent;
use k8s_openapi::api::core::v1::Event;
use log::debug;
use sentry::{add_breadcrumb, Breadcrumb, Level};
use std::collections::BTreeMap;
use std::sync::RwLock;
use std::time::SystemTime;

pub struct Processor<F: Fn(&SentryEvent)> {
    event_namespaces: Vec<String>,
    exclude_components: Vec<String>,
    exclude_reasons: Vec<String>,
    exclude_namespaces: Vec<String>,
    event_levels: Vec<String>,
    sender: F,
    last_event_ts: RwLock<Option<SystemTime>>,
}

impl<F: Fn(&SentryEvent)> Processor<F> {
    pub fn new(
        event_namespaces: Vec<String>,
        exclude_components: Vec<String>,
        exclude_reasons: Vec<String>,
        exclude_namespaces: Vec<String>,
        event_levels: Vec<String>,
        sender: F,
    ) -> Self {
        Self {
            event_namespaces,
            exclude_components,
            exclude_reasons,
            exclude_namespaces,
            event_levels,
            sender,
            last_event_ts: RwLock::default(),
        }
    }

    pub fn process(&self, event: Event) {
        let sentry_event = SentryEvent::from(event);
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

        if let Some(ts) = sentry_event.creation_timestamp {
            let mut last_ts = self.last_event_ts.write().unwrap();
            if let Some(last_ts) = *last_ts {
                if last_ts < ts {
                    return;
                }
            }

            let _ = last_ts.insert(ts.clone());
        }

        if self
            .event_levels
            .iter()
            .any(|e| e == &sentry_event.level.to_string())
            || sentry_event.level == Level::Error
        {
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

    #[test]
    pub fn test_processor_should_send_event() {
        let event = generate_event();
        let passed = AtomicBool::new(false);
        let processor = Processor::new(
            vec![],
            vec![],
            vec![],
            vec![],
            vec!["warning".to_string(), "error".to_string()],
            |se| {
                assert_eq!(se.type_, "warning".to_string());
                passed.store(true, Ordering::SeqCst);
            },
        );

        processor.process(event);
        assert_eq!(passed.load(Ordering::SeqCst), true);
    }

    #[test]
    pub fn test_processor_should_not_send_past_events() {
        let first_event = generate_event();
        let mut second_event = generate_event();
        second_event.metadata.creation_timestamp = Some(Time(
            DateTime::parse_from_rfc3339("2023-04-07T22:27:40Z")
                .unwrap()
                .into(),
        ));
        let third_event = generate_event();

        let passed = AtomicUsize::default();
        let processor = Processor::new(
            vec![],
            vec![],
            vec![],
            vec![],
            vec!["warning".to_string(), "error".to_string()],
            |_| {
                passed.fetch_add(1, Ordering::SeqCst);
            },
        );

        for event in [first_event, second_event, third_event] {
            processor.process(event);
        }

        assert_eq!(passed.load(Ordering::SeqCst), 2);
    }
}
