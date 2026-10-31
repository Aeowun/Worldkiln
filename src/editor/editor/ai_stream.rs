// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::ai_provider::Provider;
use serde_json::{Value, json};
pub(super) struct Collector {
    provider: Provider,
    parts: Vec<Value>,
    message: Value,
    calls: std::collections::BTreeMap<usize, Value>,
    completed: Option<Value>,
}
impl Collector {
    pub(super) fn new(provider: Provider) -> Self {
        Self {
            provider,
            parts: Vec::new(),
            message: json!({"role":"assistant","content":""}),
            calls: Default::default(),
            completed: None,
        }
    }
    pub(super) fn accept(&mut self, event: Value, emit: impl FnMut(&str)) -> Result<(), String> {
        self.accept_with_thinking(event, emit, |_| {})
    }
    pub(super) fn accept_with_thinking(
        &mut self,
        event: Value,
        mut emit: impl FnMut(&str),
        mut thinking: impl FnMut(&str),
    ) -> Result<(), String> {
        if event.get("error").is_some()
            || matches!(event["type"].as_str(), Some("error" | "response.failed"))
        {
            return Err("Provider failed while streaming the agent response.".into());
        }
        match self.provider {
            Provider::Gemini => {
                if let Some(parts) = event["candidates"][0]["content"]["parts"].as_array() {
                    for part in parts {
                        if let Some(text) = part["text"].as_str() {
                            if part["thought"] == true {
                                thinking(text);
                            } else {
                                emit(text);
                            }
                        }
                        self.parts.push(part.clone());
                    }
                }
            }
            Provider::OpenAi => match event["type"].as_str() {
                Some("response.output_text.delta") => {
                    if let Some(text) = event["delta"].as_str() {
                        emit(text);
                    }
                }
                Some("response.reasoning_summary_text.delta") => {
                    if let Some(text) = event["delta"].as_str() {
                        thinking(text);
                    }
                }
                Some("response.completed" | "response.incomplete") => {
                    self.completed = Some(event["response"].clone())
                }
                _ => {}
            },
            _ => {
                let delta = &event["choices"][0]["delta"];
                if let Some(text) = delta["reasoning_content"]
                    .as_str()
                    .or_else(|| delta["reasoning"].as_str())
                {
                    thinking(text);
                }
                if let Some(text) = delta["content"].as_str() {
                    emit(text);
                    let mut content = self.message["content"].as_str().unwrap_or("").to_string();
                    content.push_str(text);
                    self.message["content"] = json!(content);
                }
                if let Some(calls) = delta["tool_calls"].as_array() {
                    for call in calls {
                        let index = call["index"]
                            .as_u64()
                            .ok_or("Stream tool call has no index.")?
                            as usize;
                        if index > 48 {
                            return Err("Stream tool call limit exceeded.".into());
                        }
                        let current=self.calls.entry(index).or_insert_with(|| json!({"id":"","type":"function","function":{"name":"","arguments":""}}));
                        if let Some(id) = call["id"].as_str() {
                            current["id"] = json!(id);
                        }
                        for key in ["name", "arguments"] {
                            if let Some(text) = call["function"][key].as_str() {
                                let mut value =
                                    current["function"][key].as_str().unwrap_or("").to_string();
                                value.push_str(text);
                                current["function"][key] = json!(value);
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn finish(mut self) -> Result<Value, String> {
        match self.provider {
            Provider::Gemini => {
                Ok(json!({"candidates":[{"content":{"role":"model","parts":self.parts}}]}))
            }
            Provider::OpenAi => self
                .completed
                .ok_or("OpenAI stream ended without a completed response.".into()),
            _ => {
                if !self.calls.is_empty() {
                    self.message["tool_calls"] =
                        json!(self.calls.into_values().collect::<Vec<_>>());
                }
                Ok(json!({"choices":[{"message":self.message}]}))
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reasoning_is_separate_from_visible_answer_when_provider_emits_it() {
        let mut collector = Collector::new(Provider::Compatible);
        let mut answer = String::new();
        let mut thought = String::new();
        collector.accept_with_thinking(
            json!({"choices":[{"delta":{"reasoning_content":"Checking syntax","content":"Done"}}]}),
            |s| answer.push_str(s), |s| thought.push_str(s)
        ).unwrap();
        assert_eq!(thought, "Checking syntax");
        assert_eq!(answer, "Done");
        let mut gemini = Collector::new(Provider::Gemini);
        let mut thinking = String::new();
        gemini.accept_with_thinking(
            json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"Working"},{"text":"Complete"}]}}]}),
            |_| {}, |s| thinking.push_str(s)
        ).unwrap();
        assert_eq!(thinking, "Working");
        let mut responses = Collector::new(Provider::OpenAi);
        let mut summary = String::new();
        responses
            .accept_with_thinking(
                json!({"type":"response.reasoning_summary_text.delta","delta":"Reviewed tools"}),
                |_| {},
                |s| summary.push_str(s),
            )
            .unwrap();
        assert_eq!(summary, "Reviewed tools");
    }
    #[test]
    fn compatible_fragments_preserve_arguments_and_text() {
        let mut c = Collector::new(Provider::Compatible);
        let mut text = String::new();
        for event in [
            json!({"choices":[{"delta":{"content":"Working ","tool_calls":[{"index":0,"id":"c1","function":{"name":"write_script","arguments":"{\"path\":"}}]}}]}),
            json!({"choices":[{"delta":{"content":"now","tool_calls":[{"index":0,"function":{"arguments":"\"scripts/test.aeo\"}"}}]}}]}),
        ] {
            c.accept(event, |s| text.push_str(s)).unwrap();
        }
        let response = c.finish().unwrap();
        let call = &response["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(text, "Working now");
        assert_eq!(call["id"], "c1");
        assert_eq!(
            serde_json::from_str::<Value>(call["function"]["arguments"].as_str().unwrap()).unwrap()
                ["path"],
            "scripts/test.aeo"
        );
    }
    #[test]
    fn gemini_preserves_signed_calls_and_hides_thought_text() {
        let mut c = Collector::new(Provider::Gemini);
        let mut text = String::new();
        c.accept(json!({"candidates":[{"content":{"parts":[{"thought":true,"text":"private thought"},{"text":"Hello"},{"functionCall":{"name":"inspect_project","args":{},"id":"g1"},"thoughtSignature":"signature"}]}}]}),|s| text.push_str(s)).unwrap();
        assert_eq!(text, "Hello");
        assert_eq!(
            c.finish().unwrap()["candidates"][0]["content"]["parts"][2]["thoughtSignature"],
            "signature"
        );
    }
    #[test]
    fn responses_stream_keeps_completed_reasoning_and_tools() {
        let mut c = Collector::new(Provider::OpenAi);
        let mut text = String::new();
        c.accept(
            json!({"type":"response.output_text.delta","delta":"Done"}),
            |s| text.push_str(s),
        )
        .unwrap();
        let response = json!({"status":"completed","output":[{"type":"reasoning","encrypted_content":"encrypted"},{"type":"function_call","call_id":"o1","arguments":"{}","name":"inspect_project"}]});
        c.accept(
            json!({"type":"response.completed","response":response}),
            |_| {},
        )
        .unwrap();
        assert_eq!(text, "Done");
        assert_eq!(c.finish().unwrap(), response);
        assert!(Collector::new(Provider::OpenAi).finish().is_err());
    }
}
