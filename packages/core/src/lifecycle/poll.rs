use crate::agent::{Agent, GatherContext};
use crate::lifecycle::graph::{PollContext, StateGraph};
use crate::plugins::tracker::{Tracker, TrackerState};
use crate::runtime::Runtime;
use crate::session_store::{SessionMetadata, SessionStore, StoreError};
use crate::types::SessionStatus;
use std::sync::Arc;
use tracing::info;

pub struct PollTick<'a> {
    pub graph: &'a StateGraph,
    pub store: &'a SessionStore,
    pub agent: Arc<dyn Agent>,
    pub runtime: Arc<dyn Runtime>,
    pub tracker: Arc<dyn Tracker>,
    pub terminal_states: Vec<String>,
}

impl<'a> PollTick<'a> {
    /// Run one poll tick: gather -> evaluate -> transition (sequentially).
    pub async fn run(&self) -> Result<Vec<(String, SessionStatus, SessionStatus)>, StoreError> {
        let sessions = self.store.list_active().await?;
        let mut transitions = Vec::new();

        for session in sessions {
            if let Some(new_status) = self.process_session(&session).await {
                transitions.push((
                    session.id.clone(),
                    session.status.clone(),
                    new_status.clone(),
                ));
                self.apply_transition(&session, new_status).await;
            }
        }

        Ok(transitions)
    }

    async fn process_session(&self, session: &SessionMetadata) -> Option<SessionStatus> {
        let ctx = self.gather(session).await;
        self.graph.evaluate(session.status.clone(), &ctx)
    }

    async fn gather(&self, session: &SessionMetadata) -> PollContext {
        // 1. Runtime liveness (cheapest -- short-circuit if dead)
        let runtime_alive = self.runtime.is_alive(&session.id).await.unwrap_or(false);

        // 2. Activity state (requires terminal output)
        let activity_state = if runtime_alive {
            let output = self
                .runtime
                .get_output(&session.id, 50)
                .await
                .unwrap_or_default();
            let aux_path = self.agent.auxiliary_log_path();
            let aux_log = if let Some(ref p) = aux_path {
                tokio::fs::read_to_string(p).await.ok()
            } else {
                None
            };
            let gather_ctx = GatherContext {
                terminal_output: output,
                auxiliary_log: aux_log,
                auxiliary_log_path: aux_path,
            };
            self.agent.detect_activity(&gather_ctx)
        } else {
            crate::agent::ActivityState::Exited
        };

        // 3. PR state -- MVP stub (no GitHub API calls yet)
        let pr = None;

        // 4. Tracker state -- classify from issue state
        let tracker_state = self.gather_tracker_state(session).await;

        PollContext {
            runtime_alive,
            activity_state,
            pr,
            tracker_state,
            // MVP: kill is applied directly by handle_kill (sets Killed in
            // SessionStore); poll-driven kill (graph edges 28-30) is reserved
            // for post-MVP. These fields are structurally present so the graph
            // evaluator compiles, but they never fire from this code path.
            budget_exceeded: false,
            manual_kill: false,
        }
    }

    async fn gather_tracker_state(&self, session: &SessionMetadata) -> TrackerState {
        match self.tracker.get_issue(&session.issue_url).await {
            Ok(Some(issue)) => {
                let state = issue["state"].as_str().unwrap_or("open");
                crate::plugins::tracker::classify_state(state, &self.terminal_states)
            }
            _ => TrackerState::Active, // fail open
        }
    }

    async fn apply_transition(&self, session: &SessionMetadata, new_status: SessionStatus) {
        info!(
            session_id = %session.id,
            from = %session.status,
            to = %new_status,
            "lifecycle transition"
        );

        let mut updated = session.clone();
        updated.status = new_status.clone();
        updated.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        if let Err(e) = self.store.write_metadata(&updated).await {
            tracing::error!(session_id = %session.id, error = %e, "failed to write metadata after transition");
            return;
        }

        // Entry actions for terminal transitions
        match new_status {
            SessionStatus::Killed | SessionStatus::Cleanup => {
                let _ = self.runtime.destroy(&session.id).await;
            }
            _ => {}
        }
    }
}
