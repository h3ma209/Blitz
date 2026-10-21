//! Smoke tests mirroring main UI flows. Network cases are `#[ignore]` by default.
//!
//! ```bash
//! cargo test ui_flow_smoke
//! cargo test ui_flow_smoke -- --ignored --nocapture   # needs network
//! ```

#[cfg(test)]
mod tests {
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
        let draft = RequestDraft {
            method: HttpMethod::Get,
            url: "{{baseUrl}}/get".into(),
            params: vec![KeyValue::new("hello", "world")],
            test_script: r#"
                blitz.test("status 200", function () {
                    blitz.expect(blitz.response.code).to.equal(200);
                });
                blitz.test("has args", function () {
                    var j = blitz.response.json();
                    blitz.expect(j.args.hello).to.equal("world");
                });
            "#
            .into(),
            ..Default::default()
        };

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
        let mut draft = RequestDraft {
            method: HttpMethod::Post,
            url: "https://postman-echo.com/post".into(),
            body_mode: BodyMode::Raw,
            body: r#"{"name":"ada"}"#.into(),
            ..Default::default()
        };
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
        let mut draft = RequestDraft {
            url: "https://postman-echo.com/headers".into(),
            ..Default::default()
        };
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

        let req = RequestDraft {
            name: "Smoke Req".into(),
            url: "https://example.com/smoke".into(),
            ..Default::default()
        };
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
        let r = RequestDraft {
            name: "A".into(),
            url: "https://example.com".into(),
            ..Default::default()
        };
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
        let mut r = RequestDraft {
            name: "echo".into(),
            method: HttpMethod::Get,
            url: "http://127.0.0.1/mock-smoke".into(),
            ..Default::default()
        };
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
    fn flow_full_mock_recording_and_live_playback_pipeline() {
        // 1. Create request with live mock server endpoint
        let mut col = Collection::new("PlaybackCol");
        let mut r = RequestDraft {
            name: "get_user".into(),
            method: HttpMethod::Get,
            url: "http://127.0.0.1/api/v1/user/42".into(),
            ..Default::default()
        };

        // 2. Simulate response received and record as example
        let example = SavedExample {
            id: uuid::Uuid::new_v4(),
            name: "Recorded 200 OK".into(),
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: r#"{"id":42,"username":"blitz_user","role":"admin"}"#.into(),
        };
        r.examples.push(example.clone());
        col.add_request(r);

        // 3. Start mock server from collection
        let routes = routes_from_collections(&[col]);
        let handle = start_mock(routes, 0).expect("start mock server");
        let port = handle.port;

        // 4. Send request through runtime pipeline against local mock
        let test_draft = RequestDraft {
            method: HttpMethod::Get,
            url: format!("http://127.0.0.1:{port}/api/v1/user/42"),
            test_script: r#"
                blitz.test("user is blitz_user", function() {
                    var json = blitz.response.json();
                    blitz.expect(json.id).to.equal(42);
                    blitz.expect(json.username).to.equal("blitz_user");
                });
            "#
            .into(),
            ..Default::default()
        };

        let mut ctx = VariableContext::default();
        let view = send_request_with_options(&test_draft, &mut ctx, SendOptions::default())
            .expect("send request to mock server");

        assert_eq!(view.status, 200);
        assert!(view.body.contains("blitz_user"));
        assert_eq!(view.test_results.len(), 1);
        assert!(view.test_results[0].passed);

        handle.stop();
    }

