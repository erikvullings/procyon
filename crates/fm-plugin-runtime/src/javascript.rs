use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use fm_plugin_api::{Permission, PluginManifest, SelectedEntryContext};
use rquickjs::{Context, Ctx, Exception, Function, Object, Runtime, Value, function::Func};

use super::{MAX_OUTPUT_BYTES, PluginCancellation, PluginRuntime};

// Matches JS_INTERRUPT_COUNTER_INIT in the vendored QuickJS 0.11 engine.
const INTERRUPT_INSTRUCTION_STEP: usize = 10_000;
const MAX_SETUP_TIME: std::time::Duration = std::time::Duration::from_secs(1);
const CANCELLED: u8 = 1;
const TIMED_OUT: u8 = 2;
const INSTRUCTION_LIMIT: u8 = 3;

pub(super) fn execute(
    runtime: &PluginRuntime,
    manifest: &PluginManifest,
    source: &str,
    contribution: &str,
    cancellation: &PluginCancellation,
) -> Result<String, String> {
    execute_with_setup(runtime, manifest, source, contribution, cancellation, || {})
}

fn execute_with_setup(
    runtime: &PluginRuntime,
    manifest: &PluginManifest,
    source: &str,
    contribution: &str,
    cancellation: &PluginCancellation,
    setup: impl FnOnce(),
) -> Result<String, String> {
    let started = Instant::now();
    let setup_deadline = started + MAX_SETUP_TIME;
    let deadline = Arc::new(Mutex::new(setup_deadline));
    let (engine, interrupted) = new_engine(runtime, cancellation, &deadline)?;
    setup();
    let context = Context::full(&engine).map_err(|error| error.to_string())?;
    check_setup(setup_deadline, cancellation)?;
    let clipboard = Arc::new(Mutex::new(None::<String>));
    let mut execution_deadline = setup_deadline;
    let result = context.with(|ctx| {
        install_host(&ctx, manifest, &[], &clipboard)?;
        check_setup(setup_deadline, cancellation)?;
        execution_deadline = Instant::now() + runtime.timeout;
        *deadline.lock().unwrap_or_else(|error| error.into_inner()) = execution_deadline;
        let module: Object<'_> = ctx.eval(source).map_err(|error| diagnostic(&ctx, error))?;
        let function: Function<'_> = module
            .get(contribution)
            .map_err(|_| format!("malformed plugin result: missing {contribution} function"))?;
        let value: Value<'_> = function.call(()).map_err(|error| diagnostic(&ctx, error))?;
        let output = ctx
            .json_stringify(value)
            .map_err(|error| diagnostic(&ctx, error))?
            .ok_or_else(|| "malformed plugin result: undefined contribution".to_owned())?
            .to_string()
            .map_err(|error| error.to_string())?;
        if output.len() > MAX_OUTPUT_BYTES {
            return Err("plugin output budget exceeded".to_owned());
        }
        Ok(output)
    });
    match interrupted.load(Ordering::Relaxed) {
        CANCELLED => Err("plugin call cancelled".to_owned()),
        TIMED_OUT => Err("plugin execution timed out".to_owned()),
        INSTRUCTION_LIMIT => Err("plugin instruction budget exceeded".to_owned()),
        _ if cancellation.is_cancelled() => Err("plugin call cancelled".to_owned()),
        _ if Instant::now() >= execution_deadline => Err("plugin execution timed out".to_owned()),
        _ => result,
    }
}

pub(super) fn invoke(
    runtime: &PluginRuntime,
    manifest: &PluginManifest,
    source: &str,
    action_id: &str,
    selection: &[SelectedEntryContext],
    cancellation: &PluginCancellation,
) -> Result<Option<String>, String> {
    invoke_with_setup(
        runtime,
        manifest,
        source,
        action_id,
        selection,
        cancellation,
        || {},
    )
}

