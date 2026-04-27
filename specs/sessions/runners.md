# Script Runners

## Overview

The `runner/` module contains `EnvironmentScriptRunner` and `StepScriptRunner`, which
orchestrate the execution of environment and step actions respectively. They handle
format string resolution, embedded file materialization, let binding evaluation, and
delegate to `run_subprocess()` for actual process execution.

## CancelMethod

```rust
pub enum CancelMethod {
    Terminate,
    NotifyThenTerminate { terminate_delay: Duration },
}
```

Derived from the action's `cancelation` field in the template:
- `TerminateCancelMethod` → `CancelMethod::Terminate` (immediate kill)
- `NotifyThenTerminate` → `CancelMethod::NotifyThenTerminate` with the specified
  `notifyPeriodInSeconds` (graceful notify → grace period → forced kill)

Default grace periods match the Python library:
- `onRun` actions: 120 seconds
- Environment actions (`onEnter`/`onExit`): 30 seconds

### Default grace period rationale

The default grace periods differ by action type:

| Action type | Default grace | Rationale |
|-------------|--------------|----------|
| `onRun` (task) | 120 seconds | Tasks may need significant time to save state, flush render output, or checkpoint progress. A 2-minute window accommodates most graceful shutdown patterns. |
| `onEnter`/`onExit` (env) | 30 seconds | Environment scripts are typically short-lived (install packages, start/stop services). A 30-second window is generous for these operations. |
| `exit()` overall timeout | 300 seconds | Environment exit may involve tearing down containers, unmounting filesystems, or deregistering services. The 5-minute overall timeout accommodates complex teardown. |

