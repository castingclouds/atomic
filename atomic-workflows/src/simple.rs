//! Simple MVP Workflow System
//!
//! Minimal workflow definitions for testing with design partners.
//! Just the essentials - no complex features yet.

#![allow(unreachable_patterns)] // Macro-generated code may have unreachable patterns

use atomic_config::Author;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Simple workflow context for MVP
#[derive(Debug, Clone)]
pub struct WorkflowContext {
    pub change_id: String,
    pub author: Author,
    pub user_roles: HashSet<String>,
    pub current_state: String,
}

impl WorkflowContext {
    pub fn new(change_id: String, author: Author, current_state: String) -> Self {
        Self {
            change_id,
            author,
            user_roles: HashSet::new(),
            current_state,
        }
    }

    pub fn user_has_role(&self, role: &str) -> bool {
        self.user_roles.contains(role)
    }

    pub fn add_role(&mut self, role: String) {
        self.user_roles.insert(role);
    }
}

/// Simple workflow events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowEvent {
    StateChanged { from: String, to: String },
    ApprovalRequired { reviewer_role: String },
    ChangeApproved { approver: String },
    ChangeRejected { reason: String },
}

/// Simple workflow errors
#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("Need role '{0}' to perform this action")]
    NeedRole(String),
    #[error("Cannot transition from '{from}' to '{to}'")]
    InvalidTransition { from: String, to: String },
}

/// Simple workflow macro - just the essentials
#[macro_export]
macro_rules! simple_workflow {
    (
        name: $name:literal,
        initial_state: $initial:ident,

        states: {
            $(
                $state:ident {
                    name: $state_name:literal,
                    $(can_approve: $can_approve:literal,)?
                }
            )*
        },

        transitions: {
            $(
                $from_state:ident -> $to_state:ident {
                    $(needs_role: $role:literal,)?
                    trigger: $trigger:literal,
                }
            )*
        }
    ) => {
        paste::paste! {
            #[derive(Debug, Clone, PartialEq)]
            pub enum [<$name State>] {
                $( $state, )*
            }

            pub struct [<$name Workflow>];

            impl [<$name Workflow>] {
                #[allow(dead_code)]
                pub const NAME: &'static str = $name;
                #[allow(dead_code)]
                pub const INITIAL_STATE: [<$name State>] = [<$name State>]::$initial;

                #[allow(dead_code)]
                pub fn get_state_name(state: &[<$name State>]) -> &'static str {
                    match state {
                        $( [<$name State>]::$state => $state_name, )*
                    }
                }

                pub fn can_transition(
                    from: &[<$name State>],
                    to: &[<$name State>],
                    context: &$crate::simple::WorkflowContext,
                ) -> Result<(), $crate::simple::WorkflowError> {
                    match (from, to) {
                        $(
                            ([<$name State>]::$from_state, [<$name State>]::$to_state) => {
                                $(
                                    if !context.user_has_role($role) {
                                        return Err($crate::simple::WorkflowError::NeedRole($role.to_string()));
                                    }
                                )?
                                Ok(())
                            },
                        )*
                        _ => Err($crate::simple::WorkflowError::InvalidTransition {
                            from: format!("{:?}", from),
                            to: format!("{:?}", to),
                        }),
                    }
                }

                pub fn execute_transition(
                    from: [<$name State>],
                    to: [<$name State>],
                    context: &mut $crate::simple::WorkflowContext,
                ) -> Result<$crate::simple::WorkflowEvent, $crate::simple::WorkflowError> {
                    Self::can_transition(&from, &to, context)?;

                    context.current_state = format!("{:?}", to);

                    Ok($crate::simple::WorkflowEvent::StateChanged {
                        from: format!("{:?}", from),
                        to: format!("{:?}", to),
                    })
                }

                #[allow(dead_code)]
                pub fn get_available_transitions(
                    state: &[<$name State>]
                ) -> Vec<(&'static str, [<$name State>])> {
                    match state {
                        $(
                            [<$name State>]::$from_state => {
                                vec![($trigger, [<$name State>]::$to_state)]
                            }
                        )*
                        _ => vec![],
                    }
                }
            }
        }
    };
}

