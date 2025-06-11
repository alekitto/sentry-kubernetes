use crate::caching_client::CachingClient;
use crate::config::ConfigMonitor;
use crate::sentry_event::SentryEvent;
use crate::GlobalConfiguration;
use anyhow::Result;
pub use event::EventMonitor;
pub use event_subsystem::EventMonitorSubsystem;
use kube::Client;
use log::debug;
use sentry::types::Dsn;
use sentry::Hub;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

mod event;
mod event_subsystem;

static CLIENTS: LazyLock<Mutex<HashMap<Option<Dsn>, Arc<Hub>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn init_event_monitors(
    global_configuration: &GlobalConfiguration,
    config: Vec<ConfigMonitor>,
) -> Result<Vec<EventMonitor<impl Fn(&Hub, &SentryEvent) + use<>>>> {
    let mut monitors = vec![];
    let client = Arc::new(CachingClient::new(Client::try_default().await?));
    for config in config.into_iter() {
        monitors.push(EventMonitor::new(
            config,
            global_configuration,
            client.clone(),
            |hub, sentry_event| {
                let uuid = hub.capture_event(sentry::protocol::Event::from(sentry_event));
                debug!(target: "sentry_kubernetes::sentry_client", "Captured event (uuid = {})", uuid);
            }
        ))
    }

    Ok(monitors)
}
