use deeperseeker::api::chat_stream::extract_chunks_from_event;
use serde_json::json;

#[test]
fn test_sse_content_extraction() {
    let mut think_open = false;

    let batch_event = json!({
        "o": "BATCH",
        "v": [{
            "p": "response/fragments",
            "o": "APPEND",
            "v": [{"type": "RESPONSE", "content": "Hello"}]
        }]
    });
    let chunks = extract_chunks_from_event(&batch_event, &mut think_open);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content.as_deref(), Some("Hello"));

    let think_delta = json!({
        "p": "response/fragments/0/content",
        "o": "APPEND",
        "v": "thinking..."
    });
    let chunks2 = extract_chunks_from_event(&think_delta, &mut think_open);
    assert_eq!(chunks2.len(), 1);

    let resp_delta = json!({
        "p": "response/fragments/1/content",
        "o": "APPEND",
        "v": "15"
    });
    let chunks3 = extract_chunks_from_event(&resp_delta, &mut think_open);
    assert_eq!(chunks3.len(), 1);
    assert_eq!(chunks3[0].content.as_deref(), Some("15"));
}

#[test]
fn test_sse_status_events_filtering() {
    let mut think_open = false;

    let quasi_status = json!({
        "p": "quasi_status",
        "v": "FINISHED"
    });
    let chunks = extract_chunks_from_event(&quasi_status, &mut think_open);
    assert!(chunks.is_empty(), "quasi_status must not produce chunks");

    let batch_status = json!({
        "p": "response",
        "o": "BATCH",
        "v": [
            {"p": "accumulated_token_usage", "v": 39},
            {"p": "quasi_status", "v": "FINISHED"}
        ]
    });
    let chunks2 = extract_chunks_from_event(&batch_status, &mut think_open);
    assert!(
        chunks2.is_empty(),
        "batch status events must not produce chunks"
    );

    let set_status = json!({
        "p": "response/status",
        "o": "SET",
        "v": "FINISHED"
    });
    let chunks3 = extract_chunks_from_event(&set_status, &mut think_open);
    assert!(
        chunks3.is_empty(),
        "response/status must not produce chunks"
    );
}

#[test]
fn test_sse_error_detection() {
    use deeperseeker::infra::sse::{parse_sse_line, SseLineResult};
    let mut think_open = false;

    let err_line =
        "data: {\"code\": 40005, \"msg\": \"There is a message being generated\", \"data\": null}";
    match parse_sse_line(err_line, &mut think_open) {
        SseLineResult::Error(code, msg) => {
            assert_eq!(code, 40005);
            assert_eq!(msg, "There is a message being generated");
        }
        _ => panic!("Expected SseLineResult::Error for non-zero code"),
    }
}
