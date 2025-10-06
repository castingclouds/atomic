//! Simple Usage Example for Atomic Workflows MVP
//!
//! This example shows how to define and use simple workflows
//! for change approval in Atomic VCS.

use atomic_config::Author;
use atomic_workflows::{simple_workflow, WorkflowContext, WorkflowError};

// Define a simple single-approval workflow
simple_workflow! {
    name: "BasicApproval",
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

// Define a two-stage approval workflow
simple_workflow! {
    name: "SecurityCodeReview",
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

fn main() -> Result<(), WorkflowError> {
    println!("=== Atomic Workflows MVP Demo ===\n");

    // Example 1: Simple single approval workflow
    println!("1. Basic Approval Workflow");
    println!("-------------------------");

    let mut context = WorkflowContext::new(
        "change-abc123".to_string(),
        Author {
            username: "alice".to_string(),
            display_name: "Alice Developer".to_string(),
            email: "alice@example.com".to_string(),
            origin: "local".to_string(),
            key_path: None,
        },
        "Recorded".to_string(),
    );

    println!("Initial state: {}", context.current_state);
    println!(
        "Available transitions: {:?}",
        BasicApprovalWorkflow::get_available_transitions(&BasicApprovalState::Recorded)
    );

    // Developer submits for review
    context.add_role("developer".to_string());
    println!("\nDeveloper submitting for review...");

    let event = BasicApprovalWorkflow::execute_transition(
        BasicApprovalState::Recorded,
        BasicApprovalState::Review,
        &mut context,
    )?;

    println!("✓ Transition successful: {:?}", event);
    println!("New state: {}", context.current_state);

    // Reviewer approves
    context.add_role("reviewer".to_string());
    println!("\nReviewer approving...");

    let event = BasicApprovalWorkflow::execute_transition(
        BasicApprovalState::Review,
        BasicApprovalState::Approved,
        &mut context,
    )?;

    println!("✓ Transition successful: {:?}", event);
    println!("Final state: {}", context.current_state);

    println!("\n");

    // Example 2: Two-stage approval workflow
    println!("2. Security + Code Review Workflow");
    println!("----------------------------------");

    let mut context2 = WorkflowContext::new(
        "change-xyz789".to_string(),
        Author {
            username: "bob".to_string(),
            display_name: "Bob Developer".to_string(),
            email: "bob@example.com".to_string(),
            origin: "local".to_string(),
            key_path: None,
        },
        "Recorded".to_string(),
    );

    println!("Initial state: {}", context2.current_state);

    // Developer submits to security
    context2.add_role("developer".to_string());
    println!("\nDeveloper submitting to security review...");

    let _event = SecurityCodeReviewWorkflow::execute_transition(
        SecurityCodeReviewState::Recorded,
        SecurityCodeReviewState::SecurityReview,
        &mut context2,
    )?;

    println!("✓ Now in security review: {}", context2.current_state);

    // Security reviewer approves
    context2.add_role("security_reviewer".to_string());
    println!("\nSecurity reviewer approving...");

    let _event = SecurityCodeReviewWorkflow::execute_transition(
        SecurityCodeReviewState::SecurityReview,
        SecurityCodeReviewState::CodeReview,
        &mut context2,
    )?;

    println!("✓ Now in code review: {}", context2.current_state);

    // Code reviewer approves
    context2.add_role("code_reviewer".to_string());
    println!("\nCode reviewer approving...");

    let _event = SecurityCodeReviewWorkflow::execute_transition(
        SecurityCodeReviewState::CodeReview,
        SecurityCodeReviewState::Approved,
        &mut context2,
    )?;

    println!("✓ Final state: {}", context2.current_state);

    println!("\n");

    // Example 3: Error handling
    println!("3. Error Handling Example");
    println!("-------------------------");

    let mut context3 = WorkflowContext::new(
        "change-err123".to_string(),
        Author {
            username: "charlie".to_string(),
            display_name: "Charlie User".to_string(),
            email: "charlie@example.com".to_string(),
            origin: "local".to_string(),
            key_path: None,
        },
        "Recorded".to_string(),
    );

    // Try to submit without developer role
    println!("Attempting transition without required role...");
    let result = BasicApprovalWorkflow::execute_transition(
        BasicApprovalState::Recorded,
        BasicApprovalState::Review,
        &mut context3,
    );

    match result {
        Err(WorkflowError::NeedRole(role)) => {
            println!("✓ Correctly blocked: Need role '{}'", role);
        }
        _ => {
            println!("✗ Expected permission error");
        }
    }

    println!("\n=== Demo Complete ===");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_approval_happy_path() {
        let mut context = WorkflowContext::new(
            "test-change".to_string(),
            Author::default(),
            "Recorded".to_string(),
        );

        // Add necessary roles
        context.add_role("developer".to_string());
        context.add_role("reviewer".to_string());

        // Submit for review
        let _event = BasicApprovalWorkflow::execute_transition(
            BasicApprovalState::Recorded,
            BasicApprovalState::Review,
            &mut context,
        )
        .unwrap();

        assert_eq!(context.current_state, "Review");

        // Approve
        let _event = BasicApprovalWorkflow::execute_transition(
            BasicApprovalState::Review,
            BasicApprovalState::Approved,
            &mut context,
        )
        .unwrap();

        assert_eq!(context.current_state, "Approved");
    }

    #[test]
    fn test_two_stage_workflow() {
        let mut context = WorkflowContext::new(
            "test-change".to_string(),
            Author::default(),
            "Recorded".to_string(),
        );

        // Add all necessary roles
        context.add_role("developer".to_string());
        context.add_role("security_reviewer".to_string());
        context.add_role("code_reviewer".to_string());

        // Go through all stages
        let _event = SecurityCodeReviewWorkflow::execute_transition(
            SecurityCodeReviewState::Recorded,
            SecurityCodeReviewState::SecurityReview,
            &mut context,
        )
        .unwrap();

        let _event = SecurityCodeReviewWorkflow::execute_transition(
            SecurityCodeReviewState::SecurityReview,
            SecurityCodeReviewState::CodeReview,
            &mut context,
        )
        .unwrap();

        let _event = SecurityCodeReviewWorkflow::execute_transition(
            SecurityCodeReviewState::CodeReview,
            SecurityCodeReviewState::Approved,
            &mut context,
        )
        .unwrap();

        assert_eq!(context.current_state, "Approved");
    }

    #[test]
    fn test_permission_checking() {
        let mut context = WorkflowContext::new(
            "test-change".to_string(),
            Author::default(),
            "Recorded".to_string(),
        );

        // Try without proper role
        let result = BasicApprovalWorkflow::execute_transition(
            BasicApprovalState::Recorded,
            BasicApprovalState::Review,
            &mut context,
        );

        assert!(matches!(result, Err(WorkflowError::NeedRole(_))));
    }
}
