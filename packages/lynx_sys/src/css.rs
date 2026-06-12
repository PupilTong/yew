//! CSS token metadata shared with Lynx style bindings.

/// CSS token type id used by the host-side tokenizer ABI.
pub type TokenType = u8;

pub const EOF_TOKEN: u8 = 0; // <EOF-token>
pub const IDENT_TOKEN: u8 = 1; // <ident-token>
pub const FUNCTION_TOKEN: u8 = 2; // <function-token>
pub const AT_KEYWORD_TOKEN: u8 = 3; // <at-keyword-token>
pub const HASH_TOKEN: u8 = 4; // <hash-token>
pub const STRING_TOKEN: u8 = 5; // <string-token>
pub const BAD_STRING_TOKEN: u8 = 6; // <bad-string-token>
pub const URL_TOKEN: u8 = 7; // <url-token>
pub const BAD_URL_TOKEN: u8 = 8; // <bad-url-token>
pub const DELIM_TOKEN: u8 = 9; // <delim-token>
pub const NUMBER_TOKEN: u8 = 10; // <number-token>
pub const PERCENTAGE_TOKEN: u8 = 11; // <percentage-token>
pub const DIMENSION_TOKEN: u8 = 12; // <dimension-token>
pub const WHITESPACE_TOKEN: u8 = 13; // <whitespace-token>
pub const CDO_TOKEN: u8 = 14; // <CDO-token>
pub const CDC_TOKEN: u8 = 15; // <CDC-token>
pub const COLON_TOKEN: u8 = 16; // <colon-token>
pub const SEMICOLON_TOKEN: u8 = 17; // <semicolon-token>
pub const COMMA_TOKEN: u8 = 18; // <comma-token>
pub const LEFT_SQUARE_BRACKET_TOKEN: u8 = 19; // <[-token>
pub const RIGHT_SQUARE_BRACKET_TOKEN: u8 = 20; // <]-token>
pub const LEFT_PARENTHESES_TOKEN: u8 = 21; // <(-token>
pub const RIGHT_PARENTHESES_TOKEN: u8 = 22; // <)-token>
pub const LEFT_CURLY_BRACKET_TOKEN: u8 = 23; // <{-token>
pub const RIGHT_CURLY_BRACKET_TOKEN: u8 = 24; // <}-token>
pub const COMMENT_TOKEN: u8 = 25; // <comment-token>

/// CSS token type enum mirroring the numeric tokenizer ABI.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CSSTokenType {
    /// <EOF-token>
    Eof = EOF_TOKEN,
    /// <ident-token>
    Ident = IDENT_TOKEN,
    /// <function-token>
    Function = FUNCTION_TOKEN,
    /// <at-keyword-token>
    AtKeyword = AT_KEYWORD_TOKEN,
    /// <hash-token>
    Hash = HASH_TOKEN,
    /// <string-token>
    String = STRING_TOKEN,
    /// <bad-string-token>
    BadString = BAD_STRING_TOKEN,
    /// <url-token>
    Url = URL_TOKEN,
    /// <bad-url-token>
    BadUrl = BAD_URL_TOKEN,
    /// <delim-token>
    Delim = DELIM_TOKEN,
    /// <number-token>
    Number = NUMBER_TOKEN,
    /// <percentage-token>
    Percentage = PERCENTAGE_TOKEN,
    /// <dimension-token>
    Dimension = DIMENSION_TOKEN,
    /// <whitespace-token>
    Whitespace = WHITESPACE_TOKEN,
    /// <CDO-token>
    Cdo = CDO_TOKEN,
    /// <CDC-token>
    Cdc = CDC_TOKEN,
    /// <colon-token>
    Colon = COLON_TOKEN,
    /// <semicolon-token>
    Semicolon = SEMICOLON_TOKEN,
    /// <comma-token>
    Comma = COMMA_TOKEN,
    /// <[-token>
    LeftSquareBracket = LEFT_SQUARE_BRACKET_TOKEN,
    /// <]-token>
    RightSquareBracket = RIGHT_SQUARE_BRACKET_TOKEN,
    /// <(-token>
    LeftParentheses = LEFT_PARENTHESES_TOKEN,
    /// <)-token>
    RightParentheses = RIGHT_PARENTHESES_TOKEN,
    /// <{-token>
    LeftCurlyBracket = LEFT_CURLY_BRACKET_TOKEN,
    /// <}-token>
    RightCurlyBracket = RIGHT_CURLY_BRACKET_TOKEN,
    /// <comment-token>
    Comment = COMMENT_TOKEN,
}

impl CSSTokenType {
    /// Returns the ABI token id.
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

impl From<CSSTokenType> for TokenType {
    fn from(token_type: CSSTokenType) -> Self {
        token_type.as_u8()
    }
}

/// One CSS token.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    /// Token kind, one of the `*_TOKEN` constants in this module.
    pub token_type: TokenType,
    /// Token payload, if the token carries source text.
    pub value: String,
}

impl Token {
    /// Creates a CSS token with a token type and source payload.
    pub fn new(token_type: impl Into<TokenType>, value: impl Into<String>) -> Self {
        Self {
            token_type: token_type.into(),
            value: value.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_type_ids_match_css_token_order() {
        assert_eq!(EOF_TOKEN, 0);
        assert_eq!(IDENT_TOKEN, 1);
        assert_eq!(COMMENT_TOKEN, 25);
        assert_eq!(CSSTokenType::Comment.as_u8(), COMMENT_TOKEN);
    }

    #[test]
    fn token_preserves_type_and_value() {
        let token = Token::new(IDENT_TOKEN, "display");

        assert_eq!(token.token_type, IDENT_TOKEN);
        assert_eq!(token.value, "display");
    }
}
