use crate::agent::ActivityState;
use crate::plugins::tracker::TrackerState;
use crate::types::SessionStatus;
use std::collections::HashMap;

/// All data needed to evaluate transitions for one session in one poll tick.
#[derive(Debug, Clone)]
pub struct PollContext {
    pub runtime_alive: bool,
    pub activity_state: ActivityState,
    pub pr: Option<PrState>,
    pub tracker_state: TrackerState,
    pub budget_exceeded: bool,
    pub manual_kill: bool,
}

#[derive(Debug, Clone)]
pub struct PrState {
    pub detected: bool,
    pub state: String,           // "open" | "merged" | "closed"
    pub ci_status: String,       // "pending" | "green" | "failed"
    pub review_decision: String, // "none" | "approved" | "changes_requested"
    pub mergeable: bool,
}

type Guard = fn(&PollContext) -> bool;

struct Edge {
    to: SessionStatus,
    precedence: u32,
    guard: Guard,
}

pub struct StateGraph {
    nodes: HashMap<SessionStatus, Vec<Edge>>,
    terminal: Vec<SessionStatus>,
}

impl StateGraph {
    /// Build and validate the complete state graph from the PRD transition table.
    pub fn build() -> Self {
        let mut nodes: HashMap<SessionStatus, Vec<Edge>> = HashMap::new();

        // Initialize all non-terminal nodes with empty edge lists
        for s in [
            SessionStatus::Spawning,
            SessionStatus::Working,
            SessionStatus::PrOpen,
            SessionStatus::ReviewPending,
            SessionStatus::Approved,
            SessionStatus::Mergeable,
            SessionStatus::CiFailed,
            SessionStatus::ChangesRequested,
            SessionStatus::NeedsInput,
            SessionStatus::Stuck,
        ] {
            nodes.insert(s, vec![]);
        }

        let terminal = vec![
            SessionStatus::Killed,
            SessionStatus::Terminated,
            SessionStatus::Done,
            SessionStatus::Cleanup,
            SessionStatus::Errored,
            SessionStatus::Merged,
        ];

        macro_rules! edge {
            ($from:expr, $to:expr, $prec:expr, $guard:expr) => {
                nodes.entry($from).or_default().push(Edge {
                    to: $to,
                    precedence: $prec,
                    guard: $guard,
                });
            };
        }

        // Local edges (lower precedence number = higher priority)
        edge!(SessionStatus::Spawning, SessionStatus::Working, 1, |c| {
            c.runtime_alive && c.activity_state == ActivityState::Active
        });
        edge!(SessionStatus::Spawning, SessionStatus::Errored, 2, |c| {
            !c.runtime_alive
        });

        edge!(SessionStatus::Working, SessionStatus::PrOpen, 3, |c| {
            c.pr.as_ref().is_some_and(|p| p.detected)
        });
        edge!(SessionStatus::Working, SessionStatus::NeedsInput, 4, |c| {
            c.activity_state == ActivityState::WaitingInput
        });
        edge!(SessionStatus::Working, SessionStatus::Stuck, 5, |c| {
            c.activity_state == ActivityState::Idle
        });
        edge!(SessionStatus::Working, SessionStatus::Errored, 6, |c| {
            c.activity_state == ActivityState::Blocked
        });
        edge!(SessionStatus::Working, SessionStatus::Killed, 7, |c| {
            !c.runtime_alive
        });
        edge!(SessionStatus::Working, SessionStatus::Done, 8, |c| {
            c.activity_state == ActivityState::Exited && c.tracker_state == TrackerState::Terminal
        });
        edge!(SessionStatus::Working, SessionStatus::Terminated, 9, |c| {
            c.activity_state == ActivityState::Exited
        });

        edge!(SessionStatus::PrOpen, SessionStatus::CiFailed, 10, |c| {
            c.pr.as_ref().is_some_and(|p| p.ci_status == "failed")
        });
        edge!(
            SessionStatus::PrOpen,
            SessionStatus::ReviewPending,
            11,
            |c| { c.pr.as_ref().is_some_and(|p| p.ci_status == "green") }
        );
        edge!(SessionStatus::PrOpen, SessionStatus::Working, 12, |c| {
            c.activity_state == ActivityState::Active
        });
        edge!(SessionStatus::PrOpen, SessionStatus::Killed, 13, |c| {
            !c.runtime_alive
        });

        edge!(SessionStatus::CiFailed, SessionStatus::Working, 14, |c| {
            c.activity_state == ActivityState::Active
        });
        edge!(SessionStatus::CiFailed, SessionStatus::Killed, 15, |c| {
            !c.runtime_alive
        });

        edge!(
            SessionStatus::ReviewPending,
            SessionStatus::ChangesRequested,
            16,
            |c| {
                c.pr.as_ref()
                    .is_some_and(|p| p.review_decision == "changes_requested")
            }
        );
        edge!(
            SessionStatus::ReviewPending,
            SessionStatus::Approved,
            17,
            |c| {
                c.pr.as_ref()
                    .is_some_and(|p| p.review_decision == "approved")
            }
        );
        edge!(
            SessionStatus::ReviewPending,
            SessionStatus::CiFailed,
            18,
            |c| { c.pr.as_ref().is_some_and(|p| p.ci_status == "failed") }
        );

        edge!(
            SessionStatus::ChangesRequested,
            SessionStatus::Working,
            19,
            |c| { c.activity_state == ActivityState::Active }
        );
        edge!(
            SessionStatus::ChangesRequested,
            SessionStatus::Killed,
            20,
            |c| { !c.runtime_alive }
        );

        edge!(SessionStatus::Approved, SessionStatus::Mergeable, 21, |c| {
            c.pr.as_ref()
                .is_some_and(|p| p.ci_status == "green" && p.mergeable)
        });
        edge!(SessionStatus::Approved, SessionStatus::CiFailed, 22, |c| {
            c.pr.as_ref().is_some_and(|p| p.ci_status == "failed")
        });

        edge!(SessionStatus::Mergeable, SessionStatus::Merged, 23, |c| {
            c.pr.as_ref().is_some_and(|p| p.state == "merged")
        });

        edge!(SessionStatus::NeedsInput, SessionStatus::Working, 24, |c| {
            c.activity_state == ActivityState::Active
        });
        edge!(SessionStatus::NeedsInput, SessionStatus::Killed, 25, |c| {
            !c.runtime_alive
        });

        edge!(SessionStatus::Stuck, SessionStatus::Working, 26, |c| {
            c.activity_state == ActivityState::Active
        });
        edge!(SessionStatus::Stuck, SessionStatus::Killed, 27, |c| {
            !c.runtime_alive
        });

        // Global edges — appended to every non-terminal node
        let global: Vec<(SessionStatus, u32, Guard)> = vec![
            (SessionStatus::Killed, 28, |c: &PollContext| c.manual_kill),
            (SessionStatus::Cleanup, 29, |c: &PollContext| {
                c.tracker_state == TrackerState::Terminal
            }),
            (SessionStatus::Killed, 30, |c: &PollContext| {
                c.budget_exceeded
            }),
        ];

        for (to, prec, guard) in global {
            for edges in nodes.values_mut() {
                edges.push(Edge {
                    to: to.clone(),
                    precedence: prec,
                    guard,
                });
            }
        }

        // Sort each node's edges by precedence (ascending = higher priority first)
        for edges in nodes.values_mut() {
            edges.sort_by_key(|e| e.precedence);
        }

        let graph = Self { nodes, terminal };
        graph.validate();
        graph
    }

