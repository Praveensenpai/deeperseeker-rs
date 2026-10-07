use deeperseeker::infra::dsml::{find_dsml_block_start, parse_dsml, safe_unambiguous_len};

#[test]
fn test_parse_dsml_fullwidth_bash() {
    let raw = "I'll explore your projects directory.\n\n<｜｜DSML｜｜ calls>\n<｜｜DSML｜｜ invoke name=\"bash\">\n<｜｜DSML｜｜ parameter name=\"command\" string=\"true\">ls -la ~/Projects</｜｜DSML｜｜ parameter>\n</｜｜DSML｜｜ invoke>\n</｜｜DSML｜｜ calls>FINISHED";
    let parsed = parse_dsml(raw);
    assert_eq!(parsed.text_content, "I'll explore your projects directory.");
    assert_eq!(parsed.tool_calls.len(), 1);
    let call = &parsed.tool_calls[0];
    assert_eq!(call.function.name, "bash");
    assert_eq!(
        call.function.arguments,
        r#"{"command":"ls -la ~/Projects"}"#
    );
}

#[test]
fn test_parse_dsml_multiple_invokes() {
    let raw = "<｜DSML｜calls>\n<｜DSML｜invoke name=\"read_file\">\n<｜DSML｜parameter name=\"path\" string=\"true\">/foo.txt</｜DSML｜parameter>\n</｜DSML｜invoke>\n<｜DSML｜invoke name=\"write_file\">\n<｜DSML｜parameter name=\"path\" string=\"true\">/bar.txt</｜DSML｜parameter>\n</｜DSML｜invoke>\n</｜DSML｜calls>";
    let parsed = parse_dsml(raw);
    assert!(parsed.text_content.is_empty());
    assert_eq!(parsed.tool_calls.len(), 2);
    assert_eq!(parsed.tool_calls[0].function.name, "read_file");
    assert_eq!(parsed.tool_calls[1].function.name, "write_file");
}

