//! Core Evolution Workflow Worker
//!
//! Background worker that claims workflows from the EvolutionWorkflowStore,
//! advances them step-by-step through the CoreEvolution engine, and persists
//! step results.
//!
//! Follows the DreamService pattern: `run_loop(self, shutdown)` + `tokio::select!`.

use blockcell_agent::EvolutionNotifier;
use blockcell_core::ProviderKind;
use blockcell_skills::{CoreEvolution, EvolutionStep};
use blockcell_storage::evolution_workflow::WorkflowRecord;
use blockcell_storage::EvolutionWorkflowStore;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, Mutex, Notify};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Default poll interval in seconds.
const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;

/// Default lease duration in seconds.
const DEFAULT_LEASE_DURATION_SECS: i64 = 300;

/// Core Evolution Workflow Worker.
///
/// Runs in the background, claims workflows from the store,
/// and advances them step-by-step through the CoreEvolution engine.
pub struct EvolutionWorker {
    store: EvolutionWorkflowStore,
    engine: Arc<Mutex<CoreEvolution>>,
    worker_id: String,
    poll_interval_secs: u64,
    lease_duration_secs: i64,
    wakeup: Notify,
}

impl EvolutionWorker {
    pub fn new(store: EvolutionWorkflowStore, engine: Arc<Mutex<CoreEvolution>>) -> Self {
        let worker_id = uuid::Uuid::new_v4().to_string();
        Self {
            store,
            engine,
            worker_id,
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            lease_duration_secs: DEFAULT_LEASE_DURATION_SECS,
            wakeup: Notify::new(),
        }
    }

    /// Wake up the worker (called from runtime tick, lightweight).
    pub fn notify(&self) {
        self.wakeup.notify_one();
    }

