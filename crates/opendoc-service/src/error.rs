//! One error type for every way a service request can fail.
//!
//! Every variant maps to exactly one HTTP status and one stable wire code, so
//! a client can branch on the code and an operator can grep the log. The
//! *message* may name the resource; it never names why a credential failed,
//! because "no such subject" and "wrong key" must be indistinguishable.

use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceError {
    /// No credential, an expired session, or a credential that did not verify.
    Unauthenticated(String),
    /// Authenticated, but the server's own permission state says no.
    Forbidden(String),
    /// The request was well-formed but named something that does not exist.
    NotFound(String),
    /// The request was malformed: bad JSON, a missing field, an operation
    /// whose actor does not match its author.
    BadRequest(String),
    /// The request lost a race it is allowed to retry.
    Conflict(String),
    /// Durable storage refused.
    Storage(String),
    /// The document actor is gone, the runtime is shutting down, or an
    /// invariant the service maintains was violated.
    Internal(String),
}

impl ServiceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthenticated(_) => "unauthenticated",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not-found",
            Self::BadRequest(_) => "bad-request",
            Self::Conflict(_) => "conflict",
            Self::Storage(_) => "storage",
            Self::Internal(_) => "internal",
        }
    }

    pub fn status(&self) -> u16 {
        match self {
            Self::Unauthenticated(_) => 401,
            Self::Forbidden(_) => 403,
            Self::NotFound(_) => 404,
            Self::BadRequest(_) => 400,
            Self::Conflict(_) => 409,
            Self::Storage(_) | Self::Internal(_) => 500,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Unauthenticated(message)
            | Self::Forbidden(message)
            | Self::NotFound(message)
            | Self::BadRequest(message)
            | Self::Conflict(message)
            | Self::Storage(message)
            | Self::Internal(message) => message,
        }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for ServiceError {}

impl From<opendoc_store::StoreError> for ServiceError {
    fn from(error: opendoc_store::StoreError) -> Self {
        Self::Storage(error.to_string())
    }
}

pub type ServiceResult<T> = Result<T, ServiceError>;
