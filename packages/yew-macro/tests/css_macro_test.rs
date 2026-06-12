#[test]
fn css_macro() {
    let t = trybuild::TestCases::new();
    t.pass("tests/css_macro/*-pass.rs");
}

#[test]
fn css_macro_returns_tokens() {
    let tokens: Vec<yew::css::Token> = yew::CSS!(".item { color: red; background-color: #fff; }");

    assert!(!tokens.is_empty());
    assert!(tokens
        .iter()
        .any(|token| { token.token_type == yew::css::IDENT_TOKEN && token.value == "color" }));
    assert!(tokens
        .iter()
        .any(|token| token.token_type == yew::css::LEFT_CURLY_BRACKET_TOKEN));
}
