use crate::error::LLMError;
use crate::request::LLMRequest;
use async_trait::async_trait;
use futures::Stream;
use maukode_protocol::LLMEvent;
use std::pin::Pin;

pub type LLMStream = Pin<Box<dyn Stream<Item = Result<LLMEvent, LLMError>> + Send>>;

#[async_trait]
pub trait LLMClient: Send + Sync {
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError>;
}
