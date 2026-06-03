use yew::prelude::*;

const SVG: &str = inline_image!("tests/inline_image_macro/square.svg");
const CUSTOM_MIME: &str = inline_image!("tests/inline_image_macro/raw.image", "image/custom");

#[test]
fn inlines_image_with_inferred_mime_type() {
    assert_eq!(
        SVG,
        "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxIiBoZWlnaHQ9IjEiLz4K",
    );
}

#[test]
fn inlines_image_with_explicit_mime_type() {
    assert_eq!(CUSTOM_MIME, "data:image/custom;base64,aW1hZ2UtYnl0ZXMK");
}