    /// Run the worker main loop. Blocks until shutdown signal received.
    pub async fn run_loop(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(self.poll_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!(worker_id = %self.worker_id, "EvolutionWorker started");

        loop {
            tokio::select! {
                _ = shutdown.recv() => {
                    info!(worker_id = %self.worker_id, "EvolutionWorker shutting down");
                    return;
                }
                _ = self.wakeup.notified() => {
                    debug!(worker_id = %self.worker_id, "Woken up by notify");
                    self.tick().await;
                }
                _ = interval.tick() => {
                    debug!(worker_id = %self.worker_id, "Poll tick");
                    self.tick().await;
                }
            }
        }
    }

    /// Single tick: recover expired leases, claim a workflow, advance one step, release.
    async fn tick(&self) {
        // 1. Recover any workflows with expired leases (crash recovery)
        if let Err(e) = self.recover() {
            warn!(error = %e, "Failed to recover expired leases");
        }

        // 2. Claim next workflow
        let workflow = match self
            .store
            .claim_next(&self.worker_id, self.lease_duration_secs)
        {
            Ok(Some(w)) => w,
            Ok(None) => {
                debug!(worker_id = %self.worker_id, "No workflow to claim");
                return;
            }
            Err(e) => {
                error!(error = %e, "Failed to claim workflow");
                return;
            }
        };

        info!(
            worker_id = %self.worker_id,
            workflow_id = %workflow.id,
            capability_id = %workflow.capability_id,
            "Claimed workflow"
        );

        // 3. Check for cancel events
        if let Ok(events) = self.store.read_pending_events(&workflow.id) {
            for event in &events {
                if event.event_type == "cancel" {
                    info!(workflow_id = %workflow.id, "Workflow cancelled by event");
                    let _ = self.store.update_workflow_status_if_owned(
                        &workflow.id,
                        &self.worker_id,
                        "Cancelled",
                        None,
                    );
                    let _ = self.store.release_lease(&workflow.id, &self.worker_id);
                    return;
                }
            }
        }

        // 4. Renew lease before running step (heartbeat)
        match self
            .store
            .renew_lease(&workflow.id, &self.worker_id, self.lease_duration_secs)
        {
            Ok(true) => {}
            Ok(false) => {
                warn!(workflow_id = %workflow.id, "Lost workflow lease before step; skipping");
                return;
            }
            Err(e) => {
                warn!(workflow_id = %workflow.id, error = %e, "Failed to renew lease before step");
                return;
            }
        }

        // 5. Determine next step to run
        let next_step = self.determine_next_step(&workflow);

        let mut wake_after_release = false;

        match next_step {
            Some(step) => {
                // Insert step record
                let step_id = match self.store.insert_step(&workflow.id, step.name(), None) {
                    Ok(id) => id,
                    Err(e) => {
                        error!(workflow_id = %workflow.id, error = %e, "Failed to insert step record");
                        let _ = self.store.release_lease(&workflow.id, &self.worker_id);
                        return;
                    }
                };

                info!(
                    workflow_id = %workflow.id,
                    step = step.name(),
                    "Running evolution step"
                );

                // 6. Run the step via the engine
                let (heartbeat_stop, heartbeat_handle) = self.spawn_lease_heartbeat(&workflow.id);
                let result = self.run_step(&workflow, step).await;
                let _ = heartbeat_stop.send(());
                if let Err(e) = heartbeat_handle.await {
                    if !e.is_cancelled() {
                        warn!(workflow_id = %workflow.id, error = %e, "Lease heartbeat task failed");
                    }
                }

                // 7. Renew lease after step completes (keep lease alive for next tick)
                match self.store.renew_lease(
                    &workflow.id,
                    &self.worker_id,
                    self.lease_duration_secs,
                ) {
                    Ok(true) => {}
                    Ok(false) => {
                        warn!(workflow_id = %workflow.id, "Lost workflow lease after step; discarding step result");
                        return;
                    }
                    Err(e) => {
                        warn!(workflow_id = %workflow.id, error = %e, "Failed to renew lease after step");
                        return;
                    }
                }

                // 8. Record step result
                match result {
                    Ok(output_json) => {
                        info!(
                            workflow_id = %workflow.id,
                            step = step.name(),
                            "Step completed"
                        );
                        match self.store.complete_step_if_owned(
                            &workflow.id,
                            &step_id,
                            &self.worker_id,
                            Some(&output_json),
                        ) {
                            Ok(true) => {}
                            Ok(false) => {
                                warn!(workflow_id = %workflow.id, step = step.name(), "Lost workflow lease before completing step");
                                return;
                            }
                            Err(e) => {
                                warn!(workflow_id = %workflow.id, step = step.name(), error = %e, "Failed to complete step");
                                return;
                            }
                        }

                        // Check if this was the last step
                        if EvolutionStep::next_after(step.name()).is_none() {
                            // All steps done — mark workflow as Promoted
                            info!(workflow_id = %workflow.id, "All steps completed, workflow promoted");
                            match self.store.update_workflow_status_if_owned(
                                &workflow.id,
                                &self.worker_id,
                                "Promoted",
                                None,
                            ) {
                                Ok(true) => {}
                                Ok(false) => {
                                    warn!(workflow_id = %workflow.id, "Lost workflow lease before marking promoted");
                                    return;
                                }
                                Err(e) => {
                                    warn!(workflow_id = %workflow.id, error = %e, "Failed to mark workflow promoted");
                                    return;
                                }
                            }
                        } else {
                            // Check for cancellation before requeueing the next step
                            if self.cancelled(&workflow) {
                                info!(workflow_id = %workflow.id, "Workflow cancelled between steps");
                                let _ = self.store.update_workflow_status_if_owned(
                                    &workflow.id,
                                    &self.worker_id,
                                    "Cancelled",
                                    None,
                                );
                                let _ = self.store.release_lease(&workflow.id, &self.worker_id);
                                return;
                            }

                            match self.store.update_workflow_status_if_owned(
                                &workflow.id,
                                &self.worker_id,
                                "Requested",
                                None,
                            ) {
                                Ok(true) => {
                                    wake_after_release = true;
                                }
                                Ok(false) => {
                                    warn!(workflow_id = %workflow.id, step = step.name(), "Lost workflow lease before requeueing next step");
                                    return;
                                }
                                Err(e) => {
                                    warn!(workflow_id = %workflow.id, error = %e, "Failed to requeue workflow for next step");
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            workflow_id = %workflow.id,
                            step = step.name(),
                            error = %e,
                            "Step failed"
                        );
                        match self.store.fail_step_if_owned(
                            &workflow.id,
                            &step_id,
                            &self.worker_id,
                            &e.to_string(),
                        ) {
                            Ok(true) => {}
                            Ok(false) => {
                                warn!(workflow_id = %workflow.id, step = step.name(), "Lost workflow lease before failing step");
                                return;
                            }
                            Err(err) => {
                                warn!(workflow_id = %workflow.id, step = step.name(), error = %err, "Failed to mark step failed");
                                return;
                            }
                        }

                        // Increment attempt and schedule retry with backoff.
                        match self.store.schedule_retry_if_owned(
                            &workflow.id,
                            &self.worker_id,
                            Some(&format!("[{}] {}", step.name(), e)),
                        ) {
                            Ok(true) => {}
                            Ok(false) => {
                                warn!(workflow_id = %workflow.id, step = step.name(), "Lost workflow lease before scheduling retry");
                                return;
                            }
                            Err(err) => {
                                warn!(workflow_id = %workflow.id, step = step.name(), error = %err, "Failed to schedule retry");
                                return;
                            }
                        }
                    }
                }
            }
            None => {
                // No next step — workflow should already be in terminal state.
                // This can happen if all steps were already completed.
                debug!(workflow_id = %workflow.id, "No pending step found, workflow may be complete");
                // Ensure status is Promoted if not already terminal
                let _ = self.store.update_workflow_status_if_owned(
                    &workflow.id,
                    &self.worker_id,
                    "Promoted",
                    None,
                );
            }
        }

        // 9. Release lease
        match self.store.release_lease(&workflow.id, &self.worker_id) {
            Ok(true) => {}
            Ok(false) => {
                debug!(workflow_id = %workflow.id, "Workflow lease already released or moved");
            }
            Err(e) => {
                warn!(workflow_id = %workflow.id, error = %e, "Failed to release lease");
            }
        }

        if wake_after_release {
            self.wakeup.notify_one();
        }
    }

    /// Determine the next step to run for a workflow.
    ///
    /// Looks at the last completed step in the store and returns the next one.
    /// If no steps have been completed, returns the first step (BuildPrompt).
    fn determine_next_step(&self, workflow: &WorkflowRecord) -> Option<EvolutionStep> {
        match self.store.get_last_completed_step(&workflow.id) {
            Ok(Some(last_step)) => {
                debug!(
                    workflow_id = %workflow.id,
                    last_step = %last_step.step_name,
                    "Found last completed step"
                );
                EvolutionStep::next_after(&last_step.step_name)
            }
            Ok(None) => {
                // No completed steps — start from the beginning
                Some(EvolutionStep::first())
            }
            Err(e) => {
                warn!(
                    workflow_id = %workflow.id,
                    error = %e,
                    "Failed to query last completed step, starting from beginning"
                );
                Some(EvolutionStep::first())
            }
        }
    }

    /// Run a single evolution step via the CoreEvolution engine.
    ///
    /// For the BuildPrompt step (first step), uses `run_step_with_context`
    /// to create the CoreEvolutionRecord if it doesn't exist yet.
    /// For subsequent steps, uses `run_step` directly.
    async fn run_step(
        &self,
        workflow: &WorkflowRecord,
        step: EvolutionStep,
    ) -> blockcell_core::Result<String> {
        let engine = self.engine.lock().await;

        // Parse provider_kind from workflow record
        let provider_kind = match workflow.provider_kind.as_str() {
            "python" | "ExternalApi" => ProviderKind::ExternalApi,
            "process" | "Process" => ProviderKind::Process,
            "rust" | "dylib" | "DynamicLibrary" => ProviderKind::DynamicLibrary,
            "rhai" | "RhaiScript" => ProviderKind::RhaiScript,
            _ => ProviderKind::Process,
        };

        if step == EvolutionStep::BuildPrompt {
            engine
                .run_step_with_context(
                    &workflow.id,
                    step,
                    &workflow.capability_id,
                    &workflow.description,
                    provider_kind,
                )
                .await
        } else {
            engine.run_step(&workflow.id, step).await
        }
    }

    fn spawn_lease_heartbeat(&self, workflow_id: &str) -> (oneshot::Sender<()>, JoinHandle<()>) {
        let store = self.store.clone();
        let workflow_id = workflow_id.to_string();
        let worker_id = self.worker_id.clone();
        let lease_duration_secs = self.lease_duration_secs;
        let heartbeat_secs = (lease_duration_secs / 3).clamp(5, 60) as u64;
        let (stop_tx, mut stop_rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(heartbeat_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        break;
                    }
                    _ = interval.tick() => {
                        match store.renew_lease(&workflow_id, &worker_id, lease_duration_secs) {
                            Ok(true) => {
                                debug!(workflow_id = %workflow_id, worker_id = %worker_id, "Renewed evolution workflow lease");
                            }
                            Ok(false) => {
                                warn!(workflow_id = %workflow_id, worker_id = %worker_id, "Lost evolution workflow lease during heartbeat");
                                break;
                            }
                            Err(e) => {
                                warn!(workflow_id = %workflow_id, worker_id = %worker_id, error = %e, "Failed to heartbeat evolution workflow lease");
                                break;
                            }
                        }
                    }
                }
            }
        });

        (stop_tx, handle)
    }

