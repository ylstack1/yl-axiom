//! Run-scoped `ToolApprovalBroker` with exact previews, unserializable one-use permits bound
//! to raw arguments, denial suppression, synchronous invalidation, and `ask_user` prompts.
//!
//! Security contracts:
//! - Memory-only run grants; `computer_use` NEVER accepts run-wide grants (`ALLOW_EXACT_ACTION_FOR_RUN`).
//! - Permits bind `(scope, tool_name, arguments_json, policy_revision)` and claim atomically once.
//! - Calling `invalidate()` synchronously revokes all in-flight permits and cached run grants.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use tokio::sync::{Mutex, oneshot};

use crate::permissions::{
    AgentPermissionGate, COMPUTER_USE_TOOL_NAME, HarnessToolDefinition, ToolAccessScope,
};

#[derive(Debug, Clone)]
pub struct ToolApprovalRequest {
    pub request_id: String,
    pub run_id: String,
    pub chat_id: String,
    pub call_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub scope: ToolAccessScope,
    pub policy_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolApprovalDecision {
    AllowOnce,
    AllowExactActionForRun,
    Deny,
    Stop,
}

#[derive(Debug, Clone)]
pub struct UserQuestion {
    pub request_id: String,
    pub chat_id: String,
    pub call_id: String,
    pub question: String,
    pub options: Vec<String>,
}

/// Deliberately NOT serializable: proves live authorization for one exact tool invocation.
pub struct ToolExecutionPermit {
    broker: Arc<ToolApprovalBroker>,
    pub scope: ToolAccessScope,
    pub tool_name: String,
    pub arguments_json: String,
    policy_revision: u64,
    consumed: AtomicBool,
}

impl ToolExecutionPermit {
    pub fn is_revoked(&self) -> bool {
        self.broker.is_permit_revoked(self)
    }

    pub fn claim(
        &self,
        scope: &ToolAccessScope,
        external_tools: &[HarnessToolDefinition],
        name: &str,
        arguments_json: &str,
    ) -> Result<(), String> {
        if scope != &self.scope || name != self.tool_name || arguments_json != self.arguments_json {
            return Err("Approval does not match this exact action.".into());
        }
        if let Some(reason) = self.broker.permit_denial(self, external_tools) {
            return Err(reason);
        }
        if self
            .consumed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("This one-use execution permit was already consumed.".into());
        }
        Ok(())
    }
}

pub enum ToolAuthorization {
    Allowed(ToolExecutionPermit),
    Denied(String),
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ActionKey {
    name: String,
    arguments: String,
}

struct PendingApproval {
    request: ToolApprovalRequest,
    tx: Option<oneshot::Sender<ToolApprovalDecision>>,
}

struct PendingQuestion {
    question: UserQuestion,
    tx: Option<oneshot::Sender<Option<String>>>,
}

struct BrokerInner {
    observed_scope: Option<ToolAccessScope>,
    observed_revision: u64,
    sequence: u64,
    grants: HashSet<ActionKey>,
    denials: HashSet<ActionKey>,
    pending: HashMap<String, PendingApproval>,
    pending_questions: HashMap<String, PendingQuestion>,
}

pub struct ToolApprovalBroker {
    pub run_id: String,
    pub chat_id: String,
    scope_source: RwLock<Option<ToolAccessScope>>,
    closed: AtomicBool,
    revision: AtomicU64,
    inner: Mutex<BrokerInner>,
}

impl ToolApprovalBroker {
    pub fn new(
        run_id: impl Into<String>,
        chat_id: impl Into<String>,
        initial_scope: ToolAccessScope,
    ) -> Arc<Self> {
        Arc::new(Self {
            run_id: run_id.into(),
            chat_id: chat_id.into(),
            scope_source: RwLock::new(Some(initial_scope.clone())),
            closed: AtomicBool::new(false),
            revision: AtomicU64::new(0),
            inner: Mutex::new(BrokerInner {
                observed_scope: Some(initial_scope),
                observed_revision: 0,
                sequence: 0,
                grants: HashSet::new(),
                denials: HashSet::new(),
                pending: HashMap::new(),
                pending_questions: HashMap::new(),
            }),
        })
    }

    pub fn current_scope(&self) -> Option<ToolAccessScope> {
        if self.closed.load(Ordering::SeqCst) {
            return None;
        }
        self.scope_source.read().ok()?.clone()
    }

