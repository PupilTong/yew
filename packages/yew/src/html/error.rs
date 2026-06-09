use thiserror::Error;

/// Render Error.
///
/// Currently uninhabited: rendering cannot fail now that suspense has been
/// removed. Retained as the seam for re-introducing fallible rendering.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum RenderError {}

/// Render Result.
pub type RenderResult<T> = std::result::Result<T, RenderError>;
