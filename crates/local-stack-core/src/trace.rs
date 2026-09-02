use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{fs, io::AsyncWriteExt};

use crate::{GpuSnapshot, OllamaClient, Result, RunningModel, StackError, inspect_environment};

const STREAM_EVENT_TYPES: &[&str] = &[
    "assistant/chunk",
    "reasoning-chunks",
    "text-chunks",
    "tool-call-chunks",
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSessionSummary {
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub preset: Option<String>,
    pub model: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub event_count: usize,
    pub branch_count: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceReplay {
    pub session: TraceSessionSummary,
    pub source: String,
    pub max_slots: usize,
    pub start_time: u64,
    pub end_time: u64,
    pub branches: Vec<TraceBranch>,
    pub events: Vec<TraceEvent>,
    pub edges: Vec<TraceEdge>,
    pub leases: Vec<TraceLease>,
    pub telemetry: Vec<TraceTelemetrySample>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceBranch {
    pub id: String,
    pub parent_id: Option<String>,
    pub label: String,
    pub role: String,
    pub context_id: String,
    pub forked: bool,
    pub created_at: u64,
    pub removed_at: Option<u64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEvent {
    pub id: String,
    pub index: usize,
    pub time: u64,
    pub sequence: Option<u64>,
    pub branch_id: String,
    pub event_type: String,
    pub label: String,
    pub message: String,
    pub turn: Option<u64>,
    pub step: Option<u64>,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
    pub output_tokens: Option<u64>,
    pub stream_chunks: usize,
    pub raw_event_count: usize,
    pub folded_event_types: Vec<String>,
    pub raw_records: Vec<Value>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceLease {
    pub id: String,
    pub slot: usize,
    pub branch_id: String,
    pub request_event_id: String,
    pub response_event_id: Option<String>,
    pub requested_at: u64,
    pub started_at: u64,
    pub ended_at: u64,
    pub context_tokens: Option<u64>,
    pub context_window: Option<u64>,
    pub context_percent: Option<f64>,
    pub model: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceTelemetrySample {
    pub time: u64,
    pub gpu: Option<GpuSnapshot>,
    pub running_models: Vec<RunningModel>,
}

#[derive(Debug, Clone)]
pub struct TraceStore {
    sessions_root: PathBuf,
    projection_root: PathBuf,
    telemetry_root: PathBuf,
    inference_slots: usize,
    enabled: bool,
}

#[derive(Debug, Clone)]
struct ArtifactHeader {
    id: String,
    created_at: u64,
    cwd: String,
    parent_session: Option<String>,
    preset: Option<String>,
    path: PathBuf,
    updated_at: u64,
}

#[derive(Debug, Clone, Default)]
struct Projection {
    title: Option<String>,
    label: Option<String>,
    mode: Option<String>,
    model: Option<String>,
    context_window: Option<u64>,
    context_tokens: Option<u64>,
    event_count: usize,
    settled: bool,
}

#[derive(Debug, Clone)]
struct RawSession {
    header: ArtifactHeader,
    projection: Projection,
    records: Vec<Value>,
}

#[derive(Debug, Clone, Copy, Default)]
struct StepUsage {
    input: Option<u64>,
    output: Option<u64>,
    context_window: Option<u64>,
}

impl TraceStore {
    pub fn discover(
        enabled: bool,
        configured_session_root: Option<&str>,
        inference_slots: usize,
    ) -> Result<Self> {
        let base = BaseDirs::new().ok_or_else(|| {
            StackError::Config("the operating-system home directory is unavailable".into())
        })?;
        let sessions_root = configured_session_root
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| base.home_dir().join(".dsh/sessions"));
        let projection_root = base
            .home_dir()
            .join(".dsh/storages/session_projcache/sessions");
        let project =
            ProjectDirs::from("dev", "localagentstack", "Local Agent Stack").ok_or_else(|| {
                StackError::Config("the operating-system data directory is unavailable".into())
            })?;
        Ok(Self {
            sessions_root,
            projection_root,
            telemetry_root: project.data_dir().join("traces/telemetry"),
            inference_slots: inference_slots.clamp(1, 16),
            enabled,
        })
    }

    pub fn sessions_root(&self) -> &Path {
        &self.sessions_root
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub async fn list_sessions(&self) -> Result<Vec<TraceSessionSummary>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.list_sessions_blocking())
            .await
            .map_err(|error| StackError::Config(format!("trace session scan failed: {error}")))?
    }

    pub async fn load_session(&self, session_id: &str) -> Result<TraceReplay> {
        let store = self.clone();
        let session_id = session_id.to_owned();
        tokio::task::spawn_blocking(move || store.load_session_blocking(&session_id))
            .await
            .map_err(|error| StackError::Config(format!("trace reconstruction failed: {error}")))?
    }

    pub async fn record_telemetry(&self, ollama_url: &str) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        let environment = inspect_environment().await;
        let running_models = match OllamaClient::new(ollama_url.to_owned()) {
            Ok(client) => client.running_models().await.unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        let sample = TraceTelemetrySample {
            time: now_ms(),
            gpu: environment.gpus.into_iter().next(),
            running_models,
        };
        fs::create_dir_all(&self.telemetry_root).await?;
        let path = self
            .telemetry_root
            .join(format!("telemetry-{}.jsonl", sample.time / 86_400_000));
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(&serde_json::to_vec(&sample)?).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        Ok(())
    }

    fn list_sessions_blocking(&self) -> Result<Vec<TraceSessionSummary>> {
        let headers = scan_headers(&self.sessions_root)?;
        let roots: Vec<_> = headers
            .iter()
            .filter(|header| header.parent_session.is_none())
            .collect();
        let mut summaries = Vec::with_capacity(roots.len());
        for root in roots {
            let projection = read_projection(&self.projection_root, &root.id).unwrap_or_default();
            let branch_count = headers
                .iter()
                .filter(|candidate| descends_from(candidate, &root.id, &headers))
                .count();
            summaries.push(summary_for(root, &projection, branch_count.max(1)));
        }
        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }

    fn load_session_blocking(&self, session_id: &str) -> Result<TraceReplay> {
        let headers = scan_headers(&self.sessions_root)?;
        let root = headers
            .iter()
            .find(|header| header.id == session_id)
            .ok_or_else(|| {
                StackError::Config(format!("Harness session {session_id} was not found"))
            })?;
        let root_id = root_ancestor(root, &headers);
        let selected: Vec<_> = headers
            .iter()
            .filter(|header| header.id == root_id || descends_from(header, &root_id, &headers))
            .cloned()
            .collect();
        let mut sessions = Vec::with_capacity(selected.len());
        for header in selected {
            sessions.push(RawSession {
                projection: read_projection(&self.projection_root, &header.id).unwrap_or_default(),
                records: read_records(&header.path)?,
                header,
            });
        }
        reconstruct(self, &root_id, sessions)
    }
}

fn reconstruct(
    store: &TraceStore,
    root_id: &str,
    sessions: Vec<RawSession>,
) -> Result<TraceReplay> {
    let root = sessions
        .iter()
        .find(|session| session.header.id == root_id)
        .ok_or_else(|| {
            StackError::Config("trace root session disappeared during reconstruction".into())
        })?;
    let mut branches = sessions
        .iter()
        .map(|session| branch_for(session, root_id))
        .collect::<Vec<_>>();
    let mut workflow_branches = workflow_branches(&sessions, &branches);
    apply_workflow_parentage(&sessions, &mut branches, &workflow_branches);
    branches.append(&mut workflow_branches);

    let branch_roles: HashMap<_, _> = branches
        .iter()
        .map(|branch| (branch.id.clone(), branch.role.clone()))
        .collect();
    let mut events = Vec::new();
    for session in &sessions {
        events.extend(normalize_session(session, &branch_roles));
    }
    events.sort_by(|a, b| {
        a.time
            .cmp(&b.time)
            .then_with(|| a.sequence.cmp(&b.sequence))
            .then_with(|| a.branch_id.cmp(&b.branch_id))
    });
    for (index, event) in events.iter_mut().enumerate() {
        event.index = index;
    }

    let edges = build_edges(&branches, &events);
    let leases = build_leases(&events, store.inference_slots);
    let start_time = events
        .first()
        .map(|event| event.time)
        .unwrap_or(root.header.created_at);
    let end_time = events
        .last()
        .map(|event| event.time)
        .unwrap_or(root.header.updated_at);
    let telemetry = load_telemetry(&store.telemetry_root, start_time, end_time)?;
    let summary = summary_for(
        &root.header,
        &root.projection,
        branches
            .iter()
            .filter(|branch| branch.role != "workflow")
            .count(),
    );
    Ok(TraceReplay {
        session: summary,
        source: root.header.path.display().to_string(),
        max_slots: store.inference_slots,
        start_time,
        end_time,
        branches,
        events,
        edges,
        leases,
        telemetry,
    })
}

fn branch_for(session: &RawSession, root_id: &str) -> TraceBranch {
    let root = session.header.id == root_id;
    let role = if root { "supervisor" } else { "subagent" };
    let label = if root {
        session
            .projection
            .title
            .clone()
            .unwrap_or_else(|| "Supervisor".into())
    } else {
        session
            .projection
            .label
            .clone()
            .or_else(|| session.projection.title.clone())
            .unwrap_or_else(|| short_id(&session.header.id))
    };
    let forked = session.projection.mode.as_deref() == Some("fork");
    let removed_at = session
        .projection
        .settled
        .then_some(session.header.updated_at);
    TraceBranch {
        id: session.header.id.clone(),
        parent_id: session.header.parent_session.clone(),
        label,
        role: role.into(),
        context_id: if forked {
            session
                .header
                .parent_session
                .clone()
                .unwrap_or_else(|| session.header.id.clone())
        } else {
            session.header.id.clone()
        },
        forked,
        created_at: session.header.created_at,
        removed_at,
        status: if removed_at.is_some() {
            "removed".into()
        } else {
            "recorded".into()
        },
    }
}

fn workflow_branches(sessions: &[RawSession], branches: &[TraceBranch]) -> Vec<TraceBranch> {
    let context_by_id: HashMap<_, _> = branches
        .iter()
        .map(|branch| (branch.id.as_str(), branch.context_id.as_str()))
        .collect();
    let mut rows: HashMap<String, TraceBranch> = HashMap::new();
    for session in sessions {
        for record in &session.records {
            if string_at(record, &["type"]) != Some("tool-workflow/run-start") {
                continue;
            }
            let run_id = string_at(record, &["data", "runId"]).unwrap_or("unknown");
            let id = format!("workflow:{}:{run_id}", session.header.id);
            let time = u64_at(record, &["time"]).unwrap_or(session.header.created_at);
            let name = string_at(record, &["data", "name"]).unwrap_or("Dynamic workflow");
            rows.entry(id.clone()).or_insert_with(|| TraceBranch {
                id,
                parent_id: Some(session.header.id.clone()),
                label: name.to_owned(),
                role: "workflow".into(),
                context_id: context_by_id
                    .get(session.header.id.as_str())
                    .copied()
                    .unwrap_or(session.header.id.as_str())
                    .to_owned(),
                forked: false,
                created_at: time,
                removed_at: None,
                status: "recorded".into(),
            });
        }
        for record in &session.records {
            if string_at(record, &["type"]) != Some("tool-workflow/run-end") {
                continue;
            }
            let run_id = string_at(record, &["data", "runId"]).unwrap_or("unknown");
            let id = format!("workflow:{}:{run_id}", session.header.id);
            if let Some(branch) = rows.get_mut(&id) {
                branch.removed_at = u64_at(record, &["time"]);
                branch.status = "removed".into();
            }
        }
    }
    let mut values: Vec<_> = rows.into_values().collect();
    values.sort_by_key(|branch| branch.created_at);
    values
}

fn apply_workflow_parentage(
    sessions: &[RawSession],
    branches: &mut [TraceBranch],
    workflows: &[TraceBranch],
) {
    let workflow_by_run: HashMap<_, _> = workflows
        .iter()
        .filter_map(|branch| {
            Some((
                (
                    branch.parent_id.as_deref()?.to_owned(),
                    branch.id.rsplit(':').next()?.to_owned(),
                ),
                branch.id.as_str(),
            ))
        })
        .collect();
    for session in sessions {
        for record in &session.records {
            if string_at(record, &["type"]) != Some("tool-workflow/agent-start") {
                continue;
            }
            let Some(child_id) = string_at(record, &["data", "childId"]) else {
                continue;
            };
            let Some(run_id) = string_at(record, &["data", "runId"]) else {
                continue;
            };
            let Some(workflow_id) =
                workflow_by_run.get(&(session.header.id.clone(), run_id.to_owned()))
            else {
                continue;
            };
            if let Some(child) = branches.iter_mut().find(|branch| branch.id == child_id) {
                child.parent_id = Some((*workflow_id).to_owned());
                if let Some(label) = string_at(record, &["data", "label"]) {
                    child.label = label.to_owned();
                }
            }
        }
    }
}

fn normalize_session(
    session: &RawSession,
    branch_roles: &HashMap<String, String>,
) -> Vec<TraceEvent> {
    let usage = collect_step_usage(session);
    let chunks = collect_stream_chunks(session);
    let mut turn = 0;
    let mut step = 0;
    let mut context_window = session.projection.context_window;
    let mut normalized = Vec::new();
    for record in &session.records {
        let Some(event_type) = string_at(record, &["type"]) else {
            continue;
        };
        if event_type == "session" {
            continue;
        }
        if event_type == "turn/start" {
            turn = u64_at(record, &["data", "turn"]).unwrap_or(turn);
        }
        if event_type == "step/start" {
            turn = u64_at(record, &["data", "turn"]).unwrap_or(turn);
            step = u64_at(record, &["data", "step"]).unwrap_or(step);
        }
        turn = u64_at(record, &["data", "turn"]).unwrap_or(turn);
        step = u64_at(record, &["data", "step"]).unwrap_or(step);
        if event_type == "request/context" {
            context_window = u64_at(record, &["data", "contextWindow"]).or(context_window);
        }
        let step_usage = usage.get(&(turn, step)).copied().unwrap_or_default();
        let window = step_usage.context_window.or(context_window);
        let input = step_usage.input;
        let context_percent = input
            .zip(window)
            .filter(|(_, total)| *total > 0)
            .map(|(used, total)| used as f64 / total as f64 * 100.0);
        let workflow_run = string_at(record, &["data", "runId"]);
        let branch_id = if event_type.starts_with("tool-workflow/") {
            workflow_run
                .map(|run_id| format!("workflow:{}:{run_id}", session.header.id))
                .filter(|id| branch_roles.contains_key(id))
                .unwrap_or_else(|| session.header.id.clone())
        } else {
            session.header.id.clone()
        };
        let role = branch_roles.get(&branch_id).map(String::as_str).unwrap_or(
            if event_type.starts_with("tool-workflow/") {
                "workflow"
            } else {
                "subagent"
            },
        );
        let sequence = u64_at(record, &["seq"]);
        let time = u64_at(record, &["time"]).unwrap_or(session.header.created_at);
        let stream_chunks = chunks.get(&(turn, step)).copied().unwrap_or(0);
        let id = format!(
            "{}:{}",
            branch_id,
            sequence
                .map(|value| value.to_string())
                .unwrap_or_else(|| format!("{time}-{}", normalized.len()))
        );
        normalized.push(TraceEvent {
            id,
            index: 0,
            time,
            sequence,
            branch_id,
            event_type: event_type.to_owned(),
            label: event_label(event_type, record),
            message: event_message(event_type, record, role),
            turn: (turn > 0).then_some(turn),
            step: (step > 0).then_some(step),
            context_tokens: input.or(session.projection.context_tokens),
            context_window: window,
            context_percent,
            output_tokens: step_usage.output,
            stream_chunks: if event_type == "assistant/message" {
                stream_chunks
            } else {
                0
            },
            raw_event_count: 1,
            folded_event_types: vec![event_type.to_owned()],
            raw_records: vec![record.clone()],
            raw: record.clone(),
        });
    }
    compact_logical_events(normalized)
}

fn compact_logical_events(events: Vec<TraceEvent>) -> Vec<TraceEvent> {
    let mut logical: Vec<TraceEvent> = Vec::new();
    let mut pending = Vec::new();
    let mut tools_by_call = HashMap::new();

    for mut event in events {
        if is_fold_only_event(&event.event_type) {
            if is_trailing_lifecycle_event(&event.event_type)
                && let Some(previous) = logical.last_mut()
            {
                append_folded_event(previous, event);
            } else {
                pending.push(event);
            }
            continue;
        }

        let logical_type = match event.event_type.as_str() {
            "user/message" => "turn/input",
            "request/header" | "request/context" | "assistant/message" => "model/execution",
            value if STREAM_EVENT_TYPES.contains(&value) => "model/execution",
            "tool/call" => "tool/execution",
            value => value,
        };

        if logical_type == "turn/input"
            && let Some(position) = logical.iter().rposition(|candidate| {
                candidate.event_type == "turn/input"
                    && candidate.turn == event.turn
                    && candidate.step == event.step
            })
        {
            append_pending(&mut logical[position], &mut pending);
            append_folded_event(&mut logical[position], event);
            refresh_logical_event(&mut logical[position]);
            continue;
        }

        if logical_type == "model/execution"
            && let Some(position) = logical.iter().rposition(|candidate| {
                candidate.event_type == "model/execution"
                    && candidate.turn == event.turn
                    && candidate.step == event.step
            })
        {
            append_pending(&mut logical[position], &mut pending);
            append_folded_event(&mut logical[position], event);
            refresh_logical_event(&mut logical[position]);
            continue;
        }

        if event.event_type == "tool/result"
            && let Some(call_id) = string_at(&event.raw, &["data", "message", "source", "callId"])
            && let Some(position) = tools_by_call.get(call_id).copied()
        {
            append_pending(&mut logical[position], &mut pending);
            append_folded_event(&mut logical[position], event);
            refresh_logical_event(&mut logical[position]);
            continue;
        }

        event.event_type = logical_type.to_owned();
        append_pending(&mut event, &mut pending);
        refresh_logical_event(&mut event);
        if event.event_type == "tool/execution"
            && let Some(call_id) = string_at(&event.raw, &["data", "callId"])
        {
            tools_by_call.insert(call_id.to_owned(), logical.len());
        }
        logical.push(event);
    }

    if let Some(previous) = logical.last_mut() {
        append_pending(previous, &mut pending);
        refresh_logical_event(previous);
    }
    logical
}

fn is_fold_only_event(event_type: &str) -> bool {
    matches!(
        event_type,
        "permission/preset"
            | "sandbox/mode"
            | "approval/policy"
            | "agent/inbox/spliced"
            | "turn/start"
            | "step/start"
            | "step/end"
            | "turn/end"
            | "session/title"
            | "session/title-llm-request"
            | "session/end-seed"
            | "subagent/descriptor"
    )
}

fn is_trailing_lifecycle_event(event_type: &str) -> bool {
    matches!(event_type, "step/end" | "turn/end" | "session/end-seed")
}

fn append_pending(target: &mut TraceEvent, pending: &mut Vec<TraceEvent>) {
    if pending.is_empty() {
        return;
    }
    let mut records = Vec::new();
    let mut event_types = Vec::new();
    let mut count = 0;
    for event in std::mem::take(pending) {
        records.extend(event.raw_records);
        count += event.raw_event_count;
        for event_type in event.folded_event_types {
            if !event_types.contains(&event_type) {
                event_types.push(event_type);
            }
        }
    }
    records.append(&mut target.raw_records);
    target.raw_records = records;
    event_types.append(&mut target.folded_event_types);
    event_types.dedup();
    target.folded_event_types = event_types;
    target.raw_event_count += count;
}

fn append_folded_event(target: &mut TraceEvent, incoming: TraceEvent) {
    target.raw_event_count += incoming.raw_event_count;
    target.raw_records.extend(incoming.raw_records);
    for event_type in incoming.folded_event_types {
        if !target.folded_event_types.contains(&event_type) {
            target.folded_event_types.push(event_type);
        }
    }
    if incoming.context_tokens.is_some() {
        target.context_tokens = incoming.context_tokens;
    }
    if incoming.context_window.is_some() {
        target.context_window = incoming.context_window;
    }
    if incoming.context_percent.is_some() {
        target.context_percent = incoming.context_percent;
    }
    if incoming.output_tokens.is_some() {
        target.output_tokens = incoming.output_tokens;
    }
}

fn refresh_logical_event(event: &mut TraceEvent) {
    event.stream_chunks = event
        .raw_records
        .iter()
        .filter(|record| {
            STREAM_EVENT_TYPES.contains(&string_at(record, &["type"]).unwrap_or_default())
        })
        .count();
    match event.event_type.as_str() {
        "turn/input" => {
            let messages: Vec<_> = event
                .raw_records
                .iter()
                .filter(|record| string_at(record, &["type"]) == Some("user/message"))
                .collect();
            let human: Vec<_> = messages
                .iter()
                .filter(|record| string_at(record, &["data", "source", "kind"]) == Some("user"))
                .filter_map(|record| message_text(record))
                .collect();
            let injected = messages.len().saturating_sub(human.len());
            event.label = "User input".into();
            let text = if human.is_empty() {
                "Injected model context".into()
            } else {
                compact_text(&human.join("\n"), 240)
            };
            event.message = if injected > 0 {
                format!("{text} · {injected} injected context record(s) folded")
            } else {
                text
            };
        }
        "model/execution" => {
            let request = record_of_type(event, "request/header");
            let response = record_of_type(event, "assistant/message");
            let label = request
                .and_then(|record| string_at(record, &["data", "header", "config", "model"]))
                .map(|model| format!("Model execution · {model}"))
                .unwrap_or_else(|| "Model execution".into());
            let message = response
                .and_then(message_text)
                .map(|text| compact_text(&text, 240))
                .or_else(|| request.map(|record| event_message("request/header", record, "model")))
                .unwrap_or_else(|| "Model inference lifecycle recorded".into());
            event.label = label;
            event.message = message;
        }
        "tool/execution" => {
            if let Some(call) = record_of_type(event, "tool/call") {
                let label = event_label("tool/call", call);
                let message = event_message("tool/call", call, "tool");
                event.label = label;
                event.message = message;
            }
        }
        _ => {}
    }
}

fn record_of_type<'a>(event: &'a TraceEvent, event_type: &str) -> Option<&'a Value> {
    event
        .raw_records
        .iter()
        .find(|record| string_at(record, &["type"]) == Some(event_type))
}

fn collect_step_usage(session: &RawSession) -> HashMap<(u64, u64), StepUsage> {
    let mut result = HashMap::new();
    let mut turn = 0;
    let mut step = 0;
    let mut context_window = session.projection.context_window;
    for record in &session.records {
        let event_type = string_at(record, &["type"]).unwrap_or_default();
        turn = u64_at(record, &["data", "turn"]).unwrap_or(turn);
        step = u64_at(record, &["data", "step"]).unwrap_or(step);
        if event_type == "request/context" {
            context_window = u64_at(record, &["data", "contextWindow"]).or(context_window);
        }
        if event_type == "assistant/message" {
            result.insert(
                (turn, step),
                StepUsage {
                    input: u64_at(record, &["data", "usage", "inputTokens"]),
                    output: u64_at(record, &["data", "usage", "outputTokens"]),
                    context_window,
                },
            );
        }
    }
    result
}

fn collect_stream_chunks(session: &RawSession) -> HashMap<(u64, u64), usize> {
    let mut result = HashMap::new();
    let mut turn = 0;
    let mut step = 0;
    for record in &session.records {
        turn = u64_at(record, &["data", "turn"]).unwrap_or(turn);
        step = u64_at(record, &["data", "step"]).unwrap_or(step);
        if STREAM_EVENT_TYPES.contains(&string_at(record, &["type"]).unwrap_or_default()) {
            *result.entry((turn, step)).or_insert(0) += 1;
        }
    }
    result
}

fn build_edges(branches: &[TraceBranch], events: &[TraceEvent]) -> Vec<TraceEdge> {
    let mut edges = Vec::new();
    let mut by_branch: HashMap<&str, Vec<&TraceEvent>> = HashMap::new();
    for event in events {
        by_branch.entry(&event.branch_id).or_default().push(event);
    }
    for branch_events in by_branch.values() {
        for pair in branch_events.windows(2) {
            edges.push(TraceEdge {
                from: pair[0].id.clone(),
                to: pair[1].id.clone(),
                kind: "same".into(),
                label: String::new(),
            });
        }
    }

    let first_by_branch: HashMap<_, _> = by_branch
        .iter()
        .filter_map(|(branch, rows)| {
            rows.first()
                .map(|event| ((*branch).to_owned(), event.id.clone()))
        })
        .collect();
    let last_by_branch: HashMap<_, _> = by_branch
        .iter()
        .filter_map(|(branch, rows)| {
            rows.last()
                .map(|event| ((*branch).to_owned(), event.id.clone()))
        })
        .collect();
    for event in events
        .iter()
        .filter(|event| event.event_type == "tool/execution")
    {
        let Some(call) = record_of_type(event, "tool/call") else {
            continue;
        };
        let text = compact_json(&Value::Array(event.raw_records.clone()));
        for branch in branches.iter().filter(|branch| branch.role == "subagent") {
            if !text.contains(&branch.id) {
                continue;
            }
            let Some(first) = first_by_branch.get(&branch.id) else {
                continue;
            };
            let forked = branch.forked
                || string_at(call, &["data", "name"]).is_some_and(|name| name.contains("fork"));
            edges.push(TraceEdge {
                from: event.id.clone(),
                to: first.clone(),
                kind: if forked { "fork" } else { "spawn" }.into(),
                label: if forked { "fork" } else { "spawn" }.into(),
            });
        }
    }

    for event in events {
        match event.event_type.as_str() {
            "tool-workflow/run-start" => {
                if let Some(parent) = branches
                    .iter()
                    .find(|branch| branch.id == event.branch_id)
                    .and_then(|workflow| workflow.parent_id.as_ref())
                    .and_then(|parent_id| {
                        by_branch.get(parent_id.as_str()).and_then(|rows| {
                            rows.iter()
                                .rev()
                                .find(|candidate| candidate.time <= event.time)
                                .copied()
                        })
                    })
                {
                    edges.push(TraceEdge {
                        from: parent.id.clone(),
                        to: event.id.clone(),
                        kind: "workflow".into(),
                        label: "workflow".into(),
                    });
                }
            }
            "tool-workflow/agent-start" => {
                if let Some(child_id) = string_at(&event.raw, &["data", "childId"])
                    && let Some(first) = first_by_branch.get(child_id)
                {
                    edges.push(TraceEdge {
                        from: event.id.clone(),
                        to: first.clone(),
                        kind: "workflow".into(),
                        label: "member".into(),
                    });
                }
            }
            "tool-workflow/agent-end" => {
                if let Some(child_id) = string_at(&event.raw, &["data", "childId"])
                    && let Some(last) = last_by_branch.get(child_id)
                {
                    edges.push(TraceEdge {
                        from: last.clone(),
                        to: event.id.clone(),
                        kind: "report".into(),
                        label: "report".into(),
                    });
                }
            }
            "tool/execution"
                if record_of_type(event, "tool/call")
                    .and_then(|record| string_at(record, &["data", "name"]))
                    == Some("report") =>
            {
                let arguments = record_of_type(event, "tool/call")
                    .and_then(|record| string_at(record, &["data", "arguments"]))
                    .unwrap_or_default();
                let consultation = arguments.contains("consultation");
                if let Some(parent_id) = branches
                    .iter()
                    .find(|branch| branch.id == event.branch_id)
                    .and_then(|branch| branch.parent_id.as_ref())
                    && let Some(target) = by_branch.get(parent_id.as_str()).and_then(|rows| {
                        rows.iter()
                            .find(|candidate| candidate.time >= event.time)
                            .copied()
                    })
                {
                    edges.push(TraceEdge {
                        from: event.id.clone(),
                        to: target.id.clone(),
                        kind: if consultation {
                            "consultation"
                        } else {
                            "report"
                        }
                        .into(),
                        label: if consultation {
                            "consultation"
                        } else {
                            "report"
                        }
                        .into(),
                    });
                }
            }
            _ => {}
        }
    }

    let mut seen = HashSet::new();
    edges.retain(|edge| seen.insert((edge.from.clone(), edge.to.clone(), edge.kind.clone())));
    edges
}

fn build_leases(events: &[TraceEvent], max_slots: usize) -> Vec<TraceLease> {
    let mut requests: Vec<_> = events
        .iter()
        .filter(|event| event.event_type == "model/execution")
        .filter_map(|event| {
            let request = record_of_type(event, "request/header")?;
            let response = record_of_type(event, "assistant/message");
            Some((event, request, response))
        })
        .collect();
    requests.sort_by_key(|(_, request, _)| u64_at(request, &["time"]).unwrap_or_default());

    let mut slot_ends = vec![0_u64; max_slots.max(1)];
    let mut leases = Vec::with_capacity(requests.len());
    for (event, request, response) in requests {
        let requested_at = u64_at(request, &["time"]).unwrap_or(event.time);
        let free = slot_ends
            .iter()
            .enumerate()
            .find(|(_, end)| **end <= requested_at)
            .map(|(index, _)| index);
        let slot_index = free.unwrap_or_else(|| {
            slot_ends
                .iter()
                .enumerate()
                .min_by_key(|(_, end)| **end)
                .map(|(index, _)| index)
                .unwrap_or(0)
        });
        let started_at = requested_at.max(slot_ends[slot_index]);
        let observed_end = response
            .and_then(|record| u64_at(record, &["time"]))
            .unwrap_or(started_at + 1);
        let ended_at = observed_end.max(started_at + 1);
        slot_ends[slot_index] = ended_at;
        leases.push(TraceLease {
            id: format!("lease-{}", event.id),
            slot: slot_index + 1,
            branch_id: event.branch_id.clone(),
            request_event_id: event.id.clone(),
            response_event_id: response.map(|_| event.id.clone()),
            requested_at,
            started_at,
            ended_at,
            context_tokens: event.context_tokens,
            context_window: event.context_window,
            context_percent: event.context_percent,
            model: string_at(request, &["data", "header", "config", "model"]).map(str::to_owned),
            source: "derived from request/header → assistant/message lifecycle".into(),
        });
    }
    leases
}

fn event_label(event_type: &str, record: &Value) -> String {
    match event_type {
        "user/message" => "User message".into(),
        "request/header" => string_at(record, &["data", "header", "config", "model"])
            .map(|model| format!("Model request · {model}"))
            .unwrap_or_else(|| "Model request".into()),
        "request/context" => "Context window".into(),
        "assistant/message" => "Model response".into(),
        "tool/call" => string_at(record, &["data", "name"])
            .map(|name| format!("Call {name}"))
            .unwrap_or_else(|| "Tool call".into()),
        "tool/result" => "Tool result".into(),
        "tool-workflow/run-start" => string_at(record, &["data", "name"])
            .map(|name| format!("Workflow {name} starts"))
            .unwrap_or_else(|| "Workflow starts".into()),
        "tool-workflow/agent-start" => string_at(record, &["data", "label"])
            .map(|label| format!("Member {label} starts"))
            .unwrap_or_else(|| "Workflow member starts".into()),
        "tool-workflow/agent-end" => string_at(record, &["data", "label"])
            .map(|label| format!("Member {label} ends"))
            .unwrap_or_else(|| "Workflow member ends".into()),
        "tool-workflow/run-end" => "Workflow settles".into(),
        value => value.replace(['/', '-'], " "),
    }
}

fn event_message(event_type: &str, record: &Value, role: &str) -> String {
    match event_type {
        "user/message" | "assistant/message" => message_text(record)
            .map(|text| compact_text(&text, 240))
            .unwrap_or_else(|| format!("{role} message recorded")),
        "request/header" => {
            let system = string_at(record, &["data", "header", "system"])
                .map(str::len)
                .unwrap_or(0);
            let tools = record
                .pointer("/data/header/tools")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            let messages = record
                .pointer("/data/header/messages")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(0);
            format!(
                "Complete model input · system {system} chars · {tools} tools · {messages} messages"
            )
        }
        "request/context" => u64_at(record, &["data", "contextWindow"])
            .map(|window| format!("Provider context window: {window} tokens"))
            .unwrap_or_else(|| "Provider context metadata recorded".into()),
        "tool/call" => string_at(record, &["data", "arguments"])
            .map(|arguments| compact_text(arguments, 240))
            .unwrap_or_else(|| "Tool arguments recorded".into()),
        "tool/result" => message_text(record)
            .map(|text| compact_text(&text, 240))
            .unwrap_or_else(|| "Tool output recorded".into()),
        "tool-workflow/run-start" => format!(
            "Dynamic workflow {} created",
            string_at(record, &["data", "runId"]).unwrap_or("unknown")
        ),
        "tool-workflow/agent-start" => format!(
            "Workflow member {} assigned to child {}",
            string_at(record, &["data", "label"]).unwrap_or("unnamed"),
            string_at(record, &["data", "childId"]).unwrap_or("unknown")
        ),
        "tool-workflow/agent-end" => format!(
            "Workflow member finished with {}",
            string_at(record, &["data", "outcome"]).unwrap_or("unknown outcome")
        ),
        "tool-workflow/run-end" => format!(
            "Workflow stopped: {}",
            string_at(record, &["data", "stopReason"]).unwrap_or("unknown")
        ),
        _ => compact_text(
            &compact_json(record.pointer("/data").unwrap_or(&Value::Null)),
            240,
        ),
    }
}

fn scan_headers(root: &Path) -> Result<Vec<ArtifactHeader>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    visit_session_files(root, &mut paths)?;
    let mut headers = Vec::new();
    for path in paths {
        if let Ok(header) = read_header(&path) {
            headers.push(header);
        }
    }
    Ok(headers)
}

fn visit_session_files(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            visit_session_files(&entry.path(), paths)?;
        } else if matches!(
            entry.file_name().to_str(),
            Some("session.jsonl" | "session.jsonl.zstd")
        ) {
            paths.push(entry.path());
        }
    }
    Ok(())
}

