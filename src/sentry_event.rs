use k8s_openapi::api::core::v1::Event;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use lazy_static::lazy_static;
use sentry::protocol::ClientSdkInfo;
use sentry::types::protocol::v7;
use sentry::Level;
use serde_json::{to_value, Value};
use std::borrow::Borrow;
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::env;
use std::str::FromStr;
use std::time::SystemTime;

lazy_static! {
    static ref CLUSTER_NAME: String = env::var("CLUSTER_NAME").unwrap_or_default();
    static ref SDK_VALUE: Cow<'static, ClientSdkInfo> = {
        let info = ClientSdkInfo {
            name: "sentry-kubernetes".to_string(),
            version: "1.0.0".to_string(),
            integrations: vec![],
            packages: vec![],
        };

        Cow::Owned(info)
    };
}

pub struct SentryEvent {
    pub type_: String,
    pub level: Level,
    pub component: String,
    pub source_host: String,
    pub reason: String,
    pub metadata: ObjectMeta,
    pub namespace: String,
    pub kind: Option<String>,
    pub name: String,
    pub message: Option<String>,
    pub creation_timestamp: Option<SystemTime>,
}

impl SentryEvent {
    pub fn obj_name(&self) -> String {
        if !self.namespace.is_empty() && !self.name.is_empty() {
            format!("{}/{}", self.namespace, self.name)
        } else {
            self.namespace.to_string()
        }
    }

    pub fn metadata_map(&self) -> BTreeMap<String, Value> {
        match to_value(&self.metadata) {
            Ok(Value::Object(tree)) => {
                let mut map = BTreeMap::new();
                for (k, v) in tree.iter() {
                    if k != "managedFields" {
                        map.insert(k.clone(), v.clone());
                    }
                }

                Some(map)
            }
            _ => None,
        }
        .unwrap_or_default()
    }
}

impl From<Event> for SentryEvent {
    fn from(value: Event) -> Self {
        let meta = value.metadata;
        let namespace = value
            .involved_object
            .namespace
            .or(meta.namespace.clone())
            .unwrap_or_else(|| "default".to_string());
        let creation_timestamp = meta.creation_timestamp.as_ref().map(|t| t.0.into());
        let event_type = value.type_.unwrap_or_default().to_lowercase();
        let level = if event_type == "normal" {
            "info".to_string()
        } else {
            event_type.clone()
        };

        Self {
            type_: event_type,
            level: Level::from_str(&level).unwrap(),
            component: value
                .source
                .as_ref()
                .and_then(|s| s.component.clone())
                .unwrap_or_default(),
            source_host: value
                .source
                .and_then(|s| s.host)
                .unwrap_or("n/a".to_string()),
            reason: value.reason.unwrap_or_default(),
            metadata: meta,
            namespace,
            kind: value.involved_object.kind,
            name: value.involved_object.name.unwrap_or_default(),
            message: value.message,
            creation_timestamp,
        }
    }
}

impl From<&SentryEvent> for v7::Event<'_> {
    fn from(value: &SentryEvent) -> Self {
        let mut tags = BTreeMap::new();
        let mut fingerprint: Vec<Cow<str>> = vec![];

        if !CLUSTER_NAME.is_empty() {
            tags.insert("cluster".to_string(), CLUSTER_NAME.clone());
        }

        if !value.component.is_empty() {
            tags.insert("component".to_string(), value.component.clone());
        }

        if !value.reason.is_empty() {
            tags.insert("reason".to_string(), value.reason.clone());
            fingerprint.push(value.reason.clone().into());
        }

        if !value.namespace.is_empty() {
            tags.insert("namespace".to_string(), value.namespace.clone());
            fingerprint.push(value.namespace.clone().into());
        }

        if !value.name.is_empty() {
            tags.insert("name".to_string(), value.name.clone());
            fingerprint.push(value.name.clone().into());
        }

        if let Some(kind) = value.kind.clone() {
            if !kind.is_empty() {
                tags.insert("kind".to_string(), kind.clone());
                fingerprint.push(kind.into());
            }
        }

        let mut v7_event = v7::Event::new();
        v7_event.message = value.message.clone();
        v7_event.culprit = Some(format!("{} {}", value.obj_name(), value.reason));
        v7_event.server_name = Some(value.source_host.clone().into());
        v7_event.sdk = Some(Cow::Borrowed(SDK_VALUE.borrow()));
        if let Some(timestamp) = value.creation_timestamp {
            v7_event.timestamp = timestamp;
        }
        v7_event.extra = value.metadata_map();
        v7_event.fingerprint = fingerprint.into();
        v7_event.level = value.level;
        v7_event.tags = tags;

        v7_event
    }
}
