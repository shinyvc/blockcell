//! Durable workflow worker for skill evolution.
//!
//! The agent runtime only triggers records and wakes this worker. The long
//! LLM/audit/compile/deploy pipeline runs here, behind the workflow store's
//! claim/lease protocol, so runtime select loops stay responsive.

use blockcell_agent::EvolutionNotifier;
use blockcell_core::{Error, Result};
use blockcell_skills::evolution::EvolutionStatus;
use blockcell_skills::{EvolutionService, EvolutionServiceConfig, LLMProvider};
use blockcell_storage::evolution_workflow::WorkflowRecord;
use blockcell_storage::EvolutionWorkflowStore;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot, Notify};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const DEFAULT_LEASE_DURATION_SECS: i64 = 300;
const SKILL_PIPELINE_STEP: &str = "SkillEvolutionPipeline";

pub struct SkillEvolutionWorker {
    store: EvolutionWorkflowStore,
    service: EvolutionService,
    worker_id: String,
    poll_interval_secs: u64,
    lease_duration_secs: i64,
    wakeup: Notify,
}

impl SkillEvolutionWorker {
    pub fn new(
        store: EvolutionWorkflowStore,
        skills_dir: PathBuf,
        config: EvolutionServiceConfig,
        llm_provider: Option<Arc<dyn LLMProvider>>,
    ) -> Self {
        let mut service = EvolutionService::new(skills_dir, config);
        if let Some(provider) = llm_provider {
            service.set_llm_provider(provider);
        }

        Self {
            store,
            service,
            worker_id: uuid::Uuid::new_v4().to_string(),
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            lease_duration_secs: DEFAULT_LEASE_DURATION_SECS,
            wakeup: Notify::new(),
        }
    }

    pub fn notify(&self) {
        self.wakeup.notify_one();
    }

    pub async fn run_loop(self: Arc<Self>, mut shutdown: broadcast::Receiver<()>) {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(self.poll_interval_secs));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        info!(worker_id = %self.worker_id, "SkillEvolutionWorker started");

