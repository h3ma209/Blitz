//! Send pipeline: resolve vars → pre-request → auth → HTTP → tests.
//! Order from reports/03 + reports/07 (inferred Postman runtime convention).

#![allow(unused_imports)]

pub mod pipeline;
pub mod postman_defaults;
pub mod runner;

pub use pipeline::{
    send_request, send_request_with_options, send_request_with_secrets, ExecutionHints, SendError,
    SendOptions, SendResult,
};
pub use runner::{
    load_data_rows, ordered_requests, run_collection, run_collection_with_config, RunnerConfig,
    RunnerItemResult, RunnerReport,
};
