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
use tokio::sync::{broadcast, Mutex, Notify};
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
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(self.poll_interval_secs));
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
        let workflow = match self.store.claim_next(&self.worker_id, self.lease_duration_secs) {
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
                    let _ = self.store.update_workflow_status(&workflow.id, "Cancelled", None);
                    let _ = self.store.release_lease(&workflow.id, &self.worker_id);
                    return;
                }
            }
        }

        // 4. Renew lease before running step (heartbeat)
        if let Err(e) = self.store.renew_lease(&workflow.id, &self.worker_id, self.lease_duration_secs) {
            warn!(workflow_id = %workflow.id, error = %e, "Failed to renew lease before step");
            // Continue anyway — the step may still complete within the original lease
        }

        // 5. Determine next step to run
        let next_step = self.determine_next_step(&workflow);

        match next_step {
            Some(step) => {
                // Insert step record
                let step_id = match self.store.insert_step(
                    &workflow.id,
                    step.name(),
                    None,
                ) {
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
                let result = self.run_step(&workflow, step).await;

                // 7. Renew lease after step completes (keep lease alive for next tick)
                if let Err(e) = self.store.renew_lease(&workflow.id, &self.worker_id, self.lease_duration_secs) {
                    warn!(workflow_id = %workflow.id, error = %e, "Failed to renew lease after step");
                }

                // 8. Record step result
                match result {
                    Ok(output_json) => {
                        info!(
                            workflow_id = %workflow.id,
                            step = step.name(),
                            "Step completed"
                        );
                        let _ = self.store.complete_step(&step_id, Some(&output_json));

                        // Check if this was the last step
                        if EvolutionStep::next_after(step.name()).is_none() {
                            // All steps done — mark workflow as Promoted
                            info!(workflow_id = %workflow.id, "All steps completed, workflow promoted");
                            let _ = self.store.update_workflow_status(&workflow.id, "Promoted", None);
                        }
                    }
                    Err(e) => {
                        error!(
                            workflow_id = %workflow.id,
                            step = step.name(),
                            error = %e,
                            "Step failed"
                        );
                        let _ = self.store.fail_step(&step_id, &e.to_string());

                        // Increment attempt
                        let _ = self.store.increment_attempt(&workflow.id);

                        // Schedule retry with backoff
                        let _ = self.store.schedule_retry(
                            &workflow.id,
                            Some(&format!("[{}] {}", step.name(), e)),
                        );
                    }
                }
            }
            None => {
                // No next step — workflow should already be in terminal state.
                // This can happen if all steps were already completed.
                debug!(workflow_id = %workflow.id, "No pending step found, workflow may be complete");
                // Ensure status is Promoted if not already terminal
                let _ = self.store.update_workflow_status(&workflow.id, "Promoted", None);
            }
        }

        // 9. Release lease
        if let Err(e) = self.store.release_lease(&workflow.id, &self.worker_id) {
            warn!(workflow_id = %workflow.id, error = %e, "Failed to release lease");
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
