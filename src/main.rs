use crate::monitor::{init_event_monitors, EventMonitorSubsystem};
use anyhow::Result;
use clap::Parser;
use futures::prelude::*;
use k8s_openapi::api::core::v1::Event;
use k8s_openapi::chrono;
use kube::runtime::{watcher, WatchStreamExt};
use kube::{Api, Client};
use log::debug;
use sentry::types::Dsn;
use std::time::Duration;
use tokio::select;
use tokio_graceful_shutdown::{SubsystemBuilder, Toplevel};

mod caching_client;
mod config;
mod k8s;
mod monitor;
mod selector;
mod sentry_event;

/// Monitor kubernetes resources and send errors to Sentry.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Set output log level
    #[arg(short, long, default_value = "ERROR")]
    log_level: String,
}

pub struct GlobalConfiguration {
    pub dsn: Option<Dsn>,
    pub environment: Option<String>,
    pub release: Option<String>,
    pub levels: Vec<String>,
    pub historical: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let now = chrono::Utc::now();

    let (global_config, monitors_config) = config::parse_config()?;

    let (sender, _) = tokio::sync::broadcast::channel(512);
    let monitors = init_event_monitors(&global_config, monitors_config).await?;

    let kube_client = Client::try_default().await?;
    let mut event_stream = watcher(Api::<Event>::all(kube_client.clone()), Default::default())
        .default_backoff()
        .applied_objects()
        .boxed();

    Toplevel::<anyhow::Error>::new(move |s| async move {
        for (i, monitor) in monitors.into_iter().enumerate() {
            let monitor_subsystem = EventMonitorSubsystem::new(monitor, sender.subscribe());
            s.start(SubsystemBuilder::new(format!("monitor_{}", i), |a| {
                monitor_subsystem.run(a)
            }));
        }

        let mut historical = global_config.historical;
        loop {
            select! {
                v = event_stream.try_next() => {
                    match v {
                        Ok(Some(e)) => {
                            if !historical {
                                let Some(ref t) = e.event_time else { continue };
                                if t.0 < now {
                                    continue;
                                }

                                historical = true;
                            }

                            let metadata = e.metadata.clone();
                            debug!(
                                "received event {}/{}",
                                metadata.namespace.unwrap_or_default(),
                                metadata.name.unwrap_or_default()
                            );

                            sender.send(e).unwrap();
                        },
                        Ok(None) => continue,
                        Err(_) => {
                            s.request_shutdown();
                            break;
                        }
                    }
                },
                _ = s.on_shutdown_requested() => break,
                else => continue,
            }
        }

        s.wait_for_children().await;
    })
    .catch_signals()
    .handle_shutdown_requests(Duration::from_secs(60))
    .await
    .map_err(Into::into)
}
