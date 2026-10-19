//! The translators' view of internal/cache (ported in `cpa_common::cache`): the
//! Antigravity reasoning replay cache, which the Gemini Responses translator uses to keep
//! thought signatures that trail visible text out of the client-visible output, and the
//! thinking signature cache, which the Claude -> Antigravity translators use to recover
//! signatures of thinking text the client sent back unsigned.

pub use cpa_common::cache::set_signature_cache_config;
pub(crate) use cpa_common::cache::{
    cache_signature, cached_signature, has_valid_signature, model_group, replay_get as get, replay_put as put,
    signature_bypass_strict, signature_cache_enabled,
};
