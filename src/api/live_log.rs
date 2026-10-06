use crate::domain::openai::ChatMessage;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;
use tokio::sync::broadcast;

/// Number of requests kept in the in-memory ring buffer.
const LOG_CAPACITY: usize = 100;
/// Broadcast channel depth. Generous enough that slow SSE clients lag, not drop.
const BROADCAST_CAPACITY: usize = 512;

/// One captured request/response pair.
#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub id: u64,
    pub ts: f64,
    pub model: String,
    pub token_id: Option<i64>,
    pub session_id: String,
    pub stream: bool,
    pub status: String,
    pub duration_ms: u64,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
    pub finish_reason: String,
    pub tool_calls: usize,
    pub input: String,
    pub output: String,
    pub reasoning: String,
    pub error: Option<String>,
}

/// Events pushed to dashboard subscribers over SSE.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogEvent {
    Begin(LogEntry),
    Delta {
        id: u64,
        content: String,
        reasoning: String,
    },
    End(LogEntry),
}

/// Fields required to open a log entry.
pub struct LogSeed {
    pub model: String,
    pub token_id: Option<i64>,
    pub session_id: String,
    pub stream: bool,
    pub input: String,
}

/// Terminal metrics attached to a completed request.
pub struct LogFinish {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub cached_tokens: u32,
    pub finish_reason: String,
    pub tool_calls: usize,
}

/// In-memory ring buffer of recent requests plus a broadcast fan-out.
pub struct LiveLog {
    entries: Mutex<VecDeque<LogEntry>>,
    tx: broadcast::Sender<LogEvent>,
    next_id: AtomicU64,
}

impl Default for LiveLog {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveLog {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(BROADCAST_CAPACITY);
        Self {
            entries: Mutex::new(VecDeque::with_capacity(LOG_CAPACITY)),
            tx,
            next_id: AtomicU64::new(1),
        }
    }

    /// Register a new in-flight request and return its handle.
    pub fn begin(self: &Arc<Self>, seed: LogSeed) -> LogHandle {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let entry = LogEntry {
            id,
            ts: crate::infra::db::now_timestamp(),
            model: seed.model,
            token_id: seed.token_id,
            session_id: seed.session_id,
            stream: seed.stream,
            status: "in_progress".to_string(),
            duration_ms: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            cached_tokens: 0,
            finish_reason: String::new(),
            tool_calls: 0,
            input: seed.input,
            output: String::new(),
            reasoning: String::new(),
            error: None,
        };

        {
            let mut guard = self.lock_entries();
            while guard.len() >= LOG_CAPACITY {
                guard.pop_front();
            }
            guard.push_back(entry.clone());
        }
        self.emit(LogEvent::Begin(entry));

        LogHandle {
            log: Arc::clone(self),
            id,
            started: Instant::now(),
        }
    }

    /// Oldest-to-newest snapshot of buffered entries.
    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.lock_entries().iter().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogEvent> {
        self.tx.subscribe()
    }

    fn lock_entries(&self) -> MutexGuard<'_, VecDeque<LogEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn mutate(&self, id: u64, update: impl FnOnce(&mut LogEntry)) {
        let mut guard = self.lock_entries();
        if let Some(entry) = guard.iter_mut().find(|e| e.id == id) {
            update(entry);
        }
    }

    fn complete(&self, id: u64, update: impl FnOnce(&mut LogEntry)) -> Option<LogEntry> {
        let mut guard = self.lock_entries();
        let entry = guard.iter_mut().find(|e| e.id == id)?;
        update(entry);
        Some(entry.clone())
    }

    fn emit(&self, event: LogEvent) {
        let _ = self.tx.send(event);
    }
}

/// Per-request handle used by the chat handlers to feed the log.
#[derive(Clone)]
pub struct LogHandle {
    log: Arc<LiveLog>,
    id: u64,
    started: Instant,
}

impl LogHandle {
    pub fn append_content(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.log.mutate(self.id, |e| e.output.push_str(text));
        self.log.emit(LogEvent::Delta {
            id: self.id,
            content: text.to_string(),
            reasoning: String::new(),
        });
    }

    pub fn append_reasoning(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.log.mutate(self.id, |e| e.reasoning.push_str(text));
        self.log.emit(LogEvent::Delta {
            id: self.id,
            content: String::new(),
            reasoning: text.to_string(),
        });
    }

