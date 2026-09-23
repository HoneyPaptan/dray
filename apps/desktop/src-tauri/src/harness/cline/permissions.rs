//! Cline's `session/request_permission` onto Dray's permission card.
//!
//! ACP's own shape, so this is opencode's module with the ids renamed: the
//! options are the server's and go back as they came.
//!
//! The same bargain every ACP harness here makes: **the options are the
//! server's**. ACP names each one with an `optionId` and a `kind`, and the
//! button carries the id back untouched inside the outcome envelope — so a
//! decision Dray never composes is one it can never compose wrongly.

use crate::events::{PermissionBehavior, PermissionOption, PermissionOptionKind};
use crate::harness::claude_code::permissions::{PendingRequest, Reply, ResolvedOption};
use serde_json::{json, Value};

use super::parser::PermissionRequest;

/// Builds the held request and its buttons in one pass.
pub fn pending_for(
    request: &PermissionRequest,
    rpc_id: i64,
) -> (PendingRequest, Vec<PermissionOption>) {
    let resolved = build_options(request);
    let offered = resolved.iter().map(|r| r.option.clone()).collect();

    let options = resolved
        .into_iter()
        .map(|r| (r.option.id.clone(), r))
        .collect();

    let pending = PendingRequest {
        tool_use_id: request.tool_call.tool_call_id.clone(),
        tool_name: super::tool_name_of(request),
        // Cline rebuilds nothing from the answer — it is the option alone.
        input: Value::Null,
        options,
        reply: Reply::Rpc(rpc_id),
    };

    (pending, offered)
}

/// The reply `session/request_permission` wants for a picked option.
pub fn selected(option_id: &str) -> Value {
    json!({"outcome": {"outcome": "selected", "optionId": option_id}})
}

/// One button per option the server offered, in the order the card reads:
/// allow first, standing grants in the middle, refusal last. A kind this build
/// cannot spell is dropped rather than drawn generically.
fn build_options(request: &PermissionRequest) -> Vec<ResolvedOption> {
    use PermissionBehavior::{Allow, Deny};
    use PermissionOptionKind as Kind;

    let mut options: Vec<ResolvedOption> = request
        .options
        .iter()
        .filter_map(|choice| {
            let (kind, behavior) = match choice.kind.as_str() {
                "allow_once" => (Kind::Once, Allow),
                "allow_always" => (Kind::AlwaysRule, Allow),
                "reject_once" | "reject_always" => (Kind::Deny, Deny),
                _ => return None,
            };
            Some(ResolvedOption {
                option: PermissionOption {
                    id: format!("cline-{}", choice.option_id),
                    // The server's own words: "Allow once", "Allow always",
                    // "Reject".
                    label: choice.name.clone(),
                    kind,
                    behavior,
                },
                updates: Vec::new(),
                decision: Some(selected(&choice.option_id)),
            })
        })
        .collect();

    options.sort_by_key(|o| match o.option.kind {
        Kind::Once => 0,
        Kind::AlwaysRule | Kind::AlwaysDirectory | Kind::SwitchMode => 1,
        Kind::Deny => 2,
    });

    // A card with no way to say no cannot be answered honestly. ACP's
    // `cancelled` outcome is always a legal answer, whatever was offered.
    if !options.iter().any(|o| o.option.behavior == Deny) {
        options.push(ResolvedOption {
            option: PermissionOption {
                id: "cline-cancelled".to_string(),
                label: "Deny".to_string(),
                kind: Kind::Deny,
                behavior: Deny,
            },
            updates: Vec::new(),
            decision: Some(json!({"outcome": {"outcome": "cancelled"}})),
        });
    }

    options
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::cline::parser::{PermissionChoice, ToolCallRef};
    use serde_json::Value;

    /// The capture's own request, options and all.
    const LIVE_TURN: &str = include_str!("fixtures/live_turn.jsonl");

    fn captured() -> PermissionRequest {
        let line = LIVE_TURN
            .lines()
            .find(|l| l.contains("session/request_permission"))
            .expect("the capture holds a permission request");
        let value: Value = serde_json::from_str(line).expect("fixture line is JSON");
        serde_json::from_value(value["params"].clone()).expect("the request parses")
    }

    fn request(kinds: &[(&str, &str)]) -> PermissionRequest {
        PermissionRequest {
            session_id: "x".into(),
            tool_call: ToolCallRef {
                tool_call_id: "call_1".into(),
                title: Some("run_commands: ls".into()),
                ..Default::default()
            },
            options: kinds
                .iter()
                .map(|(id, kind)| PermissionChoice {
                    option_id: id.to_string(),
                    name: id.replace('_', " "),
                    kind: kind.to_string(),
                })
                .collect(),
        }
    }

    /// The live request's options, drawn in card order with Cline's own ids
    /// carried back whole inside the outcome envelope.
    #[test]
    fn the_captured_request_becomes_buttons_in_card_order() {
        let (pending, offered) = pending_for(&captured(), 7);

        assert!(offered.len() >= 2);
        assert_eq!(offered[0].kind, PermissionOptionKind::Once);
        assert_eq!(
            offered.last().expect("at least one option").behavior,
            PermissionBehavior::Deny,
            "a card always ends in a way to say no",
        );
        assert_eq!(pending.reply, Reply::Rpc(7));
        assert_eq!(
            pending.options["cline-allow_once"]
                .decision
                .as_ref()
                .expect("allow carries a decision"),
            &json!({"outcome": {"outcome": "selected", "optionId": "allow_once"}}),
        );
    }

    /// The card's tool name is the tool, not Cline's title — which is the tool
    /// with its whole argument blob pasted after it.
    #[test]
    fn the_card_names_the_tool_alone() {
        let (pending, _) = pending_for(&captured(), 1);
        assert_eq!(pending.tool_name, "run_commands");
    }

    /// No refusal offered → one is added, as ACP's `cancelled` outcome.
    #[test]
    fn a_request_with_no_refusal_gets_one() {
        let request = request(&[("allow_once", "allow_once"), ("later", "some_new_kind")]);
        let (pending, offered) = pending_for(&request, 2);

        assert_eq!(offered.len(), 2, "the unknown kind is dropped, a Deny is added");
        assert_eq!(
            pending.options["cline-cancelled"]
                .decision
                .as_ref()
                .expect("the added refusal carries one"),
            &json!({"outcome": {"outcome": "cancelled"}}),
        );
    }
}
