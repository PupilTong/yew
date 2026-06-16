#[test]
fn css_macro() {
    let t = trybuild::TestCases::new();
    t.pass("tests/css_macro/*-pass.rs");
}

#[test]
fn css_macro_returns_static_token_stream_bytes() {
    let tokens: yew::css::CSSTokenStream =
        yew::CSS!(".item { color: red; background-color: #fff; }");

    assert!(!tokens.is_empty());
    assert!(tokens.has_valid_header());
    assert!(tokens.token_count().is_some_and(|count| count > 0));
    assert!(tokens.payload_len().is_some_and(|len| len > 0));
    assert!(tokens
        .as_bytes()
        .starts_with(&yew::css::CSS_TOKEN_STREAM_MAGIC));
}
