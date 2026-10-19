//! Zero-Cloud Interactive Static API Documentation Exporter.
//! Compiles any Blitz collection into a single, self-contained, responsive
//! glassmorphic HTML documentation website with live search and multi-language snippets.

use crate::core::codegen::generate_snippet;
use crate::core::collection::{Collection, CollectionItem};
use crate::core::request::RequestDraft;

/// Compiles a collection into a standalone HTML file string.
pub fn generate_static_doc_html(collection: &Collection) -> String {
    let mut endpoints = Vec::new();
    collect_endpoints(&collection.items, &mut endpoints, "");

    let title = html_escape(&collection.name);
    let desc = html_escape(&collection.description);

    let mut nav_items = String::new();
    let mut sections = String::new();
    let mut last_folder = String::new();

    for (idx, (folder_path, req)) in endpoints.iter().enumerate() {
        let ep_id = format!("ep-{}", idx);
        let req_name = if req.name.is_empty() {
            "Untitled Request"
        } else {
            &req.name
        };
        let escaped_name = html_escape(req_name);
        let method = req.method.as_str();
        let method_class = match method {
            "GET" => "method-get",
            "POST" => "method-post",
            "PUT" => "method-put",
            "DELETE" => "method-delete",
            "PATCH" => "method-patch",
            _ => "method-other",
        };

        let current_folder_display = if folder_path.is_empty() {
            "Root / General"
        } else {
            folder_path.as_str()
        };

        // If folder changed, render folder heading in sidebar nav
        if current_folder_display != last_folder {
            last_folder = current_folder_display.to_string();
            let escaped_folder = html_escape(current_folder_display);
            nav_items.push_str(&format!(
                "<div class=\"nav-folder-heading\">\n\
                    <span class=\"folder-icon\">📁</span>\n\
                    <span>{escaped_folder}</span>\n\
                </div>"
            ));
        }

        // Navigation entry with folder sub-path and method tag
        nav_items.push_str(&format!(
            "<a href=\"#{ep_id}\" class=\"nav-item\">\n\
                <span class=\"badge {method_class}\">{method}</span>\n\
                <span class=\"nav-label\">{escaped_name}</span>\n\
            </a>"
        ));

        // Snippets in cURL, Rust, Python, TypeScript
        let curl_snippet = html_escape(&generate_snippet("cURL", req));
        let rust_snippet = html_escape(&generate_snippet("Rust (reqwest)", req));
        let py_snippet = html_escape(&generate_snippet("Python (httpx)", req));
        let ts_snippet = html_escape(&generate_snippet("TypeScript (axios)", req));

        // Headers table
        let mut headers_rows = String::new();
        for h in req.headers.iter().filter(|h| h.enabled && !h.key.is_empty()) {
            headers_rows.push_str(&format!(
                "<tr><td><code>{}</code></td><td>{}</td></tr>",
                html_escape(&h.key),
                html_escape(&h.value)
            ));
        }

        let headers_table = if headers_rows.is_empty() {
            String::new()
        } else {
            format!(
                r#"<div class="sub-section">
                    <h4>Headers</h4>
                    <table class="doc-table">
                        <thead><tr><th>Key</th><th>Value</th></tr></thead>
                        <tbody>{headers_rows}</tbody>
                    </table>
                </div>"#
            )
        };

        // Body section
        let body_section = if !req.body.is_empty() {
            format!(
                r#"<div class="sub-section">
                    <h4>Request Body</h4>
                    <pre class="code-block"><code>{}</code></pre>
                </div>"#,
                html_escape(&req.body)
            )
        } else {
            String::new()
        };

        // Description
        let doc_desc = if !req.description.is_empty() {
            format!(
                r#"<div class="ep-desc">{}</div>"#,
                html_escape(&req.description)
            )
        } else {
            String::new()
        };

        sections.push_str(&format!(
            r#"<section id="{ep_id}" class="endpoint-card">
                <div class="ep-header">
                    <span class="folder-tag">📁 {folder_tag}</span>
                    <h3 class="ep-title">{escaped_name}</h3>
                </div>
                {doc_desc}
                <div class="ep-url-bar">
                    <span class="badge {method_class}">{method}</span>
                    <span class="ep-url">{url}</span>
                </div>
                {headers_table}
                {body_section}
                <div class="sub-section">
                    <h4>Code Example</h4>
                    <div class="snippet-tabs" data-target="{ep_id}">
                        <button class="tab-btn active" onclick="switchTab('{ep_id}', 'curl')">cURL</button>
                        <button class="tab-btn" onclick="switchTab('{ep_id}', 'rust')">Rust</button>
                        <button class="tab-btn" onclick="switchTab('{ep_id}', 'python')">Python</button>
                        <button class="tab-btn" onclick="switchTab('{ep_id}', 'ts')">TypeScript</button>
                    </div>
                    <div id="{ep_id}-curl" class="tab-content active"><pre class="code-block"><code>{curl_snippet}</code></pre></div>
                    <div id="{ep_id}-rust" class="tab-content"><pre class="code-block"><code>{rust_snippet}</code></pre></div>
                    <div id="{ep_id}-python" class="tab-content"><pre class="code-block"><code>{py_snippet}</code></pre></div>
                    <div id="{ep_id}-ts" class="tab-content"><pre class="code-block"><code>{ts_snippet}</code></pre></div>
                </div>
            </section>"#,
            folder_tag = html_escape(current_folder_display),
            url = html_escape(&req.url),
        ));
    }

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>{title} — API Documentation</title>
  <style>
    :root {{
      --bg: #0c0d0e;
      --surface: #141618;
      --surface-border: rgba(255, 255, 255, 0.08);
      --text: #eaeaea;
      --text-muted: #8b949e;
      --green: #1ed760;
      --blue: #58a6ff;
      --yellow: #f1e05a;
      --red: #f85149;
      --purple: #bc8cff;
    }}
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    body {{
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      background: var(--bg);
      color: var(--text);
      display: flex;
      height: 100vh;
      overflow: hidden;
    }}
    aside {{
      width: 300px;
      background: var(--surface);
      border-right: 1px solid var(--surface-border);
      display: flex;
      flex-direction: column;
      flex-shrink: 0;
    }}
    .sidebar-header {{
      padding: 20px;
      border-bottom: 1px solid var(--surface-border);
    }}
    .sidebar-header h1 {{ font-size: 16px; font-weight: 700; color: #fff; margin-bottom: 4px; }}
    .sidebar-header p {{ font-size: 12px; color: var(--text-muted); }}
    .search-box {{
      padding: 12px 16px;
      border-bottom: 1px solid var(--surface-border);
    }}
    .search-input {{
      width: 100%;
      background: rgba(255, 255, 255, 0.04);
      border: 1px solid var(--surface-border);
      padding: 8px 12px;
      border-radius: 6px;
      color: #fff;
      font-size: 13px;
    }}
    .nav-list {{
      flex: 1;
      overflow-y: auto;
      padding: 12px;
      display: flex;
      flex-direction: column;
      gap: 4px;
    }}
    .nav-folder-heading {{
      display: flex;
      align-items: center;
      gap: 6px;
      font-size: 11px;
      font-weight: 700;
      color: var(--green);
      text-transform: uppercase;
      letter-spacing: 0.5px;
      padding: 14px 10px 6px 10px;
      border-top: 1px solid rgba(255, 255, 255, 0.05);
      margin-top: 4px;
    }}
    .nav-folder-heading:first-child {{
      border-top: none;
      margin-top: 0;
      padding-top: 4px;
    }}
    .nav-item {{
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      border-radius: 6px;
      text-decoration: none;
      color: var(--text-muted);
      font-size: 13px;
      transition: all 0.15s ease;
    }}
    .nav-item:hover {{
      background: rgba(255, 255, 255, 0.06);
      color: #fff;
    }}
    .badge {{
      font-size: 10px;
      font-weight: 700;
      padding: 2px 6px;
      border-radius: 4px;
      font-family: monospace;
      flex-shrink: 0;
    }}
    .method-get {{ background: rgba(30, 215, 96, 0.15); color: var(--green); }}
    .method-post {{ background: rgba(241, 224, 90, 0.15); color: var(--yellow); }}
    .method-put {{ background: rgba(88, 166, 255, 0.15); color: var(--blue); }}
    .method-delete {{ background: rgba(248, 81, 73, 0.15); color: var(--red); }}
    .method-patch {{ background: rgba(188, 140, 255, 0.15); color: var(--purple); }}
    .method-other {{ background: rgba(255, 255, 255, 0.1); color: #fff; }}
    main {{
      flex: 1;
      overflow-y: auto;
      padding: 40px;
    }}
    .endpoint-card {{
      background: var(--surface);
      border: 1px solid var(--surface-border);
      border-radius: 12px;
      padding: 24px;
      margin-bottom: 32px;
      scroll-margin-top: 24px;
    }}
    .folder-tag {{ font-size: 11px; color: var(--green); text-transform: uppercase; font-weight: 700; letter-spacing: 0.5px; }}
    .ep-title {{ font-size: 20px; font-weight: 700; margin: 4px 0 12px 0; }}
    .ep-desc {{ font-size: 14px; color: var(--text-muted); margin-bottom: 16px; line-height: 1.5; }}
    .ep-url-bar {{
      display: flex;
      align-items: center;
      gap: 12px;
      background: rgba(0,0,0,0.4);
      padding: 10px 14px;
      border-radius: 8px;
      font-family: monospace;
      font-size: 13px;
      margin-bottom: 20px;
      border: 1px solid var(--surface-border);
    }}
    .sub-section {{ margin-top: 20px; }}
    .sub-section h4 {{ font-size: 12px; font-weight: 700; text-transform: uppercase; color: var(--text-muted); margin-bottom: 8px; }}
    .doc-table {{
      width: 100%;
      border-collapse: collapse;
      font-size: 13px;
      margin-bottom: 16px;
    }}
    .doc-table th, .doc-table td {{
      text-align: left;
      padding: 8px 12px;
      border-bottom: 1px solid var(--surface-border);
    }}
    .doc-table th {{ color: var(--text-muted); font-size: 11px; text-transform: uppercase; }}
    .code-block {{
      background: #000;
      border: 1px solid var(--surface-border);
      border-radius: 8px;
      padding: 14px;
      overflow-x: auto;
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 12px;
      line-height: 1.5;
      color: #d1d5db;
    }}
    .snippet-tabs {{
      display: flex;
      gap: 4px;
      margin-bottom: 8px;
      border-bottom: 1px solid var(--surface-border);
      padding-bottom: 4px;
    }}
    .tab-btn {{
      background: transparent;
      border: none;
      color: var(--text-muted);
      font-size: 12px;
      padding: 4px 10px;
      border-radius: 4px;
      cursor: pointer;
    }}
    .tab-btn.active {{
      background: rgba(255, 255, 255, 0.08);
      color: #fff;
      font-weight: 600;
    }}
    .tab-content {{ display: none; }}
    .tab-content.active {{ display: block; }}
  </style>
</head>
<body>
  <aside>
    <div class="sidebar-header">
      <h1>{title}</h1>
      <p>{desc}</p>
    </div>
    <div class="search-box">
      <input type="text" id="filter-input" class="search-input" placeholder="Filter endpoints…" oninput="filterEndpoints(this.value)">
    </div>
    <nav id="nav-list" class="nav-list">
      {nav_items}
    </nav>
  </aside>
  <main>
    {sections}
  </main>
  <script>
    function switchTab(epId, lang) {{
      const langs = ['curl', 'rust', 'python', 'ts'];
      langs.forEach(l => {{
        const el = document.getElementById(epId + '-' + l);
        if (el) el.classList.remove('active');
      }});
      const target = document.getElementById(epId + '-' + lang);
      if (target) target.classList.add('active');

      const parent = target.parentElement;
      const btns = parent.querySelectorAll('.tab-btn');
      btns.forEach(b => {{
        b.classList.toggle('active', b.innerText.toLowerCase().includes(lang));
      }});
    }}

    function filterEndpoints(q) {{
      q = q.toLowerCase();
      const items = document.querySelectorAll('.nav-item');
      items.forEach(el => {{
        const text = el.innerText.toLowerCase();
        el.style.display = text.includes(q) ? 'flex' : 'none';
      }});
    }}
  </script>
</body>
</html>"#
    )
}

fn collect_endpoints<'a>(
    items: &'a [CollectionItem],
    out: &mut Vec<(String, &'a RequestDraft)>,
    current_folder: &str,
) {
    for item in items {
        match item {
            CollectionItem::Folder(f) => {
                let name = if f.name.is_empty() {
                    "Folder"
                } else {
                    &f.name
                };
                let nested_path = if current_folder.is_empty() {
                    name.to_string()
                } else {
                    format!("{current_folder} / {name}")
                };
                collect_endpoints(&f.items, out, &nested_path);
            }
            CollectionItem::Request(r) => {
                out.push((current_folder.to_string(), r));
            }
        }
    }
}

/// Compiles a collection into a standalone interactive Swagger UI HTML file.
pub fn generate_swagger_ui_html(collection: &Collection) -> Result<String, crate::core::impex::ImpexError> {
    let openapi_json = crate::core::impex::export_openapi3(collection)?;
    let escaped_title = html_escape(&collection.name);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{title} — Swagger UI</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
  <style>
    body {{
      margin: 0;
      padding: 0;
      background: #0d1117;
      color: #c9d1d9;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    }}
    .swagger-ui {{
      filter: invert(88%) hue-rotate(180deg);
    }}
    .swagger-ui .topbar {{
      display: none;
    }}
    .top-header {{
      background: #161b22;
      border-bottom: 1px solid #30363d;
      padding: 12px 24px;
      display: flex;
      align-items: center;
      justify-content: space-between;
    }}
    .top-header h1 {{
      margin: 0;
      font-size: 18px;
      color: #58a6ff;
      font-weight: 600;
      display: flex;
      align-items: center;
      gap: 8px;
    }}
    .top-header .tag {{
      background: #238636;
      color: #fff;
      font-size: 11px;
      font-weight: 700;
      padding: 2px 8px;
      border-radius: 12px;
      text-transform: uppercase;
      letter-spacing: 0.5px;
    }}
    .top-header .meta {{
      font-size: 12px;
      color: #8b949e;
    }}
  </style>
</head>
<body>
  <div class="top-header">
    <h1><span>⚡</span> {title} <span class="tag">OpenAPI 3.0</span></h1>
    <div class="meta">Exported from Blitz API Workbench</div>
  </div>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js" crossorigin></script>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-standalone-preset.js" crossorigin></script>
  <script id="openapi-spec" type="application/json">
{openapi_json}
  </script>
  <script>
    window.onload = () => {{
      const spec = JSON.parse(document.getElementById('openapi-spec').textContent);
      window.ui = SwaggerUIBundle({{
        spec: spec,
        dom_id: '#swagger-ui',
        deepLinking: true,
        presets: [
          SwaggerUIBundle.presets.apis,
          SwaggerUIStandalonePreset
        ],
        plugins: [
          SwaggerUIBundle.plugins.DownloadUrl
        ],
        layout: "BaseLayout",
        defaultModelsExpandDepth: -1
      }});
    }};
  </script>
</body>
</html>
"#,
        title = escaped_title,
        openapi_json = openapi_json
    );

    Ok(html)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::collection::Folder;

    #[test]
    fn test_generate_static_doc_html() {
        let mut col = Collection::new("Demo API");
        col.description = "Test Description".into();

        let mut folder = Folder::new("Authentication");
        let mut req = RequestDraft::default();
        req.name = "Get Users".into();
        req.url = "https://api.example.com/users".into();
        folder.items.push(CollectionItem::Request(req));
        col.items.push(CollectionItem::Folder(folder));

        let html = generate_static_doc_html(&col);
        assert!(html.contains("Demo API"));
        assert!(html.contains("Authentication"));
        assert!(html.contains("Get Users"));
        assert!(html.contains("https://api.example.com/users"));
        assert!(html.contains("cURL"));
        assert!(html.contains("reqwest"));
    }

    #[test]
    fn test_generate_swagger_ui_html() {
        let mut col = Collection::new("Petstore API");
        col.description = "Swagger UI Export Test".into();

        let mut req = RequestDraft::default();
        req.name = "List Pets".into();
        req.url = "https://petstore.example.com/pets".into();
        col.items.push(CollectionItem::Request(req));

        let html = generate_swagger_ui_html(&col).expect("swagger export");
        assert!(html.contains("Petstore API"));
        assert!(html.contains("SwaggerUIBundle"));
        assert!(html.contains("openapi-spec"));
        assert!(html.contains("/pets"));
    }
}