#[test]
fn test_parse_dsml_non_string_param() {
    let raw =
        r#"<invoke name="test"><parameter name="count" string="false">42</parameter></invoke>"#;
    let parsed = parse_dsml(raw);
    assert_eq!(parsed.tool_calls.len(), 1);
    assert_eq!(parsed.tool_calls[0].function.arguments, r#"{"count":42}"#);
}

#[test]
fn test_safe_unambiguous_len() {
    assert_eq!(safe_unambiguous_len("hello world"), 11);
    assert_eq!(safe_unambiguous_len("hello <"), 6);
    assert_eq!(safe_unambiguous_len("hello <｜"), 6);
    assert_eq!(safe_unambiguous_len("hello <｜｜DSML"), 6);
    assert_eq!(safe_unambiguous_len("5 < 10"), 6);
}

#[test]
fn test_safe_unambiguous_len_multibyte_utf8() {
    // em-dashes, en-dashes, and emojis near boundary
    let sample = "anime4k-cli – Rust tool that manages Anime4K GLSL shaders for mpv.\ndubstrip — Lossless audio-stream stri 🦀";
    let len = safe_unambiguous_len(sample);
    assert_eq!(len, sample.len());
}

#[test]
fn test_find_dsml_block_start() {
    assert_eq!(find_dsml_block_start("Hello <｜｜DSML｜｜ calls>"), Some(6));
    assert_eq!(find_dsml_block_start("<|dsml|calls>"), Some(0));
    assert_eq!(find_dsml_block_start("Let's edit <tool_call>"), Some(11));
    assert_eq!(find_dsml_block_start("Plain text"), None);
}

#[test]
fn test_parse_hermes_tool_call() {
    let raw = "I'll update the configuration file.\n<tool_call>\n<function=edit>\n<parameter=filePath>/tmp/config.rs</parameter>\n<parameter=oldString>timeout: 5</parameter>\n<parameter=newString>timeout: 10</parameter>\n</function>\n</tool_call>";
    let parsed = parse_dsml(raw);
    assert_eq!(parsed.text_content, "I'll update the configuration file.");
    assert_eq!(parsed.tool_calls.len(), 1);
    let call = &parsed.tool_calls[0];
    assert_eq!(call.function.name, "edit");
    assert_eq!(
        call.function.arguments,
        r#"{"filePath":"/tmp/config.rs","newString":"timeout: 10","oldString":"timeout: 5"}"#
    );
}

#[test]
fn test_parse_json_tool_call() {
    let raw = "<tool_call>{\"name\": \"bash\", \"arguments\": {\"command\": \"cargo check\"}}</tool_call>";
    let parsed = parse_dsml(raw);
    assert!(parsed.text_content.is_empty());
    assert_eq!(parsed.tool_calls.len(), 1);
    assert_eq!(parsed.tool_calls[0].function.name, "bash");
    assert_eq!(
        parsed.tool_calls[0].function.arguments,
        r#"{"command":"cargo check"}"#
    );
}

#[test]
fn test_parse_json_tool_call_with_nested_objects_and_code() {
    let raw = "Here is the edit.\n<tool_call>{\"name\": \"edit\", \"arguments\": {\"filePath\": \"src/services.py\", \"oldString\": \"def foo(): { return 1; }\", \"newString\": \"def bar(): { return 2; }\"}}</｜｜DSML｜｜ calls>";
    let parsed = parse_dsml(raw);
    assert_eq!(parsed.text_content, "Here is the edit.");
    assert_eq!(parsed.tool_calls.len(), 1);
    assert_eq!(parsed.tool_calls[0].function.name, "edit");
    assert!(parsed.tool_calls[0]
        .function
        .arguments
        .contains("src/services.py"));
    assert!(parsed.tool_calls[0]
        .function
        .arguments
        .contains("def foo(): { return 1; }"));
}

#[test]
fn test_parse_multiple_json_tool_calls_mixed_tags() {
    let raw = "Reading files.\n<tool_call>{\"name\": \"read\", \"arguments\": {\"filePath\": \"a.py\"}}</tool_call>\n<tool_call>{\"name\": \"read\", \"arguments\": {\"filePath\": \"b.py\"}}</call>\n</｜DSML｜ invoke>";
    let parsed = parse_dsml(raw);
    assert_eq!(parsed.text_content, "Reading files.");
    assert_eq!(parsed.tool_calls.len(), 2);
    assert_eq!(parsed.tool_calls[0].function.name, "read");
    assert_eq!(parsed.tool_calls[1].function.name, "read");
}

#[test]
fn test_parse_dsml_malformed_parameter_tags() {
    let raw = r#"<｜DSML｜invoke name="edit"><｜DSML｜parameter name="filePath" string="true">src/services.py</｜｜DSML｜｜ parameter name="oldString" string="true">def old(): pass</｜｜DSML｜｜ parameter name="newString" string="true">def new(): pass</｜｜DSML｜｜ invoke>"#;
    let parsed = parse_dsml(raw);
    assert_eq!(parsed.tool_calls.len(), 1);
    let call = &parsed.tool_calls[0];
    assert_eq!(call.function.name, "edit");
    assert!(call
        .function
        .arguments
        .contains(r#""filePath":"src/services.py""#));
    assert!(call
        .function
        .arguments
        .contains(r#""oldString":"def old(): pass""#));
    assert!(call
        .function
        .arguments
        .contains(r#""newString":"def new(): pass""#));
}

#[test]
fn test_parse_hybrid_spaced_dsml_leak() {
    let raw = r#"Regression test passes.

<tool_call>{"name": "shell", "arguments": {"command": "curl http://localhost:8096"}
< / | DSML | | parameter>
< / | DSML | | invoke>
< | | DSML | | invoke name="shell" string="false">true< / | DSML | | parameter>
< | | DSML | | parameter name="arguments">{"command": "cargo build --release"}
< / | DSML | | invoke>
< / | DSML | | calls>"#;
    let parsed = parse_dsml(raw);
    assert_eq!(parsed.text_content, "Regression test passes.");
    assert_eq!(parsed.tool_calls.len(), 2);
    assert_eq!(parsed.tool_calls[0].function.name, "shell");
    assert!(parsed.tool_calls[0]
        .function
        .arguments
        .contains("curl http://localhost:8096"));
    assert_eq!(parsed.tool_calls[1].function.name, "shell");
    assert!(parsed.tool_calls[1]
        .function
        .arguments
        .contains("cargo build --release"));
}
