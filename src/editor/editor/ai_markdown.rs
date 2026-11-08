// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::sync::{Arc, Mutex};

pub(super) fn show(ui: &mut egui::Ui, text: &str) {
    let id = egui::Id::new("worldkiln_ai_markdown_cache");
    let cache = ui.ctx().data_mut(|data| {
        data.get_temp_mut_or_insert_with(id, || {
            let mut cache = CommonMarkCache::default();
            cache.add_syntax_from_str(
                r#"%YAML 1.2
---
name: AeoScript
file_extensions: [aeo, aeoscript, worldkiln-aeoscript]
scope: source.aeoscript
contexts:
  main:
    - match: '//.*$'
      scope: comment.line.aeoscript
    - match: '"([^"\\]|\\.)*"'
      scope: string.quoted.double.aeoscript
    - match: '\b(const|else|entity|fn|for|if|import|in|on|return|while)\b'
      scope: keyword.control.aeoscript
    - match: '\b(true|false|nil)\b'
      scope: constant.language.aeoscript
    - match: '\b[0-9]+(\.[0-9]+)?\b'
      scope: constant.numeric.aeoscript
"#,
                Some("AeoScript"),
            );
            Arc::new(Mutex::new(cache))
        })
        .clone()
    });
    if let Ok(mut cache) = cache.lock() {
        CommonMarkViewer::new()
            .max_image_width(Some(0))
            .show(ui, &mut cache, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_v2_markdown_renders_partial_streams_and_aeoscript() {
        let ctx = egui::Context::default();
        let text = "## Heading\n\n**Bold** and `inline`\n\n- First\n- Second\n\n| Name | Value |\n|---|---|\n| Speed | 3 |\n\n```aeoscript\nconst speed = 3\ndebug.log(\"hello\")\n";
        for end in text
            .char_indices()
            .map(|(i, _)| i)
            .step_by(13)
            .chain(std::iter::once(text.len()))
        {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_max_width(240.0);
                    show(ui, &text[..end]);
                });
            });
        }
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_max_width(240.0);
                show(ui, text);
            });
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text)
                } else {
                    None
                }
            })
            .collect();
        assert!(labels.iter().any(|text| text.galley.job.text == "Heading"));
        assert!(
            !labels
                .iter()
                .any(|text| text.galley.job.text.contains("## Heading"))
        );
        assert!(
            labels
                .iter()
                .any(|text| text.galley.job.text.contains("Speed"))
        );
        let code = labels
            .iter()
            .find(|text| text.galley.job.text.contains("const speed"))
            .expect("code remains visible during streaming");
        let colors: std::collections::HashSet<_> = code
            .galley
            .job
            .sections
            .iter()
            .map(|section| section.format.color)
            .collect();
        assert!(
            colors.len() > 1,
            "AeoScript code receives syntax highlighting"
        );
    }
}