fn read_header(path: &Path) -> Result<ArtifactHeader> {
    let mut reader = jsonl_reader(path)?;
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let value: Value = serde_json::from_str(line.trim_end())?;
    let id = string_at(&value, &["id"])
        .ok_or_else(|| StackError::Config(format!("{} has no session id", path.display())))?
        .to_owned();
    let metadata = std::fs::metadata(path)?;
    let updated_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_else(now_ms);
    Ok(ArtifactHeader {
        id,
        created_at: u64_at(&value, &["createdAt"]).unwrap_or(updated_at),
        cwd: string_at(&value, &["cwd"]).unwrap_or_default().to_owned(),
        parent_session: string_at(&value, &["parentSession"]).map(str::to_owned),
        preset: string_at(&value, &["agentPreset"]).map(str::to_owned),
        path: path.to_owned(),
        updated_at,
    })
}

fn read_records(path: &Path) -> Result<Vec<Value>> {
    let reader = jsonl_reader(path)?;
    let mut rows = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if !line.trim().is_empty() {
            rows.push(serde_json::from_str(&line)?);
        }
    }
    Ok(rows)
}

fn jsonl_reader(path: &Path) -> Result<Box<dyn BufRead>> {
    let file = File::open(path)?;
    let reader: Box<dyn Read> = if path.extension().and_then(|value| value.to_str()) == Some("zstd")
    {
        Box::new(zstd::stream::read::Decoder::new(file)?)
    } else {
        Box::new(file)
    };
    Ok(Box::new(BufReader::new(reader)))
}

