/*****************************************************************************
 * Session.p — the SessionState machine (specs/sessions/session.md).
 *
 * Drives one action at a time, manages a LIFO environment stack and the
 * cumulative env-var change set, and enforces the brittle-session contract.
 * Announces monitor events so the specs in PSpec/ can check invariants.
 *****************************************************************************/

/* Config passed in by the driver at creation. */
type tSessionConfig = (
    crossUser: bool,   // route actions through the cross-user helper
    hasCancelToken: bool  // whether SessionConfig.cancel_token is wired
);

/* Fault-injection knob for the cross-user token-cancel channel.
 *   true  = the cross-user path observes a cancel delivered over EITHER the
 *           watch/pipe channel or the token. All test cases check green.
 *   false = the cross-user path observes a cancel ONLY over the watch/pipe
 *           channel, so a token-only external cancel is not seen and
 *           tcExternalCancelCrossUser fails CancelDeliverySpec.
 * Exists so CancelDeliverySpec can be shown to have teeth (flip → red on the
 * cross-user token-only path, green elsewhere). Keep true. */
fun CROSS_USER_HONORS_TOKEN_CANCEL(): bool { return true; }

/* Per-environment record: the set/unset env-var names contributed by that env
 * (from `variables` + openjd_env/openjd_unset_env directives). Popping the env
 * must remove exactly this contribution from the cumulative set. */
type tEnvRecord = (envId: tEnvId, sets: set[int], unsets: set[int]);

