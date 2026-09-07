/*****************************************************************************
 * Events.p — Event and type declarations for the openjd-sessions model.
 *
 * Every event here abstracts a concrete interaction described in
 * specs/sessions/*.md. Payloads carry only what the invariants need — no
 * bytes, no real paths, no crypto. See README.md § "Out of scope".
 *****************************************************************************/

/* ---- Abstract identifiers -------------------------------------------------
 * We model identifiers as ints. Environments and actions get fresh ids from
 * the driver; tokens are abstract equality-only values.
 */
type tEnvId    = int;    // environment identifier on the LIFO stack
type tActionId = int;    // one per action instance (fresh each action)
type tToken    = int;    // helper auth token — compared only for equality
type tExitCode = int;

/* ---- Action kinds ---------------------------------------------------------
 * The four ways an action enters the Session: onEnter, onExit, onRun (task),
 * and the ad-hoc run_subprocess path.
 */
enum tActionKind {
    ENTER_ENV,
    EXIT_ENV,
    RUN_TASK,
    RUN_SUBPROCESS
}

/* ---- ActionState (src/action_status.rs) -----------------------------------
 * Terminal states: Success, Failed, Canceled, Timeout. Running is the only
 * non-terminal state.
 */
enum tActionState {
    ACT_RUNNING,
    ACT_SUCCESS,
    ACT_FAILED,
    ACT_CANCELED,
    ACT_TIMEOUT
}

/* ---- SessionState (src/session.rs) ---------------------------------------- */
enum tSessionState {
    S_READY,
    S_RUNNING,
    S_CANCELING,
    S_READY_ENDING,
    S_ENDED
}

/* ---- CancelMethod (runner/mod.rs) ----------------------------------------- */
enum tCancelMethod {
    CM_TERMINATE,             // immediate kill
    CM_NOTIFY_THEN_TERMINATE  // SIGTERM, grace, then SIGKILL
}

/* ===========================================================================
 * Client → Session : lifecycle commands (the public Session API).
 * ===========================================================================
 */

// enter_environment(env, .., identifier, ..)
type tEnterReq = (client: machine, envId: tEnvId);
event eEnterEnv : tEnterReq;

// exit_environment(identifier, .., keep_session_running, ..)
type tExitReq = (client: machine, envId: tEnvId, keepRunning: bool);
event eExitEnv : tExitReq;

// run_task(script, ..)  — task action. Payload: the client machine.
// (P has no single-field named tuples, so single-value payloads are bare.)
event eRunTask : machine;

// run_subprocess(command, ..) — ad-hoc subprocess. Payload: client machine.
event eRunSubprocess : machine;

// cancel_action(time_limit, mark_action_failed). Payload: mark_action_failed.
event eCancelAction : bool;

// SessionConfig.cancel_token fired externally (permanent, cascades).
event eSessionCancelToken;

// cleanup(). Payload: client machine.
event eCleanup : machine;

/* Replies back to the client so a driver can sequence its next command. */
event eActionDone   : tActionState;    // action completed (payload: final state)
event eCmdRejected  : tSessionState;   // command refused (payload: rejecting state)
event eCleanupDone;

/* The Subprocess machine receives its action parameters via its constructor
 * (see Subprocess.p `start state Init` entry), so there is no separate
 * "start action" event on the same-user path. */

/* ===========================================================================
 * Subprocess / ActionFilter → Session : the ActionMessage enum
 * (specs/sessions/action-messages.md).
 *
 * These are the openjd_* directives parsed from stdout, delivered in real
 * time over the (unbounded) mpsc channel and applied with &mut self.
 * ===========================================================================
 */
event eProgress        : (actionId: tActionId, value: int);            // openjd_progress
event eStatus          : tActionId;                                    // openjd_status
event eFail            : tActionId;                                    // openjd_fail
event eSetEnv          : (actionId: tActionId, envId: tEnvId, name: int);   // openjd_env
event eUnsetEnv        : (actionId: tActionId, envId: tEnvId, name: int);   // openjd_unset_env
event eRedactedEnv     : (actionId: tActionId, envId: tEnvId, name: int);   // openjd_redacted_env
event eCancelMarkFailed: tActionId;                                    // malformed directive

/* Subprocess lifecycle → Session. eProcessExited is the action_future
 * completing; drive_action drains remaining messages before finalizing. */
event eProcessExited : (actionId: tActionId, st: tActionState, code: tExitCode);

/* Self-scheduled continuation "tick": lets a streaming machine yield between
 * emitted lines so external events (a cancel) can interleave. Without this, a
 * goto-loop would run to completion and no cancel could ever land mid-stream. */
event eTick;

/* ===========================================================================
 * Session → Subprocess : cancellation / signals.
 * ===========================================================================
 */
event eCancelRequest : (actionId: tActionId, method: tCancelMethod); // over watch channel
event eGraceExpired  : tActionId;                        // notify grace elapsed → SIGKILL
event eExitGrace     : tActionId;                        // 5s post-EOF grace → SIGKILL

/* ===========================================================================
 * Session ↔ CrossUserHelper : newline-delimited JSON wire protocol
 * (specs/sessions/embedded-cross-user-helper.md). Every command carries a
 * token; responses do not.
 * ===========================================================================
 */
type tHelperRun    = (session: machine, actionId: tActionId, token: tToken);
type tHelperCancel = (actionId: tActionId, method: tCancelMethod, token: tToken);

event eHelperRun     : tHelperRun;               // {"token","command","args","env","cwd"}
event eHelperCancel  : tHelperCancel;            // {"token","cancel":...}
event eHelperShutdown: (session: machine, token: tToken); // {"token","shutdown":true}

// Responses (helper → session)
event eHelperPid    : tActionId;
event eHelperOut    : tActionId;
event eHelperExited : (actionId: tActionId, code: tExitCode);
event eHelperError  : tActionId;                     // {"error": "..."}
event eHelperInvalidToken;                            // {"error":"invalid token"} — run untouched