fn read_projection(root: &Path, id: &str) -> Result<Projection> {
    let path = root.join(format!("{id}.json"));
    if !path.is_file() {
        return Ok(Projection::default());
    }
    let value: Value = serde_json::from_reader(BufReader::new(File::open(path)?))?;
    let rows = value.pointer("/record/rows").unwrap_or(&Value::Null);
    let event_count = rows
        .as_object()
        .into_iter()
        .flat_map(|object| object.values())
        .filter_map(|row| u64_at(row, &["seq"]))
        .max()
        .map(|seq| seq as usize + 1)
        .unwrap_or(0);
    Ok(Projection {
        title: string_at(rows, &["title", "val"]).map(str::to_owned),
        label: string_at(rows, &["subagent", "val", "identity", "label"]).map(str::to_owned),
        mode: string_at(rows, &["subagent", "val", "identity", "mode"]).map(str::to_owned),
        model: string_at(rows, &["modelSelection", "val", "lastUsed", "model"]).map(str::to_owned),
        context_window: u64_at(rows, &["contextPressure", "val", "contextWindow"]),
        context_tokens: u64_at(rows, &["contextPressure", "val", "pressureTokens"]),
        event_count,
        settled: u64_at(rows, &["subagentTiming", "val", "settledMs"]).unwrap_or(0) > 0,
    })
}

