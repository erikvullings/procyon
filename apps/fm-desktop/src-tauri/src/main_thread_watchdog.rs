//! Detects a blocked main thread, the condition behind the macOS rotating-ball cursor.
//!
//! A helper thread posts a trivial closure to the main thread once a second and measures how long
//! the main thread takes to run it. A long delay means something held the main thread (a
//! synchronous command, a native call, window-system work) and is logged with its duration and the
//! wall-clock time, so it can be lined up with the other `fm_perf` entries around it.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Runtime};

const PROBE_INTERVAL: Duration = Duration::from_secs(1);
/// Below this the main thread is merely busy; above it the UI visibly stutters.
const STALL_THRESHOLD: Duration = Duration::from_millis(250);
/// A probe that never returns is reported instead of waited on forever.
const PROBE_GIVE_UP: Duration = Duration::from_secs(30);

/// How long a synchronous command may run before it is logged (see the invoke handler).
pub(crate) const SLOW_SYNC_COMMAND: Duration = Duration::from_millis(50);

/// Starts the watchdog thread. It exits by itself once the app handle can no longer reach the
/// main thread (the application is shutting down).
pub(crate) fn start<R: Runtime>(app: AppHandle<R>) {
    let spawned = std::thread::Builder::new()
        .name("main-thread-watchdog".to_owned())
        .spawn(move || {
            loop {
                std::thread::sleep(PROBE_INTERVAL);
                let (reply, receive) = mpsc::channel::<()>();
                let posted = Instant::now();
                if app
                    .run_on_main_thread(move || {
                        let _ = reply.send(());
                    })
                    .is_err()
                {
                    return;
                }
                match receive.recv_timeout(PROBE_GIVE_UP) {
                    Ok(()) => {
                        let delay = posted.elapsed();
                        if delay >= STALL_THRESHOLD {
                            tracing::warn!(
                                target: "fm_perf",
                                op = "main_thread_stall",
                                delay_ms = delay.as_secs_f64() * 1000.0,
                                "main thread was unresponsive"
                            );
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        tracing::error!(
                            target: "fm_perf",
                            op = "main_thread_stall",
                            delay_ms = PROBE_GIVE_UP.as_secs_f64() * 1000.0,
                            "main thread did not respond within the probe limit"
                        );
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the main-thread watchdog");
    }
}
