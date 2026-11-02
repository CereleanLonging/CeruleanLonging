use super::*;
use cerulean_adapters::api_generation::ToolCall;
use cerulean_domain::api_worker::ApiWriteScope;
use sha2::Digest;

#[test]
fn isolated_model_file_tools_cannot_read_or_write_shared_or_sibling_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = plan(dir.path(), selection(targets().remove(2)));
    let binding = WorkerDirectory::for_task(&p.execution[0]).unwrap();
    let own = std::path::PathBuf::from(binding.root(&p.project.root).unwrap());
    fs::create_dir_all(&own).unwrap();
    let sibling = own.parent().unwrap().join("other-model");
    fs::create_dir(&sibling).unwrap();
    fs::write(sibling.join("private.txt"), "other-model-private").unwrap();
    fs::write(
        Path::new(&p.project.root).join("private.txt"),
        "deputy-private",
    )
    .unwrap();
    fs::write(own.join("local.txt"), "local-input").unwrap();
    fs::hard_link(sibling.join("private.txt"), own.join("linked.txt")).unwrap();
    p.execution[0].worker_directory = Some(binding);
    let store = assigned(dir.path(), p.clone());
    let (_, _, identity, _) = evidence_scope(&store, "task", "attempt").unwrap();
    assert_eq!(Path::new(&identity.binding.workspace), own);
    let mut tools = ApiFileTools::create(
        &p,
        store.approval().unwrap(),
        "user",
        &identity,
        &dir.path().join("isolated-tools"),
    )
    .unwrap();
    let mut ordinal = 0;
    for path in [
        "../other-model/private.txt".to_owned(),
        "../../private.txt".into(),
        sibling.join("private.txt").to_string_lossy().into(),
        "linked.txt".into(),
    ] {
        for name in ["cerulean_read_file", "cerulean_write_file"] {
            ordinal += 1;
            let arguments = if name == "cerulean_read_file" {
                json!({"path":path,"offset":0})
            } else {
                json!({"path":path,"content":"bad","expectedSha256":"absent"})
            };
            let result = tools
                .execute(
                    ordinal,
                    &ToolCall {
                        id: format!("boundary-{ordinal}"),
                        name: name.into(),
                        arguments,
                    },
                )
                .unwrap();
            assert!(result.is_error, "{name}: {path}");
        }
    }
    ordinal += 1;
    assert!(
        !tools
            .execute(
                ordinal,
                &ToolCall {
                    id: "own-read".into(),
                    name: "cerulean_read_file".into(),
                    arguments: json!({"path":"local.txt","offset":0})
                }
            )
            .unwrap()
            .is_error
    );
    ordinal += 1;
    assert!(!tools.execute(ordinal, &ToolCall{id:"own-write".into(),name:"cerulean_write_file".into(),arguments:json!({"path":"result.txt","content":"own-result","expectedSha256":"absent"})}).unwrap().is_error);
    assert_eq!(
        fs::read_to_string(own.join("result.txt")).unwrap(),
        "own-result"
    );
    assert_eq!(
        fs::read_to_string(sibling.join("private.txt")).unwrap(),
        "other-model-private"
    );
    assert_eq!(
        fs::read_to_string(Path::new(&p.project.root).join("private.txt")).unwrap(),
        "deputy-private"
    );
}

#[test]
fn write_scope_is_explicit_and_changes_approval_without_changing_legacy_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let original = plan(dir.path(), selection(targets().remove(2)));
    let wire = serde_json::to_value(&original).unwrap();
    assert!(wire["execution"][0]["apiWorker"]
        .get("writeScope")
        .is_none());
    let parsed: PlanRevision = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
    let mut scoped = original.clone();
    scoped.execution[0].api_worker.as_mut().unwrap().write_scope =
        Some(ApiWriteScope::DeclaredArtifacts);
    scoped.validate().unwrap();
    assert_ne!(
        original.approval_digest().unwrap(),
        scoped.approval_digest().unwrap()
    );
    let approval = Approval {
        plan_id: original.workflow.id.clone(),
        revision: 1,
        policy_version: 1,
        revision_digest: original.approval_digest().unwrap(),
        user_id: "user".into(),
        approved_at: chrono::Utc::now().to_rfc3339(),
    };
    assert!(approval.validate(&scoped, "user", 1).is_err());
    let mut selected =
        serde_json::to_value(scoped.execution[0].api_worker.as_ref().unwrap()).unwrap();
    selected["writeScope"] = json!("unrecognized");
    assert!(serde_json::from_value::<ApiWorkerSelection>(selected).is_err());
    scoped.execution[0].api_worker.as_mut().unwrap().file_tools = vec![ToolPermission::ReadFiles];
    assert!(scoped.validate().is_err());
}

