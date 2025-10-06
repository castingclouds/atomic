//! # Atomic Workflows - Simple Workflow System for MVP
//!
//! A minimal, type-safe workflow definition system for Atomic VCS change approval.
//! This MVP version focuses on 1-2 simple approval workflows for testing with design partners.
//!
//! ## Example Usage
//!
//! ```rust
//! use atomic_workflows::simple_workflow;
//!
//! simple_workflow! {
//!     name: "SimpleApproval",
//!     initial_state: Recorded,
//!
//!     states: {
//!         Recorded {
//!             name: "Recorded Locally",
//!         }
//!         Review {
//!             name: "Under Review",
//!         }
//!         Approved {
//!             name: "Approved",
//!         }
//!     },
//!
//!     transitions: {
//!         Recorded -> Review {
//!             needs_role: "developer",
//!             trigger: "submit",
//!         }
//!         Review -> Approved {
//!             needs_role: "reviewer",
//!             trigger: "approve",
//!         }
//!     }
//! }
//! ```

pub mod simple;

// Re-export the main types and macros
pub use simple::{WorkflowContext, WorkflowError, WorkflowEvent};

// Re-export the macro (automatically available due to #[macro_export])

// Re-export paste for the macro
pub use paste;

#[cfg(test)]
mod tests {
    use super::*;
    use atomic_config::Author;

    simple_workflow! {
        name: "TestWorkflow",
        initial_state: Start,

        states: {
            Start {
                name: "Starting State",
            }
            End {
                name: "End State",
            }
        },

        transitions: {
            Start -> End {
                needs_role: "user",
                trigger: "finish",
            }
        }
    }

    #[test]
    fn test_basic_workflow() {
        let mut context = WorkflowContext::new(
            "test-change".to_string(),
            Author::default(),
            "Start".to_string(),
        );
        context.add_role("user".to_string());

        let event = TestWorkflowWorkflow::execute_transition(
            TestWorkflowState::Start,
            TestWorkflowState::End,
            &mut context,
        )
        .unwrap();

        assert!(matches!(event, WorkflowEvent::StateChanged { .. }));
    }
}
