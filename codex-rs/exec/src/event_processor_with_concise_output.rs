use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use codex_common::create_config_summary_entries;
use codex_common::elapsed::format_duration;
use codex_common::elapsed::format_elapsed;
use codex_core::config::Config;
use codex_core::plan_tool::StepStatus;
use codex_core::plan_tool::UpdatePlanArgs;
use codex_core::protocol::AgentMessageEvent;
use codex_core::protocol::BackgroundEventEvent;
use codex_core::protocol::ErrorEvent;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::ExecCommandBeginEvent;
use codex_core::protocol::ExecCommandEndEvent;
use codex_core::protocol::PatchApplyBeginEvent;
use codex_core::protocol::PatchApplyEndEvent;
use codex_core::protocol::SessionConfiguredEvent;
use codex_core::protocol::StreamErrorEvent;
use codex_core::protocol::TaskCompleteEvent;
use codex_core::protocol::TokenCountEvent;
use codex_core::protocol::TokenUsageInfo;
use owo_colors::OwoColorize;
use owo_colors::Style;
use shlex::try_join;

use crate::event_processor::CodexStatus;
use crate::event_processor::EventProcessor;
use crate::event_processor::handle_last_message;
use crate::transcript_log::TranscriptLog;

pub(crate) struct EventProcessorWithConciseOutput {
    transcript_log: Option<TranscriptLog>,
    call_id_to_command: HashMap<String, ExecCommandBegin>,
    call_id_to_patch: HashMap<String, PatchApplyBegin>,
    status_style: Style,
    success_style: Style,
    error_style: Style,
    info_style: Style,
    timestamp_style: Style,
    last_message_path: Option<PathBuf>,
    latest_token_usage: Option<TokenUsageInfo>,
}

impl EventProcessorWithConciseOutput {
    pub(crate) fn new(
        with_ansi: bool,
        last_message_path: Option<PathBuf>,
        transcript_log: Option<TranscriptLog>,
    ) -> Self {
        let (status_style, success_style, error_style, info_style, timestamp_style) = if with_ansi {
            (
                Style::new().bold(),
                Style::new().green(),
                Style::new().red(),
                Style::new().cyan(),
                Style::new().dimmed(),
            )
        } else {
            (
                Style::new(),
                Style::new(),
                Style::new(),
                Style::new(),
                Style::new(),
            )
        };

        Self {
            transcript_log,
            call_id_to_command: HashMap::new(),
            call_id_to_patch: HashMap::new(),
            status_style,
            success_style,
            error_style,
            info_style,
            timestamp_style,
            last_message_path,
            latest_token_usage: None,
        }
    }

    fn emit_status(&mut self, message: impl AsRef<str>, style: Style) {
        let message = message.as_ref();
        let timestamp = chrono::Utc::now().format("[%Y-%m-%dT%H:%M:%S]").to_string();
        if let Some(log) = &mut self.transcript_log {
            log.write_line(&format!("{timestamp} {message}"));
        }
        let styled_prefix = timestamp.style(self.timestamp_style);
        println!("{styled_prefix} {}", message.style(style));
    }

    fn emit_plain_line(&mut self, message: impl AsRef<str>) {
        let message = message.as_ref();
        if let Some(log) = &mut self.transcript_log {
            log.write_line(message);
        }
        println!("{message}");
    }

    fn emit_multiline(&mut self, message: &str) {
        for line in message.lines() {
            self.emit_plain_line(line);
        }
    }

    fn handle_exec_begin(&mut self, ev: ExecCommandBeginEvent) {
        let ExecCommandBeginEvent {
            call_id, command, ..
        } = ev;
        self.call_id_to_command.insert(
            call_id,
            ExecCommandBegin {
                command: command.clone(),
                start_time: Instant::now(),
            },
        );
        let escaped = escape_command(&command);
        self.emit_status(format!("Running command: {escaped}"), self.status_style);
    }

