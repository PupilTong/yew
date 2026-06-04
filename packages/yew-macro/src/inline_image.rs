use std::path::{Path, PathBuf};

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Token};

pub struct InlineImageInput {
    path: LitStr,
    mime_type: Option<LitStr>,
}

impl Parse for InlineImageInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let path = input.parse()?;
        let mime_type = if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            Some(input.parse()?)
        } else {
            None
        };

        if !input.is_empty() {
            return Err(input.error(
                "expected `inline_image!(\"path\")` or `inline_image!(\"path\", \"mime/type\")`",
            ));
        }

        Ok(Self { path, mime_type })
    }
}

impl InlineImageInput {
    pub fn into_token_stream(self) -> TokenStream {
        match self.try_into_token_stream() {
            Ok(tokens) => tokens,
            Err(err) => err.into_compile_error(),
        }
    }

    fn try_into_token_stream(self) -> syn::Result<TokenStream> {
        let path = self.path.value();
        if path.is_empty() {
            return Err(syn::Error::new(
                self.path.span(),
                "inline image path cannot be empty",
            ));
        }

        let mime_type = match self.mime_type {
            Some(mime_type) => {
                let mime_type_value = mime_type.value();
                if mime_type_value.is_empty() {
                    return Err(syn::Error::new(
                        mime_type.span(),
                        "inline image MIME type cannot be empty",
                    ));
                }
                mime_type_value
            }
            None => infer_image_mime_type(Path::new(&path)).ok_or_else(|| {
                syn::Error::new(
                    self.path.span(),
                    "could not infer image MIME type from file extension; pass it explicitly as the second argument",
                )
            })?,
        };

        let resolved_path = resolve_path(&path, self.path.span())?;
        let canonical_path = std::fs::canonicalize(&resolved_path).map_err(|err| {
            syn::Error::new(
                self.path.span(),
                format!(
                    "failed to resolve inline image `{}`: {err}",
                    resolved_path.display()
                ),
            )
        })?;
        let bytes = std::fs::read(&canonical_path).map_err(|err| {
            syn::Error::new(
                self.path.span(),
                format!(
                    "failed to read inline image `{}`: {err}",
                    canonical_path.display()
                ),
            )
        })?;

        let include_path = canonical_path.to_str().ok_or_else(|| {
            syn::Error::new(
                self.path.span(),
                "inline image path must be valid UTF-8 for include_bytes!",
            )
        })?;
        let include_path = LitStr::new(include_path, self.path.span());

        let data_url = format!("data:{mime_type};base64,{}", encode_base64(&bytes));
        let data_url = LitStr::new(&data_url, self.path.span());

        Ok(quote!({
            const _: &[u8] = include_bytes!(#include_path);
            #data_url
        }))
    }
}

fn resolve_path(path: &str, span: proc_macro2::Span) -> syn::Result<PathBuf> {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return Ok(path);
    }

    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| {
        syn::Error::new(
            span,
            "CARGO_MANIFEST_DIR is not set; inline_image! cannot resolve relative paths",
        )
    })?;

    Ok(PathBuf::from(manifest_dir).join(path))
}

fn infer_image_mime_type(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime_type = match extension.as_str() {
        "apng" => "image/apng",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "svg" | "svgz" => "image/svg+xml",
        "webp" => "image/webp",
        _ => return None,
    };

    Some(mime_type.to_owned())
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(((bytes.len() + 2) / 3) * 4);
    let mut chunks = bytes.chunks_exact(3);

    for chunk in &mut chunks {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        encoded.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        encoded.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        encoded.push(TABLE[(n & 0x3f) as usize] as char);
    }

    match chunks.remainder() {
        [a] => {
            let n = (*a as u32) << 16;
            encoded.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            encoded.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            encoded.push('=');
            encoded.push('=');
        }
        [a, b] => {
            let n = ((*a as u32) << 16) | ((*b as u32) << 8);
            encoded.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
            encoded.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
            encoded.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
            encoded.push('=');
        }
        [] => {}
        _ => unreachable!("chunks_exact remainder is at most two bytes"),
    }

    encoded
}
