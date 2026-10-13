// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:17 -05:00
//! Mock transports exercise production protocols and the real editor adapter without API requests.
use super::{
    Editor,
    ai_agent::run_with_transport,
    ai_project::{AgentAccess, AgentSettings, ProjectAccess},
    ai_provider::{Config, Provider},
};
use crate::world::World;
use serde_json::{Value, json};
use std::{fs, sync::atomic::AtomicBool};
fn response(provider: Provider, round: usize, actor: u64) -> Value {
    if round == 3 {
        return match provider {
            Provider::Gemini => {
                json!({"candidates":[{"content":{"role":"model","parts":[{"text":"Created and edited scripts/hello.aeo."}]}}]})
            }
            Provider::OpenAi => {
                json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Created and edited scripts/hello.aeo."}]}]})
            }
            _ => {
                json!({"choices":[{"message":{"role":"assistant","content":"Created and edited scripts/hello.aeo."}}]})
            }
        };
    }
    let args = match round {
        0 => {
            json!({"mode":"create","path":"scripts/hello.aeo","source":"fn broken( {","actor_id":actor})
        }
        1 => {
            json!({"mode":"create","path":"scripts/hello.aeo","source":"debug.log(\"Hello World\")","actor_id":actor})
        }
        _ => {
            json!({"mode":"replace","path":"scripts/hello.aeo","expected_source":"debug.log(\"Hello World\")","source":"debug.log(\"Hello edited\")"})
        }
    };
    let call_id = format!("call_{round}");
    match provider {
        Provider::Gemini => {
            json!({"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":call_id,"name":"write_script","args":args},"thoughtSignature":"mock-signature"}]}}]})
        }
        Provider::OpenAi => {
            json!({"status":"completed","output":[{"type":"reasoning","id":format!("reasoning_{round}"),"encrypted_content":"mock-encrypted-reasoning","summary":[]},{"type":"function_call","id":format!("fc_{round}"),"call_id":call_id,"name":"write_script","arguments":args.to_string()}]})
        }
        _ => {
            json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":call_id,"type":"function","function":{"name":"write_script","arguments":args.to_string()}}]}}]})
        }
    }
}
fn roundtrip(provider: Provider) {
    let root = std::env::temp_dir().join(format!(
        "wo79_protocol_{:?}_{}",
        provider,
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut editor = Editor::new();
    let mut world = World::new();
    let actor = world.add_actor("Mock NPC", glam::Vec3::ZERO);
    editor.synchronize_project_actor_nodes(&world);
    let config = Config {
        provider,
        base_url: "http://127.0.0.1:9/v1".into(),
        key: "mock-key".into(),
        model: "mock-model".into(),
    };
    let access = ProjectAccess::open(&root).unwrap();
    let settings = AgentSettings {
        access: AgentAccess::FullAccess,
        ..Default::default()
    };
    let mut executions = 0;
    let mut round = 0;
    let answer = run_with_transport(
        &config,
        &[(
            "user".into(),
            "Create Hello World, attach it, then edit it.".into(),
        )],
        &access,
        &settings,
        &AtomicBool::new(false),
        |_| {},
        |name, args| {
            executions += 1;
            let result = editor
                .execute_agent_tool(&mut world, &root, &settings, false, name, args)
                .unwrap();
            if result.1 {
                crate::project::save_project_metadata(&root, &editor.project_hierarchy).unwrap();
                crate::world::persistence::save_world(&world, &root.join("world.dat")).unwrap();
            }
            result.0
        },
        |_, request, url| {
            let text = request.to_string();
            assert!(text.contains("write_script"));
            match provider {
                Provider::Gemini => assert!(url.ends_with("/models/mock-model:generateContent")),
                Provider::OpenAi => assert!(url.ends_with("/responses")),
                _ => assert!(url.ends_with("/chat/completions")),
            }
            if round == 1 {
                assert!(text.contains("validation_error"));
                assert!(text.contains("Syntax error at"));
            }
            if round >= 2 {
                assert!(text.contains("Saved scripts/hello.aeo"));
            }
            if round > 0 {
                assert!(text.contains(&format!("call_{}", round - 1)));
                match provider {
                    Provider::Gemini => {
                        assert!(text.contains("functionResponse"));
                        assert!(text.contains("mock-signature"));
                    }
                    Provider::OpenAi => {
                        assert!(text.contains("function_call_output"));
                        assert!(text.contains("mock-encrypted-reasoning"));
                    }
                    _ => assert!(text.contains("tool_call_id")),
                }
            }
            let output = response(provider, round, actor);
            round += 1;
            Ok(output)
        },
    )
    .unwrap();
    assert_eq!(round, 4);
    assert_eq!(executions, 2, "invalid source must not reach the editor");
    assert!(answer.contains("scripts/hello.aeo"));
    assert_eq!(
        fs::read_to_string(root.join("scripts/hello.aeo")).unwrap(),
        "debug.log(\"Hello edited\")"
    );
    let hierarchy = crate::project::load_project_metadata(&root).unwrap();
    assert_eq!(
        hierarchy.attached_actor_id(hierarchy.find_script_nodes("scripts/hello.aeo")[0]),
        Some(actor)
    );
    assert!(!editor.script_editor.open_documents[&root.join("scripts/hello.aeo")].dirty);
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn compatible_mock_repairs_creates_attaches_edits_and_saves() {
    roundtrip(Provider::Compatible);
}
#[test]
fn gemini_mock_preserves_tool_ids_and_thought_signatures() {
    roundtrip(Provider::Gemini);
}
#[test]
fn responses_mock_preserves_call_ids_and_reasoning() {
    roundtrip(Provider::OpenAi);
}
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:24 -05:00
#[test]
fn failed_corrections_preserve_existing_file_and_never_reach_editor() {
    let root = std::env::temp_dir().join(format!("wo79_failed_repair_{}", std::process::id()));
    fs::create_dir_all(root.join("scripts")).unwrap();
    fs::write(root.join("scripts/good.aeo"), "debug.log(1)").unwrap();
    let access = ProjectAccess::open(&root).unwrap();
    let config = Config {
        provider: Provider::Compatible,
        base_url: "http://127.0.0.1:9/v1".into(),
        model: "mock".into(),
        key: "mock-key".into(),
    };
    let mut rounds = 0;
    let mut executed = 0;
    let result = run_with_transport(
        &config,
        &[],
        &access,
        &AgentSettings::default(),
        &AtomicBool::new(false),
        |_| {},
        |_, _| {
            executed += 1;
            String::new()
        },
        |_, request, _| {
            if rounds > 0 {
                assert!(request.to_string().contains("validation_error"));
            }
            rounds += 1;
            Ok(
                json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{
                "id":format!("bad_{rounds}"),"type":"function","function":{"name":"write_script","arguments":json!({"mode":"replace","path":"scripts/good.aeo","source":"fn broken( {","expected_source":"debug.log(1)"}).to_string()}}]}}]}),
            )
        },
    );
    assert!(result.unwrap_err().contains("Automatic correction stopped"));
    assert_eq!(rounds, 4);
    assert_eq!(executed, 0);
    assert_eq!(
        fs::read_to_string(root.join("scripts/good.aeo")).unwrap(),
        "debug.log(1)"
    );
    fs::remove_dir_all(root).unwrap();
}
