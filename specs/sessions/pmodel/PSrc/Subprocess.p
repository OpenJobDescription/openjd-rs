/*****************************************************************************
 * Subprocess.p — abstract async subprocess (specs/sessions/subprocess.md).
 *
 * Models the same-user path: spawn a child, stream a nondeterministic number
 * of openjd_* directives, then exit — OR respond to a cancel request with
 * notify-then-terminate / immediate terminate, always eventually exiting.
 *
 * Byte-level I/O, UTF-8 decoding, 64KB truncation, setsid/killpg/dup2 are all
 * out of scope; a "line" is an opaque directive choice.
 *****************************************************************************/

machine Subprocess {
    var session: machine;
    var actionId: tActionId;
    var kind: tActionKind;
    var cancelMethod: tCancelMethod;
    var envId: tEnvId;
    var emitsLeft: int;   // how many more directives this run will emit
    //
    // NOTE ON CANCEL CLASSIFICATION. The subprocess here reports only its
    // NATURAL termination (Success / Failed / Timeout). It does NOT itself
    // decide "Canceled". In the code, the Canceled verdict is computed from
    // shared state checked after the read loop — the sticky
    // `cancel_token.is_cancelled()` (same-user, subprocess.rs) or the watch
    // channel `has_changed()` (cross-user, cross_user_helper.rs). That is
    // session-controlled state, not a raceable in-band message, so the Session
    // applies the cancel overlay in finalizeAction (see Session.p). Modeling it
    // there — rather than racing an eCancelRequest against EOF — is what makes
    // the same-user path correctly deterministic (a cancel is never "lost to
    // EOF") while still letting the cross-user token-drop bug surface.

    start state Init {
        entry (cfg: (session: machine, actionId: tActionId, kind: tActionKind,
                     cancelMethod: tCancelMethod, envId: tEnvId)) {
            session = cfg.session;
            actionId = cfg.actionId;
            kind = cfg.kind;
            cancelMethod = cfg.cancelMethod;
            envId = cfg.envId;
            // Emit between 0 and 3 directives before finishing.
            emitsLeft = choose(4);
            goto Streaming;
        }
    }

    /* ---- Streaming: emit directives, then terminate NATURALLY. ---------------
     * We yield via a self-scheduled eTick between lines so the checker can
     * schedule an incoming eCancelRequest between any two emitted directives.
     * A received cancel just stops the stream promptly (liveness) and lets the
     * process finish; it does NOT decide the terminal state — the Session
     * applies the Canceled overlay in finalizeAction based on session-level
     * cancel state (sticky token / watch channel), mirroring the code.
     */
    state Streaming {
        entry { send this, eTick; }

        on eTick do {
            if (emitsLeft > 0) {
                emitsLeft = emitsLeft - 1;
                emitOneDirective();
                send this, eTick;    // yield, then continue streaming
            } else {
                finishNaturally();
            }
        }

        // A cancel received in-band (same-user path) stops the stream promptly.
        // The Session decides the terminal classification.
        on eCancelRequest do (m: (actionId: tActionId, method: tCancelMethod)) {
            if (m.actionId == actionId) { finishNaturally(); }
        }
    }

    fun finishNaturally() {
        // The process's OWN exit outcome, independent of cancel: Success,
        // Failed (nonzero exit), or Timeout (killed by the timeout path).
        if ($)      { finish(ACT_SUCCESS, 0); }
        else if ($) { finish(ACT_FAILED, 1); }
        else        { finish(ACT_TIMEOUT, -9); }
    }

    fun emitOneDirective() {
        // Pick one openjd_* directive. envId is the environment being entered
        // (or -1 for tasks / ad-hoc subprocess).
        var pick: int;
        pick = choose(6);
        if      (pick == 0) { send session, eProgress, (actionId = actionId, value = choose(101)); }
        else if (pick == 1) { send session, eStatus, actionId; }
        else if (pick == 2) { send session, eSetEnv, (actionId = actionId, envId = envId, name = choose(3)); }
        else if (pick == 3) { send session, eUnsetEnv, (actionId = actionId, envId = envId, name = choose(3)); }
        else if (pick == 4) { send session, eRedactedEnv, (actionId = actionId, envId = envId, name = choose(3)); }
        else                { /* a malformed directive → CancelMarkFailed */
                              send session, eCancelMarkFailed, actionId; }
    }

    /* ---- Done: exited and finalized. A cancel arriving AFTER finalization is
     * a genuine no-op (the process is already gone and the result reported);
     * this differs from a cancel arriving before EOF, which the sticky flag
     * turns into a Canceled result above. ----------------------------------- */
    state Done {
        ignore eCancelRequest, eGraceExpired, eExitGrace, eTick;
    }

    fun finish(finalSt: tActionState, code: tExitCode) {
        send session, eProcessExited, (actionId = actionId, st = finalSt, code = code);
        goto Done;
    }
}
