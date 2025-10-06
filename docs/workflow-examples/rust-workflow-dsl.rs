//! Rust-based Workflow DSL for Atomic VCS
//!
//! This demonstrates a type-safe, compile-time checked workflow definition
//! system that avoids the complexity and debugging issues of YAML/JSON configs.

use atomic_config::Author;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Workflow definition macro that generates type-safe workflow structs
macro_rules! workflow {
    (
        name: $name:literal,
        initial_state: $initial:ident,
        states: {
            $(
                $state:ident {
                    name: $state_name:literal,
                    description: $state_desc:literal,
                    $(permissions: [$($perm:literal),*],)?
                    $(timeout: $timeout:expr,)?
                    $(parallel: $parallel:literal,)?
                }
            )*
        },
        transitions: {
            $(
                $transition:ident: $from:ident -> $to:ident {
                    trigger: $trigger:ident,
                    $(guard: $guard:expr,)?
                    $(action: $action:expr,)?
                    required_roles: [$($role:literal),*],
                    $(conditions: [$($condition:expr),*],)?
                }
            )*
        },
        $(
        rules: {
            $(
                $rule_name:ident: $rule_condition:expr => $rule_action:expr,
            )*
        }
        )?
    ) => {
        paste::paste! {
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
            pub enum [<$name State>] {
                $(
                    #[doc = $state_desc]
                    $state,
                )*
            }

            #[derive(Debug, Clone, PartialEq, Eq, Hash)]
            pub enum [<$name Transition>] {
                $(
                    #[doc = concat!("Transition from ", stringify!($from), " to ", stringify!($to))]
                    $transition,
                )*
            }

            #[derive(Debug, Clone)]
            pub enum [<$name Trigger>] {
                $(
                    $transition,
                )*
            }

            pub struct [<$name Workflow>];

            impl [<$name Workflow>] {
                pub const NAME: &'static str = $name;
                pub const INITIAL_STATE: [<$name State>] = [<$name State>]::$initial;

                pub fn get_state_metadata(state: &[<$name State>]) -> StateMetadata {
                    match state {
                        $(
                            [<$name State>]::$state => StateMetadata {
                                name: $state_name,
                                description: $state_desc,
                                permissions: vec![$($($perm.to_string()),*)?],
                                timeout: workflow!(@timeout $($timeout)?),
                                parallel: workflow!(@parallel $($parallel)?),
                            },
                        )*
                    }
                }

                pub fn can_transition(
                    from: &[<$name State>],
                    trigger: &[<$name Trigger>],
                    context: &WorkflowContext
                ) -> Result<[<$name State>], WorkflowError> {
                    match (from, trigger) {
                        $(
                            ([<$name State>]::$from, [<$name Trigger>]::$transition) => {
                                // Check required roles
                                let required_roles: HashSet<String> = [$($role.to_string()),*].into_iter().collect();
                                if !context.user_has_roles(&required_roles) {
                                    return Err(WorkflowError::InsufficientPermissions);
                                }

                                // Check guard conditions
                                $(
                                    if !($guard)(context) {
                                        return Err(WorkflowError::GuardFailed(stringify!($guard).to_string()));
                                    }
                                )?

                                // Check additional conditions
                                $(
                                    $(
                                        if !($condition)(context) {
                                            return Err(WorkflowError::ConditionFailed(stringify!($condition).to_string()));
                                        }
                                    )*
                                )?

                                // Execute action
                                $(
                                    ($action)(context)?;
                                )?

                                Ok([<$name State>]::$to)
                            },
                        )*
                        _ => Err(WorkflowError::InvalidTransition),
                    }
                }

                pub fn get_available_transitions(
                    state: &[<$name State>],
                    context: &WorkflowContext
                ) -> Vec<([<$name Trigger>], [<$name State>])> {
                    let mut transitions = Vec::new();
                    $(
                        if matches!(state, [<$name State>]::$from) {
                            match Self::can_transition(state, &[<$name Trigger>]::$transition, context) {
                                Ok(next_state) => transitions.push(([<$name Trigger>]::$transition, next_state)),
                                Err(_) => {} // Skip unavailable transitions
                            }
                        }
                    )*
                    transitions
                }

                $(
                    pub fn apply_rules(context: &mut WorkflowContext) -> Result<Vec<WorkflowEvent>, WorkflowError> {
                        let mut events = Vec::new();
                        $(
                            if ($rule_condition)(context) {
                                events.push(($rule_action)(context)?);
                            }
                        )*
                        Ok(events)
                    }
                )?
            }
        }
    };

    (@timeout) => { None };
    (@timeout $timeout:expr) => { Some($timeout) };
    (@parallel) => { false };
    (@parallel $parallel:literal) => { $parallel };
}