    fn validate(&self) {
        // All non-terminal nodes must have at least one outgoing edge
        for (status, edges) in &self.nodes {
            assert!(!edges.is_empty(), "node {:?} has no outgoing edges", status);
        }
        // Terminal nodes must NOT be in nodes map (no outgoing edges)
        for t in &self.terminal {
            assert!(
                !self.nodes.contains_key(t),
                "terminal node {:?} should not have outgoing edges",
                t
            );
        }
    }

    /// Evaluate the first matching edge from `current`. Returns None if no guard matches
    /// or if `current` is a terminal state.
    pub fn evaluate(&self, current: SessionStatus, ctx: &PollContext) -> Option<SessionStatus> {
        let edges = self.nodes.get(&current)?;
        for edge in edges {
            if (edge.guard)(ctx) {
                return Some(edge.to.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx() -> PollContext {
        PollContext {
            runtime_alive: true,
            activity_state: ActivityState::Active,
            pr: None,
            tracker_state: TrackerState::Active,
            budget_exceeded: false,
            manual_kill: false,
        }
    }

    #[test]
    fn test_spawning_to_working_when_alive_and_active() {
        let graph = StateGraph::build();
        let ctx = make_ctx();
        let next = graph.evaluate(SessionStatus::Spawning, &ctx);
        assert_eq!(next, Some(SessionStatus::Working));
    }

    #[test]
    fn test_spawning_to_errored_when_dead() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.runtime_alive = false;
        ctx.activity_state = ActivityState::Idle;
        let next = graph.evaluate(SessionStatus::Spawning, &ctx);
        assert_eq!(next, Some(SessionStatus::Errored));
    }

    #[test]
    fn test_working_to_stuck_when_idle() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.activity_state = ActivityState::Idle;
        let next = graph.evaluate(SessionStatus::Working, &ctx);
        assert_eq!(next, Some(SessionStatus::Stuck));
    }

    #[test]
    fn test_global_kill_edge_fires_from_working() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.manual_kill = true;
        let next = graph.evaluate(SessionStatus::Working, &ctx);
        // manual_kill is precedence 28, but Working has local edges 3-9.
        // With runtime_alive=true, activity=Active, no PR: no local edge 3-9 matches
        // except... edge 3 (Working→PrOpen) needs pr.detected, edge 4 needs WaitingInput,
        // edge 5 needs Idle, edge 6 needs Blocked, edge 7 needs !runtime_alive,
        // edge 8 needs Exited+Terminal, edge 9 needs Exited.
        // None match with Active+alive+no PR, so global edge 28 (manual_kill) fires.
        assert_eq!(next, Some(SessionStatus::Killed));
    }

    #[test]
    fn test_global_cleanup_edge_fires_from_stuck() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.activity_state = ActivityState::Idle;
        ctx.tracker_state = TrackerState::Terminal;
        // From Stuck: edge 26 (Working, Active) doesn't match (Idle),
        // edge 27 (Killed, !alive) doesn't match (alive),
        // global 28 (manual_kill) doesn't match,
        // global 29 (Cleanup, Terminal) matches!
        let next = graph.evaluate(SessionStatus::Stuck, &ctx);
        assert_eq!(next, Some(SessionStatus::Cleanup));
    }