    fn handle_exec_end(&mut self, ev: ExecCommandEndEvent) {
        let ExecCommandEndEvent {
            call_id,
            exit_code,
            duration,
            ..
        } = ev;

        let (command, started_at) = match self.call_id_to_command.remove(&call_id) {
            Some(ExecCommandBegin {
                command,
                start_time,
            }) => (escape_command(&command), Some(start_time)),
            None => (format!("command {call_id}"), None),
        };

        let duration_str = format_duration(duration);
        let elapsed_str = started_at.map(format_elapsed);
        let suffix = elapsed_str
            .filter(|runtime| !runtime.is_empty())
            .unwrap_or(duration_str);

        if exit_code == 0 {
            self.emit_status(
                format!("Command succeeded (exit {exit_code}, {suffix}): {command}"),
                self.success_style,
            );
        } else {
            self.emit_status(
                format!("Command failed (exit {exit_code}, {suffix}): {command}"),
                self.error_style,
            );
        }
    }

    fn handle_patch_begin(&mut self, ev: PatchApplyBeginEvent) {
        let PatchApplyBeginEvent {
            call_id,
            auto_approved,
            changes,
        } = ev;

        self.call_id_to_patch.insert(
            call_id,
            PatchApplyBegin {
                start_time: Instant::now(),
                auto_approved,
            },
        );

        let file_count = changes.len();
        let approval = if auto_approved {
            "auto-approved"
        } else {
            "awaiting approval"
        };
        self.emit_status(
            format!("Applying patch ({approval}, {file_count} files)"),
            self.status_style,
        );
    }

    fn handle_patch_end(&mut self, ev: PatchApplyEndEvent) {
        let PatchApplyEndEvent {
            call_id, success, ..
        } = ev;

        let (auto_approved, start_time) = match self.call_id_to_patch.remove(&call_id) {
            Some(PatchApplyBegin {
                start_time,
                auto_approved,
            }) => (auto_approved, Some(start_time)),
            None => (false, None),
        };

        let duration = start_time.map(format_elapsed);
        let approval = if auto_approved {
            "auto-approved"
        } else {
            "manual"
        };
        let summary = match (success, duration) {
            (true, Some(dur)) => format!("Patch applied ({approval}, {dur})"),
            (true, None) => format!("Patch applied ({approval})"),
            (false, Some(dur)) => format!("Patch failed ({approval}, {dur})"),
            (false, None) => format!("Patch failed ({approval})"),
        };

        let style = if success {
            self.success_style
        } else {
            self.error_style
        };
        self.emit_status(summary, style);
    }

    fn handle_plan_update(&mut self, plan: UpdatePlanArgs) {
        let mut summary = Vec::new();
        for item in plan.plan {
            let marker = match item.status {
                StepStatus::Completed => "✓",
                StepStatus::InProgress => "→",
                StepStatus::Pending => "•",
            };
            summary.push(format!("{marker} {step}", step = item.step));
        }
        self.emit_status("Plan update", self.info_style);
        if let Some(explanation) = plan.explanation
            && !explanation.trim().is_empty()
        {
            self.emit_plain_line(explanation);
        }
        for line in summary {
            self.emit_plain_line(line);
        }
    }

    fn handle_token_count(&mut self, event: TokenCountEvent) {
        if let Some(info) = event.info {
            self.latest_token_usage = Some(info);
        }
    }

    fn emit_final_token_usage(&mut self) {
        if let Some(info) = self.latest_token_usage.take() {
            let total = info.total_token_usage.blended_total();
            self.emit_status(format!("Total tokens used: {total}"), self.info_style);
        }
    }
}

impl EventProcessor for EventProcessorWithConciseOutput {
    fn print_config_summary(
        &mut self,
        config: &Config,
        prompt: &str,
        session_configured: &SessionConfiguredEvent,
    ) {
        const VERSION: &str = env!("CARGO_PKG_VERSION");
        self.emit_status(
            format!("Codex (v{VERSION}) non-interactive session"),
            self.status_style,
        );

        let SessionConfiguredEvent {
            session_id, model, ..
        } = session_configured;
        self.emit_status(
            format!("Session {session_id} using model {model}"),
            self.info_style,
        );

        self.emit_status(format!("model: {model}"), self.info_style);

        for (key, value) in create_config_summary_entries(config) {
            if key == "sandbox" {
                self.emit_status(format!("{key}: {value}"), self.info_style);
            }
        }

        self.emit_status(
            format!("Working directory: {}", config.cwd.display()),
            self.info_style,
        );

        self.emit_status("Prompt:", self.status_style);
        self.emit_multiline(prompt);
    }