machine Session {
    var sstate: tSessionState;   // 'state' is a reserved word in P
    var endingOnly: bool;          // ReadyEnding "brittle" flag

    // Environment LIFO stack (index 0 = bottom, last = top).
    var stack: seq[tEnvRecord];

    // Current action bookkeeping.
    var nextActionId: tActionId;
    var curActionId: tActionId;
    var curKind: tActionKind;
    var curState: tActionState;
    var curClient: machine;
    var curEnvId: tEnvId;          // env being entered/exited (for env-var changes)
    var curCancelMethod: tCancelMethod; // the action's configured cancel method
    var cancelRequested: bool;     // cancel_action or cancel-token seen for cur action
    var markFailed: bool;          // cancel_action(mark_action_failed = true)
    // Whether the WORKER's cancel-detection channel actually observed a cancel
    // for the current action. Same-user: true whenever any cancel is delivered
    // (the subprocess awaits the sticky token). Cross-user: true only when the
    // cancel reached the helper over the watch/pipe channel. This is what the
    // code's terminal-state computation keys off (subprocess.rs is_cancelled /
    // cross_user_helper.rs has_changed), so finalizeAction overlays Canceled
    // from it rather than trusting the subprocess's natural exit.
    var workerObservedCancel: bool;

    // Permanent session-level cancel (SessionConfig.cancel_token). Once set it
    // cascades to the current AND all future actions.
    var sessionCanceled: bool;

    // Cross-user helper wiring.
    var crossUser: bool;
    var helper: machine;
    var token: tToken;             // the helper's auth token (abstract)

    var worker: machine;           // the Subprocess machine driving cur action

    start state Init {
        entry (cfg: tSessionConfig) {
            crossUser = cfg.crossUser;
            nextActionId = 1;
            sessionCanceled = false;
            endingOnly = false;
            if (crossUser) {
                token = 42;                 // abstract, fixed for this session
                helper = new CrossUserHelper((session = this, token = token));
            }
            setState(S_READY);
            goto Ready;
        }
    }

    /* ---- Ready / ReadyEnding -------------------------------------------------
     * Ready and ReadyEnding share a state here; `endingOnly` distinguishes them.
     * In ending-only mode, only exit_environment and cleanup are accepted.
     */
    state Ready {
        on eEnterEnv do (req: tEnterReq) {
            if (endingOnly) { reject(req.client); return; }
            // Duplicate identifier rejection (spec: session.md).
            if (envOnStack(req.envId)) { reject(req.client); return; }
            curClient = req.client;
            curEnvId = req.envId;
            // Push the (empty) env record now; onEnter env-var changes fill it in.
            stack += (sizeof(stack), (envId = req.envId, sets = default(set[int]),
                                      unsets = default(set[int])));
            announce eMonEnter, req.envId;
            beginAction(ENTER_ENV, chooseCancelMethod());
        }

        on eExitEnv do (req: tExitReq) {
            // Allowed in both Ready and ReadyEnding.
            // LIFO enforcement: identifier must match top of stack.
            if (sizeof(stack) == 0 || topEnv() != req.envId) { reject(req.client); return; }
            if (!req.keepRunning) { endingOnly = true; }
            curClient = req.client;
            curEnvId = req.envId;
            // The environment is removed from tracking BEFORE the onExit script
            // runs: "a failed exit is still an exit," and later exits must
            // proceed in LIFO order regardless of the onExit result.
            popTop();
            beginAction(EXIT_ENV, chooseCancelMethod());
        }

        on eRunTask do (client: machine) {
            if (endingOnly) { reject(client); return; }
            curClient = client;
            curEnvId = -1;
            beginAction(RUN_TASK, chooseCancelMethod());
        }

        on eRunSubprocess do (client: machine) {
            if (endingOnly) { reject(client); return; }
            curClient = client;
            curEnvId = -1;
            beginAction(RUN_SUBPROCESS, CM_TERMINATE); // ad-hoc always Terminate
        }

        on eCleanup do (client: machine) {
            doCleanup(client);
        }

        // The permanent session cancel token can fire while idle; remember it.
        on eSessionCancelToken do { sessionCanceled = true; }

        // A stray cancel while idle is a no-op (SessionCancelHandle returns false).
        on eCancelAction do (markFailedReq: bool) { /* no action running */ }
    }

    /* ---- Running -------------------------------------------------------------
     * Exactly one action in flight. Process ActionMessages, handle cancel, and
     * finalize on eProcessExited.
     */
    state Running {
        // Real code awaits inside the action; a well-behaved driver issues no
        // new lifecycle command until eActionDone. Defer any that race in.
        defer eEnterEnv, eExitEnv, eRunTask, eRunSubprocess, eCleanup;
        // Helper transport responses are logging/relay only; state changes come
        // via eProcessExited (sent alongside eHelperExited).
        ignore eHelperPid, eHelperOut, eHelperExited, eHelperInvalidToken;

        on eProgress do (m: (actionId: tActionId, value: int)) {
            if (m.actionId == curActionId) { /* update action_status.progress */ }
        }
        on eStatus do (aid: tActionId) { }
        on eFail do (aid: tActionId) {
            // openjd_fail records a fail_message ONLY; it does NOT cancel. The
            // process runs to EOF and the result becomes Failed (unless a
            // timeout/cancel supersedes). No state change here.
        }
        // openjd_env changes are folded into the cumulative set ONLY while
        // entering an environment. Verified against session.rs: run_task /
        // run_subprocess pass a fresh identifier that is NOT a key in
        // created_env_vars, so apply_message's get_mut(identifier) returns None
        // and the change is silently discarded; on exit the env is already
        // removed from environments_entered, so its onExit changes are dropped
        // from the fold too. Only ENTER_ENV attributes changes (to the env being
        // entered, which is the current top of stack).
        on eSetEnv do (m: (actionId: tActionId, envId: tEnvId, name: int)) {
            if (curKind == ENTER_ENV) { applySet(m.name); }
        }
        on eUnsetEnv do (m: (actionId: tActionId, envId: tEnvId, name: int)) {
            if (curKind == ENTER_ENV) { applyUnset(m.name); }
        }
        on eRedactedEnv do (m: (actionId: tActionId, envId: tEnvId, name: int)) {
            if (curKind == ENTER_ENV) { applySet(m.name); } // SetEnv + redaction
        }
        on eCancelMarkFailed do (aid: tActionId) {
            if (aid == curActionId) {
                markFailed = true; cancelRequested = true;
                // A malformed directive routes through cancel_action(None, true)
                // (session.rs), which delivers the action's CONFIGURED cancel
                // method — NOT an unconditional Terminate.
                requestCancel(curCancelMethod);
                goto Canceling;
            }
        }

        on eCancelAction do (markFailedReq: bool) {
            cancelRequested = true;
            if (markFailedReq) { markFailed = true; }
            requestCancel(chooseCancelMethod());
            setState(S_CANCELING);
            goto Canceling;
        }

        on eSessionCancelToken do {
            sessionCanceled = true;
            cancelRequested = true;
            // External token cancel: token-only delivery (viaWatch = false).
            deliverCancel(CM_TERMINATE, false);
            setState(S_CANCELING);
            goto Canceling;
        }

        on eProcessExited do (m: (actionId: tActionId, st: tActionState, code: tExitCode)) {
            if (m.actionId == curActionId) { finalizeAction(m.st); }
        }
    }

    /* ---- Canceling -----------------------------------------------------------
     * Cancel requested; waiting for the subprocess to exit. Late ActionMessages
     * are still drained (drive_action drains after exit).
     */
    state Canceling {
        defer eEnterEnv, eExitEnv, eRunTask, eRunSubprocess, eCleanup;
        ignore eProgress, eStatus, eSetEnv, eUnsetEnv, eRedactedEnv, eFail;
        ignore eHelperPid, eHelperOut, eHelperExited, eHelperInvalidToken;

        on eCancelAction do (markFailedReq: bool) { if (markFailedReq) { markFailed = true; } }
        on eSessionCancelToken do { sessionCanceled = true; }
        on eCancelMarkFailed do (aid: tActionId) { if (aid == curActionId) { markFailed = true; } }

        on eProcessExited do (m: (actionId: tActionId, st: tActionState, code: tExitCode)) {
            // finalizeAction applies the cancel overlay from session state
            // (workerObservedCancel / markFailed), so both the Running and
            // Canceling paths funnel through the same classification logic.
            if (m.actionId == curActionId) { finalizeAction(m.st); }
        }
    }

    /* ---- Ended (terminal) ---------------------------------------------------- */
    state Ended {
        ignore eProgress, eStatus, eFail, eSetEnv, eUnsetEnv, eRedactedEnv,
               eCancelMarkFailed, eProcessExited, eCancelAction, eSessionCancelToken,
               eHelperPid, eHelperOut, eHelperExited, eHelperInvalidToken;
        on eEnterEnv do (req: tEnterReq) { reject(req.client); }
        on eExitEnv  do (req: tExitReq)  { reject(req.client); }
        on eRunTask  do (client: machine) { reject(client); }
        on eRunSubprocess do (client: machine) { reject(client); }
        on eCleanup  do (client: machine) { send client, eCleanupDone; }
    }

    /* ===================== helper functions ===================== */

    fun beginAction(kind: tActionKind, cm: tCancelMethod) {
        curActionId = nextActionId;
        nextActionId = nextActionId + 1;
        curKind = kind;
        curCancelMethod = cm;
        curState = ACT_RUNNING;
        markFailed = false;
        workerObservedCancel = false;
        // Every action installs FRESH cancel state (session.md): a prior
        // cancel_action never poisons a later action...
        cancelRequested = false;
        // ...but SessionConfig.cancel_token is permanent and cascades.
        if (sessionCanceled) { cancelRequested = true; }

        setState(S_RUNNING);
        announce eMonActionStart, (actionId = curActionId, kind = kind);

        if (crossUser) {
            worker = helper; // helper owns the child; it will spawn/relay
            send helper, eHelperRun, (session = this, actionId = curActionId, token = token);
        } else {
            worker = new Subprocess((
                session = this, actionId = curActionId, kind = kind,
                cancelMethod = cm, envId = curEnvId));
        }
        // SessionConfig.cancel_token is permanent: if it already fired, every
        // future action's cancel token is born cancelled, so cascade now. This
        // is a token-only cascade (viaWatch = false), like eSessionCancelToken.
        if (sessionCanceled) {
            deliverCancel(CM_TERMINATE, false);
            setState(S_CANCELING);
            goto Canceling;
        }
        goto Running;
    }

    // Cancellation is delivered over TWO channels:
    //   - the watch channel (cancel_request_rx), which the session mirrors onto
    //     the cross-user helper stdin via cancel_writer; AND
    //   - the per-action CancellationToken.
    // cancel_action fires BOTH; a bare external SessionConfig.cancel_token cancel
    // fires ONLY the token. The same-user subprocess awaits the token directly,
    // so it observes a cancel from EITHER channel. The cross-user helper is
    // driven by the pipe, so it observes the watch/pipe channel; whether it also
    // honours the token is the CROSS_USER_HONORS_TOKEN_CANCEL knob.
    //
    // `viaWatch` = the cancel travelled the watch channel (true for cancel_action
    // and malformed-directive cancels; false for a bare external token cancel).
    fun requestCancel(cm: tCancelMethod) { deliverCancel(cm, true); }

    fun deliverCancel(cm: tCancelMethod, viaWatch: bool) {
        announce eMonCancelIssued, (actionId = curActionId, viaWatch = viaWatch);
        if (crossUser) {
            // The cross-user helper observes a cancel over the watch/pipe channel
            // (viaWatch); it additionally honours a token-only cancel iff the
            // knob is set. With the knob off, a token-only cancel is not observed
            // and CancelDeliverySpec fails on tcExternalCancelCrossUser — the
            // fault injection that shows the spec has teeth.
            if (viaWatch || CROSS_USER_HONORS_TOKEN_CANCEL()) {
                workerObservedCancel = true;
                send helper, eHelperCancel, (actionId = curActionId, method = cm, token = token);
            }
        } else {
            // Same-user subprocess awaits the sticky token directly, so a cancel
            // over EITHER channel is observed — zero-latency, never lost to EOF.
            workerObservedCancel = true;
            send worker, eCancelRequest, (actionId = curActionId, method = cm);
        }
    }

    fun finalizeAction(reportedState: tActionState) {
        var finalState: tActionState;
        finalState = reportedState;

        // Apply the cancel overlay from session-level state, mirroring the
        // code's terminal-state computation. If the worker's detection channel
        // observed a cancel, the outcome is Canceled — regardless of the
        // process's natural exit (subprocess.rs: the sticky is_cancelled()
        // check sits ABOVE the success() check). Timeout still wins over Cancel
        // (checked first in the code), so a reported Timeout is preserved. The
        // sole rewrite on top is Canceled → Failed when mark_action_failed was
        // requested (session.rs).
        if (workerObservedCancel && finalState != ACT_TIMEOUT) {
            if (markFailed) { finalState = ACT_FAILED; }
            else            { finalState = ACT_CANCELED; }
        }

        curState = finalState;
        announce eMonActionEnd, (actionId = curActionId, st = finalState);

        // Brittle-session rule: any failure/cancel/timeout ⇒ ending-only.
        if (finalState == ACT_FAILED || finalState == ACT_CANCELED || finalState == ACT_TIMEOUT) {
            endingOnly = true;
        }

        // A FAILED or CANCELED onEnter leaves the environment ON the stack — it
        // is pushed before the action runs and only removed by a later exit, so
        // the agent can still run its onExit during brittle-session teardown.
        //
        // EXIT_ENV already popped the environment at exit-start (see eExitEnv),
        // so nothing to pop here regardless of the onExit result.

        if (endingOnly) { setState(S_READY_ENDING); }
        else { setState(S_READY); }

        send curClient, eActionDone, finalState;
        goto Ready;
    }

    fun doCleanup(client: machine) {
        if (crossUser) { send helper, eHelperShutdown, (session = this, token = token); }
        // cleanup() while environments remain leaves onExit un-run (documented
        // hazard). The monitor flags a non-empty stack at cleanup.
        announce eMonCleanup, sizeof(stack);
        setState(S_ENDED);
        send client, eCleanupDone;
        goto Ended;
    }

    /* ---- env-var change set (attach to the env being entered) ---- */
    fun applySet(name: int) {
        var rec: tEnvRecord;
        if (sizeof(stack) == 0) { return; }
        rec = stack[sizeof(stack) - 1];
        rec.unsets -= (name);   // unset-wins is per-env; a later set overrides
        rec.sets += (name);
        stack[sizeof(stack) - 1] = rec;
    }
    fun applyUnset(name: int) {
        var rec: tEnvRecord;
        if (sizeof(stack) == 0) { return; }
        rec = stack[sizeof(stack) - 1];
        rec.sets -= (name);
        rec.unsets += (name);
        stack[sizeof(stack) - 1] = rec;
    }

    fun popTop() {
        if (sizeof(stack) > 0) {
            announce eMonExit, topEnv();
            stack -= (sizeof(stack) - 1);
        }
    }

    fun topEnv(): tEnvId { return stack[sizeof(stack) - 1].envId; }

    fun envOnStack(id: tEnvId): bool {
        var i: int;
        i = 0;
        while (i < sizeof(stack)) {
            if (stack[i].envId == id) { return true; }
            i = i + 1;
        }
        return false;
    }

    fun reject(client: machine) { send client, eCmdRejected, sstate; }

    fun chooseCancelMethod(): tCancelMethod {
        if ($) { return CM_TERMINATE; } else { return CM_NOTIFY_THEN_TERMINATE; }
    }

    fun setState(s: tSessionState) {
        announce eMonStateChanged, (fromState = sstate, toState = s);
        sstate = s;
    }
}