        loop {
            tokio::select! {
                _ = shutdown.recv() => {
                    info!(worker_id = %self.worker_id, "SkillEvolutionWorker shutting down");
                    return;
                }
                _ = self.wakeup.notified() => {
                    debug!(worker_id = %self.worker_id, "SkillEvolutionWorker woken up");
                    self.tick().await;
                }
                _ = interval.tick() => {
                    debug!(worker_id = %self.worker_id, "SkillEvolutionWorker poll tick");
                    self.tick().await;
                }
            }
        }
    }

    async fn tick(&self) {
        if let Err(e) = self.recover() {
            warn!(error = %e, "Failed to recover skill evolution workflows");
        }

        if let Err(e) = self.enqueue_pending_records().await {
            warn!(error = %e, "Failed to sync pending skill evolution records");
        }

        let workflow =
            match self
                .store
                .claim_next(&self.worker_id, self.lease_duration_secs, Some("skill"))
            {
                Ok(Some(workflow)) => workflow,
                Ok(None) => {
                    if let Err(e) = self.service.tick_observations().await {
                        warn!(error = %e, "Skill evolution observation tick failed");
                    }
                    return;
                }
                Err(e) => {
                    error!(error = %e, "Failed to claim skill evolution workflow");
                    return;
                }
            };

        info!(
            worker_id = %self.worker_id,
            workflow_id = %workflow.id,
            skill = %workflow.capability_id,
            evolution_id = %workflow.description,
            "Claimed skill evolution workflow"
        );

        if self.cancelled(&workflow) {
            let _ = self.store.update_workflow_status_if_owned(
                &workflow.id,
                &self.worker_id,
                "Cancelled",
                None,
            );
            let _ = self.store.release_lease(&workflow.id, &self.worker_id);
            return;
        }

        match self
            .store
            .renew_lease(&workflow.id, &self.worker_id, self.lease_duration_secs)
        {
            Ok(true) => {}
            Ok(false) => {
                warn!(workflow_id = %workflow.id, "Lost skill evolution lease before pipeline");
                return;
            }
            Err(e) => {
                warn!(workflow_id = %workflow.id, error = %e, "Failed to renew skill evolution lease");
                return;
            }
        }

        let step_id = match self
            .store
            .insert_step(&workflow.id, SKILL_PIPELINE_STEP, None)
        {
            Ok(id) => id,
            Err(e) => {
                error!(workflow_id = %workflow.id, error = %e, "Failed to insert skill evolution step");
                let _ = self.store.release_lease(&workflow.id, &self.worker_id);
                return;
            }
        };

        let (heartbeat_stop, heartbeat_handle) = self.spawn_lease_heartbeat(&workflow.id);
        let result = self.run_skill_workflow(&workflow).await;
        let _ = heartbeat_stop.send(());
        if let Err(e) = heartbeat_handle.await {
            if !e.is_cancelled() {
                warn!(workflow_id = %workflow.id, error = %e, "Skill evolution lease heartbeat failed");
            }
        }

        match self
            .store
            .renew_lease(&workflow.id, &self.worker_id, self.lease_duration_secs)
        {
            Ok(true) => {}
            Ok(false) => {
                warn!(workflow_id = %workflow.id, "Lost skill evolution lease after pipeline");
                return;
            }
            Err(e) => {
                warn!(workflow_id = %workflow.id, error = %e, "Failed to renew skill evolution lease after pipeline");
                return;
            }
        }

        match result {
            Ok(output_json) => {
                if !self.complete_step(&workflow, &step_id, Some(&output_json)) {
                    return;
                }
                // Use "Observing" instead of "Promoted" — the skill is in
                // observation window and may still be rolled back if error
                // rate exceeds threshold. "Promoted" would prevent rollback.
                let _ = self.store.update_workflow_status_if_owned(
                    &workflow.id,
                    &self.worker_id,
                    "Observing",
                    None,
                );
            }
            Err(e) => {
                let message = e.to_string();
                let _ = self.store.fail_step_if_owned(
                    &workflow.id,
                    &step_id,
                    &self.worker_id,
                    &message,
                );
                if self.skill_record_is_terminal(&workflow) {
                    let _ = self.store.update_workflow_status_if_owned(
                        &workflow.id,
                        &self.worker_id,
                        "Failed",
                        Some(&message),
                    );
                } else {
                    let _ = self.store.schedule_retry_if_owned(
                        &workflow.id,
                        &self.worker_id,
                        Some(&message),
                    );
                }
            }
        }

        let _ = self.store.release_lease(&workflow.id, &self.worker_id);

        if let Err(e) = self.service.tick_observations().await {
            warn!(error = %e, "Skill evolution observation tick failed");
        }
    }

    async fn enqueue_pending_records(&self) -> Result<()> {
        let pending = self.service.list_pending_ids().await;
        if pending.is_empty() {
            return Ok(());
        }

        let existing = self.store.list_workflows(None)?;
        for (skill_name, evolution_id) in pending {
            if existing.iter().any(|workflow| {
                workflow.description == evolution_id && Self::workflow_blocks_enqueue(workflow)
            }) {
                continue;
            }

            let workflow_id = self.store.enqueue(&skill_name, &evolution_id, "skill")?;
            info!(
                workflow_id = %workflow_id,
                skill = %skill_name,
                evolution_id = %evolution_id,
                "Enqueued durable skill evolution workflow"
            );
            self.wakeup.notify_one();
        }

        Ok(())
    }

    fn workflow_blocks_enqueue(workflow: &WorkflowRecord) -> bool {
        !matches!(
            workflow.status.as_str(),
            "Promoted" | "Failed" | "Cancelled"
        )
    }

    async fn run_skill_workflow(&self, workflow: &WorkflowRecord) -> Result<String> {
        let skill_name = workflow.capability_id.as_str();
        let evolution_id = workflow.description.as_str();

        // Check if the evolution pipeline is already running via EvolutionService.tick().
        // If so, wait for it to complete (with timeout) to avoid duplicate processing.
        if self
            .service
            .is_evolution_pipeline_active(evolution_id)
            .await
        {
            info!(
                evolution_id = %evolution_id,
                "Skill evolution pipeline already in progress via tick(), waiting for completion"
            );
            let max_wait = std::time::Duration::from_secs(90);
            let start = std::time::Instant::now();
            while start.elapsed() < max_wait {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if !self
                    .service
                    .is_evolution_pipeline_active(evolution_id)
                    .await
                {
                    break;
                }
            }
        }

        self.service
            .process_pending_evolution(skill_name, evolution_id)
            .await?;

        let record = self.service.evolution().load_record(evolution_id)?;
        let status = record.status.normalize();
        match status {
            EvolutionStatus::Observing | EvolutionStatus::Completed => Ok(serde_json::json!({
                "skill_name": skill_name,
                "evolution_id": evolution_id,
                "status": format!("{:?}", record.status),
            })
            .to_string()),
            EvolutionStatus::Failed | EvolutionStatus::RolledBack => Err(Error::Evolution(
                format!("skill evolution ended in {:?}", record.status),
            )),
            other => Err(Error::Evolution(format!(
                "skill evolution did not reach observing state: {:?}",
                other
            ))),
        }
    }

    fn skill_record_is_terminal(&self, workflow: &WorkflowRecord) -> bool {
        self.service
            .evolution()
            .load_record(&workflow.description)
            .map(|record| {
                matches!(
                    *record.status.normalize(),
                    EvolutionStatus::Completed
                        | EvolutionStatus::Failed
                        | EvolutionStatus::RolledBack
                        | EvolutionStatus::Observing
                )
            })
            .unwrap_or(false)
    }

    fn complete_step(
        &self,
        workflow: &WorkflowRecord,
        step_id: &str,
        output_json: Option<&str>,
    ) -> bool {
        match self
            .store
            .complete_step_if_owned(&workflow.id, step_id, &self.worker_id, output_json)
        {
            Ok(true) => true,
            Ok(false) => {
                warn!(workflow_id = %workflow.id, "Lost skill evolution lease before completing step");
                false
            }
            Err(e) => {
                warn!(workflow_id = %workflow.id, error = %e, "Failed to complete skill evolution step");
                false
            }
        }
    }

    fn cancelled(&self, workflow: &WorkflowRecord) -> bool {
        match self.store.read_pending_events(&workflow.id) {
            Ok(events) => events.iter().any(|event| event.event_type == "cancel"),
            Err(e) => {
                warn!(workflow_id = %workflow.id, error = %e, "Failed to read skill evolution events");
                false
            }
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
            let mut consecutive_failures = 0u32;
            const MAX_CONSECUTIVE_FAILURES: u32 = 3;

            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = interval.tick() => {
                        match store.renew_lease(&workflow_id, &worker_id, lease_duration_secs) {
                            Ok(true) => {
                                consecutive_failures = 0;
                            }
                            Ok(false) => {
                                warn!(workflow_id = %workflow_id, worker_id = %worker_id, "Lost skill evolution lease during heartbeat");
                                break;
                            }
                            Err(e) => {
                                consecutive_failures += 1;
                                warn!(workflow_id = %workflow_id, worker_id = %worker_id, error = %e, consecutive_failures = consecutive_failures, max_retries = MAX_CONSECUTIVE_FAILURES, "Failed to heartbeat skill evolution lease (will retry)");
                                if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                                    warn!(workflow_id = %workflow_id, worker_id = %worker_id, "Too many consecutive heartbeat failures, giving up lease");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        });

        (stop_tx, handle)
    }

    fn recover(&self) -> Result<()> {
        let recovered = self.store.recover_expired_leases()?;
        if !recovered.is_empty() {
            info!(
                count = recovered.len(),
                "Recovered skill evolution workflows"
            );
            self.wakeup.notify_one();
        }
        Ok(())
    }
}

impl EvolutionNotifier for SkillEvolutionWorker {
    fn notify(&self) {
        self.wakeup.notify_one();
    }
}
