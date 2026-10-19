//! Smoke tests mirroring main UI flows. Network cases are `#[ignore]` by default.
//!
//! ```bash
//! cargo test ui_flow_smoke
//! cargo test ui_flow_smoke -- --ignored --nocapture   # needs network
//! ```

#[cfg(test)]
mod ui_flow_smoke {
    use crate::core::{
        add_folder_under, add_request_under, duplicate_across, export_collection_v21,
        flatten_collections, generate_snippet, import_collection_v21, import_curl, import_openapi3,
        move_item_to, purge_from_trash, rename_across, restore_from_trash, routes_from_collections,
        start_mock, trash_collection, AuthType, BodyMode, Collection, CookieJar, Folder,
        HttpMethod, KeyValue, RequestDraft, SavedExample, TreeRowKind, VariableContext,
    };
    use crate::runtime::{
        run_collection_with_config, send_request_with_options, RunnerConfig, SendOptions,
    };

    fn demo_ctx() -> VariableContext {
        let mut ctx = VariableContext::default();
        ctx.environment
            .insert("baseUrl".into(), "https://postman-echo.com".into());
        ctx
    }

    #[test]
    #[ignore = "network + full pipeline smoke"]
    fn flow_send_get_with_vars() {
        let mut draft = RequestDraft::default();
        draft.method = HttpMethod::Get;
        draft.url = "{{baseUrl}}/get".into();
        draft.params = vec![KeyValue::new("hello", "world")];
        draft.test_script = r#"
            epm.test("status 200", function () {
                epm.expect(epm.response.code).to.equal(200);
            });
            epm.test("has args", function () {
                var j = epm.response.json();
                epm.expect(j.args.hello).to.equal("world");
            });
        "#
        .into();

        let mut ctx = demo_ctx();
        let view = send_request_with_options(&draft, &mut ctx, SendOptions::default())
            .expect("send");
        assert!(view.error.is_none(), "{:?}", view.error);
        assert_eq!(view.status, 200);
        assert!(
            view.test_results.iter().all(|t| t.passed),
            "{:?}",
            view.test_results
        );
        assert!(view.resolved_url.contains("postman-echo.com/get"));
    }

    #[test]
    #[ignore = "network"]
    fn flow_post_json_body() {
        let mut draft = RequestDraft::default();
        draft.method = HttpMethod::Post;
        draft.url = "https://postman-echo.com/post".into();
        draft.body_mode = BodyMode::Raw;
        draft.body = r#"{"name":"ada"}"#.into();
        draft
            .headers
            .push(KeyValue::new("Content-Type", "application/json"));

        let mut ctx = VariableContext::default();
        let view =
            send_request_with_options(&draft, &mut ctx, SendOptions::default()).expect("send");
        assert_eq!(view.status, 200);
        assert!(view.body.contains("ada"));
    }

    #[test]
    #[ignore = "network"]
    fn flow_bearer_auth() {
        let mut draft = RequestDraft::default();
        draft.url = "https://postman-echo.com/headers".into();
        draft.auth.auth_type = AuthType::Bearer;
        draft.auth.token = "smoke-token".into();

        let mut ctx = VariableContext::default();
        let view =
            send_request_with_options(&draft, &mut ctx, SendOptions::default()).expect("send");
        assert_eq!(view.status, 200);
        assert!(
            view.body.to_lowercase().contains("smoke-token")
                || view.body.to_lowercase().contains("authorization"),
            "body={}",
            view.body
        );
    }

    /// Sidebar happy path: collection → folder → request → rename → move → delete/trash.
    #[test]
    fn flow_sidebar_tree_crud_and_move() {
        let mut cols = Vec::new();
        let mut trash = Vec::new();

        let col = Collection::new("Smoke Col");
        let col_id = col.id;
        cols.push(col);

        let folder = Folder::new("Smoke Folder");
        let folder_id = folder.id;
        assert!(add_folder_under(&mut cols, col_id, folder));

        let nested = Folder::new("Nested");
        let nested_id = nested.id;
        assert!(add_folder_under(&mut cols, folder_id, nested));

        let mut req = RequestDraft::default();
        req.name = "Smoke Req".into();
        req.url = "https://example.com/smoke".into();
        let req_id = req.id;
        assert!(add_request_under(&mut cols, nested_id, req));

        let before_rename = flatten_collections(&cols).len();
        assert!(rename_across(&mut cols, req_id, "Smoke Req Renamed".into()));
        assert_eq!(flatten_collections(&cols).len(), before_rename);
        let found = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == req_id)
            .unwrap();
        assert_eq!(found.name, "Smoke Req Renamed");
        assert_eq!(found.parent_id, Some(nested_id));

