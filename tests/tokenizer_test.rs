use deeperseeker::infra::tokenizer::Tokenizer;

#[test]
fn tokenizer_loads_from_bundled_asset() {
    let tokenizer = Tokenizer::load();
    assert!(
        tokenizer.is_exact(),
        "bundled assets/tokenizer.json must load the real tokenizer"
    );
}

#[test]
fn real_tokenizer_counts_english_and_code() {
    let tokenizer = Tokenizer::load();
    assert!(tokenizer.is_exact(), "requires the real tokenizer");

    // "Hello, world!" is a stable short English string.
    let hello = tokenizer.count("Hello, world!");
    assert!(
        (2..=6).contains(&hello),
        "unexpected token count for greeting: {hello}"
    );

    // Longer text must count more than shorter text.
    let short = tokenizer.count("hello");
    let long = tokenizer.count("hello hello hello hello hello");
    assert!(long > short, "longer text must tokenize to more tokens");

    // Empty input is zero; count_min_one clamps to at least one.
    assert_eq!(tokenizer.count(""), 0);
    assert_eq!(tokenizer.count_min_one(""), 1);
}

#[test]
fn real_tokenizer_handles_multibyte_text() {
    let tokenizer = Tokenizer::load();
    assert!(tokenizer.is_exact(), "requires the real tokenizer");

    let count = tokenizer.count("日本語のテキストです");
    assert!(count > 0, "multibyte text must tokenize to >= 1 token");
}