// Example MVP workflows for testing

simple_workflow! {
    name: "SimpleApproval",
    initial_state: Recorded,

    states: {
        Recorded {
            name: "Recorded Locally",
        }
        Review {
            name: "Under Review",
        }
        Approved {
            name: "Approved",
        }
        Rejected {
            name: "Rejected",
        }
    },

    transitions: {
        Recorded -> Review {
            needs_role: "developer",
            trigger: "submit",
        }
        Review -> Approved {
            needs_role: "reviewer",
            trigger: "approve",
        }
        Review -> Rejected {
            needs_role: "reviewer",
            trigger: "reject",
        }
    }
}

simple_workflow! {
    name: "TwoStageApproval",
    initial_state: Recorded,

    states: {
        Recorded {
            name: "Recorded Locally",
        }
        SecurityReview {
            name: "Security Review",
        }
        CodeReview {
            name: "Code Review",
        }
        Approved {
            name: "Approved",
        }
        Rejected {
            name: "Rejected",
        }
    },

    transitions: {
        Recorded -> SecurityReview {
            needs_role: "developer",
            trigger: "submit_security",
        }
        SecurityReview -> CodeReview {
            needs_role: "security_reviewer",
            trigger: "security_approve",
        }
        SecurityReview -> Rejected {
            needs_role: "security_reviewer",
            trigger: "security_reject",
        }
        CodeReview -> Approved {
            needs_role: "code_reviewer",
            trigger: "code_approve",
        }
        CodeReview -> Rejected {
            needs_role: "code_reviewer",
            trigger: "code_reject",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_approval_workflow() {
        let mut context = WorkflowContext::new(
            "change-123".to_string(),
            Author::default(),
            "Recorded".to_string(),
        );

        // Developer submits for review
        context.add_role("developer".to_string());
        let event = SimpleApprovalWorkflow::execute_transition(
            SimpleApprovalState::Recorded,
            SimpleApprovalState::Review,
            &mut context,
        )
        .unwrap();

        assert!(matches!(event, WorkflowEvent::StateChanged { .. }));
        assert_eq!(context.current_state, "Review");

        // Reviewer approves
        context.add_role("reviewer".to_string());
        let event = SimpleApprovalWorkflow::execute_transition(
            SimpleApprovalState::Review,
            SimpleApprovalState::Approved,
            &mut context,
        )
        .unwrap();

        assert!(matches!(event, WorkflowEvent::StateChanged { .. }));
        assert_eq!(context.current_state, "Approved");
    }

    #[test]
    fn test_insufficient_permissions() {
        let mut context = WorkflowContext::new(
            "change-123".to_string(),
            Author::default(),
            "Recorded".to_string(),
        );

        // Try to transition without required role
        let result = SimpleApprovalWorkflow::execute_transition(
            SimpleApprovalState::Recorded,
            SimpleApprovalState::Review,
            &mut context,
        );

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WorkflowError::NeedRole(_)));
    }

    #[test]
    fn test_two_stage_workflow() {
        let mut context = WorkflowContext::new(
            "change-456".to_string(),
            Author::default(),
            "Recorded".to_string(),
        );

        // Step 1: Developer submits to security
        context.add_role("developer".to_string());
        let _event = TwoStageApprovalWorkflow::execute_transition(
            TwoStageApprovalState::Recorded,
            TwoStageApprovalState::SecurityReview,
            &mut context,
        )
        .unwrap();

        // Step 2: Security reviewer approves
        context.add_role("security_reviewer".to_string());
        let _event = TwoStageApprovalWorkflow::execute_transition(
            TwoStageApprovalState::SecurityReview,
            TwoStageApprovalState::CodeReview,
            &mut context,
        )
        .unwrap();

        // Step 3: Code reviewer approves
        context.add_role("code_reviewer".to_string());
        let _event = TwoStageApprovalWorkflow::execute_transition(
            TwoStageApprovalState::CodeReview,
            TwoStageApprovalState::Approved,
            &mut context,
        )
        .unwrap();

        assert_eq!(context.current_state, "Approved");
    }
}