        // Repeated rename sets explicit name; still no second item.
        assert!(rename_across(&mut cols, req_id, "Smoke Req Final".into()));
        assert_eq!(flatten_collections(&cols).len(), before_rename);
        let found = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == req_id)
            .unwrap();
        assert_eq!(found.name, "Smoke Req Final");

        // Open = find across (editor selection)
        let (owner, draft) = crate::core::find_request_across(&cols, req_id).unwrap();
        assert_eq!(owner, col_id);
        assert_eq!(draft.name, "Smoke Req Final");

        // Move request up to collection root
        assert!(move_item_to(&mut cols, req_id, col_id));
        let found = flatten_collections(&cols)
            .into_iter()
            .find(|r| r.id == req_id)
            .unwrap();
        assert_eq!(found.parent_id, Some(col_id));

        // Move folder under… rejected into self
        assert!(!move_item_to(&mut cols, folder_id, nested_id));

        let before_dup = flatten_collections(&cols).len();
        let dup = duplicate_across(&mut cols, req_id).unwrap();
        assert_ne!(dup, req_id);
        assert_eq!(flatten_collections(&cols).len(), before_dup + 1);
        assert!(flatten_collections(&cols)
            .iter()
            .any(|r| r.id == dup && matches!(r.kind, TreeRowKind::Request { .. })));

        assert!(trash_collection(&mut cols, &mut trash, col_id));
        assert!(cols.is_empty());
        assert!(restore_from_trash(&mut cols, &mut trash, col_id));
        assert_eq!(cols.len(), 1);
        assert!(trash_collection(&mut cols, &mut trash, col_id));
        assert!(purge_from_trash(&mut trash, col_id));
        assert!(trash.is_empty());
    }

    #[test]
    fn flow_import_export_roundtrip() {
        let mut col = Collection::new("Smoke");
        let mut r = RequestDraft::default();
        r.name = "A".into();
        r.url = "https://example.com".into();
        col.add_request(r);
        let json = export_collection_v21(&col).unwrap();
        let back = import_collection_v21(&json).unwrap();
        assert_eq!(back.name, "Smoke");
        assert_eq!(back.items.len(), 1);
    }

    #[test]
    fn flow_curl_and_openapi_import() {
        let d =
            import_curl("curl -X GET 'https://example.com/x' -H 'Accept: application/json'").unwrap();
        assert_eq!(d.method, HttpMethod::Get);
        assert!(d.url.contains("example.com/x"));

        let col = import_openapi3(
            r#"{"openapi":"3.0.0","info":{"title":"T"},"servers":[{"url":"https://api.test"}],"paths":{"/a":{"get":{}}}}"#,
        )
        .unwrap();
        assert_eq!(col.items.len(), 1);
    }

    #[test]
    fn flow_codegen_snippets() {
        let d = RequestDraft::default();
        assert!(generate_snippet("cURL", &d).contains("curl"));
        assert!(generate_snippet("JavaScript fetch", &d).contains("fetch"));
        assert!(generate_snippet("Python requests", &d).contains("requests"));
    }

    #[test]
    fn flow_local_mock_from_example() {
        let mut col = Collection::new("MockSmoke");
        let mut r = RequestDraft::default();
        r.name = "echo".into();
        r.method = HttpMethod::Get;
        r.url = "http://127.0.0.1/mock-smoke".into();
        r.examples.push(SavedExample {
            id: uuid::Uuid::new_v4(),
            name: "ok".into(),
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), "text/plain".into())],
            body: "mock-ok".into(),
        });
        col.add_request(r);

        let routes = routes_from_collections(&[col]);
        let handle = start_mock(routes, 0).expect("start mock");
        let port = handle.port;
        let url = format!("http://127.0.0.1:{port}/mock-smoke");
        let body = reqwest::blocking::get(&url).unwrap().text().unwrap();
        assert_eq!(body, "mock-ok");
        handle.stop();
    }

    #[test]
    #[ignore = "network"]
    fn flow_runner_two_requests() {
        let mut col = Collection::new("RunSmoke");
        let mut a = RequestDraft::default();
        a.name = "one".into();
        a.url = "https://postman-echo.com/get?n=1".into();
        a.test_script =
            r#"epm.test("ok", function(){ epm.expect(epm.response.code).to.equal(200); });"#.into();
        let mut b = RequestDraft::default();
        b.name = "two".into();
        b.url = "https://postman-echo.com/get?n=2".into();
        col.add_request(a);
        col.add_request(b);

        let mut jar = CookieJar::default();
        let mut ctx = VariableContext::default();
        let report = run_collection_with_config(
            &col,
            &mut ctx,
            &mut jar,
            None,
            None,
            &RunnerConfig {
                iterations: 1,
                delay_ms: 0,
                data_file: String::new(),
            },
        );
        assert_eq!(report.results.len(), 2);
        assert_eq!(report.failed_count(), 0, "{:?}", report.results);
    }
}
