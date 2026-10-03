use deeperseeker::domain::openai::{ChatCompletionRequest, MessageContent};
use deeperseeker::infra::deepseek_client::DeepSeekClient;
use deeperseeker::infra::media::{detect_image_format, parse_image_source};

#[tokio::test]
async fn test_parse_data_uri_png() {
    let client = DeepSeekClient::new();
    let data_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let decoded = parse_image_source(&client, data_uri).await.unwrap();

    assert_eq!(decoded.mime_type, "image/png");
    assert!(decoded.filename.ends_with(".png"));
    assert!(!decoded.bytes.is_empty());
}

#[tokio::test]
async fn test_parse_raw_base64_jpeg() {
    let client = DeepSeekClient::new();
    // 1x1 JPEG minimal bytes encoded as base64
    let raw_b64 = "/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=";
    let decoded = parse_image_source(&client, raw_b64).await.unwrap();

    assert_eq!(decoded.mime_type, "image/jpeg");
    assert!(decoded.filename.ends_with(".jpg"));
}

#[test]
fn test_openai_payload_with_image_url() {
    let json_str = r#"{
        "model": "v4.1flash",
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe this image:"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="}}
                ]
            }
        ]
    }"#;

    let req: ChatCompletionRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(req.messages.len(), 1);

    if let MessageContent::Parts(parts) = &req.messages[0].content {
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].r#type, "text");
        assert_eq!(parts[0].text.as_deref(), Some("Describe this image:"));
        assert_eq!(parts[1].r#type, "image_url");
        assert!(parts[1].image_url.is_some());
    } else {
        panic!("Expected Parts variant");
    }
}

#[test]
fn test_detect_image_magic_bytes() {
    let png_magic = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let (mime, ext) = detect_image_format(&png_magic);
    assert_eq!(mime, "image/png");
    assert_eq!(ext, "png");

    let jpg_magic = [0xFF, 0xD8, 0xFF, 0xE0];
    let (mime, ext) = detect_image_format(&jpg_magic);
    assert_eq!(mime, "image/jpeg");
    assert_eq!(ext, "jpg");
}

#[test]
fn test_opencode_file_payload() {
    let json_str = r#"{
        "model": "v4.1flash",
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "file",
                        "mime": "image/png",
                        "url": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC",
                        "filename": "test_square.png"
                    },
                    {
                        "type": "text",
                        "text": "What color is this image?"
                    }
                ]
            }
        ]
    }"#;

    let req: ChatCompletionRequest = serde_json::from_str(json_str).unwrap();
    assert_eq!(req.messages.len(), 1);

    if let MessageContent::Parts(parts) = &req.messages[0].content {
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].r#type, "file");
        assert!(parts[0].url.is_some());
        assert_eq!(parts[0].mime.as_deref(), Some("image/png"));
        assert_eq!(parts[0].filename.as_deref(), Some("test_square.png"));
    } else {
        panic!("Expected Parts variant");
    }
}
