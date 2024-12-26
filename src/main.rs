use crate::caching_client::CachingClient;
use crate::config::{ConfigMonitor, SentryConfig};
use crate::monitor::{Monitor, MonitorSubsystem};
use ::config::{Config, Environment, File};
use anyhow::Result;
use clap::Parser;
use futures::prelude::*;
use k8s_openapi::api::core::v1::Event;
use k8s_openapi::chrono;
use kube::runtime::{watcher, WatchStreamExt};
use kube::{Api, Client};
use log::{debug, warn, LevelFilter};
use sentry::types::Dsn;
use simple_logger::SimpleLogger;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let log_level = LevelFilter::from_str(&args.log_level).unwrap_or(LevelFilter::Error);
    SimpleLogger::new().with_level(log_level).init()?;

    let config: SentryConfig = Config::builder()
        .add_source(
            Environment::with_prefix("SENTRY")
                .try_parsing(true)
                .list_separator(","),
        )
        .add_source(File::from(PathBuf::from(args.config)))
        .set_default("log_level", Some(args.log_level))?
        .set_default("historical", true)?
        .build()?
        .try_deserialize()?;

    let log_level = &config.log_level.unwrap_or_else(|| "ERROR".to_string());
    let log_level = LevelFilter::from_str(log_level).unwrap_or(LevelFilter::Error);
    log::set_max_level(log_level);

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

    let (sender, _) = tokio::sync::broadcast::channel(512);

    let mut monitors = vec![];
    let client = Arc::new(CachingClient::new(Client::try_default().await?));
    for monitor_config in monitors_config.into_iter() {
        monitors.push(Monitor::new(
            monitor_config,
            &global_config,
            client.clone(),
            |hub, sentry_event| {
                let uuid = hub.capture_event(sentry::protocol::Event::from(sentry_event));
                debug!(target: "sentry_kubernetes::sentry_client", "Captured event (uuid = {})", uuid);
            }
        ))
    }

    let now = chrono::Utc::now();

    let mut stream = watcher(
        Api::<Event>::all(Client::try_default().await?),
        Default::default(),
    )
    .default_backoff()
    .applied_objects()
    .boxed();

    Toplevel::<anyhow::Error>::new(move |s| async move {
        for (i, notifier) in monitors.into_iter().enumerate() {
            let notifier_subsys = MonitorSubsystem::new(notifier, sender.subscribe());
            s.start(SubsystemBuilder::new(format!("notifier_{}", i), |a| {
                notifier_subsys.run(a)
            }));
        }

        let mut historical = config.historical;
        loop {
            select! {
                v = stream.try_next() => {
                    match v {
                        Ok(Some(e)) => {
                            if !historical {
                                let Some(ref t) = e.event_time else { continue };
                                if t.0 < now {
                                    continue;
                                }
                            }

                            historical = true;
                            let metadata = e.metadata.clone();
                            debug!(
                                "changes detected for object {}/{}",
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