/// Supporting types and traits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub permissions: Vec<String>,
    pub timeout: Option<std::time::Duration>,
    pub parallel: bool,
}

#[derive(Debug, Clone)]
pub struct WorkflowContext {
    pub change_id: String,
    pub author: Author,
    pub user_roles: HashSet<String>,
    pub metadata: HashMap<String, String>,
    pub dependencies: Vec<String>,
}

impl WorkflowContext {
    pub fn user_has_roles(&self, required_roles: &HashSet<String>) -> bool {
        !required_roles.is_disjoint(&self.user_roles)
    }

    pub fn has_dependency(&self, dep: &str) -> bool {
        self.dependencies.contains(&dep.to_string())
    }

    pub fn metadata_contains(&self, key: &str, value: &str) -> bool {
        self.metadata.get(key).map_or(false, |v| v == value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("Insufficient permissions")]
    InsufficientPermissions,
    #[error("Invalid transition")]
    InvalidTransition,
    #[error("Guard condition failed: {0}")]
    GuardFailed(String),
    #[error("Condition failed: {0}")]
    ConditionFailed(String),
    #[error("Action failed: {0}")]
    ActionFailed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowEvent {
    StateChanged {
        from: String,
        to: String,
        reason: String,
    },
    NotificationSent {
        recipient: String,
        message: String,
    },
    DependencyResolved {
        dependency: String,
    },
    AutoApproved {
        reason: String,
    },
}

/// Example workflow definition using the DSL
workflow! {
    name: "EnterpriseApproval",
    initial_state: Recorded,
    states: {
        Recorded {
            name: "Locally Recorded",
            description: "Change has been recorded locally but not submitted for review",
            permissions: ["read", "submit"],
        }
        SecurityReview {
            name: "Security Team Review",
            description: "Change is under security team review",
            permissions: ["read"],
            timeout: std::time::Duration::from_secs(3600 * 24 * 3), // 3 days
        }
        CodeReview {
            name: "Code Review",
            description: "Change is under code review",
            permissions: ["read", "approve", "reject"],
            parallel: true,
        }
        QaReview {
            name: "QA Review",
            description: "Change is under QA review",
            permissions: ["read", "approve", "reject"],
            parallel: true,
        }
        Approved {
            name: "Approved",
            description: "Change has been approved and can be applied",
            permissions: ["read", "apply"],
        }
        Applied {
            name: "Applied",
            description: "Change has been successfully applied",
            permissions: ["read"],
        }
        Rejected {
            name: "Rejected",
            description: "Change has been rejected",
            permissions: ["read", "resubmit"],
        }
    },
    transitions: {
        SubmitForSecurity: Recorded -> SecurityReview {
            trigger: SubmitForSecurity,
            guard: |ctx| !ctx.metadata_contains("security_bypass", "true"),
            action: |ctx| {
                // Send notification to security team
                Ok(WorkflowEvent::NotificationSent {
                    recipient: "security-team".to_string(),
                    message: format!("New change {} submitted for security review", ctx.change_id),
                })
            },
            required_roles: ["developer"],
            conditions: [
                |ctx| !ctx.dependencies.is_empty(), // Must have dependencies resolved
            ],
        }
        SecurityApprove: SecurityReview -> CodeReview {
            trigger: SecurityApprove,
            action: |ctx| {
                Ok(WorkflowEvent::StateChanged {
                    from: "SecurityReview".to_string(),
                    to: "CodeReview".to_string(),
                    reason: "Security approved".to_string(),
                })
            },
            required_roles: ["security-reviewer"],
        }
        CodeApprove: CodeReview -> QaReview {
            trigger: CodeApprove,
            guard: |ctx| ctx.user_has_roles(&["senior-developer", "tech-lead"].into_iter().map(String::from).collect()),
            required_roles: ["code-reviewer"],
        }
        QaApprove: QaReview -> Approved {
            trigger: QaApprove,
            required_roles: ["qa-reviewer"],
        }
        Apply: Approved -> Applied {
            trigger: Apply,
            action: |ctx| {
                // Execute the actual change application
                Ok(WorkflowEvent::StateChanged {
                    from: "Approved".to_string(),
                    to: "Applied".to_string(),
                    reason: "Change successfully applied".to_string(),
                })
            },
            required_roles: ["deployer", "admin"],
        }
        Reject: SecurityReview -> Rejected {
            trigger: Reject,
            required_roles: ["security-reviewer", "code-reviewer", "qa-reviewer"],
        }
        Reject2: CodeReview -> Rejected {
            trigger: Reject,
            required_roles: ["code-reviewer"],
        }
        Reject3: QaReview -> Rejected {
            trigger: Reject,
            required_roles: ["qa-reviewer"],
        }
    },
    rules: {
        auto_approve_trusted: |ctx| {
            ctx.user_has_roles(&["trusted-committer"].into_iter().map(String::from).collect())
                && ctx.metadata_contains("change_type", "documentation")
        } => |ctx| {
            Ok(WorkflowEvent::AutoApproved {
                reason: "Trusted committer documentation change".to_string(),
            })
        },
        timeout_reminder: |ctx| {
            ctx.metadata_contains("days_in_review", "2")
        } => |ctx| {
            Ok(WorkflowEvent::NotificationSent {
                recipient: "reviewers".to_string(),
                message: "Change has been in review for 2 days".to_string(),
            })
        },
    }
}

/// Usage example
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_transitions() {
        let mut context = WorkflowContext {
            change_id: "change-123".to_string(),
            author: Author::default(),
            user_roles: ["developer"].into_iter().map(String::from).collect(),
            metadata: HashMap::new(),
            dependencies: vec!["dep-1".to_string()],
        };

        let initial_state = EnterpriseApprovalWorkflow::INITIAL_STATE;
        assert_eq!(initial_state, EnterpriseApprovalState::Recorded);

        // Test valid transition
        let result = EnterpriseApprovalWorkflow::can_transition(
            &EnterpriseApprovalState::Recorded,
            &EnterpriseApprovalTrigger::SubmitForSecurity,
            &context,
        );
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), EnterpriseApprovalState::SecurityReview);

        // Test invalid transition (insufficient roles)
        context.user_roles.clear();
        let result = EnterpriseApprovalWorkflow::can_transition(
            &EnterpriseApprovalState::Recorded,
            &EnterpriseApprovalTrigger::SubmitForSecurity,
            &context,
        );
        assert!(matches!(
            result,
            Err(WorkflowError::InsufficientPermissions)
        ));
    }

    #[test]
    fn test_available_transitions() {
        let context = WorkflowContext {
            change_id: "change-123".to_string(),
            author: Author::default(),
            user_roles: ["developer"].into_iter().map(String::from).collect(),
            metadata: HashMap::new(),
            dependencies: vec!["dep-1".to_string()],
        };

        let transitions = EnterpriseApprovalWorkflow::get_available_transitions(
            &EnterpriseApprovalState::Recorded,
            &context,
        );

        assert!(!transitions.is_empty());
        assert!(transitions.iter().any(|(trigger, state)| matches!(
            trigger,
            EnterpriseApprovalTrigger::SubmitForSecurity
        ) && matches!(
            state,
            EnterpriseApprovalState::SecurityReview
        )));
    }
}
