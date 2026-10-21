//! Send pipeline: resolve vars → pre-request → auth → HTTP → tests.
//! Order from reports/03 + reports/07 (inferred Postman runtime convention).

#![allow(unused_imports)]

pub mod decode_body;
pub mod pipeline;
pub mod postman_defaults;
pub mod runner;
pub mod runner_export;

pub use pipeline::{
    send_request_with_options, ExecutionHints, SendError,
    SendOptions, SendResult,
};
#[cfg(test)]
pub use pipeline::send_request;
pub use runner::{
    load_data_rows, ordered_requests, run_collection_with_config, RunnerConfig,
    RunnerItemResult, RunnerReport,
};
pub use runner_export::{to_html, to_junit_xml};