    #[test]
    fn flow_collection_runner_local_pipeline_with_scripts() {
        // Test pre-request script variable setting + tests validation in Collection Runner
        let mut col = Collection::new("RunnerLocalCol");
        let mut r1 = RequestDraft {
            name: "Setup Vars".into(),
            method: HttpMethod::Get,
            url: "http://127.0.0.1/ping".into(),
            pre_request_script: r#"
                blitz.environment.set("auth_token", "secret_jwt_xyz");
                blitz.collectionVariables.set("apiVersion", "v2");
            "#
            .into(),
            ..Default::default()
        };
        r1.examples.push(SavedExample {
            id: uuid::Uuid::new_v4(),
            name: "pong".into(),
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            body: "pong".into(),
        });

        let mut r2 = RequestDraft {
            name: "Verify Vars".into(),
            method: HttpMethod::Get,
            url: "http://127.0.0.1/{{apiVersion}}/data".into(),
            ..Default::default()
        };
        r2.examples.push(SavedExample {
            id: uuid::Uuid::new_v4(),
            name: "pong".into(),
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            body: "data-ok".into(),
        });
        r2.test_script = r#"
            blitz.test("auth_token set by r1", function() {
                blitz.expect(blitz.environment.get("auth_token")).to.equal("secret_jwt_xyz");
            });
        "#.into();

        col.add_request(r1);
        col.add_request(r2);

        let routes = routes_from_collections(&[col.clone()]);
        let handle = start_mock(routes, 0).expect("start mock server");
        let port = handle.port;

        // Rewrite ports for runner execution
        let mut runnable_col = col.clone();
        for item in &mut runnable_col.items {
            if let crate::core::CollectionItem::Request(req) = item {
                req.url = req.url.replace("http://127.0.0.1", &format!("http://127.0.0.1:{port}"));
            }
        }

        let mut ctx = VariableContext::default();
        let mut jar = CookieJar::default();
        let report = run_collection_with_config(
            &runnable_col,
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
        assert_eq!(ctx.environment.get("auth_token").map(|s| s.as_str()), Some("secret_jwt_xyz"));
        assert_eq!(ctx.collection.get("apiVersion").map(|s| s.as_str()), Some("v2"));

        handle.stop();
    }

    #[test]
    fn flow_openapi3_non_destructive_reconciliation_workflow() {
        use crate::core::reconcile_openapi3_collection;

        // 1. Initial collection created with user modifications (custom tests & descriptions)
        let mut col = Collection::new("Petstore");
        let r = RequestDraft {
            name: "List Pets".into(),
            method: HttpMethod::Get,
            url: "https://petstore.example.com/api/pets".into(),
            description: "User added important documentation notes".into(),
            test_script: r#"
                blitz.test("pets is array", function() {
                    blitz.expect(Array.isArray(blitz.response.json())).to.be.true;
                });
            "#
            .into(),
            ..Default::default()
        };
        col.add_request(r);

        // 2. Upstream engineering updates OpenAPI spec: changes path summary, adds new query param and new /api/pets/{id} endpoint
        let updated_spec = r#"{
            "openapi": "3.0.0",
            "info": { "title": "Petstore Updated", "version": "1.1.0" },
            "servers": [{ "url": "https://petstore.example.com/api" }],
            "paths": {
                "/pets": {
                    "get": {
                        "summary": "Find All Pets (Upstream)",
                        "parameters": [
                            { "name": "limit", "in": "query", "schema": { "type": "integer" } }
                        ]
                    }
                },
                "/pets/{id}": {
                    "get": {
                        "summary": "Find Pet By Id"
                    }
                }
            }
        }"#;

        // 3. Reconcile collection with updated OpenAPI spec
        let (added, preserved) = reconcile_openapi3_collection(&mut col, updated_spec)
            .expect("reconcile openapi3");

        assert_eq!(added, 1);
        assert_eq!(preserved, 1);

        // Verify custom tests and descriptions were preserved!
        let pet_req = col.items.iter().find_map(|item| match item {
            crate::core::CollectionItem::Request(req) if req.url.ends_with("/pets") => Some(req),
            _ => None,
        }).expect("find pets request");

        assert_eq!(pet_req.description, "User added important documentation notes");
        assert!(pet_req.test_script.contains("pets is array"));

        // Verify new endpoint was added
        let has_new_endpoint = col.items.iter().any(|item| match item {
            crate::core::CollectionItem::Request(req) => req.url.contains("/pets/{id}"),
            _ => false,
        });
        assert!(has_new_endpoint);
    }

    #[test]
    fn flow_git_conflict_detection_and_resolution_workflow() {
        use crate::core::cloud_sync::{
            commit_local_changes, detect_git_conflicts, resolve_git_conflicts,
        };
        use std::fs;
        use std::process::Command;

        // Setup temporary repository
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("blitz-conflict-flow-{nanos}"));
        let root = base.join("wt");
        let git_meta = base.join("git-meta");
        fs::create_dir_all(&root).unwrap();

