use std::fmt;

use thiserror::Error;

use crate::{CancellationToken, ProviderId, SearchQuery, SearchResult};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProviderClass {
    /// In-memory/local providers that should start on every keypress.
    Instant,
    /// Potentially expensive providers that may apply their own short debounce.
    Delayed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderDescriptor {
    pub id: ProviderId,
    pub display_name: String,
    pub class: ProviderClass,
    pub default_priority: i32,
}

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider search was cancelled")]
    Cancelled,
    #[error("{0}")]
    Message(String),
}

impl ProviderError {
    pub fn message(value: impl fmt::Display) -> Self {
        Self::Message(value.to_string())
    }
}

/// Failure-isolated provider boundary.
pub trait Provider: Send + Sync + 'static {
    fn descriptor(&self) -> &ProviderDescriptor;

    fn search(
        &self,
        query: &SearchQuery,
        cancellation: &CancellationToken,
    ) -> Result<Vec<SearchResult>, ProviderError>;
}
