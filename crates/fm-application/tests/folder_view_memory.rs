//! Per-folder view memory through `apply_workspace_command` (task 0270).

use fm_application::FileManagerService;
use fm_domain::Location;
use fm_transport_dto::{
    DirectoryViewModeDto, LocationDto, RuntimeKindDto, SettingsDto, WorkspaceCommandDto,
    WorkspaceDto,
};
use serde_json::json;

fn service(root: &tempfile::TempDir) -> FileManagerService {
    FileManagerService::new(
        RuntimeKindDto::BrowserServer,
        root.path().join("workspaces"),
        root.path().join("settings"),
    )
}

fn location(path: &std::path::Path) -> LocationDto {
    LocationDto::from(Location::from_native_path(path).expect("location"))
}

fn command(value: serde_json::Value) -> WorkspaceCommandDto {
    serde_json::from_value(value).expect("valid workspace command")
}

async fn navigate(
    service: &FileManagerService,
    workspace: &WorkspaceDto,
    to: &LocationDto,
) -> WorkspaceDto {
    let pane = &workspace.panes[0];
    service
        .apply_workspace_command(command(json!({
            "type": "navigateTab",
            "workspaceId": workspace.id,
            "paneId": pane.id,
            "tabId": pane.active_tab_id,
            "location": to,
            "navigationMode": "push",
            "expectedRevision": workspace.revision,
        })))
        .await
        .expect("navigate")
}

async fn set_grid(service: &FileManagerService, workspace: &WorkspaceDto) -> WorkspaceDto {
    let pane = &workspace.panes[0];
    service
        .apply_workspace_command(command(json!({
            "type": "updateView",
            "workspaceId": workspace.id,
            "paneId": pane.id,
            "tabId": pane.active_tab_id,
            "patch": {
                "viewMode": "grid",
                "sort": [{ "columnId": "core.modified", "direction": "descending" }]
            },
            "expectedRevision": workspace.revision,
        })))
        .await
        .expect("update view")
}

fn active_view(workspace: &WorkspaceDto) -> &fm_transport_dto::DirectoryViewConfigurationDto {
    let pane = &workspace.panes[0];
    &pane
        .tabs
        .iter()
        .find(|tab| tab.id == pane.active_tab_id)
        .expect("active tab")
        .view
}

fn folders(root: &tempfile::TempDir) -> (LocationDto, LocationDto) {
    let photos = root.path().join("photos");
    let other = root.path().join("other");
    std::fs::create_dir_all(&photos).expect("photos");
    std::fs::create_dir_all(&other).expect("other");
    (location(&photos), location(&other))
}

#[tokio::test]
async fn a_customised_folder_gets_its_view_back_and_others_stay_default() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let (photos, other) = folders(&root);
    let mut workspace = service.create_workspace(None).await.expect("workspace");

    workspace = navigate(&service, &workspace, &photos).await;
    assert_eq!(
        active_view(&workspace).view_mode,
        DirectoryViewModeDto::Table
    );
    workspace = set_grid(&service, &workspace).await;
    assert_eq!(
        active_view(&workspace).view_mode,
        DirectoryViewModeDto::Grid
    );

    // The remembered folder restores its own view when returned to, whatever the tab showed
    // in between.
    workspace = navigate(&service, &workspace, &other).await;
    workspace = set_table_default(&service, &workspace).await;
    assert_eq!(
        active_view(&workspace).view_mode,
        DirectoryViewModeDto::Table
    );

    workspace = navigate(&service, &workspace, &photos).await;
    let restored = active_view(&workspace);
    assert_eq!(restored.view_mode, DirectoryViewModeDto::Grid);
    assert_eq!(restored.sort[0].column_id, "core.modified");
}

async fn set_table_default(service: &FileManagerService, workspace: &WorkspaceDto) -> WorkspaceDto {
    let pane = &workspace.panes[0];
    service
        .apply_workspace_command(command(json!({
            "type": "updateView",
            "workspaceId": workspace.id,
            "paneId": pane.id,
            "tabId": pane.active_tab_id,
            "patch": { "viewMode": "table", "sort": [] },
            "expectedRevision": workspace.revision,
        })))
        .await
        .expect("update view")
}

#[tokio::test]
async fn the_opt_out_setting_stops_remembering_and_restoring() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let (photos, other) = folders(&root);
    let mut settings: SettingsDto = service.get_settings();
    settings.remember_folder_views = false;
    service.update_settings(settings).expect("opt out");
    let mut workspace = service.create_workspace(None).await.expect("workspace");

    workspace = navigate(&service, &workspace, &photos).await;
    workspace = set_grid(&service, &workspace).await;
    workspace = navigate(&service, &workspace, &other).await;
    workspace = set_table_default(&service, &workspace).await;
    workspace = navigate(&service, &workspace, &photos).await;

    assert_eq!(
        active_view(&workspace).view_mode,
        DirectoryViewModeDto::Table
    );
}

#[tokio::test]
async fn forgetting_a_folder_restores_defaults_next_time_and_settings_saves_keep_the_memory() {
    let root = tempfile::tempdir().expect("temp dir");
    let service = service(&root);
    let (photos, other) = folders(&root);
    let mut workspace = service.create_workspace(None).await.expect("workspace");
    workspace = navigate(&service, &workspace, &photos).await;
    workspace = set_grid(&service, &workspace).await;

    // A client replaying its (older) settings document must not wipe the backend-owned memory.
    service
        .update_settings(service.get_settings())
        .expect("save settings");
    assert!(service.forget_folder_view(photos.clone()).expect("forget"));
    assert!(
        !service
            .forget_folder_view(photos.clone())
            .expect("forget again")
    );

    workspace = navigate(&service, &workspace, &other).await;
    workspace = set_table_default(&service, &workspace).await;
    workspace = navigate(&service, &workspace, &photos).await;
    assert_eq!(
        active_view(&workspace).view_mode,
        DirectoryViewModeDto::Table
    );
}