fn invoke_with_setup(
    runtime: &PluginRuntime,
    manifest: &PluginManifest,
    source: &str,
    action_id: &str,
    selection: &[SelectedEntryContext],
    cancellation: &PluginCancellation,
    setup: impl FnOnce(),
) -> Result<Option<String>, String> {
    let started = Instant::now();
    let setup_deadline = started + MAX_SETUP_TIME;
    let deadline = Arc::new(Mutex::new(setup_deadline));
    let (engine, interrupted) = new_engine(runtime, cancellation, &deadline)?;
    setup();
    let context = Context::full(&engine).map_err(|error| error.to_string())?;
    check_setup(setup_deadline, cancellation)?;
    let clipboard = Arc::new(Mutex::new(None::<String>));
    let mut execution_deadline = setup_deadline;
    let result = context.with(|ctx| {
        install_host(&ctx, manifest, selection, &clipboard)?;
        check_setup(setup_deadline, cancellation)?;
        execution_deadline = Instant::now() + runtime.timeout;
        *deadline.lock().unwrap_or_else(|error| error.into_inner()) = execution_deadline;
        let module: Object<'_> = ctx.eval(source).map_err(|error| diagnostic(&ctx, error))?;
        let function: Function<'_> = module
            .get("invoke")
            .map_err(|_| "malformed plugin result: missing invoke function".to_owned())?;
        let value: Value<'_> = function
            .call((action_id,))
            .map_err(|error| diagnostic(&ctx, error))?;
        if !value.is_undefined() && !value.is_null() {
            return Err("malformed plugin result: invoke must not return a value".to_owned());
        }
        Ok(clipboard
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone())
    });
    match interrupted.load(Ordering::Relaxed) {
        CANCELLED => Err("plugin call cancelled".to_owned()),
        TIMED_OUT => Err("plugin execution timed out".to_owned()),
        INSTRUCTION_LIMIT => Err("plugin instruction budget exceeded".to_owned()),
        _ if cancellation.is_cancelled() => Err("plugin call cancelled".to_owned()),
        _ if Instant::now() >= execution_deadline => Err("plugin execution timed out".to_owned()),
        _ => result,
    }
}

fn check_setup(deadline: Instant, cancellation: &PluginCancellation) -> Result<(), String> {
    if cancellation.is_cancelled() {
        return Err("plugin call cancelled".to_owned());
    }
    if Instant::now() >= deadline {
        return Err("plugin execution timed out".to_owned());
    }
    Ok(())
}

fn new_engine(
    plugin: &PluginRuntime,
    cancellation: &PluginCancellation,
    deadline: &Arc<Mutex<Instant>>,
) -> Result<(Runtime, Arc<AtomicU8>), String> {
    if cancellation.is_cancelled() {
        return Err("plugin call cancelled".to_owned());
    }
    let engine = Runtime::new().map_err(|error| error.to_string())?;
    check_setup(
        *deadline.lock().unwrap_or_else(|error| error.into_inner()),
        cancellation,
    )?;
    engine.set_memory_limit(plugin.memory_limit_bytes.max(1));
    engine.set_max_stack_size(256 * 1024);
    let deadline = Arc::clone(deadline);
    let instructions = AtomicUsize::new(0);
    let instruction_limit = plugin.instruction_limit;
    let cancellation = cancellation.clone();
    let interrupted = Arc::new(AtomicU8::new(0));
    let reason = Arc::clone(&interrupted);
    engine.set_interrupt_handler(Some(Box::new(move || {
        let outcome = if cancellation.is_cancelled() {
            CANCELLED
        } else if Instant::now() >= *deadline.lock().unwrap_or_else(|error| error.into_inner()) {
            TIMED_OUT
        } else if instructions.fetch_add(INTERRUPT_INSTRUCTION_STEP, Ordering::Relaxed)
            + INTERRUPT_INSTRUCTION_STEP
            > instruction_limit
        {
            INSTRUCTION_LIMIT
        } else {
            0
        };
        if outcome != 0 {
            reason.store(outcome, Ordering::Relaxed);
            true
        } else {
            false
        }
    })));
    Ok((engine, interrupted))
}

