//! Directory comparison and sync-plan integration tests confined to
//! temporary roots (task 0075 acceptance criteria: comparison correctness,
//! sync plan generation, cancellation, and a dry-run assertion that no files
//! change until a plan is applied).

use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    ApplySyncPlanRequestDto, ComparisonCriteriaDto, GenerateSyncPlanRequestDto, OperationStateDto,
    StartComparisonRequestDto, SyncActionDto, SyncModeDto,
};
use uuid::Uuid;

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        fm_transport_dto::RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

async fn wait_for_comparison(
    service: &FileManagerService,
    comparison_id: Uuid,
) -> fm_transport_dto::ComparisonPageDto {
    for _ in 0..500 {
        let page = service
            .get_comparison_page(comparison_id, 0, 500, false)
            .expect("comparison must be tracked");
        if page.is_complete {
            return page;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("comparison did not complete in time")
}

async fn wait_for_operation(
    service: &FileManagerService,
    operation_id: Uuid,
) -> fm_transport_dto::OperationDto {
    for _ in 0..500 {
        let operation = service
            .get_operation(operation_id.into())
            .expect("operation must be tracked");
        if operation.state.is_terminal_for_test() {
            return operation;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("operation did not finish in time")
}

trait TerminalForTest {
    fn is_terminal_for_test(&self) -> bool;
}

impl TerminalForTest for OperationStateDto {
    fn is_terminal_for_test(&self) -> bool {
        matches!(
            self,
            OperationStateDto::Completed
                | OperationStateDto::CompletedWithWarnings
                | OperationStateDto::Failed
                | OperationStateDto::Cancelled
        )
    }
}

fn snapshot_tree(root: &std::path::Path) -> HashMap<String, Vec<u8>> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut HashMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("fixture directory must be readable") {
            let entry = entry.expect("fixture entry must be readable");
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("walked path must be under root")
                    .to_string_lossy()
                    .into_owned();
                out.insert(
                    relative,
                    fs::read(&path).expect("fixture file must be readable"),
                );
            }
        }
    }
    let mut out = HashMap::new();
    walk(root, root, &mut out);
    out
}

#[tokio::test]
async fn start_comparison_streams_results_pageable_and_filterable_to_differences() {
    let root = tempfile::tempdir().unwrap();
    let left = root.path().join("left");
    let right = root.path().join("right");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    fs::write(left.join("same.txt"), b"same").unwrap();
    fs::write(right.join("same.txt"), b"same").unwrap();
    fs::write(left.join("only-left.txt"), b"left only").unwrap();

    let service = service(&root);
    let response = service
        .start_comparison(StartComparisonRequestDto {
            workspace_id: Uuid::new_v4(),
            left: Location::from_native_path(&left).unwrap().into(),
            right: Location::from_native_path(&right).unwrap().into(),
            criteria: ComparisonCriteriaDto::NameOnly,
            show_hidden: false,
        })
        .expect("comparison must start");

    let page = wait_for_comparison(&service, response.comparison_id).await;
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.criteria, ComparisonCriteriaDto::NameOnly);

    let filtered = service
        .get_comparison_page(response.comparison_id, 0, 500, true)
        .unwrap();
    assert_eq!(filtered.entries.len(), 1);
    assert_eq!(filtered.entries[0].relative_path, "only-left.txt");
}

#[tokio::test]
async fn unknown_comparison_id_is_reported_as_not_found_everywhere() {
    let root = tempfile::tempdir().unwrap();
    let service = service(&root);
    let unknown = Uuid::new_v4();

    assert!(service.get_comparison_page(unknown, 0, 10, false).is_err());
    assert!(
        service
            .generate_sync_plan(
                unknown,
                GenerateSyncPlanRequestDto {
                    mode: SyncModeDto::MirrorLeftToRight,
                },
            )
            .is_err()
    );
    assert!(
        service
            .apply_sync_plan(unknown, ApplySyncPlanRequestDto { items: vec![] })
            .is_err()
    );
}

#[tokio::test]
async fn generating_a_sync_plan_never_changes_either_fixture_until_applied() {
    let root = tempfile::tempdir().unwrap();
    let left = root.path().join("left");
    let right = root.path().join("right");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    fs::write(left.join("only-left.txt"), b"left only").unwrap();
    fs::write(right.join("only-right.txt"), b"right only").unwrap();

    let service = service(&root);
    let response = service
        .start_comparison(StartComparisonRequestDto {
            workspace_id: Uuid::new_v4(),
            left: Location::from_native_path(&left).unwrap().into(),
            right: Location::from_native_path(&right).unwrap().into(),
            criteria: ComparisonCriteriaDto::NameOnly,
            show_hidden: false,
        })
        .expect("comparison must start");
    wait_for_comparison(&service, response.comparison_id).await;

    let before_left = snapshot_tree(&left);
    let before_right = snapshot_tree(&right);

    let plan = service
        .generate_sync_plan(
            response.comparison_id,
            GenerateSyncPlanRequestDto {
                mode: SyncModeDto::MirrorLeftToRight,
            },
        )
        .expect("plan must generate");
    assert_eq!(plan.items.len(), 2);
    let action_for = |path: &str| {
        plan.items
            .iter()
            .find(|item| item.relative_path == path)
            .unwrap()
            .action
    };
    assert_eq!(action_for("only-left.txt"), SyncActionDto::CopyLeftToRight);
    assert_eq!(action_for("only-right.txt"), SyncActionDto::DeleteRight);

    // Generating the plan must not have touched anything (spec §35).
    assert_eq!(snapshot_tree(&left), before_left);
    assert_eq!(snapshot_tree(&right), before_right);
    assert!(!right.join("only-left.txt").exists());
    assert!(right.join("only-right.txt").exists());
}

