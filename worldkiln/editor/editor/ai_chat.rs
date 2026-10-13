// Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:50 -05:00
// Chat-only AI foundation. No project context or editor write tools.
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
use super::ai_project::{AgentAccess, AgentSettings, Capability, capability, permitted};
use super::ai_provider::{self, Config, Provider};
use super::ai_script;
use super::{ai_agent, ai_project::ProjectAccess, ai_secure};
use crate::editor::script_editor::ScriptEditor;
use crate::project::ProjectHierarchy;
use egui::{Color32, RichText};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};

fn legacy_provider() -> Provider {
    Provider::OpenAi
}
#[derive(Clone, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
    #[serde(default = "yes")]
    complete: bool,
    #[serde(default)]
    script_draft: bool,
}
fn yes() -> bool {
    true
}
// Worldkiln 0.8.5 | WO 0.8.7-76 | 2026-10-09 22:50 -05:00
fn composer_can_send(draft: &str, active: bool) -> bool {
    !active && !draft.trim().is_empty()
}
// Worldkiln 0.8.5 | WO 0.8.7-77 | 2026-10-09 22:50 -05:00
// Only bare Enter submits; Shift+Enter is reserved for multiline drafts.
fn composer_enter_sends(modifiers: egui::Modifiers) -> bool {
    modifiers.is_none()
}
#[derive(Serialize, Deserialize)]
struct Archive {
    model: String,
    messages: Vec<Message>,
    #[serde(default = "legacy_provider")]
    provider: Provider,
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    context_start: usize,
    #[serde(default)]
    draft: String,
    #[serde(default = "yes")]
    script_mode: bool,
    #[serde(default)]
    agent_settings: AgentSettings,
}
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
pub(crate) struct EditorToolRequest {
    pub id: u64,
    pub project: PathBuf,
    pub name: String,
    pub args: serde_json::Value,
    pub reply: Sender<String>,
}
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:10 -05:00
pub(crate) struct FileChangeView {
    pub id: u64,
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub kept: bool,
    pub reverted: bool,
    pub before: String,
    pub after: String,
}
enum Event {
    Delta(u64, String),
    Done(u64),
    Error(u64, String),
    Models(u64, Result<Vec<String>, String>),
    ProjectTool(u64, String),
    EditorTool(EditorToolRequest),
}
pub struct AiChat {
    pub open: bool,
    provider: Provider,
    endpoint: String,
    keys: HashMap<Provider, String>, // Never saved to chat archive or projects.
    key: String,
    model: String,
    models: Vec<String>,
    model_filter: String,
    loading_models: bool,
    discovery_id: u64,
    context_start: usize,
    draft: String,
    messages: Vec<Message>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    id: u64,
    active: Option<u64>,
    cancel: Option<Arc<AtomicBool>>,
    error: Option<String>,
    status: Option<String>,
    path: PathBuf,
    credential_path: PathBuf,
    credential_warning: Option<String>,
    script_mode: bool,
    bound_project: Option<PathBuf>,
    applied: bool,
    tool_activity: Vec<String>,
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
    pub(crate) agent_settings: AgentSettings,
    pending_tools: std::collections::VecDeque<EditorToolRequest>,
    approved_operation: bool,
    pub(crate) undo_requested: bool,
    pub(crate) revert_requested: Option<u64>,
    pub(crate) file_changes: Vec<FileChangeView>,
    // Worldkiln 0.8.5 | WO 0.8.7-80 | 2026-10-10 00:45 -05:00
    // One-shot engine notification; playback stays with App's existing audio service.
    pending_notification: Option<crate::engine::audio::EngineSound>,
    approval_alerted: bool,
}
fn model_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._:-/@+".contains(&c))
}
fn env_key(provider: Provider) -> String {
    match provider {
        Provider::Gemini => std::env::var("GEMINI_API_KEY").unwrap_or_default(),
        Provider::OpenAi => std::env::var("OPENAI_API_KEY").unwrap_or_default(),
        _ => String::new(),
    }
}
fn request(
    ctx: egui::Context,
    tx: Sender<Event>,
    id: u64,
    config: Config,
    turns: Vec<(String, String)>,
    stop: Arc<AtomicBool>,
    access: Option<ProjectAccess>,
    project: Option<PathBuf>,
    settings: AgentSettings,
) {
    if let Some(access) = access {
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:21 -05:00
        let streamed = std::cell::Cell::new(false);
        let result = ai_agent::run_streamed(
            &config,
            &turns,
            &access,
            &settings,
            &stop,
            |step| {
                let _ = tx.send(Event::ProjectTool(id, step));
                ctx.request_repaint();
            },
            |name, args| {
                let (reply, receiver) = mpsc::channel();
                let Some(project) = &project else {
                    return "ERROR: No open project.".into();
                };
                if tx
                    .send(Event::EditorTool(EditorToolRequest {
                        id,
                        project: project.clone(),
                        name: name.into(),
                        args: args.clone(),
                        reply,
                    }))
                    .is_err()
                {
                    return "ERROR: Editor unavailable.".into();
                }
                ctx.request_repaint();
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return "ERROR: Operation cancelled.".into();
                    }
                    match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                        Ok(result) => return result,
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            return "ERROR: Editor request discarded.".into();
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            },
            |chunk| {
                streamed.set(true);
                let _ = tx.send(Event::Delta(id, chunk.to_string()));
                ctx.request_repaint();
            },
        );
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let outcome = match result {
            Ok(reply) => {
                if !streamed.get() {
                    let _ = tx.send(Event::Delta(id, reply));
                }
                Event::Done(id)
            }
            Err(e) => Event::Error(id, e),
        };
        let _ = tx.send(outcome);
        ctx.request_repaint();
        return;
    }
    let result = ai_provider::stream_chat(&config, &turns, &stop, |chunk| {
        let _ = tx.send(Event::Delta(id, chunk));
        ctx.request_repaint();
    });
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let _ = tx.send(match result {
        Ok(()) => Event::Done(id),
        Err(e) => Event::Error(id, e),
    });
    ctx.request_repaint();
}
impl AiChat {
    pub fn new() -> Self {
        let root = std::env::var_os("WORLDKILN_AI_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(PathBuf::from))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        // Stable installation-independent storage. Existing preview conversations are migrated
        // non-destructively on the first save, without copying any credentials.
        let legacy = root.join("WorldkilnAI").join("chat.json");
        let data = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .map(|base| base.join("Aeowun").join("Worldkiln").join("AI"))
            .unwrap_or_else(|| root.join("WorldkilnAI"));
        let path = data.join("chat.json");
        let credential_path = data.join("credentials.dpapi");
        let loaded = fs::read(if path.is_file() { &path } else { &legacy })
            .ok()
            .and_then(|buf| serde_json::from_slice::<Archive>(&buf).ok());
        let (mut saved_keys, credential_warning) = match ai_secure::load(&credential_path) {
            Ok(keys) => (keys, None),
            Err(error) => (HashMap::new(), Some(error)),
        };
        let provider = loaded
            .as_ref()
            .map(|a| a.provider)
            .unwrap_or(Provider::Gemini);
        let endpoint = loaded
            .as_ref()
            .map(|a| a.endpoint.clone())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| provider.default_url().into());
        let model = loaded
            .as_ref()
            .map(|a| a.model.clone())
            .filter(|s| model_ok(s))
            .unwrap_or_default();
        let messages = loaded
            .as_ref()
            .map(|a| a.messages.clone())
            .unwrap_or_default();
        let context_start = loaded
            .as_ref()
            .map(|a| a.context_start)
            .unwrap_or(0)
            .min(messages.len());
        let draft = loaded.as_ref().map(|a| a.draft.clone()).unwrap_or_default();
        let script_mode = loaded.as_ref().map(|a| a.script_mode).unwrap_or(true);
        let agent_settings = loaded
            .as_ref()
            .map(|a| a.agent_settings.clone())
            .unwrap_or_default();
        let key = saved_keys
            .get(&provider)
            .cloned()
            .unwrap_or_else(|| env_key(provider));
        let (tx, rx) = mpsc::channel();
        Self {
            open: false,
            provider,
            endpoint,
            key,
            keys: std::mem::take(&mut saved_keys),
            model,
            models: Vec::new(),
            model_filter: String::new(),
            loading_models: false,
            discovery_id: 0,
            context_start,
            draft,
            messages,
            tx,
            rx,
            id: 0,
            active: None,
            cancel: None,
            error: None,
            status: None,
            path,
            credential_path,
            credential_warning,
            script_mode,
            applied: false,
            bound_project: None,
            tool_activity: Vec::new(),
            agent_settings,
            pending_tools: Default::default(),
            approved_operation: false,
            undo_requested: false,
            revert_requested: None,
            file_changes: Vec::new(),
            pending_notification: None,
            approval_alerted: false,
        }
    }
    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            if fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let archive = Archive {
            model: self.model.clone(),
            messages: self.messages.clone(),
            provider: self.provider,
            endpoint: self.endpoint.clone(),
            context_start: self.context_start,
            draft: self.draft.clone(),
            script_mode: self.script_mode,
            agent_settings: self.agent_settings.clone(),
        };
        if let Ok(json) = serde_json::to_vec_pretty(&archive) {
            let _ = fs::write(&self.path, json);
        }
    }
    fn persist_credentials(&mut self) {
        if self.key.trim().is_empty() {
            self.keys.remove(&self.provider);
        } else {
            self.keys.insert(self.provider, self.key.clone());
        }
        match ai_secure::save(&self.credential_path, &self.keys) {
            Ok(()) => {
                self.credential_warning = None;
            }
            Err(error) => {
                self.credential_warning = Some(error);
            }
        }
    }
    fn sync_project(&mut self, project_path: &Option<PathBuf>) {
        if self.bound_project != *project_path {
            if self.active.is_some() {
                self.stop();
            }
            self.bound_project = project_path.clone();
            self.tool_activity.clear();
            self.file_changes.clear();
            self.revert_requested = None;
            self.undo_requested = false;
            self.context_start = self.messages.len();
            self.save();
        }
    }
    fn update(&mut self) {
        let mut done = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Delta(id, text) if self.active == Some(id) => {
                    if let Some(m) = self.messages.last_mut() {
                        m.content.push_str(&text);
                    }
                }
                Event::Done(id) if self.active == Some(id) => {
                    if let Some(m) = self.messages.last_mut() {
                        m.complete = true;
                    }
                    self.active = None;
                    self.cancel = None;
                    done = true;
                    self.status = Some("Response complete".into());
                    if !self.tool_activity.is_empty() {
                        self.pending_notification =
                            Some(crate::engine::audio::EngineSound::BuildComplete);
                    }
                    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
                    // Edits are tool results, never inferred from fenced assistant text.
                }
                Event::Error(id, error) if self.active == Some(id) => {
                    self.active = None;
                    self.cancel = None;
                    done = true;
                    self.error = Some(error);
                    self.pending_notification =
                        Some(crate::engine::audio::EngineSound::BuildFailed);
                }
                Event::ProjectTool(id, step) if self.active == Some(id) => {
                    self.status = Some(step.clone());
                    self.tool_activity.push(step);
                    if self.tool_activity.len() > 22 {
                        self.tool_activity.remove(0);
                    }
                }
                Event::EditorTool(tool) => {
                    if self.active == Some(tool.id) {
                        self.pending_tools.push_back(tool);
                    } else {
                        let _ = tool
                            .reply
                            .send("ERROR: Request is no longer active.".into());
                    }
                }
                Event::Models(id, result) if id == self.discovery_id => {
                    self.loading_models = false;
                    match result {
                        Ok(models) => {
                            self.status = Some(format!(
                                "Loaded {} models from {}",
                                models.len(),
                                self.provider.label()
                            ));
                            self.error = None;
                            if !models.contains(&self.model) {
                                self.model = models.first().cloned().unwrap_or_default();
                            }
                            self.models = models;
                            self.model_filter.clear();
                            self.context_start = self.messages.len();
                            done = true;
                        }
                        Err(error) => {
                            self.models.clear();
                            self.error = Some(error);
                            self.status = None;
                        }
                    }
                }
                _ => {}
            }
        }
        if done {
            self.save();
        }
    }
    fn config(&self) -> Config {
        Config {
            provider: self.provider,
            base_url: self.endpoint.clone(),
            key: self.key.trim().to_owned(),
            model: self.model.trim().to_owned(),
        }
    }
    fn change_provider(&mut self, new_provider: Provider) {
        if new_provider == self.provider {
            return;
        }
        self.stop();
        self.persist_credentials();
        self.keys
            .insert(self.provider, std::mem::take(&mut self.key));
        self.provider = new_provider;
        self.key = self
            .keys
            .get(&new_provider)
            .cloned()
            .unwrap_or_else(|| env_key(new_provider));
        self.endpoint = new_provider.default_url().to_owned();
        self.models.clear();
        self.model.clear();
        self.model_filter.clear();
        self.loading_models = false;
        self.discovery_id = self.discovery_id.wrapping_add(1);
        self.error = None;
        self.status =
            Some("Provider changed. Earlier chat will not be sent to the new provider.".into());
        self.context_start = self.messages.len();
        self.save();
    }
    fn load_models(&mut self, ctx: &egui::Context) {
        if self.loading_models {
            return;
        }
        let config = self.config();
        if let Err(e) = ai_provider::check_config(&config) {
            self.error = Some(e);
            return;
        }
        self.discovery_id = self.discovery_id.wrapping_add(1);
        let id = self.discovery_id;
        self.loading_models = true;
        self.error = None;
        self.status = Some("Loading provider models...".into());
        self.models.clear();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = ai_provider::list_models(&config);
            let _ = tx.send(Event::Models(id, result));
            ctx.request_repaint();
        });
    }
    fn stop(&mut self) {
        if let Some(flag) = self.cancel.take() {
            flag.store(true, Ordering::Relaxed);
        }
        self.active = None;
        self.approved_operation = false;
        for tool in self.pending_tools.drain(..) {
            let _ = tool.reply.send("ERROR: Operation cancelled.".into());
        }
        self.status = Some("Request stopped".into());
        self.save();
    }
    fn send(
        &mut self,
        ctx: &egui::Context,
        editor: &ScriptEditor,
        project_path: &Option<PathBuf>,
        context: ai_script::SelectionContext,
    ) {
        if self.active.is_some() || self.draft.trim().is_empty() {
            return;
        }
        let config = self.config();
        if let Err(e) = ai_provider::check_config(&config) {
            self.error = Some(e);
            return;
        }
        if !model_ok(&config.model) {
            self.error = Some("Select a valid model in Settings > AI.".into());
            return;
        }
        if self.draft.chars().count() > 16_000 {
            self.error = Some("Message exceeds 16,000 characters.".into());
            return;
        }
        self.error = None;
        self.status = None;
        self.tool_activity.clear();
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
        self.approved_operation = false;
        self.approval_alerted = false;
        self.pending_notification = None;
        let access = match project_path.as_deref().map(ProjectAccess::open) {
            Some(Ok(access)) => Some(access),
            Some(Err(error)) => {
                self.error = Some(error);
                return;
            }
            None => None,
        };
        self.messages.push(Message {
            role: "user".into(),
            content: self.draft.trim().into(),
            complete: true,
            script_draft: false,
        });
        self.draft.clear();
        let mut turns: Vec<_> = self.messages[self.context_start.min(self.messages.len())..]
            .iter()
            .filter(|m| m.role == "user" || (m.role == "assistant" && m.complete))
            .rev()
            .take(24)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
        // Include only selection identifiers. Source is obtained via live tools.
        if let Some(last) = turns.last_mut() {
            let active_path = editor
                .active_document
                .as_ref()
                .and_then(|p| {
                    project_path
                        .as_ref()
                        .and_then(|root| p.strip_prefix(root).ok())
                })
                .map(|p| p.to_string_lossy().replace('\\', "/"));
            last.1 = format!(
                "EDITOR CONTEXT (data): selected Actor {:?}; Cell selected {}; active script {:?}.\nUSER REQUEST:\n{}",
                context.actor.map(|a| a.0),
                context.cell_selected,
                active_path,
                last.1
            );
        }
        self.messages.push(Message {
            role: "assistant".into(),
            content: String::new(),
            complete: false,
            script_draft: false,
        });
        self.save();
        self.id = self.id.wrapping_add(1);
        let id = self.id;
        self.active = Some(id);
        let flag = Arc::new(AtomicBool::new(false));
        self.cancel = Some(flag.clone());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let project = project_path.clone();
        let settings = self.agent_settings.clone();
        std::thread::spawn(move || {
            request(ctx, tx, id, config, turns, flag, access, project, settings)
        });
    }
    pub fn draw_settings(&mut self, ui: &mut egui::Ui, project_path: &Option<PathBuf>) -> bool {
        let previous_model = self.model.clone();
        self.update();
        self.sync_project(project_path);
        ui.heading("AI");
        ui.label("Configure an AI service for Worldkiln's chat assistant.");
        ui.add_space(10.0);
        ui.label(RichText::new("PROVIDER").strong());
        let old = self.provider;
        egui::ComboBox::from_id_salt("worldkiln_ai_provider")
            .selected_text(self.provider.label())
            .show_ui(ui, |ui| {
                for provider in Provider::ALL {
                    ui.selectable_value(&mut self.provider, provider, provider.label());
                }
            });
        if self.provider != old {
            let new = self.provider;
            self.provider = old;
            self.change_provider(new);
        }
        if matches!(self.provider, Provider::Compatible | Provider::Ollama) {
            ui.add_space(6.0);
            ui.label("API base URL (ending in /v1):");
            let response = ui.add_sized(
                [500.0, 24.0],
                egui::TextEdit::singleline(&mut self.endpoint),
            );
            if response.changed() {
                self.models.clear();
                self.loading_models = false;
                self.discovery_id = self.discovery_id.wrapping_add(1);
                self.context_start = self.messages.len();
                self.save();
            }
            ui.small("HTTPS is required except for localhost. OpenAI-compatible services must implement /models and /chat/completions.");
        }
        ui.add_space(10.0);
        ui.label(RichText::new("CREDENTIALS").strong());
        if !self.provider.needs_key() {
            ui.label("Local Ollama usually requires no API key.");
        }
        let mut key_changed = false;
        ui.horizontal(|ui| {
            key_changed = ui
                .add_sized(
                    [470.0, 25.0],
                    egui::TextEdit::singleline(&mut self.key)
                        .password(true)
                        .hint_text(if self.provider.needs_key() {
                            "Paste provider API key"
                        } else {
                            "Optional key"
                        }),
                )
                .changed();
            if ui.button("Clear key").clicked() {
                self.key.clear();
                key_changed = true;
            }
        });
        if key_changed {
            self.persist_credentials();
            self.loading_models = false;
            self.discovery_id = self.discovery_id.wrapping_add(1);
            self.models.clear();
            self.status = None;
            self.error = None;
        }
        ui.small("API key is saved with Windows user-account encryption (DPAPI); never in project or chat files.");
        if let Some(warning) = &self.credential_warning {
            ui.colored_label(Color32::YELLOW, format!("Credential storage: {warning}"));
        }
        ui.add_space(10.0);
        ui.label(RichText::new("AVAILABLE MODELS").strong());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.loading_models,
                    egui::Button::new(if self.loading_models {
                        "Loading..."
                    } else {
                        "Load Models"
                    }),
                )
                .clicked()
            {
                self.load_models(ui.ctx());
            }
            if self.loading_models {
                ui.spinner();
            }
            ui.small("Fetches the models returned by your current provider/key.");
        });
        if !self.models.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Model");
                egui::ComboBox::from_id_salt("worldkiln_ai_loaded_models")
                    .selected_text(if self.model.is_empty() {
                        "Choose model"
                    } else {
                        &self.model
                    })
                    .width(350.0)
                    .height(280.0)
                    .show_ui(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.model_filter)
                                .hint_text("Filter models..."),
                        );
                        let filter = self.model_filter.to_lowercase();
                        for model in &self.models {
                            if filter.is_empty() || model.to_lowercase().contains(&filter) {
                                if ui
                                    .selectable_value(&mut self.model, model.clone(), model)
                                    .clicked()
                                {
                                    self.context_start = self.messages.len();
                                }
                            }
                        }
                    });
                ui.label(format!("{} found", self.models.len()));
            });
        }
        ui.label("Or specify a custom text-chat model ID:");
        let changed = ui
            .add_sized([470.0, 24.0], egui::TextEdit::singleline(&mut self.model))
            .changed();
        if changed || self.model != previous_model {
            self.context_start = self.messages.len();
            self.save();
        }
        ui.add_space(16.0);
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
        ui.label(RichText::new("AGENT ACCESS").strong());
        let before = self.agent_settings.clone();
        for access in [
            AgentAccess::AskBeforeChanges,
            AgentAccess::AutoApply,
            AgentAccess::FullAccess,
        ] {
            ui.radio_value(&mut self.agent_settings.access, access, access.label());
        }
        ui.small(match self.agent_settings.access {
            AgentAccess::AskBeforeChanges => "Inspect context freely; approve changes for a complete operation.",
            AgentAccess::AutoApply => "Create, edit and save scripts and supported project resources automatically.",
            AgentAccess::FullAccess => "Use all exposed Worldkiln tools without individual approvals, including agent undo.",
        });
        ui.small("Project excerpts requested by tools are shared with your selected provider. Full Access is limited to Worldkiln capabilities.");
        ui.add_space(10.0);
        ui.label(RichText::new("VALIDATION").strong());
        ui.checkbox(
            &mut self.agent_settings.parser_first,
            "Run code through parser first",
        );
        ui.add_enabled(
            self.agent_settings.parser_first,
            egui::Checkbox::new(
                &mut self.agent_settings.auto_correct,
                "Automatically correct errors",
            ),
        );
        ui.horizontal(|ui| {
            ui.label("Maximum correction attempts");
            ui.add_enabled(
                self.agent_settings.parser_first && self.agent_settings.auto_correct,
                egui::DragValue::new(&mut self.agent_settings.max_corrections).range(0..=10),
            );
        });
        if self.agent_settings != before {
            self.stop(); // Settings changes revoke any in-flight grant and validation policy.
            self.save();
        }
        ui.add_space(12.0);
        let open_chat = ui.button("Open AI Chat").clicked();
        if open_chat {
            self.open = true;
        }
        if let Some(error) = &self.error {
            ui.colored_label(Color32::from_rgb(240, 100, 95), error);
        }
        if let Some(status) = &self.status {
            ui.small(status);
        }
        open_chat
    }
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
    pub(crate) fn take_notification(&mut self) -> Option<crate::engine::audio::EngineSound> {
        self.pending_notification.take()
    }
    pub fn take_applied(&mut self) -> bool {
        std::mem::take(&mut self.applied)
    }
    pub(crate) fn next_tool(&mut self, project: &Option<PathBuf>) -> Option<EditorToolRequest> {
        self.sync_project(project);
        self.update();
        while let Some(tool) = self.pending_tools.front() {
            if self.active != Some(tool.id) || project.as_ref() != Some(&tool.project) {
                let tool = self.pending_tools.pop_front().unwrap();
                let _ = tool
                    .reply
                    .send("ERROR: Project or active request changed.".into());
                continue;
            }
            if self.agent_settings.access == AgentAccess::AskBeforeChanges
                && capability(&tool.name).is_some_and(|c| c != Capability::Read)
                && !self.approved_operation
            {
                self.status = Some("Awaiting approval for this operation".into());
                if !self.approval_alerted {
                    self.pending_notification = Some(crate::engine::audio::EngineSound::Warning);
                    self.approval_alerted = true;
                }
                return None;
            }
            return self.pending_tools.pop_front();
        }
        None
    }
    pub(crate) fn tool_permitted(&self, tool: &EditorToolRequest) -> bool {
        self.active == Some(tool.id)
            && self.bound_project.as_ref() == Some(&tool.project)
            && self
                .cancel
                .as_ref()
                .is_some_and(|flag| !flag.load(Ordering::Relaxed))
            && permitted(
                self.agent_settings.access,
                &tool.name,
                self.approved_operation,
            )
    }
    pub(crate) fn record_tool_result(&mut self, name: &str, result: &str) {
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:26 -05:00
        if result.starts_with("ERROR:") {
            self.error = Some(result.to_string());
        }
        self.status = Some(format!("{name}: {result}"));
        self.tool_activity.push(format!("{name}: {result}"));
        if self.tool_activity.len() > 48 {
            self.tool_activity.remove(0);
        }
        self.applied = name == "write_script" && !result.starts_with("ERROR:");
    }
    fn formatted(ui: &mut egui::Ui, text: &str) {
        let mut code = false;
        let mut buffer = String::new();
        for line in text.lines() {
            if line.as_bytes().starts_with(&[96, 96, 96]) {
                if !buffer.is_empty() {
                    Self::block(ui, &buffer, code);
                    buffer.clear();
                }
                code = !code;
            } else {
                buffer.push_str(line);
                buffer.push('\n');
            }
        }
        if !buffer.is_empty() {
            Self::block(ui, &buffer, code);
        }
    }
    fn block(ui: &mut egui::Ui, text: &str, code: bool) {
        if code {
            ui.horizontal(|ui| {
                ui.label(RichText::new("CODE").small().color(Color32::GRAY));
                if ui.small_button("Copy code").clicked() {
                    ui.ctx().copy_text(text.to_owned());
                }
            });
            egui::Frame::group(ui.style()).show(ui, |ui| {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.label(RichText::new(text).monospace());
                });
            });
        } else {
            ui.label(text.trim_end());
        }
    }
    pub fn draw(
        &mut self,
        ui: &mut egui::Ui,
        editor: &mut ScriptEditor,
        project_path: &Option<PathBuf>,
        context: ai_script::SelectionContext,
        _hierarchy: &ProjectHierarchy,
    ) -> bool {
        self.update();
        self.sync_project(project_path);
        let mut open_settings = false;
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:37 -05:00
        // Narrow docks wrap the header; review/approval details share the chat's
        // bounded scroll viewport instead of overflowing into the pinned composer.
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("WORLDKILN AI").strong().monospace());
            if ui.small_button("Settings").clicked() {
                open_settings = true;
            }
            if ui.small_button("New chat").clicked() {
                self.stop();
                self.messages.clear();
                self.draft.clear();
                self.context_start = 0;
                self.error = None;
                self.status = None;
                self.save();
            }
        });
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(240, 100, 95), e);
        }
        if let Some(s) = &self.status {
            if self.active.is_some() || s.starts_with("AI ") {
                ui.small(s);
            }
        }
        ui.separator();
        // Worldkiln 0.8.5 | WO 0.8.7-76 | 2026-10-09 22:50 -05:00
        // Reserve the composer at the bottom FIRST; chat receives the remaining
        // height and scrolls independently, even when the right dock is resized.
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:39 -05:00
        // Reserve its complete first-frame height, including the optional Stop row.
        let composer_height = (ui.text_style_height(&egui::TextStyle::Body) * 2.0).max(48.0)
            + 18.0
            + if self.active.is_some() {
                ui.spacing().interact_size.y + ui.spacing().item_spacing.y
            } else {
                0.0
            };
        egui::TopBottomPanel::bottom("worldkiln_ai_composer")
            .resizable(false)
            .min_height(composer_height)
            .default_height(composer_height)
            .show_inside(ui, |ui| {
                if self.active.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        if ui.small_button("Stop").clicked() {
                            self.stop();
                        }
                    });
                }
                let can_send = composer_can_send(&self.draft, self.active.is_some());
                let mut send_hotkey = false;
                let mut send_clicked = false;
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let button_width = 30.0;
                            let draft_width =
                                (ui.available_width() - button_width - ui.spacing().item_spacing.x)
                                    .max(50.0);
                            let response = ui.add_sized(
                                [draft_width, 48.0],
                                // Worldkiln 0.8.5 | WO 0.8.7-77 | 2026-10-09 22:50 -05:00
                                // Reserve Shift+Enter for newlines; bare Enter now submits.
                                egui::TextEdit::multiline(&mut self.draft)
                                    .frame(false)
                                    .desired_rows(2)
                                    .return_key(egui::KeyboardShortcut::new(
                                        egui::Modifiers::SHIFT,
                                        egui::Key::Enter,
                                    ))
                                    .hint_text("Message Worldkiln AI... (Enter)"),
                            );
                            if response.changed() {
                                self.save();
                            }
                            send_hotkey = response.has_focus()
                                && ui.input(|i| {
                                    i.key_pressed(egui::Key::Enter)
                                        && composer_enter_sends(i.modifiers)
                                });
                            // Selection fill is the user-configured ThemeSettings accent.
                            let accent = ui.visuals().selection.bg_fill;
                            let on_accent = ui.visuals().selection.stroke.color;
                            send_clicked = ui
                                .add_enabled(
                                    can_send,
                                    egui::Button::new(
                                        RichText::new("↑").size(18.0).strong().color(on_accent),
                                    )
                                    .fill(accent)
                                    .min_size(egui::vec2(button_width, 30.0)),
                                )
                                .on_hover_text("Send message (Enter)")
                                .clicked();
                        });
                    });
                if can_send && (send_clicked || send_hotkey) {
                    self.send(ui.ctx(), editor, project_path, context);
                }
            });
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                if self.messages.is_empty() {
                    ui.label("Ask Worldkiln AI a question.");
                }
                for (index, msg) in self.messages.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let color = if msg.role == "user" {
                                    Color32::from_rgb(235, 150, 65)
                                } else {
                                    Color32::LIGHT_GRAY
                                };
                                ui.label(
                                    RichText::new(if msg.role == "user" { "YOU" } else { "AI" })
                                        .strong()
                                        .color(color),
                                );
                                if ui.small_button("Copy").clicked() {
                                    ui.ctx().copy_text(msg.content.clone());
                                }
                            });
                            if msg.content.is_empty() {
                                ui.spinner();
                            } else {
                                Self::formatted(ui, &msg.content);
                            }
                        });
                        ui.add_space(6.0);
                    });
                }
        ui.separator();
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
        if !self.file_changes.is_empty() {
            let added: usize = self.file_changes.iter().map(|c| c.added).sum();
            let removed: usize = self.file_changes.iter().map(|c| c.removed).sum();
            ui.collapsing(
                format!(
                    "{} files changed  +{} −{}",
                    self.file_changes.len(),
                    added,
                    removed
                ),
                |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            for change in &mut self.file_changes {
                                ui.group(|ui| {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(RichText::new(&change.path).strong());
                                        ui.colored_label(
                                            Color32::LIGHT_GREEN,
                                            format!("+{}", change.added),
                                        );
                                        ui.colored_label(
                                            Color32::LIGHT_RED,
                                            format!("−{}", change.removed),
                                        );
                                    });
                                    if change.reverted {
                                        ui.small("Reverted");
                                    } else if change.kept {
                                        ui.small("Kept");
                                    } else {
                                        ui.horizontal(|ui| {
                                            if ui.small_button("Keep").clicked() {
                                                change.kept = true;
                                            }
                                            if ui
                                                .add_enabled(
                                                    self.active.is_none(),
                                                    egui::Button::new("Revert").small(),
                                                )
                                                .clicked()
                                            {
                                                self.revert_requested = Some(change.id);
                                                ui.ctx().request_repaint();
                                            }
                                        });
                                    }
                                    ui.collapsing("View changes", |ui| {
                                        ui.label("Before");
                                        Self::block(ui, &change.before, true);
                                        ui.label("After");
                                        Self::block(ui, &change.after, true);
                                    });
                                });
                            }
                        });
                },
            );
        }
        if let Some(tool) = self.pending_tools.front() {
            if self.agent_settings.access == AgentAccess::AskBeforeChanges
                && capability(&tool.name).is_some_and(|c| c != Capability::Read)
                && !self.approved_operation
            {
                ui.label("The agent requests changes for this operation:");
                ui.label(RichText::new(&tool.name).strong());
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        ui.label(RichText::new(tool.args.to_string()).monospace());
                    });
                ui.small("Approval covers this user request's supported editor changes. Stop revokes it.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Approve operation").clicked() {
                        self.approved_operation = true;
                        ui.ctx().request_repaint();
                    }
                    if ui.button("Reject operation").clicked() {
                        self.stop();
                    }
                });
            }
        }
        if !self.tool_activity.is_empty() {
            ui.collapsing("Activity and change details", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        for step in &self.tool_activity {
                            ui.small(step);
                        }
                    });
                if ui
                    .add_enabled(
                        self.active.is_none(),
                        egui::Button::new("Undo latest agent change"),
                    )
                    .clicked()
                {
                    self.undo_requested = true;
                    ui.ctx().request_repaint();
                }
            });
        }
            });
        open_settings
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    // Worldkiln 0.8.5 | WO 0.8.7-76 | 2026-10-09 22:50 -05:00
    #[test]
    fn composer_rejects_empty_and_busy_submissions() {
        assert!(!composer_can_send("", false));
        assert!(!composer_can_send("  \n  ", false));
        assert!(composer_can_send("Where is the script?", false));
        assert!(!composer_can_send("Where is the script?", true));
    }
    #[test]
    fn model_ids_are_safely_validated() {
        assert!(model_ok("gemini-3.8-flash"));
        assert!(!model_ok("bad\nHeader: secret"));
    }
    #[test]
    fn saved_archive_round_trips_draft_and_authoring_mode() {
        let archive = Archive {
            provider: Provider::Gemini,
            endpoint: Provider::Gemini.default_url().into(),
            model: "gemini-flash".into(),
            messages: vec![Message {
                role: "user".into(),
                content: "Hello".into(),
                complete: true,
                script_draft: false,
            }],
            context_start: 0,
            draft: "unfinished prompt".into(),
            script_mode: false,
            agent_settings: AgentSettings::default(),
        };
        let encoded = serde_json::to_vec(&archive).unwrap();
        let decoded: Archive = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.draft, "unfinished prompt");
        assert!(!decoded.script_mode);
        assert_eq!(decoded.messages[0].content, "Hello");
    }
    #[test]
    fn archives_do_not_store_keys() {
        let a = Archive {
            provider: Provider::Gemini,
            endpoint: String::new(),
            model: String::new(),
            messages: Vec::new(),
            context_start: 0,
            draft: String::new(),
            script_mode: true,
            agent_settings: AgentSettings::default(),
        };
        assert!(!serde_json::to_string(&a).unwrap().contains("key"));
    }
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:15 -05:00
    #[test]
    fn settings_persist_and_legacy_archives_use_safe_defaults() {
        let legacy = json_archive();
        let mut a: Archive = serde_json::from_value(legacy).unwrap();
        assert_eq!(a.agent_settings, AgentSettings::default());
        for access in [
            AgentAccess::AskBeforeChanges,
            AgentAccess::AutoApply,
            AgentAccess::FullAccess,
        ] {
            a.agent_settings = AgentSettings {
                access,
                parser_first: false,
                auto_correct: false,
                max_corrections: 5,
            };
            let decoded: Archive =
                serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
            assert_eq!(decoded.agent_settings, a.agent_settings);
        }
    }
    fn json_archive() -> serde_json::Value {
        serde_json::json!({"model":"test","messages":[],"provider":"Gemini","endpoint":"","draft":""})
    }
    #[test]
    fn operation_approval_and_cancellation_are_request_scoped() {
        let mut chat = AiChat::new();
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:38 -05:00
        // This fixture must not inherit the installed user's access preference.
        chat.agent_settings = AgentSettings::default();
        let root = std::env::temp_dir().join(format!("wo79_chat_{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        chat.path = root.join("isolated-chat.json");
        chat.bound_project = Some(root.clone());
        chat.active = Some(40);
        chat.cancel = Some(Arc::new(AtomicBool::new(false)));
        let (reply, receiver) = mpsc::channel();
        chat.pending_tools.push_back(EditorToolRequest {
            id: 40,
            project: root.clone(),
            name: "write_script".into(),
            args: serde_json::json!({}),
            reply,
        });
        assert!(chat.next_tool(&Some(root.clone())).is_none());
        chat.approved_operation = true;
        let tool = chat.next_tool(&Some(root.clone())).unwrap();
        assert!(chat.tool_permitted(&tool));
        chat.pending_tools.push_back(tool);
        chat.stop();
        assert!(receiver.recv().unwrap().contains("cancelled"));
        assert!(!chat.approved_operation);
        chat.active = Some(41);
        chat.approved_operation = true;
        let (reply, receiver) = mpsc::channel();
        chat.tx
            .send(Event::EditorTool(EditorToolRequest {
                id: 40,
                project: root.clone(),
                name: "write_script".into(),
                args: serde_json::json!({}),
                reply,
            }))
            .unwrap();
        chat.update();
        assert!(receiver.recv().unwrap().contains("no longer active"));
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:38 -05:00
    #[test]
    fn narrow_chat_details_never_overlap_pinned_composer_or_stop_controls() {
        for (width, height) in [(220.0, 280.0), (310.0, 680.0), (600.0, 400.0)] {
            for busy in [false, true] {
                let mut chat = AiChat::new();
                chat.bound_project = None;
                chat.agent_settings = AgentSettings::default();
                chat.error = None;
                chat.status = None;
                chat.draft.clear();
                chat.messages = vec![Message {
                    role: "user".into(),
                    content: "A long conversation line that wraps in a narrow dock.\n".repeat(30),
                    complete: true,
                    script_draft: false,
                }];
                chat.tool_activity = vec!["Saved scripts/hello.aeo".into()];
                chat.file_changes = vec![FileChangeView {
                    id: 1,
                    path: "scripts/hello.aeo".into(),
                    added: 1,
                    removed: 0,
                    kept: false,
                    reverted: false,
                    before: String::new(),
                    after: "debug.log(1)".into(),
                }];
                chat.active = busy.then_some(1);
                let (reply, _receiver) = mpsc::channel();
                if busy {
                    chat.pending_tools.push_back(EditorToolRequest {
                        id: 1,
                        project: PathBuf::new(),
                        name: "write_script".into(),
                        args: serde_json::json!({"path":"scripts/hello.aeo"}),
                        reply,
                    });
                }
                let mut editor = ScriptEditor::new();
                let hierarchy = ProjectHierarchy::default();
                let ctx = egui::Context::default();
                // A second frame applies stick-to-bottom to the overflowing conversation.
                for _ in 0..2 {
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                chat.draw(
                                    ui,
                                    &mut editor,
                                    &None,
                                    ai_script::SelectionContext::default(),
                                    &hierarchy,
                                );
                            });
                        },
                    );
                    let labels: Vec<_> = output
                        .shapes
                        .iter()
                        .filter_map(|painted| {
                            if let egui::Shape::Text(text) = &painted.shape {
                                Some((
                                    text.galley.job.text.as_str(),
                                    text.visual_bounding_rect().intersect(painted.clip_rect),
                                ))
                            } else {
                                None
                            }
                        })
                        .collect();
                    let composer = labels
                        .iter()
                        .find(|(s, _)| s.starts_with("Message Worldkiln AI"))
                        .unwrap_or_else(|| panic!("pinned composer missing at {width}x{height}, busy={busy}: {labels:?}"))
                        .1;
                    for (text, rect) in &labels {
                        if text.contains("files changed")
                            || *text == "Activity and change details"
                            || *text == "Approve operation"
                            || *text == "Reject operation"
                            || *text == "Stop"
                        {
                            assert!(
                                !rect.intersects(composer),
                                "{width}x{height}, busy={busy}: {text} overlaps composer"
                            );
                        }
                    }
                    if busy {
                        let stop = labels
                            .iter()
                            .find(|(s, _)| *s == "Stop")
                            .expect("Stop remains visible")
                            .1;
                        for (text, rect) in &labels {
                            if *text == "Activity and change details"
                                || text.contains("files changed")
                            {
                                assert!(
                                    !rect.intersects(stop),
                                    "{text} overlaps Stop at {width}x{height}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