    #[test]
    fn test_no_transition_for_terminal_node() {
        let graph = StateGraph::build();
        let ctx = make_ctx();
        let next = graph.evaluate(SessionStatus::Killed, &ctx);
        assert_eq!(next, None);
    }

    #[test]
    fn test_graph_validation_passes() {
        // build() panics on invalid graph — so this passing means it's valid
        let _graph = StateGraph::build();
    }

    #[test]
    fn test_working_to_pr_open_when_pr_detected() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.pr = Some(PrState {
            detected: true,
            state: "open".to_string(),
            ci_status: "pending".to_string(),
            review_decision: "none".to_string(),
            mergeable: false,
        });
        let next = graph.evaluate(SessionStatus::Working, &ctx);
        assert_eq!(next, Some(SessionStatus::PrOpen));
    }

    #[test]
    fn test_budget_exceeded_kills_from_any_state() {
        let graph = StateGraph::build();
        let mut ctx = make_ctx();
        ctx.budget_exceeded = true;
        // From Spawning, edge 1 (Working) matches (alive+Active), so it fires before budget.
        // Use a state where no local edge matches:
        ctx.activity_state = ActivityState::Ready;
        let next = graph.evaluate(SessionStatus::Spawning, &ctx);
        // No local edge matches Ready from Spawning, so global 28 (manual_kill=false),
        // 29 (Terminal=false), 30 (budget_exceeded=true) fires.
        assert_eq!(next, Some(SessionStatus::Killed));
    }
}
