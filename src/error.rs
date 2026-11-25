use thiserror::Error;

/// Custom error types for the Discord protection bot.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum BotError {
    #[error("Twilight HTTP error: {0}")]
    TwilightHttp(#[from] Box<twilight_http::Error>),

    #[error("Twilight Gateway error: {0}")]
    TwilightGateway(#[from] Box<twilight_gateway::error::ReceiveMessageError>),

    #[error("Serialization / Deserialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Missing permission or resource not found: {0}")]
    NotFound(String),

    #[error("Unauthorized action: {0}")]
    Unauthorized(String),

    #[error("Generic error: {0}")]
    Generic(#[from] anyhow::Error),
}

impl From<twilight_http::Error> for BotError {
    fn from(err: twilight_http::Error) -> Self {
        BotError::TwilightHttp(Box::new(err))
    }
}

impl From<twilight_gateway::error::ReceiveMessageError> for BotError {
    fn from(err: twilight_gateway::error::ReceiveMessageError) -> Self {
        BotError::TwilightGateway(Box::new(err))
    }
}

pub type BotResult<T> = Result<T, BotError>;