fn summary_for(
    header: &ArtifactHeader,
    projection: &Projection,
    branch_count: usize,
) -> TraceSessionSummary {
    TraceSessionSummary {
        id: header.id.clone(),
        title: projection
            .title
            .clone()
            .unwrap_or_else(|| short_id(&header.id)),
        cwd: header.cwd.clone(),
        preset: header.preset.clone(),
        model: projection.model.clone(),
        created_at: header.created_at,
        updated_at: header.updated_at,
        event_count: projection.event_count,
        branch_count,
        status: if now_ms().saturating_sub(header.updated_at) < 15_000 {
            "live".into()
        } else {
            "recorded".into()
        },
    }
}

fn descends_from(candidate: &ArtifactHeader, root_id: &str, headers: &[ArtifactHeader]) -> bool {
    let mut current = candidate.parent_session.as_deref();
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if id == root_id {
            return true;
        }
        if !seen.insert(id) {
            return false;
        }
        current = headers
            .iter()
            .find(|header| header.id == id)
            .and_then(|header| header.parent_session.as_deref());
    }
    false
}

fn root_ancestor(header: &ArtifactHeader, headers: &[ArtifactHeader]) -> String {
    let mut current = header;
    let mut seen = HashSet::new();
    while let Some(parent_id) = current.parent_session.as_deref() {
        if !seen.insert(parent_id) {
            break;
        }
        let Some(parent) = headers.iter().find(|candidate| candidate.id == parent_id) else {
            break;
        };
        current = parent;
    }
    current.id.clone()
}