    /// Updates live permissions and synchronously invalidates all prior grants & permits.
    pub fn update_scope(&self, new_scope: Option<ToolAccessScope>) {
        if let Ok(mut guard) = self.scope_source.write() {
            *guard = new_scope;
        }
        self.invalidate();
    }

    /// Synchronous invalidation — immediately revokes old permits even across Off->On edits.
    pub fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::SeqCst);
    }

    pub async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.invalidate();
        let mut inner = self.inner.lock().await;
        self.reconcile_locked(&mut inner);
    }

    pub async fn pending_requests(&self) -> Vec<ToolApprovalRequest> {
        let inner = self.inner.lock().await;
        inner.pending.values().map(|p| p.request.clone()).collect()
    }

    pub fn try_pending_requests(&self) -> Option<Vec<ToolApprovalRequest>> {
        let inner = self.inner.try_lock().ok()?;
        Some(inner.pending.values().map(|p| p.request.clone()).collect())
    }

    pub async fn pending_questions(&self) -> Vec<UserQuestion> {
        let inner = self.inner.lock().await;
        inner
            .pending_questions
            .values()
            .map(|p| p.question.clone())
            .collect()
    }

    fn reconcile_locked(&self, inner: &mut BrokerInner) -> Option<ToolAccessScope> {
        let current = self.current_scope();
        let rev = self.revision.load(Ordering::SeqCst);
        if current != inner.observed_scope || rev != inner.observed_revision {
            if current != inner.observed_scope {
                self.invalidate();
            }
            inner.grants.clear();
            inner.denials.clear();
            for (_, mut p) in inner.pending.drain() {
                if let Some(tx) = p.tx.take() {
                    let _ = tx.send(ToolApprovalDecision::Deny);
                }
            }
            for (_, mut q) in inner.pending_questions.drain() {
                if let Some(tx) = q.tx.take() {
                    let _ = tx.send(None);
                }
            }
            inner.observed_scope = current.clone();
            inner.observed_revision = self.revision.load(Ordering::SeqCst);
        }
        current
    }

    pub async fn authorize(
        self: &Arc<Self>,
        external_tools: &[HarnessToolDefinition],
        call_id: &str,
        name: &str,
        arguments_json: &str,
    ) -> ToolAuthorization {
        let key = ActionKey {
            name: name.to_string(),
            arguments: arguments_json.to_string(),
        };

        let (request_id, rx, req_scope, req_rev) = {
            let mut inner = self.inner.lock().await;
            let Some(scope) = self.reconcile_locked(&mut inner) else {
                return ToolAuthorization::Denied("This run is no longer active.".into());
            };
            if let Some(reason) =
                AgentPermissionGate::denial_or_unknown(&scope, external_tools, name, arguments_json)
            {
                return ToolAuthorization::Denied(reason);
            }
            if inner.denials.contains(&key) {
                return ToolAuthorization::Denied(
                    "This action was denied for this run. Change its permissions to reconsider it."
                        .into(),
                );
            }
            let rev = self.revision.load(Ordering::SeqCst);
            let effect = AgentPermissionGate::effect(&scope, name, arguments_json);
            if !effect.requires_approval || inner.grants.contains(&key) {
                return ToolAuthorization::Allowed(ToolExecutionPermit {
                    broker: self.clone(),
                    scope,
                    tool_name: name.to_string(),
                    arguments_json: arguments_json.to_string(),
                    policy_revision: rev,
                    consumed: AtomicBool::new(false),
                });
            }

            inner.sequence += 1;
            let req_id = format!("{}:{}", self.run_id, inner.sequence);
            let (tx, rx) = oneshot::channel();
            let req = ToolApprovalRequest {
                request_id: req_id.clone(),
                run_id: self.run_id.clone(),
                chat_id: self.chat_id.clone(),
                call_id: call_id.to_string(),
                tool_name: name.to_string(),
                arguments_json: arguments_json.to_string(),
                scope: scope.clone(),
                policy_revision: rev,
            };
            inner.pending.insert(
                req_id.clone(),
                PendingApproval {
                    request: req,
                    tx: Some(tx),
                },
            );
            (req_id, rx, scope, rev)
        };

        let decision = rx.await.unwrap_or(ToolApprovalDecision::Deny);
        let mut inner = self.inner.lock().await;
        inner.pending.remove(&request_id);

        if decision == ToolApprovalDecision::Stop {
            return ToolAuthorization::Stopped;
        }
        let Some(scope) = self.reconcile_locked(&mut inner) else {
            return ToolAuthorization::Denied("This run is no longer active.".into());
        };
        let now_rev = self.revision.load(Ordering::SeqCst);
        if scope != req_scope || now_rev != req_rev {
            return ToolAuthorization::Denied(
                "Permissions changed while approval was pending.".into(),
            );
        }
        if let Some(reason) =
            AgentPermissionGate::denial_or_unknown(&scope, external_tools, name, arguments_json)
        {
            return ToolAuthorization::Denied(reason);
        }

        match decision {
            ToolApprovalDecision::AllowOnce | ToolApprovalDecision::AllowExactActionForRun => {
                // Stable contract: computer_use NEVER accepts run-wide grants.
                if decision == ToolApprovalDecision::AllowExactActionForRun
                    && name != COMPUTER_USE_TOOL_NAME
                {
                    inner.grants.insert(key);
                }
                ToolAuthorization::Allowed(ToolExecutionPermit {
                    broker: self.clone(),
                    scope,
                    tool_name: name.to_string(),
                    arguments_json: arguments_json.to_string(),
                    policy_revision: now_rev,
                    consumed: AtomicBool::new(false),
                })
            }
            ToolApprovalDecision::Deny => {
                inner.denials.insert(key);
                ToolAuthorization::Denied("Action denied by the user.".into())
            }
            ToolApprovalDecision::Stop => ToolAuthorization::Stopped,
        }
    }

    pub async fn respond(&self, request_id: &str, decision: ToolApprovalDecision) -> bool {
        let mut inner = self.inner.lock().await;
        self.reconcile_locked(&mut inner);
        if decision == ToolApprovalDecision::Stop {
            if !inner.pending.contains_key(request_id) {
                return false;
            }
            self.closed.store(true, Ordering::SeqCst);
            inner.grants.clear();
            inner.denials.clear();
            for (_, mut p) in inner.pending.drain() {
                if let Some(tx) = p.tx.take() {
                    let _ = tx.send(ToolApprovalDecision::Stop);
                }
            }
            for (_, mut q) in inner.pending_questions.drain() {
                if let Some(tx) = q.tx.take() {
                    let _ = tx.send(None);
                }
            }
            return true;
        }
        if let Some(mut entry) = inner.pending.remove(request_id)
            && let Some(tx) = entry.tx.take()
        {
            return tx.send(decision).is_ok();
        }
        false
    }

    pub async fn ask_user(
        &self,
        call_id: &str,
        question: &str,
        options: Vec<String>,
    ) -> Option<String> {
        let (req_id, rx) = {
            let mut inner = self.inner.lock().await;
            self.reconcile_locked(&mut inner)?;
            inner.sequence += 1;
            let req_id = format!("{}:q:{}", self.run_id, inner.sequence);
            let (tx, rx) = oneshot::channel();
            inner.pending_questions.insert(
                req_id.clone(),
                PendingQuestion {
                    question: UserQuestion {
                        request_id: req_id.clone(),
                        chat_id: self.chat_id.clone(),
                        call_id: call_id.to_string(),
                        question: question.to_string(),
                        options,
                    },
                    tx: Some(tx),
                },
            );
            (req_id, rx)
        };

        let ans = rx.await.ok().flatten();
        let mut inner = self.inner.lock().await;
        inner.pending_questions.remove(&req_id);
        ans
    }

    pub async fn answer_question(&self, request_id: &str, answer: String) -> bool {
        let mut inner = self.inner.lock().await;
        self.reconcile_locked(&mut inner);
        if let Some(mut entry) = inner.pending_questions.remove(request_id)
            && let Some(tx) = entry.tx.take()
        {
            return tx.send(Some(answer)).is_ok();
        }
        false
    }

    pub(crate) fn is_permit_revoked(&self, permit: &ToolExecutionPermit) -> bool {
        self.closed.load(Ordering::SeqCst)
            || self.revision.load(Ordering::SeqCst) != permit.policy_revision
            || self.current_scope().as_ref() != Some(&permit.scope)
    }

    pub(crate) fn permit_denial(
        &self,
        permit: &ToolExecutionPermit,
        external_tools: &[HarnessToolDefinition],
    ) -> Option<String> {
        let scope = self.current_scope()?;
        if scope != permit.scope || self.revision.load(Ordering::SeqCst) != permit.policy_revision {
            return Some("Tool permission was revoked before execution.".into());
        }
        AgentPermissionGate::denial_or_unknown(
            &scope,
            external_tools,
            &permit.tool_name,
            &permit.arguments_json,
        )
    }
}