These defaults match the Python library. The `onRun` and env script defaults come from
the `notifyPeriodInSeconds` field in the OpenJD spec's
[`<CancelationMethodNotifyThenTerminate>`](https://github.com/OpenJobDescription/openjd-specifications/wiki/2023-09-Template-Schemas#532-cancelationmethodnotifythenterminate).
The 300-second exit timeout is a session-level safety net, not a spec-defined value.

## ScriptRunnerState

```rust
pub enum ScriptRunnerState {
    Ready,
    Running,
    Canceling,
    Canceled,
    Timeout,
    Failed,
    Success,
}
```

Tracks the runner's lifecycle independently from `SessionState`. The session maps
runner completion states to session state transitions.

## resolve_action_args

```rust
pub fn resolve_action_args(
    action: &Action,
    symtab: &SymbolTable,
    library: &FunctionLibrary,
    path_mapping_rules: &[PathMappingRule],
) -> Result<Vec<String>, SessionError>
```

Resolves format strings in the action's `command` and `args` fields:

1. Resolve `command` format string → first element of the args vector
2. For each arg in `args`:
   - Resolve the format string
   - If the result is `null` (ExprValue::None), skip the arg entirely
   - If the result is a list, flatten each element into the args vector
   - Otherwise, convert to string and append

### Why null args are skipped

The EXPR extension allows format strings to evaluate to `null`. For command arguments,
`null` means "omit this argument." This enables conditional arguments:

```yaml
args:
  - "{{Param.OptionalFlag if Param.UseFlag else null}}"
```

### Why list args are flattened

List-valued expressions expand into multiple arguments:

```yaml
args:
  - "{{Param.InputFiles}}"  # LIST_PATH → multiple file paths
```

This matches the Python library's behavior and enables passing variable-length argument
lists without knowing the count at template authoring time.

## EnvironmentScriptRunner

### enter()

Runs the environment's `onEnter` action:

```rust
pub async fn enter(
    &mut self,
    env: &Environment,
    symtab: &SymbolTable,
    library: &FunctionLibrary,
    files_directory: &Path,
    path_mapping_rules: &[PathMappingRule],
    env_vars: &HashMap<String, String>,
    message_tx: mpsc::UnboundedSender<ActionMessage>,
    cancel_token: CancellationToken,
    user: Option<&PosixSessionUser>,
) -> Result<SubprocessResult, SessionError>
```

The two-phase embedded file flow handles four cases:

1. **Both let bindings and embedded files**: This is the complex case. Let bindings may
   reference `Env.File.<name>` paths, and embedded file data may reference let-bound
   values. The flow is:
   - `EmbeddedFiles::allocate_file_paths()` — determines paths, registers
     `Env.File.*` in the symbol table
   - `evaluate_let_bindings()` — evaluates let bindings against the symbol table
     (which now includes `Env.File.*` paths)
   - `EmbeddedFiles::write_file_contents()` — resolves format strings in file data
     using the let-binding-enriched symbol table

2. **Only let bindings**: Evaluate let bindings, no file materialization.

3. **Only embedded files**: Allocate paths and write contents back-to-back (no let
   bindings to interleave).

4. **Neither**: Clone the symbol table unchanged.

### Why two-phase is necessary

Consider this template:

```yaml
script:
  let:
    - script_path = Env.File.MyScript
  embeddedFiles:
    - name: MyScript
      data: "#!/bin/bash\necho {{Param.Message}}"
      runnable: true
  actions:
    onEnter:
      command: "{{script_path}}"
```

The let binding `script_path` references `Env.File.MyScript`, which is the path where
the embedded file will be written. But the file path isn't known until
`allocate_file_paths()` runs. And the file's `data` field may reference let-bound values.

The two-phase approach resolves this circular dependency:
1. Allocate paths (now `Env.File.MyScript` is in the symbol table)
2. Evaluate let bindings (now `script_path` resolves to the allocated path)
3. Write file contents (can use both `Env.File.*` and let-bound values)

### exit()

Runs the environment's `onExit` action. Same flow as `enter()` but with a default
timeout of 300 seconds (5 minutes) matching the spec.

## StepScriptRunner

### run()

Runs the step's `onRun` action:

```rust
pub async fn run(
    &mut self,
    step_script: &StepScript,
    symtab: &SymbolTable,
    library: &FunctionLibrary,
    files_directory: &Path,
    path_mapping_rules: &[PathMappingRule],
    env_vars: &HashMap<String, String>,
    message_tx: mpsc::UnboundedSender<ActionMessage>,
    cancel_token: CancellationToken,
    user: Option<&PosixSessionUser>,
) -> Result<SubprocessResult, SessionError>
```

The ordering differs from environment scripts:

1. Evaluate let bindings first (they can reference `Task.Param.*` but not `Task.File.*`
   since step-level let bindings are evaluated before embedded files)
2. Materialize embedded files (allocate paths + write contents, using the let-binding-
   enriched symbol table)
3. Resolve action args and run subprocess

### Why the ordering differs from EnvironmentScriptRunner

In the spec, `StepScript.let` bindings are scoped to the step script and are evaluated
before embedded files. This means let bindings can't reference `Task.File.*` paths
(unlike environment scripts where let bindings can reference `Env.File.*`). The simpler
ordering reflects this: let bindings first, then files, then action.

The Python library follows the same ordering distinction between environment and step
scripts.


## resolve_action_timeout

```rust
pub(crate) fn resolve_action_timeout(
    action: &Action,
    symtab: &SymbolTable,
    library: Option<&FunctionLibrary>,
    rules: &[PathMappingRule],
    default: Option<Duration>,
) -> Result<Option<Duration>, SessionError>
```

Resolves an action's `timeout` format string to a `Duration`. If the action has no
timeout field, returns the `default`. The resolved value must be a positive integer
(seconds). Zero is rejected with an error.

## Runner Builder Methods

Both `EnvironmentScriptRunner` and `StepScriptRunner` wrap a shared `ScriptRunnerBase`
and expose identical builder methods:

| Method | Effect |
|--------|--------|
| `with_redactions(bool)` | Enable/disable redaction processing in the `ActionFilter` |
| `with_debug_collect_stdout(bool)` | Enable/disable stdout accumulation in `SubprocessResult` |
| `with_initial_redacted_values(Vec<String>)` | Pre-populate the redaction set (from session's accumulated values) |
| `with_cancel_token(CancellationToken)` | Set the cancellation token for the action |
| `with_cancel_request_rx(Receiver<Option<Duration>>)` | Set the cancel request channel for time-limited cancellation |
| `with_helper(CrossUserHelper)` | Transfer the cross-user helper process to this runner |
| `take_helper() -> Option<CrossUserHelper>` | Take the helper back after the action completes |

The `with_helper` / `take_helper` pair transfers ownership of the persistent cross-user
helper process between the session and the runner for each action, avoiding the need to
spawn a new sudo process per action.