fn load_telemetry(root: &Path, start: u64, end: u64) -> Result<Vec<TraceTelemetrySample>> {
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let first_day = start.saturating_sub(5_000) / 86_400_000;
    let last_day = end.saturating_add(5_000) / 86_400_000;
    let mut samples = Vec::new();
    for day in first_day..=last_day {
        let path = root.join(format!("telemetry-{day}.jsonl"));
        if !path.is_file() {
            continue;
        }
        for line in BufReader::new(File::open(path)?).lines() {
            let sample: TraceTelemetrySample = serde_json::from_str(&line?)?;
            if sample.time >= start.saturating_sub(5_000)
                && sample.time <= end.saturating_add(5_000)
            {
                samples.push(sample);
            }
        }
    }
    samples.sort_by_key(|sample| sample.time);
    Ok(samples)
}

fn message_text(record: &Value) -> Option<String> {
    let content = record
        .pointer("/data/message/content")
        .or_else(|| record.pointer("/data/content"))?
        .as_array()?;
    let mut parts = Vec::new();
    for block in content {
        if let Some(text) = string_at(block, &["text"]) {
            parts.push(text);
        } else if let Some(text) = string_at(block, &["content", "0", "text"]) {
            parts.push(text);
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn string_at<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = if let Ok(index) = key.parse::<usize>() {
            current.as_array()?.get(index)?
        } else {
            current.get(*key)?
        };
    }
    current.as_str()
}

fn u64_at(value: &Value, path: &[&str]) -> Option<u64> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_u64()
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".into())
}