    /// Check whether the workflow has a pending cancel event.
    fn cancelled(&self, workflow: &WorkflowRecord) -> bool {
        match self.store.read_pending_events(&workflow.id) {
            Ok(events) => events.iter().any(|event| event.event_type == "cancel"),
            Err(e) => {
                warn!(workflow_id = %workflow.id, error = %e, "Failed to read pending events for cancellation check");
                false
            }
        }
    }

    /// Recover workflows with expired leases and retry-eligible workflows.
    ///
    /// Delegates to `store.recover_expired_leases()` which handles:
    /// - Claimed workflows with expired leases → reset to Requested
    /// - RetryScheduled workflows whose backoff has elapsed → eligible for claiming
    fn recover(&self) -> blockcell_core::Result<()> {
        let recovered = self.store.recover_expired_leases()?;
        if !recovered.is_empty() {
            info!(
                count = recovered.len(),
                "Recovered expired lease / retry-eligible workflows"
            );
            // Wake ourselves up to process recovered workflows
            self.wakeup.notify_one();
        }
        Ok(())
    }
}

/// Implement the `EvolutionNotifier` trait defined in `blockcell_agent`,
/// so that `Arc<EvolutionWorker>` can be passed as `Arc<dyn EvolutionNotifier + Send + Sync>`.
impl EvolutionNotifier for EvolutionWorker {
    fn notify(&self) {
        self.wakeup.notify_one();
    }
}
