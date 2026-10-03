use deeperseeker::api::chat_stream::extract_chunks_from_event;
use serde_json::json;

#[test]
fn test_sse_event_parsing() {
    let mut think_open = false;

    // Test 1: BATCH event with response/fragments
    let batch_event = json!({
        "o": "BATCH",
        "v": [
            {
                "p": "response/fragments",
                "o": "APPEND",
                "v": [
                    {"type": "RESPONSE", "content": "Hello"}
                ]
            }
        ]
    });

    let chunks = extract_chunks_from_event(&batch_event, &mut think_open);
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].content.as_deref(), Some("Hello"));

    // Test 2: string delta for fragment 0 (think)
    let string_delta_think = json!({
        "p": "response/fragments/0/content",
        "o": "APPEND",
        "v": "thinking..."
    });
    let chunks2 = extract_chunks_from_event(&string_delta_think, &mut think_open);
    assert_eq!(chunks2.len(), 1);

    // Test 3: string delta for fragment 1 (response)
    let string_delta_resp = json!({
        "p": "response/fragments/1/content",
        "o": "APPEND",
        "v": "15"
    });
    let chunks3 = extract_chunks_from_event(&string_delta_resp, &mut think_open);
    assert_eq!(chunks3.len(), 1);
    assert_eq!(chunks3[0].content.as_deref(), Some("15"));
}
