//! Builder Pattern Alternative for Workflow Definition
//!
//! This provides a fluent, programmatic API for defining workflows that's
//! easier to debug and refactor than YAML configurations, while still
//! providing compile-time type safety.

use atomic_config::Author;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Builder for creating type-safe workflow definitions
pub struct WorkflowBuilder<T> {
    name: String,
    initial_state: Option<String>,
    states: HashMap<String, StateDefinition>,
    transitions: Vec<TransitionDefinition>,
    rules: Vec<RuleDefinition>,
    _phantom: std::marker::PhantomData<T>,
}

#[derive(Debug, Clone)]
pub struct StateDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub timeout: Option<Duration>,
    pub parallel: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct TransitionDefinition {
    pub id: String,
    pub from_states: Vec<String>,
    pub to_state: String,
    pub trigger: String,
    pub required_roles: Vec<String>,
    pub guard_fn: Option<fn(&WorkflowContext) -> bool>,
    pub action_fn: Option<fn(&WorkflowContext) -> Result<WorkflowEvent, WorkflowError>>,
    pub conditions: Vec<fn(&WorkflowContext) -> bool>,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct RuleDefinition {
    pub name: String,
    pub condition_fn: fn(&WorkflowContext) -> bool,
    pub action_fn: fn(&WorkflowContext) -> Result<WorkflowEvent, WorkflowError>,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct WorkflowDefinition {
    pub name: String,
    pub initial_state: String,
    pub states: HashMap<String, StateDefinition>,
    pub transitions: Vec<TransitionDefinition>,
    pub rules: Vec<RuleDefinition>,
}

// Supporting types
#[derive(Debug, Clone)]
pub struct WorkflowContext {
    pub change_id: String,
    pub author: Author,
    pub user_roles: HashSet<String>,
    pub metadata: HashMap<String, String>,
    pub dependencies: Vec<String>,
    pub current_state: String,
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
    ActionExecuted {
        action: String,
        result: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error("Insufficient permissions: required {required:?}, got {actual:?}")]
    InsufficientPermissions {
        required: Vec<String>,
        actual: Vec<String>,
    },
    #[error("Invalid transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },
    #[error("Guard condition failed: {condition}")]
    GuardFailed { condition: String },
    #[error("Rule condition failed: {rule}")]
    RuleConditionFailed { rule: String },
    #[error("Action execution failed: {action} - {error}")]
    ActionFailed { action: String, error: String },
}

impl<T> WorkflowBuilder<T> {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            initial_state: None,
            states: HashMap::new(),
            transitions: Vec::new(),
            rules: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn initial_state(mut self, state: impl Into<String>) -> Self {
        self.initial_state = Some(state.into());
        self
    }

    pub fn state(mut self, builder: impl FnOnce(StateBuilder) -> StateBuilder) -> Self {
        let state_builder = StateBuilder::new();
        let state_def = builder(state_builder).build();
        self.states.insert(state_def.id.clone(), state_def);
        self
    }

    pub fn transition(
        mut self,
        builder: impl FnOnce(TransitionBuilder) -> TransitionBuilder,
    ) -> Self {
        let transition_builder = TransitionBuilder::new();
        let transition_def = builder(transition_builder).build();
        self.transitions.push(transition_def);
        self
    }

    pub fn rule(mut self, builder: impl FnOnce(RuleBuilder) -> RuleBuilder) -> Self {
        let rule_builder = RuleBuilder::new();
        let rule_def = builder(rule_builder).build();
        self.rules.push(rule_def);
        self
    }

    pub fn build(self) -> Result<WorkflowDefinition, String> {
        let initial_state = self
            .initial_state
            .ok_or("Initial state must be specified")?;

        if !self.states.contains_key(&initial_state) {
            return Err(format!(
                "Initial state '{}' not found in states",
                initial_state
            ));
        }

        // Validate all transitions reference existing states
        for transition in &self.transitions {
            for from_state in &transition.from_states {
                if !self.states.contains_key(from_state) {
                    return Err(format!(
                        "Transition '{}' references unknown from_state '{}'",
                        transition.id, from_state
                    ));
                }
            }
            if !self.states.contains_key(&transition.to_state) {
                return Err(format!(
                    "Transition '{}' references unknown to_state '{}'",
                    transition.id, transition.to_state
                ));
            }
        }

        Ok(WorkflowDefinition {
            name: self.name,
            initial_state,
            states: self.states,
            transitions: self.transitions,
            rules: self.rules,
        })
    }
}

pub struct StateBuilder {
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
    permissions: Vec<String>,
    timeout: Option<Duration>,
    parallel: bool,
    metadata: HashMap<String, String>,
}

impl StateBuilder {
    pub fn new() -> Self {
        Self {
            id: None,
            name: None,
            description: None,
            permissions: Vec::new(),
            timeout: None,
            parallel: false,
            metadata: HashMap::new(),
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn permissions(mut self, permissions: Vec<impl Into<String>>) -> Self {
        self.permissions = permissions.into_iter().map(|p| p.into()).collect();
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn parallel(mut self) -> Self {
        self.parallel = true;
        self
    }

    pub fn metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> StateDefinition {
        let id = self.id.expect("State ID must be specified");
        StateDefinition {
            name: self.name.unwrap_or_else(|| id.clone()),
            description: self.description.unwrap_or_default(),
            id,
            permissions: self.permissions,
            timeout: self.timeout,
            parallel: self.parallel,
            metadata: self.metadata,
        }
    }
}

pub struct TransitionBuilder {
    id: Option<String>,
    from_states: Vec<String>,
    to_state: Option<String>,
    trigger: Option<String>,
    required_roles: Vec<String>,
    guard_fn: Option<fn(&WorkflowContext) -> bool>,
    action_fn: Option<fn(&WorkflowContext) -> Result<WorkflowEvent, WorkflowError>>,
    conditions: Vec<fn(&WorkflowContext) -> bool>,
    description: Option<String>,
}

impl TransitionBuilder {
    pub fn new() -> Self {
        Self {
            id: None,
            from_states: Vec::new(),
            to_state: None,
            trigger: None,
            required_roles: Vec::new(),
            guard_fn: None,
            action_fn: None,
            conditions: Vec::new(),
            description: None,
        }
    }

    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn from(mut self, states: Vec<impl Into<String>>) -> Self {
        self.from_states = states.into_iter().map(|s| s.into()).collect();
        self
    }

    pub fn to(mut self, state: impl Into<String>) -> Self {
        self.to_state = Some(state.into());
        self
    }

    pub fn trigger(mut self, trigger: impl Into<String>) -> Self {
        self.trigger = Some(trigger.into());
        self
    }

    pub fn required_roles(mut self, roles: Vec<impl Into<String>>) -> Self {
        self.required_roles = roles.into_iter().map(|r| r.into()).collect();
        self
    }

    pub fn guard(mut self, guard_fn: fn(&WorkflowContext) -> bool) -> Self {
        self.guard_fn = Some(guard_fn);
        self
    }

    pub fn action(
        mut self,
        action_fn: fn(&WorkflowContext) -> Result<WorkflowEvent, WorkflowError>,
    ) -> Self {
        self.action_fn = Some(action_fn);
        self
    }

    pub fn condition(mut self, condition_fn: fn(&WorkflowContext) -> bool) -> Self {
        self.conditions.push(condition_fn);
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn build(self) -> TransitionDefinition {
        let id = self.id.expect("Transition ID must be specified");
        TransitionDefinition {
            id: id.clone(),
            from_states: self.from_states,
            to_state: self.to_state.expect("To state must be specified"),
            trigger: self.trigger.unwrap_or_else(|| id.clone()),
            required_roles: self.required_roles,
            guard_fn: self.guard_fn,
            action_fn: self.action_fn,
            conditions: self.conditions,
            description: self.description.unwrap_or_default(),
        }
    }
}

pub struct RuleBuilder {
    name: Option<String>,
    condition_fn: Option<fn(&WorkflowContext) -> bool>,
    action_fn: Option<fn(&WorkflowContext) -> Result<WorkflowEvent, WorkflowError>>,
    description: Option<String>,
}

impl RuleBuilder {
    pub fn new() -> Self {
        Self {
            name: None,
            condition_fn: None,
            action_fn: None,
            description: None,
        }
    }

    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn condition(mut self, condition_fn: fn(&WorkflowContext) -> bool) -> Self {
        self.condition_fn = Some(condition_fn);
        self
    }

    pub fn action(
        mut self,
        action_fn: fn(&WorkflowContext) -> Result<WorkflowEvent, WorkflowError>,
    ) -> Self {
        self.action_fn = Some(action_fn);
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn build(self) -> RuleDefinition {
        RuleDefinition {
            name: self.name.expect("Rule name must be specified"),
            condition_fn: self
                .condition_fn
                .expect("Rule condition function must be specified"),
            action_fn: self
                .action_fn
                .expect("Rule action function must be specified"),
            description: self.description.unwrap_or_default(),
        }
    }
}

/// Example workflow using the builder pattern
pub fn create_enterprise_approval_workflow() -> WorkflowDefinition {
    WorkflowBuilder::new("Enterprise Approval Process")
        .initial_state("recorded")
        .state(|s| {
            s.id("recorded")
                .name("Locally Recorded")
                .description("Change has been recorded locally but not submitted for review")
                .permissions(vec!["read", "submit"])
        })
        .state(
            |s| {
                s.id("security-review")
                    .name("Security Team Review")
                    .description("Change is under security team review")
                    .permissions(vec!["read"])
                    .timeout(Duration::from_secs(3600 * 24 * 3))
            }, // 3 days
        )
        .state(|s| {
            s.id("code-review")
                .name("Code Review")
                .description("Change is under code review")
                .permissions(vec!["read", "approve", "reject"])
                .parallel()
        })
        .state(|s| {
            s.id("qa-review")
                .name("QA Review")
                .description("Change is under QA review")
                .permissions(vec!["read", "approve", "reject"])
                .parallel()
        })
        .state(|s| {
            s.id("approved")
                .name("Approved")
                .description("Change has been approved and can be applied")
                .permissions(vec!["read", "apply"])
        })
        .state(|s| {
            s.id("applied")
                .name("Applied")
                .description("Change has been successfully applied")
                .permissions(vec!["read"])
        })
        .state(|s| {
            s.id("rejected")
                .name("Rejected")
                .description("Change has been rejected")
                .permissions(vec!["read", "resubmit"])
        })
        .transition(|t| {
            t.id("submit-for-security")
                .from(vec!["recorded"])
                .to("security-review")
                .trigger("submit_security")
                .required_roles(vec!["developer"])
                .guard(|ctx| {
                    !ctx.metadata
                        .get("security_bypass")
                        .map_or(false, |v| v == "true")
                })
                .action(|ctx| {
                    Ok(WorkflowEvent::NotificationSent {
                        recipient: "security-team".to_string(),
                        message: format!(
                            "New change {} submitted for security review",
                            ctx.change_id
                        ),
                    })
                })
                .condition(|ctx| !ctx.dependencies.is_empty())
                .description("Submit change for security review")
        })
        .transition(|t| {
            t.id("security-approve")
                .from(vec!["security-review"])
                .to("code-review")
                .trigger("approve")
                .required_roles(vec!["security-reviewer"])
                .action(|ctx| {
                    Ok(WorkflowEvent::StateChanged {
                        from: "security-review".to_string(),
                        to: "code-review".to_string(),
                        reason: "Security approved".to_string(),
                    })
                })
                .description("Security team approves the change")
        })
        .transition(|t| {
            t.id("code-approve")
                .from(vec!["code-review"])
                .to("qa-review")
                .trigger("approve")
                .required_roles(vec!["code-reviewer"])
                .guard(|ctx| {
                    ctx.user_roles.contains("senior-developer")
                        || ctx.user_roles.contains("tech-lead")
                })
                .description("Code review approval")
        })
        .transition(|t| {
            t.id("qa-approve")
                .from(vec!["qa-review"])
                .to("approved")
                .trigger("approve")
                .required_roles(vec!["qa-reviewer"])
                .description("QA team approves the change")
        })
        .transition(|t| {
            t.id("apply-change")
                .from(vec!["approved"])
                .to("applied")
                .trigger("apply")
                .required_roles(vec!["deployer", "admin"])
                .action(|ctx| {
                    Ok(WorkflowEvent::StateChanged {
                        from: "approved".to_string(),
                        to: "applied".to_string(),
                        reason: "Change successfully applied".to_string(),
                    })
                })
                .description("Apply the approved change")
        })
        .transition(|t| {
            t.id("reject-security")
                .from(vec!["security-review"])
                .to("rejected")
                .trigger("reject")
                .required_roles(vec!["security-reviewer"])
                .description("Security team rejects the change")
        })
        .transition(|t| {
            t.id("reject-code")
                .from(vec!["code-review"])
                .to("rejected")
                .trigger("reject")
                .required_roles(vec!["code-reviewer"])
                .description("Code reviewer rejects the change")
        })
        .transition(|t| {
            t.id("reject-qa")
                .from(vec!["qa-review"])
                .to("rejected")
                .trigger("reject")
                .required_roles(vec!["qa-reviewer"])
                .description("QA team rejects the change")
        })
        .rule(|r| {
            r.name("auto-approve-trusted")
                .condition(|ctx| {
                    ctx.user_roles.contains("trusted-committer")
                        && ctx
                            .metadata
                            .get("change_type")
                            .map_or(false, |v| v == "documentation")
                })
                .action(|_ctx| {
                    Ok(WorkflowEvent::AutoApproved {
                        reason: "Trusted committer documentation change".to_string(),
                    })
                })
                .description("Auto-approve documentation changes from trusted committers")
        })
        .rule(|r| {
            r.name("timeout-reminder")
                .condition(|ctx| {
                    ctx.metadata
                        .get("days_in_review")
                        .map_or(false, |v| v == "2")
                })
                .action(|_ctx| {
                    Ok(WorkflowEvent::NotificationSent {
                        recipient: "reviewers".to_string(),
                        message: "Change has been in review for 2 days".to_string(),
                    })
                })
                .description("Send reminder after 2 days in review")
        })
        .build()
        .expect("Failed to build workflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_builder() {
        let workflow = create_enterprise_approval_workflow();

        assert_eq!(workflow.name, "Enterprise Approval Process");
        assert_eq!(workflow.initial_state, "recorded");
        assert_eq!(workflow.states.len(), 7);
        assert_eq!(workflow.transitions.len(), 9);
        assert_eq!(workflow.rules.len(), 2);

        // Test that initial state exists
        assert!(workflow.states.contains_key(&workflow.initial_state));

        // Test state properties
        let recorded_state = &workflow.states["recorded"];
        assert_eq!(recorded_state.name, "Locally Recorded");
        assert!(recorded_state.permissions.contains(&"read".to_string()));

        // Test transition properties
        let submit_transition = workflow
            .transitions
            .iter()
            .find(|t| t.id == "submit-for-security")
            .expect("Submit transition should exist");
        assert_eq!(submit_transition.from_states, vec!["recorded"]);
        assert_eq!(submit_transition.to_state, "security-review");
        assert!(submit_transition
            .required_roles
            .contains(&"developer".to_string()));
    }

    #[test]
    fn test_workflow_validation() {
        // Test missing initial state
        let result = WorkflowBuilder::<()>::new("Test")
            .state(|s| s.id("test").name("Test"))
            .build();
        assert!(result.is_err());

        // Test invalid initial state reference
        let result = WorkflowBuilder::<()>::new("Test")
            .initial_state("nonexistent")
            .state(|s| s.id("test").name("Test"))
            .build();
        assert!(result.is_err());
    }
}
