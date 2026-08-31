# 0123 Extract Plugin Manager module

Status: done
Priority: medium
Subsystem: backend
Depends on: 0119

## Context

`FileManagerService` contains plugin lifecycle methods: `list_plugins()` (~80 lines), `plugin_icon_theme_asset()` (~35 lines), `plugin_logs()` (~15 lines), `set_plugin_enabled()` (~35 lines), `invoke_plugin_action()` (~50 lines), `enabled_plugin_manifests()` (~20 lines), `find_plugin_action()` (~20 lines). These are self-contained, share `enabled_plugin_manifests()` as a common helper, and can be extracted into a `PluginManager` module.

## Acceptance Criteria
- `PluginManager` module with methods for: list, enable/disable, logs, icon theme asset serving, action invocation
- `enabled_plugin_manifests()` and `find_plugin_action()` contained as internal helpers
- Plugin icon theme asset path-traversal protection contained within the module
- `FileManagerService` delegates to a single `plugins` field
- Tests for plugin enable/disable lifecycle, icon asset path-traversal rejection
- Zero behavioural changes

## Implementation Notes
- Needs: `PluginDiscovery`, `PluginRuntime`, access to `Settings` (for enabled_plugins list), `SettingsStore` (for persistence), `EventBus` (for plugin.changed events)
- The action invocation path is the trickiest — plugin actions need to flow back through `FileManagerService` for event publishing (clipboard notifications). Consider whether the manager owns event publishing or returns outcomes the facade publishes.
- ~200 lines removed from `service.rs`

## Agent Notes
