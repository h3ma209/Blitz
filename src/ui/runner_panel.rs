use dioxus::prelude::*;
use uuid::Uuid;

use crate::core::Collection;
use crate::runtime::RunnerReport;
use crate::ui::ModalShell;

#[derive(Clone, PartialEq)]
pub enum RunnerEvent {
    Close,
    Run {
        collection_id: Uuid,
        iterations: usize,
        delay_ms: u64,
        data_file: String,
    },
}

#[component]
pub fn RunnerPanel(
    collections: Vec<Collection>,
    selected_id: Option<Uuid>,
    running: bool,
    report: Option<RunnerReport>,
    on_event: EventHandler<RunnerEvent>,
) -> Element {
    let mut sel = use_signal(|| selected_id.or_else(|| collections.first().map(|c| c.id)));
    let mut iterations = use_signal(|| 1usize);
    let mut delay_ms = use_signal(|| 0u64);
    let mut data_file = use_signal(String::new);

    rsx! {
        ModalShell {
            title: "Collection Runner".to_string(),
            class: "runner-modal".to_string(),
            on_close: move |_| on_event.call(RunnerEvent::Close),
            div { class: "modal-body",
                p { class: "hint", "Tree order · env + cookies · collection/folder/request scripts · setNextRequest." }

                select {
                    value: "{sel().map(|id| id.to_string()).unwrap_or_default()}",
                    onchange: move |evt: Event<FormData>| {
                        if let Ok(id) = Uuid::parse_str(&evt.value()) {
                            sel.set(Some(id));
                        }
                    },
                    for c in collections.iter() {
                        option {
                            value: "{c.id}",
                            selected: sel() == Some(c.id),
                            "{c.name}"
                        }
                    }
                }

                label { class: "hint", "Iterations" }
                input {
                    r#type: "number",
                    value: "{iterations()}",
                    oninput: move |evt| {
                        if let Ok(n) = evt.value().parse::<usize>() {
                            iterations.set(n.max(1));
                        }
                    }
                }
                label { class: "hint", "Delay ms" }
                input {
                    r#type: "number",
                    value: "{delay_ms()}",
                    oninput: move |evt| {
                        if let Ok(n) = evt.value().parse::<u64>() {
                            delay_ms.set(n);
                        }
                    }
                }
                label { class: "hint", "Data file (CSV or JSON array)" }
                input {
                    r#type: "text",
                    placeholder: "/path/to/data.csv",
                    value: "{data_file()}",
                    oninput: move |evt| data_file.set(evt.value()),
                }

                div { class: "modal-actions",
                    button {
                        class: "btn btn-ghost",
                        onclick: move |_| on_event.call(RunnerEvent::Close),
                        "Close"
                    }
                    button {
                        class: "btn btn-send",
                        disabled: running || sel().is_none(),
                        onclick: move |_| {
                            if let Some(id) = sel() {
                                on_event.call(RunnerEvent::Run {
                                    collection_id: id,
                                    iterations: iterations(),
                                    delay_ms: delay_ms(),
                                    data_file: data_file(),
                                });
                            }
                        },
                        if running { "Running…" } else { "Run collection" }
                    }
                }

                if let Some(rep) = report.as_ref() {
                    p { class: "hint",
                        "Pass {rep.passed_count()} · Fail {rep.failed_count()}"
                    }
                    table { class: "kv-table runner-table",
                        thead {
                            tr {
                                th { "Iter" }
                                th { "Result" }
                                th { "Method" }
                                th { "Name" }
                                th { "Status" }
                                th { "Time" }
                            }
                        }
                        tbody {
                            for r in rep.results.iter() {
                                tr {
                                    td { "{r.iteration}" }
                                    td {
                                        span {
                                            class: if r.passed { "status-chip status-ok" } else { "status-chip status-err" },
                                            span { class: "status-tag", if r.passed { "OK" } else { "Fail" } }
                                            if r.passed { "PASS" } else { "FAIL" }
                                        }
                                    }
                                    td { "{r.method}" }
                                    td { "{r.name}" }
                                    td {
                                        if let Some(s) = r.status {
                                            "{s}"
                                        } else if let Some(e) = &r.error {
                                            span { class: "status-err", "{e}" }
                                        } else {
                                            "—"
                                        }
                                    }
                                    td {
                                        if let Some(ms) = r.elapsed_ms {
                                            "{ms} ms"
                                        } else {
                                            "—"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    details {
                        summary { "Console" }
                        pre { class: "response-body", "{rep.console.join(\"\\n\")}" }
                    }
                }
            }
        }
    }
}
