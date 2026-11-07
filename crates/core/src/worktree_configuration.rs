//! Pin user-selected Git and directory policy without creating a task worktree.
use super::*;
use cerulean_domain::project_setup::WorktreeSetupDraft;

impl RuntimeHost {
    pub(super) fn configure_worktree(
        &self,
        draft: &WorktreeSetupDraft,
    ) -> Result<GitWorktreePolicy, String> {
        draft.validate()?;
        let pool = crate::artifacts::root_path(&draft.pool_root)?;
        // Pin every ancestor, rejecting junctions before accepting the persisted path.
        for ancestor in pool.ancestors() {
            ordinary(ancestor, true)?;
        }
        let git = crate::verifier::PinnedCheck::describe(
            Path::new(&draft.git_executable),
            &[],
            Default::default(),
        )?
        .executable;
        require(
            Path::new(&git.path)
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("git.exe")),
            "请选择Git可执行程序git.exe",
        )?;
        let mut protected = vec![self.root.clone(), PathBuf::from(&git.path)];
        for store in self.projects.values() {
            if let Some(scope) = &store.configuration().execution_scope {
                protected.push(PathBuf::from(&scope.project.root));
            }
        }
        for validator in self.validators.values() {
            protected.push(PathBuf::from(&validator.tool.executable.path));
            protected.extend(
                validator
                    .tool
                    .inputs
                    .iter()
                    .map(|file| PathBuf::from(&file.path)),
            );
        }
        for saved in self.project_setups.values() {
            for profile in &saved.setup.profiles {
                if let Some(worker) = &profile.worker {
                    protected.push(PathBuf::from(&worker.executable.path));
                }
            }
            if let Some(worker) = &saved.setup.worker {
                protected.push(PathBuf::from(&worker.executable.path));
            }
        }
        require(
            protected.iter().all(|p| {
                !crate::artifacts::contains_path(&pool, p)
                    && !crate::artifacts::contains_path(p, &pool)
            }),
            "目录池须与项目、工作台数据和已登记工具分开",
        )?;
        Ok(GitWorktreePolicy {
            integration: draft
                .integration
                .as_ref()
                .map(|i| {
                    let validator = self.registered_validator(&i.validator_id)?;
                    Ok::<_, String>(Box::new(ProjectIntegration {
                        task_id: i.task_id.clone(),
                        artifact_paths: i.artifact_paths.clone(),
                        validator_id: i.validator_id.clone(),
                        check: AcceptanceCheck {
                            expected_assertions: vec![],
                            id: "project-integration-check".into(),
                            name: Some("项目整体检查".into()),
                            program: validator.tool.executable.path.clone(),
                            args: i.args.clone(),
                            timeout_seconds: i.timeout_seconds,
                            tool: Some(validator.tool.clone()),
                        },
                    }))
                })
                .transpose()?,
            schema_version: 1,
            base_commit: draft.base_commit.clone(),
            pool_root: pool.to_string_lossy().into(),
            git,
            input: if draft.inherit_dependencies {
                GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies
            } else {
                GitWorktreeInput::CommittedRawBlobs
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cerulean_domain::{
        management::{ManagementDecision, ProposedTask},
        project_setup::ProjectWorkerAdapter,
    };

    fn temporary() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("worktree-config-")
            .tempdir_in(
                fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports"))
                    .unwrap(),
            )
            .unwrap()
    }
    fn draft(root: &Path) -> WorktreeSetupDraft {
        fs::create_dir(root.join("pool")).unwrap();
        WorktreeSetupDraft {
            integration: None,
            inherit_dependencies: false,
            base_commit: "a".repeat(40),
            pool_root: root.join("pool").to_string_lossy().into(),
            git_executable: std::env::var("CERULEAN_TEST_GIT").expect("explicit Git"),
        }
    }

    #[test]
    #[ignore = "explicit pinned Git; dependency input configuration/approval binding"]
    fn dependency_policy_is_explicit_and_materializes_without_dispatch() {
        let root = temporary();
        let (mut host, mut setup) = super::super::project_planning::tests::fixture(root.path());
        let mut input = draft(root.path());
        input.inherit_dependencies = true;
        setup.worktree = Some(input);
        host.configure_project(setup).unwrap();
        let proposal = ManagementDecision::Plan {
            tasks: vec![
                ProposedTask {
                    artifact_paths: None,
                    objective_test: None,
                    profile_id: None,
                    id: "a".into(),
                    goal: "first".into(),
                    dependencies: vec![],
                    worker_id: "worker-codex".into(),
                    validator_id: "tests".into(),
                },
                ProposedTask {
                    artifact_paths: None,
                    objective_test: None,
                    profile_id: None,
                    id: "b".into(),
                    goal: "next".into(),
                    dependencies: vec!["a".into()],
                    worker_id: "worker-codex".into(),
                    validator_id: "tests".into(),
                },
            ],
            rationale: "fixture dependency plan".into(),
        };
        super::super::project_planning::tests::complete(&mut host, proposal);
        let plan = host.projects["project"].revision().unwrap().clone();
        assert_eq!(
            plan.worktree.as_ref().unwrap().input,
            GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies
        );
        assert!(host.projects["project"].approval().is_none());
        let mut old = plan.clone();
        old.worktree.as_mut().unwrap().input = GitWorktreeInput::CommittedRawBlobs;
        assert_ne!(
            plan.approval_digest().unwrap(),
            old.approval_digest().unwrap()
        );
        assert!(!root.path().join("pool/records").exists());
        drop(host);
        let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            reopened.projects["project"]
                .revision()
                .unwrap()
                .approval_digest()
                .unwrap(),
            plan.approval_digest().unwrap()
        );
    }

    #[test]
    #[ignore = "explicit real Git executable; configuration/approval/reopen, no model calls"]
    fn saved_policy_survives_reopen_and_enters_user_approval_without_preparation() {
        for adapter in [
            ProjectWorkerAdapter::Codex,
            ProjectWorkerAdapter::Omp,
            ProjectWorkerAdapter::Deepseek,
        ] {
            let root = temporary();
            let (mut host, mut setup) = super::super::project_planning::tests::fixture(root.path());
            setup.worker_adapter = adapter;
            if adapter.is_api() {
                setup.codex_executable = String::new();
                setup.api_worker = Some(cerulean_domain::api_worker::ApiWorkerSelection {
                    write_scope: None,
                    file_tools: vec![ToolPermission::ReadFiles, ToolPermission::WriteFiles],
                    max_rounds: 3,
                    max_tool_calls: 8,
                    max_output_tokens: 512,
                    max_total_tokens: 4096,
                    request_timeout_seconds: 30,
                    target: cerulean_domain::api_worker::ApiTarget::Provider {
                        endpoint: cerulean_domain::credentials::ProviderEndpoint::DeepSeek {},
                    },
                    model: setup.worker_model.clone(),
                });
            }
            setup.worktree = Some(draft(root.path()));
            host.configure_project(setup.clone()).unwrap();
            let saved = host.project_setups["project"].clone();
            let policy = saved.setup.worktree.clone().unwrap();
            assert_eq!(
                policy.git,
                crate::verifier::PinnedCheck::describe(
                    Path::new(&setup.worktree.as_ref().unwrap().git_executable),
                    &[],
                    Default::default()
                )
                .unwrap()
                .executable
            );
            assert!(!root.path().join("pool/records").exists());
            assert!(host.projects["project"].revision().is_none());
            drop(host);
            let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
            assert_eq!(host.project_setups["project"].digest, saved.digest);
            let proposal = ManagementDecision::Plan {
                tasks: vec![ProposedTask {
                    artifact_paths: None,
                    objective_test: None,
                    profile_id: None,
                    id: "task".into(),
                    goal: "isolated task".into(),
                    dependencies: vec![],
                    worker_id: adapter.worker_id(),
                    validator_id: "tests".into(),
                }],
                rationale: "fixture proposal".into(),
            };
            super::super::project_planning::tests::complete(&mut host, proposal);
            let plan = host.projects["project"].revision().unwrap().clone();
            assert_eq!(plan.worktree, Some(policy));
            assert!(host.projects["project"].approval().is_none());
            let digest = plan.approval_digest().unwrap();
            let mut changed = plan.clone();
            changed.worktree.as_mut().unwrap().base_commit = "b".repeat(40);
            assert_ne!(changed.approval_digest().unwrap(), digest);
            host.approve("project", &digest).unwrap();
            assert!(!root.path().join("pool/records").exists());
            drop(host);
            let host = RuntimeHost::open(&root.path().join("data")).unwrap();
            assert_eq!(
                host.projects["project"].approval().unwrap().revision_digest,
                digest
            );
        }
    }

    #[test]
    #[ignore = "explicit Git; conflicting directories and unsupported dependent plans"]
    fn rejects_alias_scope_and_dependent_plan_and_preserves_legacy_bytes() {
        let root = temporary();
        let (mut host, mut setup) = super::super::project_planning::tests::fixture(root.path());
        host.configure_project(setup.clone()).unwrap();
        let file = host.root.join("project-setups/project.00000001.json");
        let original = fs::read(&file).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        assert!(value["setup"].get("worktree").is_none());
        let valid = draft(root.path());
        for pool in [
            root.path().join("project"),
            root.path().join("data"),
            root.path().to_path_buf(),
        ] {
            let mut bad = valid.clone();
            bad.pool_root = pool.to_string_lossy().into();
            assert!(
                host.configure_worktree(&bad).is_err(),
                "accepted overlapping/alias pool: {}",
                bad.pool_root
            );
        }
        let mut bad = valid.clone();
        // PathBuf::join normalizes dot segments on a verbatim Windows base;
        // construct the actual user input string before validation instead.
        bad.pool_root = format!("{}\\pool\\..\\pool", root.path().display());
        assert!(host.configure_worktree(&bad).is_err());
        let mut bad = valid.clone();
        bad.base_commit = "HEAD".into();
        assert!(host.configure_worktree(&bad).is_err());
        let mut desktop = setup.clone();
        desktop.worker_adapter = ProjectWorkerAdapter::Workbuddy;
        desktop.worktree = Some(valid.clone());
        assert!(desktop.validate().is_err());
        setup.worktree = Some(valid);
        host.configure_project(setup).unwrap();
        assert_eq!(fs::read(&file).unwrap(), original);
        let proposal = ManagementDecision::Plan {
            tasks: vec![
                ProposedTask {
                    artifact_paths: None,
                    objective_test: None,
                    profile_id: None,
                    id: "a".into(),
                    goal: "first".into(),
                    dependencies: vec![],
                    worker_id: "worker-codex".into(),
                    validator_id: "tests".into(),
                },
                ProposedTask {
                    artifact_paths: None,
                    objective_test: None,
                    profile_id: None,
                    id: "b".into(),
                    goal: "depends on first".into(),
                    dependencies: vec!["a".into()],
                    worker_id: "worker-codex".into(),
                    validator_id: "tests".into(),
                },
            ],
            rationale: "unsupported input transfer".into(),
        };
        super::super::project_planning::tests::complete(&mut host, proposal);
        assert!(host.projects["project"].revision().is_none());
        assert!(host
            .planning_errors
            .values()
            .any(|e| e.contains("依赖产物")));
        assert!(!root.path().join("pool/records").exists());
        drop(host);
        RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(fs::read(file).unwrap(), original);
    }
}