        Command::new("git")
            .args(["init", "--template=", &format!("--separate-git-dir={}", git_meta.display())])
            .current_dir(&root)
            .output()
            .unwrap();
        Command::new("git").args(["config", "user.email", "flow@test.com"]).current_dir(&root).output().unwrap();
        Command::new("git").args(["config", "user.name", "Flow"]).current_dir(&root).output().unwrap();
        Command::new("git").args(["checkout", "-b", "main"]).current_dir(&root).output().unwrap();

        // Commit initial base file
        let file_path = root.join("request.json");
        fs::write(&file_path, "base version").unwrap();
        commit_local_changes(&root, Some("initial commit")).unwrap();

        // Create feature branch and commit changes
        Command::new("git").args(["checkout", "-b", "feature"]).current_dir(&root).output().unwrap();
        fs::write(&file_path, "remote feature version").unwrap();
        commit_local_changes(&root, Some("feature commit")).unwrap();

        // Switch back to main and make conflicting changes
        Command::new("git").args(["checkout", "main"]).current_dir(&root).output().unwrap();
        fs::write(&file_path, "local main version").unwrap();
        commit_local_changes(&root, Some("local commit")).unwrap();

        // Trigger conflict via merge
        let _ = Command::new("git").args(["merge", "feature"]).current_dir(&root).output().unwrap();

        // 1. Detect conflicts
        let conflicts = detect_git_conflicts(&root).expect("detect git conflicts");
        assert_eq!(conflicts, vec!["request.json".to_string()]);

        // 2. Resolve conflict keeping local (ours)
        resolve_git_conflicts(&root, &conflicts, true).expect("resolve git conflicts");

        // Verify conflict resolved
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "local main version");

        let remaining = detect_git_conflicts(&root).expect("detect after resolve");
        assert!(remaining.is_empty());

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn flow_cli_runner_full_execution_cycle() {
        use crate::core::cli_runner::try_run_cli;
        use std::fs;

        // Generate temporary collection and environment files
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("blitz-cli-{nanos}"));
        fs::create_dir_all(&tmp_dir).unwrap();

        // Start mock server
        let mut col = Collection::new("CliMockCol");
        let mut r = RequestDraft {
            name: "Health".into(),
            method: HttpMethod::Get,
            url: "http://127.0.0.1/{{path}}".into(),
            ..Default::default()
        };
        r.examples.push(SavedExample {
            id: uuid::Uuid::new_v4(),
            name: "ok".into(),
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            body: "all-good".into(),
        });
        col.add_request(r);

        let routes = routes_from_collections(&[col.clone()]);
        let handle = start_mock(routes, 0).expect("start mock");
        let port = handle.port;

        // Rewrite collection url with port
        let mut file_col = col.clone();
        if let crate::core::CollectionItem::Request(req) = &mut file_col.items[0] {
            req.url = format!("http://127.0.0.1:{port}/{{{{path}}}}");
        }
        let col_json = export_collection_v21(&file_col).unwrap();
        let col_file = tmp_dir.join("collection.json");
        fs::write(&col_file, col_json).unwrap();

        // Write environment file resolving `path`
        let env_file = tmp_dir.join("env.json");
        let env_json = r#"{
            "name": "LocalEnv",
            "values": [
                {"key": "path", "value": "status", "enabled": true}
            ]
        }"#;
        fs::write(&env_file, env_json).unwrap();

        // Run CLI with --env
        let args = vec![
            "blitz".to_string(),
            "run".to_string(),
            col_file.to_str().unwrap().to_string(),
            "--env".to_string(),
            env_file.to_str().unwrap().to_string(),
        ];

        let exit_code = try_run_cli(&args);
        assert_eq!(exit_code, Some(0));

        handle.stop();
        let _ = fs::remove_dir_all(tmp_dir);
    }

    #[test]
    #[ignore = "network"]
    fn flow_runner_two_requests() {
        let mut col = Collection::new("RunSmoke");
        let a = RequestDraft {
            name: "one".into(),
            url: "https://postman-echo.com/get?n=1".into(),
            test_script:
                r#"blitz.test("ok", function(){ blitz.expect(blitz.response.code).to.equal(200); });"#
                    .into(),
            ..Default::default()
        };
        let b = RequestDraft {
            name: "two".into(),
            url: "https://postman-echo.com/get?n=2".into(),
            ..Default::default()
        };
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
