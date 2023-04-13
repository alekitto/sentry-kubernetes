use crate::processor::Processor;
use anyhow::Result;
use futures::prelude::*;
use getopts::Options;
use k8s_openapi::api::core::v1::Event;
use kube::api::ListParams;
use kube::runtime::{watcher, WatchStreamExt};
use kube::{Api, Client};
use lazy_static::lazy_static;
use log::{debug, error, info, LevelFilter};
use sentry::types::Dsn;
use simple_logger::SimpleLogger;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::sleep;

mod processor;
mod sentry_event;

lazy_static! {
    static ref SENTRY_DSN: String = env::var("DSN").unwrap_or_default();
    static ref ENV: String = env::var("ENVIRONMENT").unwrap_or_default();
    static ref RELEASE: String = env::var("RELEASE").unwrap_or_default();
}

fn print_usage(program: &str, opts: Options) {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let program = args[0].clone();

    let mut opts = Options::new();
    opts.optopt("l", "log-level", "set output file name", "ERROR");
    opts.optflag("h", "help", "print this help menu");
    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            panic!("{}", f.to_string())
        }
    };

    if matches.opt_present("h") {
        print_usage(&program, opts);
        return Ok(());
    }

    let log_level = env::var("LOG_LEVEL").unwrap_or("INFO".to_string());
    let log_level = matches.opt_get_default("l", log_level).unwrap();
    let log_level = LevelFilter::from_str(&log_level).unwrap_or(LevelFilter::Error);
    SimpleLogger::new().with_level(log_level).init().unwrap();

    let client = Client::try_default().await?;
    loop {
        if let Err(e) = watch_loop(client.clone()).await {
            error!("{}", e.to_string());
            sleep(Duration::from_secs(5)).await;
        }
    }
}

fn list_env(name: &str, default: Option<String>) -> Vec<String> {
    env::var(name)
        .unwrap_or(default.unwrap_or_default())
        .split(',')
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
}

async fn watch_loop(client: Client) -> Result<()> {
    info!("Initializing Sentry client");
    let dsn = Dsn::from_str(&SENTRY_DSN)?;
    let _sentry = sentry::init(sentry::ClientOptions {
        dsn: Some(dsn),
        environment: if ENV.is_empty() {
            None
        } else {
            Some(ENV.clone().into())
        },
        release: if RELEASE.is_empty() {
            None
        } else {
            Some(RELEASE.clone().into())
        },
        ..Default::default()
    });

    info!("Staring kubernetes watcher");

    let event_namespaces = list_env("EVENT_NAMESPACES", None);
    let exclude_components = list_env("COMPONENT_FILTER", None);
    let exclude_reasons = list_env("REASON_FILTER", None);
    let exclude_namespaces = list_env("EVENT_NAMESPACES_EXCLUDED", None);
    let event_levels = list_env("EVENT_LEVELS", Some("warning,error".to_string()));

    info!("Only reporting events of levels: {:?}", &event_levels);
    let processor = Processor::new(
        event_namespaces,
        exclude_components,
        exclude_reasons,
        exclude_namespaces,
        event_levels,
        |sentry_event| {
            sentry::capture_event(sentry::protocol::Event::from(sentry_event));
        },
    );

    let api = Api::<Event>::all(client);
    watcher(api, ListParams::default())
        .applied_objects()
        .try_for_each(|event| async {
            debug!("event: {:#?}", event);
            processor.process(event);

            Ok(())
        })
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::list_env;

    #[test]
    pub fn test_list_env() {
        let def_list = list_env(
            "THIS_SHOULD_NOT_BE_DEFINED",
            Some("warning,error".to_string()),
        );
        assert_eq!(def_list, vec!["warning".to_string(), "error".to_string()]);

        let def_list = list_env(
            "THIS_SHOULD_NOT_BE_DEFINED",
            Some("warning,,,x,,error".to_string()),
        );
        assert_eq!(
            def_list,
            vec!["warning".to_string(), "x".to_string(), "error".to_string()]
        );
    }
}
