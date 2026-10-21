//! Export Collection Runner reports to JUnit XML and self-contained HTML.

use super::runner::RunnerReport;

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Format a `RunnerReport` as standard JUnit XML for CI/CD integration.
pub fn to_junit_xml(collection_name: &str, rep: &RunnerReport) -> String {
    let total_tests = rep.results.len();
    let failures = rep.failed_count();
    let total_ms: u128 = rep.results.iter().filter_map(|r| r.elapsed_ms).sum();
    let total_secs = (total_ms as f64) / 1000.0;
    let col_esc = xml_escape(collection_name);

    let mut xml = String::with_capacity(2048);
    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str(&format!(
        "<testsuites name=\"{col_esc}\" tests=\"{total_tests}\" failures=\"{failures}\" time=\"{total_secs:.3}\">\n"
    ));
    xml.push_str(&format!(
        "  <testsuite name=\"{col_esc}\" tests=\"{total_tests}\" failures=\"{failures}\" time=\"{total_secs:.3}\">\n"
    ));

    for item in &rep.results {
        let tc_name = xml_escape(&format!(
            "[Iter {}] {} {}",
            item.iteration, item.method, item.name
        ));
        let classname = xml_escape(&format!("{}.{}", collection_name, item.method));
        let item_secs = item.elapsed_ms.unwrap_or(0) as f64 / 1000.0;

        xml.push_str(&format!(
            "    <testcase name=\"{tc_name}\" classname=\"{classname}\" time=\"{item_secs:.3}\">\n"
        ));

        if !item.passed {
            let reason = if let Some(ref err) = item.error {
                err.clone()
            } else {
                let failed_assertions: Vec<String> = item
                    .test_results
                    .iter()
                    .filter(|t| !t.passed)
                    .map(|t| format!("{}: {}", t.name, t.message))
                    .collect();
                if !failed_assertions.is_empty() {
                    failed_assertions.join("; ")
                } else {
                    format!(
                        "HTTP status {}",
                        item.status
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "none".into())
                    )
                }
            };
            let msg_esc = xml_escape(&reason);
            xml.push_str(&format!(
                "      <failure message=\"{msg_esc}\">{msg_esc}</failure>\n"
            ));
        }

        if !item.test_results.is_empty() {
            xml.push_str("      <system-out>");
            for t in &item.test_results {
                let tag = if t.passed { "PASS" } else { "FAIL" };
                xml.push_str(&xml_escape(&format!(
                    "[{tag}] {} — {}\n",
                    t.name, t.message
                )));
            }
            xml.push_str("</system-out>\n");
        }

        xml.push_str("    </testcase>\n");
    }

    if !rep.console.is_empty() {
        xml.push_str("    <system-out>");
        xml.push_str(&xml_escape(&rep.console.join("\n")));
        xml.push_str("</system-out>\n");
    }

    xml.push_str("  </testsuite>\n");
    xml.push_str("</testsuites>\n");
    xml
}

/// Format a `RunnerReport` as a self-contained dark-mode HTML report.
pub fn to_html(collection_name: &str, rep: &RunnerReport) -> String {
    let total = rep.results.len();
    let passed = rep.passed_count();
    let failed = rep.failed_count();
    let total_ms: u128 = rep.results.iter().filter_map(|r| r.elapsed_ms).sum();
    let col_esc = html_escape(collection_name);

    let mut rows_html = String::new();
    for r in &rep.results {
        let badge_cls = if r.passed { "badge-pass" } else { "badge-fail" };
        let badge_txt = if r.passed { "PASS" } else { "FAIL" };
        let status_str = r
            .status
            .map(|s| s.to_string())
            .or_else(|| r.error.clone())
            .unwrap_or_else(|| "—".into());
        let ms_str = r
            .elapsed_ms
            .map(|ms| format!("{ms} ms"))
            .unwrap_or_else(|| "—".into());

        let mut tests_sub = String::new();
        if !r.test_results.is_empty() {
            tests_sub.push_str("<div class=\"assertion-list\">");
            for t in &r.test_results {
                let cls = if t.passed { "a-pass" } else { "a-fail" };
                let mark = if t.passed { "✓" } else { "✕" };
                tests_sub.push_str(&format!(
                    "<div class=\"{cls}\">{mark} {} — {}</div>",
                    html_escape(&t.name),
                    html_escape(&t.message)
                ));
            }
            tests_sub.push_str("</div>");
        }

        rows_html.push_str(&format!(
            "<tr>
                <td>{}</td>
                <td><span class=\"badge {badge_cls}\">{badge_txt}</span></td>
                <td><span class=\"method\">{}</span></td>
                <td>
                    <div class=\"req-name\">{}</div>
                    <div class=\"req-url\">{}</div>
                    {tests_sub}
                </td>
                <td>{}</td>
                <td>{}</td>
            </tr>\n",
            r.iteration,
            html_escape(&r.method),
            html_escape(&r.name),
            html_escape(&r.url),
            html_escape(&status_str),
            html_escape(&ms_str),
        ));
    }

    let console_esc = html_escape(&rep.console.join("\n"));

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<title>Blitz Runner Report — {col_esc}</title>
<style>
  :root {{
    --bg: #0c0d10;
    --panel: #14161b;
    --border: #262932;
    --text: #e6e8ee;
    --muted: #8b91a1;
    --pass: #23d18b;
    --fail: #f14c4c;
    --accent: #f59e0b;
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0;
    padding: 32px;
    background: var(--bg);
    color: var(--text);
    font-family: -apple-system, BlinkMacSystemFont, "Inter", "Segoe UI", sans-serif;
    font-size: 13px;
    line-height: 1.5;
  }}
  .container {{ max-width: 1080px; margin: 0 auto; }}
  header {{
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    border-bottom: 1px solid var(--border);
    padding-bottom: 16px;
    margin-bottom: 24px;
  }}
  h1 {{ margin: 0; font-size: 20px; font-weight: 700; }}
  .subtitle {{ color: var(--muted); font-size: 12px; }}
  .cards {{
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 12px;
    margin-bottom: 24px;
  }}
  .card {{
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px 16px;
  }}
  .card-label {{ color: var(--muted); font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; }}
  .card-val {{ font-size: 22px; font-weight: 700; margin-top: 4px; font-family: ui-monospace, monospace; }}
  .val-pass {{ color: var(--pass); }}
  .val-fail {{ color: var(--fail); }}
  table {{
    width: 100%;
    border-collapse: collapse;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
  }}
  th, td {{
    padding: 10px 12px;
    text-align: left;
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }}
  th {{
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--muted);
    background: rgba(255,255,255,0.02);
  }}
  .badge {{
    display: inline-block;
    padding: 2px 8px;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 700;
    font-family: ui-monospace, monospace;
  }}
  .badge-pass {{ background: rgba(35, 209, 139, 0.15); color: var(--pass); }}
  .badge-fail {{ background: rgba(241, 76, 76, 0.15); color: var(--fail); }}
  .method {{
    font-family: ui-monospace, monospace;
    font-weight: 700;
    color: var(--accent);
  }}
  .req-name {{ font-weight: 600; }}
  .req-url {{ color: var(--muted); font-family: ui-monospace, monospace; font-size: 11px; word-break: break-all; }}
  .assertion-list {{ margin-top: 6px; font-size: 11px; font-family: ui-monospace, monospace; }}
  .a-pass {{ color: var(--pass); }}
  .a-fail {{ color: var(--fail); }}
  pre {{
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px;
    overflow-x: auto;
    font-family: ui-monospace, monospace;
    font-size: 12px;
    color: var(--muted);
  }}
