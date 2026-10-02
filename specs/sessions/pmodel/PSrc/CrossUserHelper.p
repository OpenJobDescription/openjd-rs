/*****************************************************************************
 * CrossUserHelper.p — persistent cross-user helper
 * (specs/sessions/embedded-cross-user-helper.md).
 *
 * Models the security-relevant behavior only:
 *   - one command at a time (implicitly sequential protocol)
 *   - every command carries a token; verified for equality
 *   - bad/missing token ⇒ {"error":"invalid token"} AND the current run is
 *     left untouched (a bad-token cancel is NOT an unauthenticated cancel)
 *   - helper stays alive on bad token (log-and-ignore, not exit → no DoS)
 *
 * Token entropy, constant-time compare, JSON framing, poll(2), Job Objects,
 * DACLs, and the reader-thread/channel are out of scope.
 *****************************************************************************/

machine CrossUserHelper {
    var owner: machine;     // the Session that spawned us
    var token: tToken;      // expected auth token (from --auth-token)
    var runActionId: tActionId; // action currently running (-1 = idle)
    var emitsLeft: int;

    start state Init {
        entry (cfg: (session: machine, token: tToken)) {
            owner = cfg.session;
            token = cfg.token;
            runActionId = -1;
            goto Idle;
        }
    }

    state Idle {
        ignore eTick;  // a leftover tick from a prior run is harmless when idle

        on eHelperRun do (r: tHelperRun) {
            if (r.token != token) {
                // invalid token: no child spawn, helper stays alive.
                send r.session, eHelperInvalidToken;
                announce eMonHelperBadToken;
                return;
            }
            runActionId = r.actionId;
            emitsLeft = choose(4);
            send owner, eHelperPid, runActionId;
            goto Running;
        }

        // A cancel/shutdown while idle: verify token, otherwise ignore.
        on eHelperCancel do (c: tHelperCancel) {
            if (c.token != token) { send owner, eHelperInvalidToken; announce eMonHelperBadToken; }
        }
        on eHelperShutdown do (s: (session: machine, token: tToken)) {
            if (s.token == token) { raise halt; }
            else { send s.session, eHelperInvalidToken; announce eMonHelperBadToken; }
        }
    }

    state Running {
        entry { send this, eTick; }

        on eTick do {
            if (emitsLeft > 0) {
                emitsLeft = emitsLeft - 1;
                send owner, eHelperOut, runActionId; // {"out": line}
                send this, eTick;   // yield so a cancel can interleave
            } else {
                // Child exits normally (Success/Failed decided by exit code).
                if ($) { done(ACT_SUCCESS, 0); } else { done(ACT_FAILED, 1); }
            }
        }

        on eHelperCancel do (c: tHelperCancel) {
            if (c.token != token) {
                // SECURITY INVARIANT: bad-token cancel must NOT stop the run.
                send owner, eHelperInvalidToken;
                announce eMonHelperBadToken;
                return;
            }
            if (c.actionId != runActionId) { return; }
            if (c.method == CM_TERMINATE) { done(ACT_CANCELED, -9); }
            else {
                // NotifyThenTerminate: exit during grace or get killed.
                if ($) { done(ACT_CANCELED, 0); } else { done(ACT_CANCELED, -9); }
            }
        }

        // Shutdown mid-run: valid token tears down the helper.
        on eHelperShutdown do (s: (session: machine, token: tToken)) {
            if (s.token == token) { raise halt; }
            else { send s.session, eHelperInvalidToken; announce eMonHelperBadToken; }
        }
    }

    fun done(finalSt: tActionState, code: tExitCode) {
        // The helper reports {"exited": code}; the session maps this to a
        // SubprocessResult and finalizes the action. We reuse eProcessExited
        // so the Session state machine is agnostic to same-user vs cross-user.
        send owner, eHelperExited, (actionId = runActionId, code = code);
        send owner, eProcessExited, (actionId = runActionId, st = finalSt, code = code);
        runActionId = -1;
        goto Idle;
    }
}
