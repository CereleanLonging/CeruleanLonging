//! Original receipt and actual shared-writer read boundaries; no remote work.
use super::*;

#[test]
fn workspace_api_publication_reviews_durable_receipts_shared_owner_and_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    // This receipt has no project identity: retain the actual durable path.
    let relative = format!("api-startups/{}.pending", "a".repeat(64));
    fs::write(host.root.join(&relative), b"{\"partial\":").unwrap();
    let (digest, reviewed) = crate::api_publication::inspect(&host.root, &relative).unwrap();
    assert!(!reviewed);
    crate::api_publication::acknowledge(
        &host.root,
        &relative,
        &digest,
        "保留原文，不重发",
        |_| Ok(()),
    )
    .unwrap();
    // The old Snapshot cache is intentionally not refreshed.
    assert!(host.api_publication_reviews.is_empty());
    let query = WorkspaceQuery::ApiPublicationReviews {
        offset: 0,
        revision: None,
    };
    let before = crate::reader_generation::tests::snapshot(&host.root);
    let WorkspaceResult::ApiPublicationReviews(page) = host.workspace_query(&query).unwrap() else {
        panic!("page")
    };
    assert_eq!(page.total, 1);
    assert_eq!(page.entries[0].relative_path, relative);
    assert_eq!(page.entries[0].evidence_digest, digest);
    let detail_query = WorkspaceQuery::ApiPublicationReview {
        relative_path: relative.clone(),
        revision: page.revision.clone(),
    };
    let original = serde_json::to_value(host.workspace_query(&detail_query).unwrap()).unwrap();
    assert!(original["value"].get("projectId").is_none());
    assert_eq!(
        original["value"]["review"],
        serde_json::to_value(&page.entries[0]).unwrap()
    );
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );

    let store = host.projects.remove("project").unwrap();
    host.api_writers.insert(
        "project".into(),
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap(),
    );
    assert_eq!(
        original,
        serde_json::to_value(host.workspace_query(&detail_query).unwrap()).unwrap()
    );
    host.api_writers.get_mut("project").unwrap().interrupted = true;
    assert!(host.workspace_query(&query).is_err());
    host.api_writers.get_mut("project").unwrap().interrupted = false;
    host.base.poisoned = true;
    assert!(host.workspace_query(&query).is_err());
    host.base.poisoned = false;
    host.api_fault = Some("unknown owner state".into());
    assert!(host.workspace_query(&query).is_err());
    host.api_fault = None;
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );
    fs::write(host.root.join(&relative), b"{\"changed\":").unwrap();
    assert!(host.workspace_query(&query).is_err());
    assert!(host.workspace_query(&detail_query).is_err());
}

#[test]
fn workspace_api_publication_reviews_unknown_and_oversize_original_refused() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    let query = WorkspaceQuery::ApiPublicationReviews {
        offset: 0,
        revision: None,
    };
    let WorkspaceResult::ApiPublicationReviews(page) = host.workspace_query(&query).unwrap() else {
        panic!("page")
    };
    assert!(host
        .workspace_query(&WorkspaceQuery::ApiPublicationReview {
            relative_path: "api-startups/missing.pending".into(),
            revision: page.revision
        })
        .is_err());
    let relative = format!("api-startups/{}.pending", "a".repeat(64));
    fs::write(host.root.join(&relative), b"{").unwrap();
    let (digest, _) = crate::api_publication::inspect(&host.root, &relative).unwrap();
    crate::api_publication::acknowledge(&host.root, &relative, &digest, "保留", |_| Ok(()))
        .unwrap();
    fs::write(host.root.join(&relative), vec![b' '; 256 * 1024 + 1]).unwrap();
    let before = crate::reader_generation::tests::snapshot(&host.root);
    assert!(host.workspace_query(&query).is_err());
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );
}
