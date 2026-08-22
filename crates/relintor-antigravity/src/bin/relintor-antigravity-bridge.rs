use relintor_antigravity::{
    canonical_hook_action_digest, hook_action_matches_authority, HookAction, TaskPacket,
};
use serde_json::{json, Value};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

fn deny(reason: &str) -> Value {
    json!({ "decision": "deny", "reason": reason })
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_str))
        .map(str::to_owned)
}

fn string_array_field(value: &Value, names: &[&str]) -> Option<Vec<String>> {
    names.iter().find_map(|name| {
        value
            .get(*name)
            .and_then(Value::as_array)
            .and_then(|items| {
                items
                    .iter()
                    .map(Value::as_str)
                    .collect::<Option<Vec<_>>>()
                    .map(|items| items.into_iter().map(str::to_owned).collect())
            })
    })
}

fn action_from_tool_call(input: &Value, packet: &TaskPacket) -> Option<HookAction> {
    let call = input.get("toolCall")?;
    Some(HookAction {
        tool: string_field(call, &["tool", "name"])?,
        operation: string_field(call, &["operation", "action"])?,
        arguments: string_array_field(call, &["arguments", "args"])?,
        paths: string_array_field(call, &["paths", "workspacePaths"])
            .or_else(|| string_array_field(input, &["workspacePaths"]))?,
        working_scope: string_field(call, &["workingScope", "workingDirectory", "cwd"])
            .or_else(|| string_field(input, &["workingDirectory", "cwd"]))
            .unwrap_or_else(|| packet.authorized_action.working_scope.clone()),
        action_digest: String::new(),
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis() as u64)
        .unwrap_or_default()
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--self-test") {
        println!("RELINTOR_BRIDGE_READY");
        return;
    }
    let mut input = Vec::new();
    if io::stdin().read_to_end(&mut input).is_err() {
        let _ = writeln!(
            io::stdout(),
            "{}",
            deny("Relintor hook input was unreadable")
        );
        return;
    }
    let input: Value = match serde_json::from_slice(&input) {
        Ok(input) => input,
        Err(_) => {
            let _ = writeln!(io::stdout(), "{}", deny("Relintor hook input was invalid"));
            return;
        }
    };
    let context_path = match env::var_os("RELINTOR_ANTIGRAVITY_HOOK_CONTEXT") {
        Some(path) => path,
        None => {
            let _ = writeln!(
                io::stdout(),
                "{}",
                deny("Relintor execution context is unavailable")
            );
            return;
        }
    };
    let packet: TaskPacket = match fs::read(context_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        Some(packet) => packet,
        None => {
            let _ = writeln!(
                io::stdout(),
                "{}",
                deny("Relintor execution context is invalid")
            );
            return;
        }
    };
    if packet.task_packet_digest.is_empty()
        || packet.binding_digest().ok().as_deref() != Some(packet.task_packet_digest.as_str())
        || packet.validate(&packet.workspace).is_err()
        || now_ms() >= packet.authorized_action.expires_at_ms
    {
        let _ = writeln!(
            io::stdout(),
            "{}",
            deny("Relintor execution authority is stale")
        );
        return;
    }
    let Some(mut action) = action_from_tool_call(&input, &packet) else {
        let _ = writeln!(
            io::stdout(),
            "{}",
            deny("Relintor could not identify the exact authorized action")
        );
        return;
    };
    action.action_digest = canonical_hook_action_digest(
        &action.tool,
        &action.operation,
        &action.arguments,
        &action.paths,
        &action.working_scope,
    )
    .unwrap_or_default();
    if !hook_action_matches_authority(
        &action,
        &packet.authorized_action,
        Path::new(&packet.workspace),
    ) {
        let _ = writeln!(
            io::stdout(),
            "{}",
            deny("Antigravity requested an operation outside the sealed authority")
        );
        return;
    }
    let _ = writeln!(io::stdout(), "{}", json!({ "decision": "allow" }));
}