</style>
</head>
<body>
  <div class="container">
    <header>
      <div>
        <h1>⚡ Blitz Collection Runner Report</h1>
        <div class="subtitle">Collection: {col_esc}</div>
      </div>
      <div class="subtitle">Generated by Blitz</div>
    </header>
    <div class="cards">
      <div class="card">
        <div class="card-label">Total Requests</div>
        <div class="card-val">{total}</div>
      </div>
      <div class="card">
        <div class="card-label">Passed</div>
        <div class="card-val val-pass">{passed}</div>
      </div>
      <div class="card">
        <div class="card-label">Failed</div>
        <div class="card-val val-fail">{failed}</div>
      </div>
      <div class="card">
        <div class="card-label">Total Time</div>
        <div class="card-val">{total_ms} ms</div>
      </div>
    </div>
    <table>
      <thead>
        <tr>
          <th>Iter</th>
          <th>Result</th>
          <th>Method</th>
          <th>Request &amp; Assertions</th>
          <th>Status</th>
          <th>Time</th>
        </tr>
      </thead>
      <tbody>
        {rows_html}
      </tbody>
    </table>
    <h3 style="margin-top: 24px;">Console Output</h3>
    <pre>{console_esc}</pre>
  </div>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::TestResult;
    use crate::runtime::RunnerItemResult;
    use uuid::Uuid;

    fn sample_report() -> RunnerReport {
        RunnerReport {
            results: vec![
                RunnerItemResult {
                    request_id: Uuid::nil(),
                    name: "Get Users & Admins".into(),
                    method: "GET".into(),
                    url: "https://api.example.com/users?a=1&b=2".into(),
                    status: Some(200),
                    elapsed_ms: Some(120),
                    passed: true,
                    error: None,
                    test_results: vec![TestResult {
                        name: "status is 200".into(),
                        passed: true,
                        message: "ok".into(),
                    }],
                    iteration: 1,
                },
                RunnerItemResult {
                    request_id: Uuid::nil(),
                    name: "Create <Item>".into(),
                    method: "POST".into(),
                    url: "https://api.example.com/items".into(),
                    status: Some(500),
                    elapsed_ms: Some(45),
                    passed: false,
                    error: None,
                    test_results: vec![TestResult {
                        name: "status < 400".into(),
                        passed: false,
                        message: "expected < 400, got 500".into(),
                    }],
                    iteration: 1,
                },
            ],
            console: vec!["runner: done".into()],
        }
    }

    #[test]
    fn junit_xml_is_well_formed_and_escaped() {
        let rep = sample_report();
        let xml = to_junit_xml("Core & Auth", &rep);
        assert!(xml.starts_with("<?xml version=\"1.0\""));
        assert!(xml.contains("name=\"Core &amp; Auth\""));
        assert!(xml.contains("tests=\"2\" failures=\"1\""));
        assert!(xml.contains("<failure message=\"status &lt; 400: expected &lt; 400, got 500\">"));
        assert!(xml.ends_with("</testsuites>\n"));
    }

    #[test]
    fn html_report_contains_summary_and_escaped_names() {
        let rep = sample_report();
        let html = to_html("Core & Auth", &rep);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("Core &amp; Auth"));
        assert!(html.contains("Create &lt;Item&gt;"));
        assert!(html.contains("badge-pass"));
        assert!(html.contains("badge-fail"));
    }
}
