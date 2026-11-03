//! Opt-in native OMP delta check. Chief/deputy are fixtures, never real OpenAI.
//! Preparation performs no credential read, OMP launch, or provider request.
use super::*;
use cerulean_domain::{
    credentials::CredentialProvider, management::Purpose, project_setup::RunState,
};
use serde_json::Value;
use std::io::{Read, Write};

const OMP_SHA: &str = "5644e5d7c7cddd851d79999dbc3b98363817178b33426b355a20fdcad41a3f54";
const GOAL: &str = "In this fresh project, first call cerulean_read_file for sum.mjs with offset 0 twice, sequentially, with identical arguments. These two missing-file errors are intentional supervision probes; do not retry a third time. Then follow any supervisor advice and create only sum.mjs exporting function sum(a,b) that returns a+b for numbers. Use cerulean_write_file with expectedSha256 absent. Do not run commands, use network tools, read credentials, or access other files. Finish after the file is created.";
const CHECK: &str = "import assert from 'node:assert/strict'; import {sum} from './sum.mjs'; for (const [a,b,w] of [[2,3,5],[-4,3,-1],[0,0,0],[0.25,0.5,0.75],[100,-100,0]]) assert.equal(sum(a,b),w);";

fn save(path: &Path, value: &impl serde::Serialize) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}
fn hash(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    format!("{:x}", hash.finalize())
}
fn root() -> PathBuf {
    let root = PathBuf::from(std::env::var_os("CERULEAN_OMP_SUPERVISION_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(&reports) && root != reports);
    for ancestor in root.ancestors() {
        ordinary(ancestor, true).unwrap();
    }
    root
}
fn manager_live(i: DecisionIntent) -> Result<DecisionObservation, String> {
    let task = &i.context["workflow"]["state"]["tasks"]["task"];
    let decision = match i.purpose {
        Purpose::Refine => return Err("Document refinement uses its dedicated fixture".into()),
        Purpose::Assign => {
            require(task["state"] == "pending", "Single attempt only")?;
            ManagementDecision::Assign {
                task_id: "task".into(),
                worker_id: "worker-omp".into(),
                rationale: "Explicit single native supervision probe".into(),
            }
        }
        Purpose::Advise => {
            let b = binding(&i)?.ok_or("Original OMP supervision binding required")?;
            evidence(&i, &b)?.intent().validate_metadata()?;
            ManagementDecision::Advise { task_id: b.task_id, suggestion: "The two missing-file reads were intentional.\nStop reading the missing file; now create sum.mjs with the approved numeric sum implementation.".into(), rationale: "Original file journal contains the repeated failure".into() }
        }
        Purpose::Accept => {
            require(
                task["verified"] == true && task["state"] == "submitted",
                "Independent verification required",
            )?;
            ManagementDecision::Accept {
                task_id: "task".into(),
                attempt_id: task["attempt"].as_str().ok_or("Attempt missing")?.into(),
                rationale: "Independent numeric cases passed; synthetic chief only".into(),
            }
        }
        Purpose::Plan => return Err("Planning is prepared separately; never repeat".into()),
    };
    Ok(DecisionObservation {
        response_id: format!("resp_{}", i.operation_id.replace('-', "_")),
        requested_model: i.model.clone(),
        actual_model: i.model,
        input_tokens: 1,
        output_tokens: 1,
        envelope: DecisionEnvelope {
            state_digest: i.state_digest,
            decision,
        },
    })
}

#[test]
#[ignore = "prepare isolated real OMP probe; zero provider calls and no credentials"]
fn prepare_native_supervision() {
    let root = root();
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    let executable = PathBuf::from(std::env::var_os("CERULEAN_OMP_EXE").unwrap());
    assert_eq!(hash(&executable), OMP_SHA);
    let parent = PathBuf::from(std::env::var_os("CERULEAN_OMP_RUNTIME_PARENT").unwrap());
    crate::omp_process::check_profile_parent(&parent).unwrap();
    let node = PathBuf::from(std::env::var_os("CERULEAN_OMP_NODE").unwrap());
    let (mut host, mut draft) = crate::host::project_planning::tests::fixture(&root);
    host.register_validator(
        "numeric-sum".into(),
        "Independent numeric sum cases".into(),
        node.to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    draft.worker_adapter = project_setup::ProjectWorkerAdapter::Omp;
    draft.codex_executable = executable.to_string_lossy().into();
    draft.omp_runtime_parent = Some(parent.to_string_lossy().into());
    draft.worker_model = "deepseek-flash".into();
    draft.worker_max_requests = 1;
    draft.management_max_requests = 4;
    draft.max_seconds = 90;
    draft.requirement = GOAL.into();
    draft.validator_id = "numeric-sum".into();
    draft.check_args = vec!["--input-type=module".into(), "--eval".into(), CHECK.into()];
    draft.supervision = Some(SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 1,
        delivery_enabled: true,
    });
    host.configure_project(draft).unwrap();
    let mut plan = crate::host::project_planning::tests::proposal("numeric-sum");
    if let ManagementDecision::Plan { tasks, .. } = &mut plan {
        tasks[0].worker_id = "worker-omp".into();
        tasks[0].goal = GOAL.into();
    }
    crate::host::project_planning::tests::complete(&mut host, plan);
    let revision = host.projects["project"].revision().unwrap();
    let review = json!({
        "schemaVersion":1, "scope":"real OMP 18.4.4 worker with synthetic chief/deputy; not full three-level acceptance",
        "testExecutableSha256":hash(&std::env::current_exe().unwrap()),
        "ompExecutableSha256":OMP_SHA, "nodeExecutableSha256":hash(&node),
        "revisionDigest":revision.approval_digest().unwrap(), "setupDigest":host.project_setups["project"].digest,
        "plan":revision, "maxOriginalPrompts":1, "maxNativeSteer":1,
        "maxTaskSeconds":90, "outerTimeoutSeconds":210, "realManagementCalls":0,
        "providerHttpCallLimit":null, "monetaryHardLimit":null,
        "limitExplanation":"One prompt may cause several provider inference rounds; no exact OMP HTTP or monetary cap is claimed.",
        "automaticRetry":false, "credentialSource":"explicit authorized CeruleanLonging DPAPI root, DeepSeek only; not selected during preparation",
        "enforcedFileScope":"fresh project directory; sum.mjs is the sole requested output, checked separately, not a per-file ACL",
        "preparedWithoutApproval":true, "realOmpSteeringAccepted":false, "releaseAccepted":false
    });
    assert!(host.projects["project"].approval().is_none());
    assert!(host.omp_startups.all().is_empty());
    save(&root.join("review.json"), &review);
    save(
        &root.join("review-digest.json"),
        &json!({"digest":digest(&review).unwrap()}),
    );
}

struct RemoveIsolatedCredential(PathBuf);
impl Drop for RemoveIsolatedCredential {
    fn drop(&mut self) {
        let _ = crate::credential_vault::remove_for(&self.0, CredentialProvider::DeepSeek);
    }
}

#[test]
#[ignore = "REAL DeepSeek call: explicit reviewed digest, credential source and owned runner required; never retry"]
fn native_supervision_once() {
    assert_eq!(
        std::env::var("CERULEAN_OMP_SUPERVISION_OWNED").as_deref(),
        Ok("1")
    );
    let mut in_job = 0;
    assert_ne!(
        unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                std::ptr::null_mut(),
                &mut in_job,
            )
        },
        0
    );
    assert_ne!(in_job, 0);
    let root = root();
    let review: Value =
        serde_json::from_slice(&fs::read(root.join("review.json")).unwrap()).unwrap();
    assert_eq!(
        std::env::var("CERULEAN_OMP_SUPERVISION_APPROVED_DIGEST").unwrap(),
        digest(&review).unwrap()
    );
    assert_eq!(
        review["testExecutableSha256"],
        hash(&std::env::current_exe().unwrap())
    );
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    assert_eq!(
        review["revisionDigest"],
        host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap()
    );
    assert_eq!(review["setupDigest"], host.project_setups["project"].digest);
    assert!(host.projects["project"].approval().is_none());
    assert!(host.omp_startups.all().is_empty() && host.runs.all().is_empty());
    let source = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_CREDENTIAL_SOURCE")
            .expect("Explicit authorized credential root required"),
    )
    .canonicalize()
    .unwrap();
    assert_ne!(source, host.root.canonicalize().unwrap());
    save(
        &root.join("live-intent.json"),
        &json!({"reviewDigest":digest(&review).unwrap(),"at":chrono::Utc::now().to_rfc3339(),"credentialProvider":"deepseek","credentialSource":source,"retry":false}),
    );
    let key = crate::credential_vault::load_for(&source, CredentialProvider::DeepSeek)
        .expect("Explicit source credential unavailable; no retry");
    let cleanup = RemoveIsolatedCredential(host.root.clone());
    crate::credential_vault::save_for(&host.root, CredentialProvider::DeepSeek, &key).unwrap();
    drop(key);
    host.deepseek_key_configured = true;
    host.openai_connection.key_configured = true; // Test-only management transport, no OpenAI key.
    host.management_fixture = Some(manager_live);
    let revision = review["revisionDigest"].as_str().unwrap();
    let setup = review["setupDigest"].as_str().unwrap();
    host.approve("project", revision).unwrap();
    host.start_project_run("project", revision, setup).unwrap();
    let deadline = Instant::now() + Duration::from_secs(165);
    loop {
        host.poll_verifications();
        host.poll_project_runs();
        let idle = host.omp_active.is_empty()
            && host.active.is_empty()
            && host.management_active.is_empty();
        if idle && host.runs.all()[0].state != RunState::Active {
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let idle =
        host.omp_active.is_empty() && host.active.is_empty() && host.management_active.is_empty();
    let snapshot = host.snapshot();
    save(&root.join("after-live.json"), &snapshot);
    let advice: Vec<_> = host
        .management
        .statuses()
        .iter()
        .filter(|s| s.purpose == Purpose::Advise)
        .map(|s| host.supervision_advice_view(&s.operation_id).unwrap())
        .collect();
    let delivered = advice.len() == 1
        && advice[0]
            .as_ref()
            .is_some_and(|a| a.delivered && a.delivery_state.as_deref() == Some("messageObserved"));
    let accepted = snapshot.workflows[0].state.tasks["task"].state == TaskState::Accepted;
    let one_attempt = host.omp_startups.all().len() == 1;
    let entries: Vec<_> = fs::read_dir(root.join("project"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    let only_expected_output = entries.len() == 1
        && entries[0].file_name().is_some_and(|name| name == "sum.mjs")
        && fs::symlink_metadata(&entries[0]).unwrap().is_file();
    save(
        &root.join("native-observation.json"),
        &json!({"idle":idle,"advice":advice,"realMessageObserved":delivered,"independentlyAccepted":accepted,"oneAttempt":one_attempt,"onlyExpectedOutput":only_expected_output,"projectEntries":entries,"realManagementCalls":0,"realThreeLevelAcceptance":false,"releaseAccepted":false,"reopenChecksComplete":false}),
    );
    drop(cleanup);
    assert!(
        !crate::credential_vault::exists_for(&host.root, CredentialProvider::DeepSeek).unwrap()
    );
    assert!(
        idle,
        "Native job did not settle; outer owned runner cleans only its process tree, never retry"
    );
    let requests = host.management.statuses().len();
    let state_digest = digest(&snapshot.workflows[0].state).unwrap();
    drop(host);
    for n in 1..=2 {
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.poll_verifications();
        save(&root.join(format!("reopen-{n}.json")), &reopened.snapshot());
        assert_eq!(reopened.management.statuses().len(), requests);
        assert_eq!(reopened.omp_startups.all().len(), 1);
        assert!(reopened.omp_active.is_empty() && reopened.management_active.is_empty());
        assert_eq!(
            digest(&reopened.snapshot().workflows[0].state).unwrap(),
            state_digest
        );
    }
    save(
        &root.join("result.json"),
        &json!({"realMessageObserved":delivered,"independentlyAccepted":accepted,"oneAttempt":one_attempt,"onlyExpectedOutput":only_expected_output,"reopenChecksComplete":true,"isolatedCredentialRemoved":true,"realManagementCalls":0,"realThreeLevelAcceptance":false,"releaseAccepted":false,"passed":delivered && accepted && one_attempt && only_expected_output}),
    );
    assert!(
        delivered && accepted && one_attempt && only_expected_output,
        "Delta check failed; preserve evidence, no retry"
    );
}