fn install_host(
    ctx: &Ctx<'_>,
    manifest: &PluginManifest,
    selection: &[SelectedEntryContext],
    clipboard: &Arc<Mutex<Option<String>>>,
) -> Result<(), String> {
    if selection.len() > 1024
        || selection
            .iter()
            .try_fold(0usize, |total, entry| {
                total
                    .checked_add(entry.name.len())?
                    .checked_add(entry.uri.len())?
                    .checked_add(16)
            })
            .is_none_or(|size| size > MAX_OUTPUT_BYTES)
    {
        return Err("plugin selection input budget exceeded".to_owned());
    }
    let metadata = serde_json::to_string(selection).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_OUTPUT_BYTES {
        return Err("plugin selection input budget exceeded".to_owned());
    }
    let permissions = manifest.permissions.clone();
    let clipboard_permissions = permissions.clone();
    let clipboard = Arc::clone(clipboard);
    let globals = ctx.globals();
    globals
        .set(
            "__host_selected_entry_metadata",
            Func::new(move || -> rquickjs::Result<String> {
                permissions
                    .require(Permission::SelectedEntryMetadata)
                    .map_err(|error| {
                        rquickjs::Error::new_from_js_message("host", "metadata", error.to_string())
                    })?;
                Ok(metadata.clone())
            }),
        )
        .map_err(|error| error.to_string())?;
    globals
        .set(
            "__host_clipboard_write",
            Func::new(move |text: String| -> rquickjs::Result<()> {
                clipboard_permissions
                    .require(Permission::ClipboardWrite)
                    .map_err(|error| {
                        rquickjs::Error::new_from_js_message("host", "clipboard", error.to_string())
                    })?;
                if text.len() > MAX_OUTPUT_BYTES {
                    return Err(rquickjs::Error::new_from_js_message(
                        "host",
                        "clipboard",
                        "plugin output budget exceeded",
                    ));
                }
                *clipboard.lock().unwrap_or_else(|error| error.into_inner()) = Some(text);
                Ok(())
            }),
        )
        .map_err(|error| error.to_string())?;
    ctx.eval::<(), _>(
        "globalThis.host = Object.freeze({ selected_entry_metadata() { return JSON.parse(__host_selected_entry_metadata()) }, clipboard_write(text) { return __host_clipboard_write(text) } });",
    )
    .map_err(|error| diagnostic(ctx, error))
}

fn diagnostic(ctx: &Ctx<'_>, error: rquickjs::Error) -> String {
    if !error.is_exception() {
        return error.to_string();
    }
    let exception = ctx.catch();
    if let Some(object) = exception.into_object() {
        if let Some(exception) = Exception::from_object(object.clone())
            && let Some(message) = exception.message()
        {
            return message.chars().take(256).collect();
        }
        if let Ok(message) = object.get::<_, String>("message") {
            return message.chars().take(256).collect();
        }
    }
    "JavaScript execution failed".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn manifest() -> PluginManifest {
        PluginManifest::parse(
            "id='example.js'\nname='JS'\nversion='1'\napi_version='1'\ndescription='JS'\nruntime='javascript'\nentrypoint='plugin.js'\n[contributions]\nactions=true",
        )
        .expect("manifest")
    }

    #[test]
    fn slow_engine_setup_does_not_spend_the_script_execution_budget() {
        let runtime = PluginRuntime::default();
        let manifest = manifest();
        let cancellation = PluginCancellation::default();
        let output = execute_with_setup(
            &runtime,
            &manifest,
            "({ actions() { return [] } })",
            "actions",
            &cancellation,
            || thread::sleep(Duration::from_millis(150)),
        )
        .expect("setup is not script execution");
        assert_eq!(output, "[]");
        let clipboard = invoke_with_setup(
            &runtime,
            &manifest,
            "({ invoke() {} })",
            "example.js.open",
            &[],
            &cancellation,
            || thread::sleep(Duration::from_millis(150)),
        )
        .expect("setup is not script execution");
        assert_eq!(clipboard, None);
    }

    #[test]
    fn stalled_engine_setup_hits_a_separate_wall_time_limit() {
        let runtime = PluginRuntime::default();
        let manifest = manifest();
        let cancellation = PluginCancellation::default();
        let error = execute_with_setup(
            &runtime,
            &manifest,
            "({ actions() { return [] } })",
            "actions",
            &cancellation,
            || thread::sleep(MAX_SETUP_TIME + Duration::from_millis(20)),
        )
        .expect_err("setup timeout");
        assert_eq!(error, "plugin execution timed out");
    }

    #[test]
    fn script_timeout_is_enforced_after_slow_setup() {
        let runtime = PluginRuntime::new(Duration::from_millis(20), 4 * 1024 * 1024, usize::MAX, 3);
        let error = execute_with_setup(
            &runtime,
            &manifest(),
            "while (true) {}",
            "actions",
            &PluginCancellation::default(),
            || thread::sleep(Duration::from_millis(150)),
        )
        .expect_err("script deadline");
        assert_eq!(error, "plugin execution timed out");
    }

    #[test]
    fn cancellation_during_setup_prevents_script_execution() {
        let runtime = PluginRuntime::default();
        let cancellation = PluginCancellation::default();
        let error = execute_with_setup(
            &runtime,
            &manifest(),
            "({ actions() { return [] } })",
            "actions",
            &cancellation,
            || cancellation.cancel(),
        )
        .expect_err("cancelled during setup");
        assert_eq!(error, "plugin call cancelled");
    }
}