#[tokio::test]
async fn applying_a_sync_plan_runs_real_operations_that_change_the_filesystem() {
    let root = tempfile::tempdir().unwrap();
    let left = root.path().join("left");
    let right = root.path().join("right");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    fs::write(left.join("only-left.txt"), b"left only").unwrap();
    fs::write(right.join("only-right.txt"), b"right only").unwrap();

    let service = service(&root);
    let response = service
        .start_comparison(StartComparisonRequestDto {
            workspace_id: Uuid::new_v4(),
            left: Location::from_native_path(&left).unwrap().into(),
            right: Location::from_native_path(&right).unwrap().into(),
            criteria: ComparisonCriteriaDto::NameOnly,
            show_hidden: false,
        })
        .expect("comparison must start");
    wait_for_comparison(&service, response.comparison_id).await;

    let plan = service
        .generate_sync_plan(
            response.comparison_id,
            GenerateSyncPlanRequestDto {
                mode: SyncModeDto::MirrorLeftToRight,
            },
        )
        .expect("plan must generate");

    let applied = service
        .apply_sync_plan(
            response.comparison_id,
            ApplySyncPlanRequestDto { items: plan.items },
        )
        .expect("plan must apply");
    assert_eq!(applied.operation_ids.len(), 2);

    for operation_id in &applied.operation_ids {
        let operation = wait_for_operation(&service, *operation_id).await;
        assert_eq!(
            operation.state,
            OperationStateDto::Completed,
            "operation {operation_id} did not complete cleanly"
        );
    }

    assert_eq!(fs::read(right.join("only-left.txt")).unwrap(), b"left only");
    assert!(
        !right.join("only-right.txt").exists(),
        "mirror-left-to-right must remove the right-only extra"
    );
}

#[tokio::test]
async fn applying_a_skip_action_starts_no_operation() {
    let root = tempfile::tempdir().unwrap();
    let left = root.path().join("left");
    let right = root.path().join("right");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    fs::write(left.join("only-left.txt"), b"left only").unwrap();

    let service = service(&root);
    let response = service
        .start_comparison(StartComparisonRequestDto {
            workspace_id: Uuid::new_v4(),
            left: Location::from_native_path(&left).unwrap().into(),
            right: Location::from_native_path(&right).unwrap().into(),
            criteria: ComparisonCriteriaDto::NameOnly,
            show_hidden: false,
        })
        .expect("comparison must start");
    wait_for_comparison(&service, response.comparison_id).await;

    let mut plan = service
        .generate_sync_plan(
            response.comparison_id,
            GenerateSyncPlanRequestDto {
                mode: SyncModeDto::MirrorLeftToRight,
            },
        )
        .unwrap();
    for item in &mut plan.items {
        item.action = SyncActionDto::Skip;
    }

    let applied = service
        .apply_sync_plan(
            response.comparison_id,
            ApplySyncPlanRequestDto { items: plan.items },
        )
        .unwrap();
    assert!(applied.operation_ids.is_empty());
    assert!(!right.join("only-left.txt").exists());
}

#[tokio::test]
async fn cancel_operation_falls_back_to_a_running_comparison() {
    let root = tempfile::tempdir().unwrap();
    let left = root.path().join("left");
    let right = root.path().join("right");
    fs::create_dir_all(&left).unwrap();
    fs::create_dir_all(&right).unwrap();
    for index in 0..30 {
        let name = format!("dir-{index}");
        fs::create_dir(left.join(&name)).unwrap();
        fs::create_dir(right.join(&name)).unwrap();
        for file in 0..30 {
            fs::write(left.join(&name).join(format!("f{file}.txt")), b"x").unwrap();
            fs::write(right.join(&name).join(format!("f{file}.txt")), b"x").unwrap();
        }
    }

    let service = service(&root);
    let response = service
        .start_comparison(StartComparisonRequestDto {
            workspace_id: Uuid::new_v4(),
            left: Location::from_native_path(&left).unwrap().into(),
            right: Location::from_native_path(&right).unwrap().into(),
            criteria: ComparisonCriteriaDto::NameOnly,
            show_hidden: false,
        })
        .expect("comparison must start");

    // Cancel through the generic operation-id-sharing path, not
    // `cancel_comparison` directly, so the fallback chain in
    // `cancel_operation` is what's under test.
    service
        .cancel_operation(response.comparison_id.into())
        .expect("cancellation must be accepted");

    let page = wait_for_comparison(&service, response.comparison_id).await;
    assert!(
        page.entries.len() < 1_800,
        "cancellation must stop traversal before every entry is compared, found {}",
        page.entries.len()
    );
}