    /// Replace the buffered output, used by the non-streaming path.
    pub fn set_output(&self, content: &str, reasoning: Option<&str>) {
        let content = content.to_string();
        let reasoning = reasoning.unwrap_or_default().to_string();
        self.log.mutate(self.id, |e| {
            e.output = content;
            e.reasoning = reasoning;
        });
    }

    /// Discard accumulated text after a failed upstream attempt is retried.
    pub fn reset_output(&self) {
        self.log.mutate(self.id, |e| {
            e.output.clear();
            e.reasoning.clear();
        });
    }

    pub fn finish(&self, finish: LogFinish) {
        let duration_ms = self.started.elapsed().as_millis() as u64;
        let entry = self.log.complete(self.id, |e| {
            e.status = "ok".to_string();
            e.duration_ms = duration_ms;
            e.prompt_tokens = finish.prompt_tokens;
            e.completion_tokens = finish.completion_tokens;
            e.cached_tokens = finish.cached_tokens;
            e.finish_reason = finish.finish_reason;
            e.tool_calls = finish.tool_calls;
        });
        if let Some(entry) = entry {
            self.log.emit(LogEvent::End(entry));
        }
    }

    pub fn fail(&self, error: impl Into<String>) {
        let duration_ms = self.started.elapsed().as_millis() as u64;
        let message = error.into();
        let entry = self.log.complete(self.id, |e| {
            e.status = "error".to_string();
            e.duration_ms = duration_ms;
            e.error = Some(message.clone());
        });
        if let Some(entry) = entry {
            self.log.emit(LogEvent::End(entry));
        }
    }
}

/// Flatten a request's messages into a readable transcript for the log.
pub fn render_input(messages: &[ChatMessage]) -> String {
    let mut out = String::new();
    for message in messages {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push('[');
        out.push_str(&message.role);
        out.push_str("] ");
        out.push_str(&message.text_content());
        if let Some(tool_calls) = &message.tool_calls {
            for call in tool_calls {
                out.push_str("\n  -> ");
                out.push_str(&call.function.name);
                out.push('(');
                out.push_str(&call.function.arguments);
                out.push(')');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(index: usize) -> LogSeed {
        LogSeed {
            model: "test-model".to_string(),
            token_id: Some(1),
            session_id: format!("sess-{index}"),
            stream: true,
            input: format!("hello {index}"),
        }
    }

    fn finish() -> LogFinish {
        LogFinish {
            prompt_tokens: 1,
            completion_tokens: 2,
            cached_tokens: 0,
            finish_reason: "stop".to_string(),
            tool_calls: 0,
        }
    }

    #[test]
    fn ring_buffer_keeps_newest_entries() {
        let log = Arc::new(LiveLog::new());
        for index in 0..(LOG_CAPACITY + 5) {
            let handle = log.begin(seed(index));
            handle.append_content("hi");
            handle.finish(finish());
        }

        let snapshot = log.snapshot();
        assert_eq!(snapshot.len(), LOG_CAPACITY);
        assert_eq!(snapshot.first().map(|e| e.id), Some(6));
        assert_eq!(
            snapshot.last().map(|e| e.id),
            Some((LOG_CAPACITY + 5) as u64)
        );

        let last = snapshot.last().expect("last entry");
        assert_eq!(last.output, "hi");
        assert_eq!(last.status, "ok");
        assert_eq!(last.completion_tokens, 2);
    }

    #[test]
    fn failed_entry_records_error() {
        let log = Arc::new(LiveLog::new());
        let handle = log.begin(seed(0));
        handle.fail("upstream exploded");

        let snapshot = log.snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].status, "error");
        assert_eq!(snapshot[0].error.as_deref(), Some("upstream exploded"));
    }

    #[tokio::test]
    async fn broadcast_emits_begin_delta_and_end() {
        let log = Arc::new(LiveLog::new());
        let mut rx = log.subscribe();

        let handle = log.begin(seed(0));
        handle.append_content("chunk");
        handle.finish(finish());

        assert!(matches!(rx.recv().await, Ok(LogEvent::Begin(_))));
        match rx.recv().await {
            Ok(LogEvent::Delta { content, .. }) => assert_eq!(content, "chunk"),
            other => panic!("expected delta, got {other:?}"),
        }
        assert!(matches!(rx.recv().await, Ok(LogEvent::End(_))));
    }
}
