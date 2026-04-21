//! Compiler error types.

use ferrython_ast::SourceLocation;

/// Errors that can occur during compilation.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("syntax error at {location}: {message}")]
    SyntaxError {
        message: String,
        location: SourceLocation,
    },

    #[error("name error: {message}")]
    NameError { message: String },

    #[error("unsupported feature at {location}: {feature}")]
    Unsupported {
        feature: String,
        location: SourceLocation,
    },

    #[error("invalid assignment target at {location}")]
    InvalidAssignTarget { location: SourceLocation },

    #[error("'break' outside loop at {location}")]
    BreakOutsideLoop { location: SourceLocation },

    #[error("'continue' not properly in loop at {location}")]
    ContinueOutsideLoop { location: SourceLocation },

    #[error("'return' outside function at {location}")]
    ReturnOutsideFunction { location: SourceLocation },

    #[error("'yield' outside function at {location}")]
    YieldOutsideFunction { location: SourceLocation },

    #[error("can't delete function call at {location}")]
    CannotDeleteCall { location: SourceLocation },

    #[error("can't delete literal at {location}")]
    CannotDeleteLiteral { location: SourceLocation },

    #[error("can't delete expression at {location}")]
    CannotDeleteExpression { location: SourceLocation },

    #[error("name '{name}' is parameter and global at {location}")]
    ParameterAndGlobal { name: String, location: SourceLocation },

    #[error("name '{name}' is parameter and nonlocal at {location}")]
    ParameterAndNonlocal { name: String, location: SourceLocation },

    #[error("internal compiler error: {0}")]
    Internal(String),
}

impl CompileError {
    pub fn syntax(message: impl Into<String>, location: SourceLocation) -> Self {
        Self::SyntaxError {
            message: message.into(),
            location,
        }
    }

    pub fn unsupported(feature: impl Into<String>, location: SourceLocation) -> Self {
        Self::Unsupported {
            feature: feature.into(),
            location,
        }
    }
}