    fn process_event(&mut self, event: Event) -> CodexStatus {
        match event.msg {
            EventMsg::Error(ErrorEvent { message }) => {
                self.emit_status(format!("Error: {message}"), self.error_style);
            }
            EventMsg::BackgroundEvent(BackgroundEventEvent { .. }) => {
                // Ignore background events in concise mode.
            }
            EventMsg::StreamError(StreamErrorEvent { message }) => {
                self.emit_status(format!("Stream error: {message}"), self.error_style);
            }
            EventMsg::TaskStarted(_) => {}
            EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                if let Some(output_file) = self.last_message_path.as_deref() {
                    handle_last_message(last_agent_message.as_deref(), output_file);
                }
                match last_agent_message {
                    Some(message) if !message.trim().is_empty() => {
                        self.emit_status("Final result:", self.status_style);
                        self.emit_multiline(&message);
                    }
                    _ => {
                        self.emit_status("Task complete (no final message).", self.status_style);
                    }
                }
                self.emit_final_token_usage();
                return CodexStatus::InitiateShutdown;
            }
            EventMsg::TokenCount(event) => {
                self.handle_token_count(event);
            }
            EventMsg::ExecCommandBegin(ev) => {
                self.handle_exec_begin(ev);
            }
            EventMsg::ExecCommandEnd(ev) => {
                self.handle_exec_end(ev);
            }
            EventMsg::ExecCommandOutputDelta(_) => {
                // Suppress noisy incremental output in concise mode.
            }
            EventMsg::PatchApplyBegin(ev) => {
                self.handle_patch_begin(ev);
            }
            EventMsg::PatchApplyEnd(ev) => {
                self.handle_patch_end(ev);
            }
            EventMsg::TurnDiff(_) => {}
            EventMsg::ExecApprovalRequest(_) => {}
            EventMsg::ApplyPatchApprovalRequest(_) => {}
            EventMsg::AgentReasoning(_) => {}
            EventMsg::AgentReasoningRawContent(_) => {}
            EventMsg::AgentReasoningDelta(_) => {}
            EventMsg::AgentReasoningRawContentDelta(_) => {}
            EventMsg::AgentReasoningSectionBreak(_) => {}
            EventMsg::AgentMessageDelta(_) => {}
            EventMsg::AgentMessage(AgentMessageEvent { message }) => {
                if !message.trim().is_empty() {
                    self.emit_status("Agent message:", self.info_style);
                    self.emit_multiline(&message);
                }
            }
            EventMsg::McpToolCallBegin(_) => {}
            EventMsg::McpToolCallEnd(_) => {}
            EventMsg::WebSearchBegin(_) => {}
            EventMsg::WebSearchEnd(_) => {}
            EventMsg::SessionConfigured(_) => {}
            EventMsg::PlanUpdate(event) => {
                self.handle_plan_update(event);
            }
            EventMsg::GetHistoryEntryResponse(_) => {}
            EventMsg::McpListToolsResponse(_) => {}
            EventMsg::ListCustomPromptsResponse(_) => {}
            EventMsg::TurnAborted(_) => {
                self.emit_status("Task aborted", self.error_style);
            }
            EventMsg::ShutdownComplete => return CodexStatus::Shutdown,
            EventMsg::ConversationPath(_) => {}
            EventMsg::UserMessage(_) => {}
            EventMsg::EnteredReviewMode(_) => {}
            EventMsg::ExitedReviewMode(_) => {}
        }

        CodexStatus::Running
    }
}

struct ExecCommandBegin {
    command: Vec<String>,
    start_time: Instant,
}

struct PatchApplyBegin {
    start_time: Instant,
    auto_approved: bool,
}

fn escape_command(command: &[String]) -> String {
    try_join(command.iter().map(String::as_str)).unwrap_or_else(|_| command.join(" "))
}
