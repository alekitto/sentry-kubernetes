use crate::monitor::EventMonitor;
use crate::sentry_event::SentryEvent;
use futures::FutureExt;
use k8s_openapi::api::core::v1::Event;
use log::info;
use sentry::Hub;
use std::time::Duration;
use tokio::sync::broadcast::Receiver;
use tokio::time::sleep;
use tokio_graceful_shutdown::SubsystemHandle;

pub struct EventMonitorSubsystem<F: Fn(&Hub, &SentryEvent)> {
    notifier: EventMonitor<F>,
    receiver: Receiver<Event>,
}

impl<F: Fn(&Hub, &SentryEvent)> EventMonitorSubsystem<F> {
    pub fn new(notifier: EventMonitor<F>, receiver: Receiver<Event>) -> Self {
        Self { notifier, receiver }
    }

    pub async fn run(mut self, subsys: SubsystemHandle<anyhow::Error>) -> anyhow::Result<()> {
        loop {
            if let Some(Ok(o)) = self.receiver.recv().now_or_never() {
                self.notifier.process(o).await;
            }

            if subsys.is_shutdown_requested() {
                break;
            } else {
                sleep(Duration::from_secs(1)).await;
            }
        }

        info!("Shutdown requested, terminating...");
        Ok(())
    }
}
