//! The Meta Model API (Responses API) as pure data: building requests and reading
//! responses, no networking here (the proxy client lives elsewhere).

pub mod intent;
pub mod profile;
mod prompts;
pub mod request;
pub mod response;
pub mod sam;
pub mod sse;