fn compact_text(value: &str, limit: usize) -> String {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() <= limit {
        return value;
    }
    let mut result = value
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    result.push('…');
    result
}

fn short_id(value: &str) -> String {
    value.chars().take(18).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn event(event_type: &str, seq: u64, time: u64, data: Value) -> Value {
        json!({ "type": event_type, "seq": seq, "time": time, "data": data })
    }

    #[test]
    fn reconstructs_context_lineage_workflows_and_slot_queue() {
        let root_header = ArtifactHeader {
            id: "root".into(),
            created_at: 1_000,
            cwd: "C:/work".into(),
            parent_session: None,
            preset: Some("ultra-workflow".into()),
            path: PathBuf::from("root.jsonl"),
            updated_at: 2_000,
        };
        let child_header = ArtifactHeader {
            id: "child".into(),
            created_at: 1_120,
            cwd: "C:/work".into(),
            parent_session: Some("root".into()),
            preset: None,
            path: PathBuf::from("child.jsonl"),
            updated_at: 1_900,
        };
        let root = RawSession {
            header: root_header,
            projection: Projection {
                title: Some("Root trace".into()),
                context_window: Some(100),
                ..Projection::default()
            },
            records: vec![
                event("turn/start", 0, 1_000, json!({ "turn": 1 })),
                event("step/start", 1, 1_010, json!({ "turn": 1, "step": 1 })),
                event(
                    "request/header",
                    2,
                    1_020,
                    json!({ "header": { "config": { "model": "qwen" }, "system": "s", "tools": [], "messages": [] } }),
                ),
                event(
                    "assistant/message",
                    3,
                    1_100,
                    json!({ "turn": 1, "step": 1, "usage": { "inputTokens": 25, "outputTokens": 5 }, "message": { "content": [{ "type": "text", "text": "done" }] } }),
                ),
                event(
                    "tool-workflow/run-start",
                    4,
                    1_110,
                    json!({ "runId": "wf-1", "name": "Audit" }),
                ),
                event(
                    "tool-workflow/agent-start",
                    5,
                    1_120,
                    json!({ "runId": "wf-1", "seq": 1, "label": "Checker", "childId": "child" }),
                ),
                event(
                    "tool-workflow/agent-end",
                    6,
                    1_900,
                    json!({ "runId": "wf-1", "seq": 1, "label": "Checker", "childId": "child", "outcome": "completed" }),
                ),
                event(
                    "tool-workflow/run-end",
                    7,
                    1_910,
                    json!({ "runId": "wf-1", "stopReason": "completed" }),
                ),
            ],
        };
        let child = RawSession {
            header: child_header,
            projection: Projection {
                label: Some("Checker".into()),
                context_window: Some(100),
                settled: true,
                ..Projection::default()
            },
            records: vec![
                event("turn/start", 0, 1_130, json!({ "turn": 1 })),
                event("step/start", 1, 1_140, json!({ "turn": 1, "step": 1 })),
                event(
                    "request/header",
                    2,
                    1_150,
                    json!({ "header": { "config": { "model": "qwen" } } }),
                ),
                event(
                    "assistant/message",
                    3,
                    1_800,
                    json!({ "turn": 1, "step": 1, "usage": { "inputTokens": 50, "outputTokens": 10 }, "message": { "content": [{ "type": "text", "text": "report" }] } }),
                ),
            ],
        };
        let directory = tempdir().unwrap();
        let store = TraceStore {
            sessions_root: directory.path().join("sessions"),
            projection_root: directory.path().join("projection"),
            telemetry_root: directory.path().join("telemetry"),
            inference_slots: 1,
            enabled: true,
        };
        let replay = reconstruct(&store, "root", vec![root, child]).unwrap();
        assert_eq!(
            replay
                .branches
                .iter()
                .filter(|branch| branch.role == "workflow")
                .count(),
            1
        );
        assert_eq!(
            replay
                .branches
                .iter()
                .find(|branch| branch.id == "child")
                .unwrap()
                .parent_id
                .as_deref(),
            Some("workflow:root:wf-1")
        );
        assert_eq!(replay.leases.len(), 2);
        assert!(
            replay
                .events
                .iter()
                .any(|event| event.context_percent == Some(50.0))
        );
        assert!(replay.edges.iter().any(|edge| edge.kind == "workflow"));
    }

    #[test]
    fn reads_plain_and_zstd_session_headers() {
        let directory = tempdir().unwrap();
        let value = json!({
            "type": "session",
            "version": 0,
            "id": "session-a",
            "createdAt": 42,
            "cwd": "C:/work"
        });
        let plain = directory.path().join("session.jsonl");
        std::fs::write(&plain, format!("{}\n", value)).unwrap();
        assert_eq!(read_header(&plain).unwrap().id, "session-a");

        let compressed = directory.path().join("session.jsonl.zstd");
        let mut encoder =
            zstd::stream::write::Encoder::new(File::create(&compressed).unwrap(), 0).unwrap();
        writeln!(encoder, "{value}").unwrap();
        encoder.finish().unwrap();
        assert_eq!(read_header(&compressed).unwrap().created_at, 42);
    }

    #[test]
    fn folds_transport_records_into_logical_operations_without_loss() {
        let session = RawSession {
            header: ArtifactHeader {
                id: "hello".into(),
                created_at: 1_000,
                cwd: "C:/work".into(),
                parent_session: None,
                preset: None,
                path: PathBuf::from("hello.jsonl"),
                updated_at: 2_000,
            },
            projection: Projection {
                context_window: Some(100),
                ..Projection::default()
            },
            records: vec![
                event(
                    "permission/preset",
                    1,
                    1_000,
                    json!({ "preset": "workspace-write" }),
                ),
                event("turn/start", 2, 1_010, json!({ "turn": 1 })),
                event("step/start", 3, 1_020, json!({ "turn": 1, "step": 1 })),
                event(
                    "user/message",
                    4,
                    1_030,
                    json!({ "turn": 1, "step": 1, "content": [{ "type": "text", "text": "hello" }], "source": { "kind": "user" } }),
                ),
                event(
                    "user/message",
                    5,
                    1_031,
                    json!({ "turn": 1, "step": 1, "content": [{ "type": "text", "text": "runtime" }], "source": { "kind": "plugin" } }),
                ),
                event(
                    "user/message",
                    6,
                    1_032,
                    json!({ "turn": 1, "step": 1, "content": [{ "type": "text", "text": "skills" }], "source": { "kind": "skill-catalog" } }),
                ),
                event(
                    "request/header",
                    7,
                    1_040,
                    json!({ "turn": 1, "step": 1, "header": { "config": { "model": "qwen" }, "system": "system", "tools": [], "messages": [] } }),
                ),
                event(
                    "request/context",
                    8,
                    1_041,
                    json!({ "turn": 1, "step": 1, "contextWindow": 100 }),
                ),
                event(
                    "reasoning-chunks",
                    9,
                    1_050,
                    json!({ "turn": 1, "step": 1, "texts": ["thinking"] }),
                ),
                event(
                    "assistant/message",
                    10,
                    1_090,
                    json!({ "turn": 1, "step": 1, "usage": { "inputTokens": 25, "outputTokens": 5 }, "message": { "content": [{ "type": "text", "text": "Hello" }] } }),
                ),
                event("step/end", 11, 1_100, json!({ "turn": 1, "step": 1 })),
                event("turn/end", 12, 1_110, json!({ "turn": 1 })),
            ],
        };
        let roles = HashMap::from([("hello".to_owned(), "supervisor".to_owned())]);
        let logical = normalize_session(&session, &roles);

        assert_eq!(logical.len(), 2);
        assert_eq!(logical[0].event_type, "turn/input");
        assert_eq!(
            logical[0].message,
            "hello · 2 injected context record(s) folded"
        );
        assert_eq!(logical[1].event_type, "model/execution");
        assert_eq!(logical[1].stream_chunks, 1);
        assert_eq!(
            logical
                .iter()
                .map(|event| event.raw_event_count)
                .sum::<usize>(),
            12
        );
        assert_eq!(
            logical.iter().flat_map(|event| &event.raw_records).count(),
            12
        );
    }
}
