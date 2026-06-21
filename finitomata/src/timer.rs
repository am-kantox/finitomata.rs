//! Recurring timer for FSM `on_timer` callbacks.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A recurring timer that sends [`TimerTick`] messages at a fixed interval.
///
/// Used internally by unsupervised FSM instances. For supervised FSMs,
/// timers are managed via joerl's `send_after` mechanism instead.
pub struct FsmTimer {
    handle: JoinHandle<()>,
    cancel: mpsc::Sender<()>,
}

impl FsmTimer {
    /// Starts a new timer that sends ticks to `tick_sender` at the given interval.
    /// The first tick fires after one full interval (no immediate tick).
    pub fn start(interval: Duration, tick_sender: mpsc::Sender<TimerTick>) -> Self {
        let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);

        let handle = tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.tick().await; // skip first immediate tick

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        if tick_sender.send(TimerTick).await.is_err() {
                            break;
                        }
                    }
                    _ = cancel_rx.recv() => {
                        break;
                    }
                }
            }
        });

        Self {
            handle,
            cancel: cancel_tx,
        }
    }

    /// Stops the timer and waits for the background task to exit.
    pub async fn stop(self) {
        let _ = self.cancel.send(()).await;
        let _ = self.handle.await;
    }

    /// Cancels the timer without waiting (fire-and-forget).
    pub fn cancel(&self) {
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            let _ = cancel.send(()).await;
        });
    }
}

/// Marker message sent by [`FsmTimer`] on each tick.
#[derive(Debug, Clone)]
pub struct TimerTick;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timer_fires() {
        let (tx, mut rx) = mpsc::channel(10);
        let _timer = FsmTimer::start(Duration::from_millis(50), tx);

        let tick = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
        assert!(tick.is_ok());
    }

    #[tokio::test]
    async fn test_timer_stop() {
        let (tx, mut rx) = mpsc::channel(10);
        let timer = FsmTimer::start(Duration::from_millis(50), tx);

        timer.stop().await;

        tokio::time::sleep(Duration::from_millis(100)).await;
        let result = rx.try_recv();
        assert!(result.is_err());
    }
}
