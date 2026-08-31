//! Cancellation and duplicate-scan-id rejection for local disk-usage scans (task 0118 follow-up).
//!
//! `disk_usage::scan_disk_usage` performs the actual (now worker-capped, sequentially traversed)
//! filesystem scan; this coordinator owns the `scan_id -> CancellationToken` registry so a caller
//! can interrupt a running scan through [`Self::cancel_disk_usage`], duplicate `scan_id`s are
//! rejected rather than silently racing two scans against the same id, and dropping the async
//! scan future (an aborted Tauri command or a disconnected Axum request) still cancels the
//! underlying blocking traversal instead of letting it run to completion unobserved. Mirrors
//! `DirectoryService`'s `Mutex<HashMap<Id, CancellationToken>>` pane-request registry rather than
//! `ChecksumCoordinator`'s job-engine delegation, since disk-usage scanning has no separate engine
//! crate of its own.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use fm_events::EventBus;
use fm_transport_dto::{ScanDiskUsageRequestDto, ScanDiskUsageResponseDto};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::ApplicationError;

pub(crate) struct DiskUsageCoordinator {
    events: EventBus,
    registry: Mutex<ScanRegistry>,
}

#[derive(Default)]
struct ScanRegistry {
    active: HashMap<Uuid, CancellationToken>,
    cancelled_before_start: HashSet<Uuid>,
}

impl DiskUsageCoordinator {
    pub(crate) fn new(events: EventBus) -> Self {
        Self {
            events,
            registry: Mutex::new(ScanRegistry::default()),
        }
    }

    /// Runs a disk-usage scan, registering a fresh cancellation token under `request.scan_id` for
    /// the duration of the call. Rejects a `scan_id` that is already running with
    /// [`ApplicationError::InvalidRequest`]. The registration is released — and its token
    /// cancelled — when this future resolves, errors, or is simply dropped before completion, so
    /// an interrupted caller (an aborted Tauri command, a disconnected Axum request) still stops
    /// the underlying blocking traversal and frees the `scan_id` for reuse.
    pub(crate) async fn scan_disk_usage(
        &self,
        request: ScanDiskUsageRequestDto,
    ) -> Result<ScanDiskUsageResponseDto, ApplicationError> {
        let scan_id = request.scan_id;
        let cancellation = CancellationToken::new();
        {
            let mut registry = self
                .registry
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if registry.cancelled_before_start.remove(&scan_id) {
                return Err(ApplicationError::OperationCancelled);
            }
            if registry.active.contains_key(&scan_id) {
                return Err(ApplicationError::InvalidRequest(format!(
                    "disk-usage scan {scan_id} is already running"
                )));
            }
            registry.active.insert(scan_id, cancellation.clone());
        }
        let _guard = ScanGuard {
            coordinator: self,
            scan_id,
            cancellation: cancellation.clone(),
        };
        crate::disk_usage::scan_disk_usage(self.events.clone(), request, cancellation).await
    }

    /// Cancels a running scan. Cancellation is idempotent and is retained briefly for an unknown
    /// id so a Stop request that overtakes scan registration still prevents the scan from starting.
    pub(crate) fn cancel_disk_usage(&self, scan_id: Uuid) -> Result<(), ApplicationError> {
        let mut registry = self
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match registry.active.get(&scan_id) {
            Some(cancellation) => {
                cancellation.cancel();
            }
            None => {
                if registry.cancelled_before_start.len() >= 1_024 {
                    registry.cancelled_before_start.clear();
                }
                registry.cancelled_before_start.insert(scan_id);
            }
        }
        Ok(())
    }
}

/// Unregisters a scan's cancellation token when the guard is dropped — on normal completion,
/// error, panic, or the enclosing future being dropped without ever completing — and cancels the
/// token unconditionally so an in-flight blocking traversal stops promptly instead of running to
/// completion unobserved.
struct ScanGuard<'a> {
    coordinator: &'a DiskUsageCoordinator,
    scan_id: Uuid,
    cancellation: CancellationToken,
}

impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.coordinator
            .registry
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .remove(&self.scan_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_before_registration_prevents_the_scan_from_starting() {
        let coordinator = DiskUsageCoordinator::new(EventBus::new(16));
        let scan_id = Uuid::new_v4();
        let root = tempfile::tempdir().expect("create fixture root");
        let request = ScanDiskUsageRequestDto {
            workspace_id: Uuid::new_v4(),
            scan_id,
            location: fm_domain::Location::from_native_path(root.path())
                .expect("map fixture root to a location")
                .into(),
            expand_root: false,
        };

        assert_eq!(coordinator.cancel_disk_usage(scan_id), Ok(()));
        assert_eq!(
            coordinator.scan_disk_usage(request).await,
            Err(ApplicationError::OperationCancelled)
        );
    }

    /// Dropping the scan future before it resolves must still cancel the underlying blocking
    /// traversal and unregister the `scan_id`, so it can be reused for a fresh scan immediately.
    /// `tokio::spawn` + `JoinHandle::abort` is used rather than dropping the future directly,
    /// because an `async fn`'s body does not run at all until first polled — spawning it onto
    /// the runtime and letting it run briefly (registering the `scan_id`) before aborting is the
    /// only way to actually exercise `ScanGuard::drop` here rather than a future that never
    /// started.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dropping_the_scan_future_cancels_work_and_frees_the_scan_id() {
        let root = tempfile::tempdir().expect("create fixture root");
        for directory_index in 0..4 {
            let directory = root.path().join(format!("dir-{directory_index}"));
            std::fs::create_dir(&directory).expect("create fixture subdirectory");
            for file_index in 0..5_000 {
                std::fs::write(directory.join(format!("file-{file_index:05}.txt")), b"x")
                    .expect("write fixture file");
            }
        }
        let coordinator = std::sync::Arc::new(DiskUsageCoordinator::new(EventBus::new(16)));
        let scan_id = Uuid::new_v4();
        let request = ScanDiskUsageRequestDto {
            workspace_id: Uuid::new_v4(),
            scan_id,
            location: fm_domain::Location::from_native_path(root.path())
                .expect("map fixture root to a location")
                .into(),
            expand_root: false,
        };

        let task = {
            let coordinator = std::sync::Arc::clone(&coordinator);
            let request = request.clone();
            tokio::spawn(async move { coordinator.scan_disk_usage(request).await })
        };
        // Give the spawned task a brief moment to actually start running (and register its
        // `scan_id`) before aborting it mid-flight.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        task.abort();
        let _ = task.await;

        // The `scan_id` must have been unregistered by the guard's `Drop`, and a fresh scan reusing
        // the same id must be accepted rather than rejected as a duplicate.
        assert!(
            !coordinator
                .registry
                .lock()
                .expect("lock registry")
                .active
                .contains_key(&scan_id)
        );
        let rescan = coordinator.scan_disk_usage(request).await;
        assert!(rescan.is_ok(), "expected a rescan to succeed: {rescan:?}");
    }
}