#[test]
fn scoped_tools_reject_other_files_before_any_directory_or_write_and_keep_read_access() {
    let dir = tempfile::tempdir().unwrap();
    let mut selected = selection(targets().remove(2));
    selected.write_scope = Some(ApiWriteScope::DeclaredArtifacts);
    let mut p = plan(dir.path(), selected);
    p.execution[0].artifact_paths.push("linked.txt".into());
    fs::write(dir.path().join("project/dependency.txt"), "keep").unwrap();
    fs::write(dir.path().join("project/linked.txt"), "linked").unwrap();
    fs::hard_link(
        dir.path().join("project/linked.txt"),
        dir.path().join("outside-link.txt"),
    )
    .unwrap();
    let store = assigned(dir.path(), p.clone());
    let (_, _, identity, _) = evidence_scope(&store, "task", "attempt").unwrap();
    let root = dir.path().join("tools");
    let mut tools =
        ApiFileTools::create(&p, store.approval().unwrap(), "user", &identity, &root).unwrap();
    let defs = tools.definitions();
    assert_eq!(
        defs.iter()
            .find(|t| t.name == "cerulean_write_file")
            .unwrap()
            .parameters["properties"]["path"]["enum"],
        json!(["linked.txt", "result.txt"])
    );
    let call = |id: &str, name: &str, args: Value| ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    };
    for (index, path) in [
        "dependency.txt",
        "new-parent/blocked.txt",
        "RESULT.TXT",
        "../outside.txt",
        "result.txt:stream",
        "result.txt/child",
    ]
    .iter()
    .enumerate()
    {
        let result = tools
            .execute(
                index as u32 + 1,
                &call(
                    &format!("denied-{index}"),
                    "cerulean_write_file",
                    json!({"path":path,"content":"should not exist","expectedSha256":if *path=="dependency.txt" { use sha2::Digest; format!("{:x}",sha2::Sha256::digest(b"keep")) } else { "absent".into() }}),
                ),
            )
            .unwrap();
        assert!(result.is_error, "{path}");
        assert_eq!(
            fs::read_to_string(dir.path().join("project/dependency.txt")).unwrap(),
            "keep"
        );
        assert!(!dir.path().join("project/new-parent").exists());
        assert!(!dir.path().join("project/result.txt").exists());
        assert!(!dir.path().join("outside.txt").exists());
        assert!(fs::read_dir(root.join("writes")).unwrap().next().is_none());
    }
    let read = tools
        .execute(
            7,
            &call(
                "read-dependency",
                "cerulean_read_file",
                json!({"path":"dependency.txt","offset":0}),
            ),
        )
        .unwrap();
    assert!(!read.is_error);
    assert_eq!(
        serde_json::from_str::<Value>(&read.text).unwrap()["content"],
        "keep"
    );
    let written = tools
        .execute(
            8,
            &call(
                "allowed",
                "cerulean_write_file",
                json!({"path":"result.txt","content":"approved","expectedSha256":"absent"}),
            ),
        )
        .unwrap();
    assert!(!written.is_error);
    assert_eq!(
        fs::read_to_string(dir.path().join("project/result.txt")).unwrap(),
        "approved"
    );
    let linked_sha = format!("{:x}", sha2::Sha256::digest(b"linked"));
    let linked = tools
        .execute(
            9,
            &call(
                "linked",
                "cerulean_write_file",
                json!({"path":"linked.txt","content":"bad","expectedSha256":linked_sha}),
            ),
        )
        .unwrap();
    assert!(linked.is_error);
    assert_eq!(
        fs::read_to_string(dir.path().join("outside-link.txt")).unwrap(),
        "linked"
    );
    let head = tools.head().to_owned();
    drop(tools);
    let evidence = crate::omp_tool_journal::OmpToolJournal::<ApiRunIdentity>::read_evidence(
        &root.join("events"),
        &identity,
        &head,
    )
    .unwrap();
    assert_eq!(evidence.observation().calls.len(), 9);
    assert_eq!(evidence.observation().unresolved(), 0);
    assert!(ApiFileTools::create(&p, store.approval().unwrap(), "user", &identity, &root).is_err());
    let mut forged = identity.clone();
    forged.worker.write_scope = None;
    assert!(ApiFileTools::create(
        &p,
        store.approval().unwrap(),
        "user",
        &forged,
        &dir.path().join("forged-tools")
    )
    .is_err());
    assert!(!dir.path().join("forged-tools").exists());
}

#[test]
fn unscoped_legacy_api_tools_keep_workspace_write_behavior() {
    let dir = tempfile::tempdir().unwrap();
    let p = plan(dir.path(), selection(targets().remove(2)));
    let store = assigned(dir.path(), p.clone());
    let (_, _, identity, _) = evidence_scope(&store, "task", "attempt").unwrap();
    let mut tools = ApiFileTools::create(
        &p,
        store.approval().unwrap(),
        "user",
        &identity,
        &dir.path().join("tools"),
    )
    .unwrap();
    assert!(tools
        .definitions()
        .iter()
        .find(|t| t.name == "cerulean_write_file")
        .unwrap()
        .parameters["properties"]["path"]
        .get("enum")
        .is_none());
    let result=tools.execute(1,&ToolCall{id:"legacy".into(),name:"cerulean_write_file".into(),arguments:json!({"path":"support/helper.txt","content":"legacy behavior","expectedSha256":"absent"})}).unwrap();
    assert!(!result.is_error);
    assert_eq!(
        fs::read_to_string(dir.path().join("project/support/helper.txt")).unwrap(),
        "legacy behavior"
    );
}
