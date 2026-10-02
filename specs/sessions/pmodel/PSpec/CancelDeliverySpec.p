/*****************************************************************************
 * CancelDeliverySpec.p — a cancel that is issued must actually take effect.
 *
 * Invariant: if a cancel is issued for the currently-running action (via ANY
 * delivery channel — cancel_action, a malformed directive, OR an external
 * SessionConfig.cancel_token cascade), then that action MUST NOT complete
 * Successfully. It must end Canceled, Failed (e.g. mark_action_failed), or
 * Timeout. In other words, an acknowledged cancel is never silently dropped.
 *
 * This is exactly the guarantee the same-user path provides (its subprocess
 * loop awaits the CancellationToken directly, so a cancel over either the
 * watch channel or the token is observed). It is the guarantee the CROSS-USER
 * path currently BREAKS: run_via_helper only observes cancels that arrive over
 * the watch/pipe channel, so a token-only external cancel (viaWatch = false)
 * is not seen and the action can run to a Success/Failed exit as if no cancel
 * happened. Model-checking tcWorkerAgentCrossUser against this spec fails on
 * the current (buggy) routing and passes once the cross-user path also honours
 * a token-delivered cancel.
 *****************************************************************************/

spec CancelDeliverySpec observes eMonActionStart, eMonCancelIssued, eMonActionEnd {
    var canceled: bool;   // a cancel was issued for the in-flight action

    start state Idle {
        on eMonActionStart goto Active;
        ignore eMonCancelIssued, eMonActionEnd;
    }

    state Active {
        entry { canceled = false; }

        on eMonCancelIssued do (m: (actionId: tActionId, viaWatch: bool)) {
            canceled = true;
        }

        on eMonActionEnd do (m: (actionId: tActionId, st: tActionState)) {
            if (canceled) {
                assert m.st != ACT_SUCCESS,
                    format ("Action {0} was canceled but completed Successfully — the cancel was dropped (cross-user token-only cancel not observed by run_via_helper?).", m.actionId);
            }
            goto Idle;
        }

        // Only one action runs at a time (SessionStateSpec enforces ≤1), so a
        // second start here would be a bug in the harness; ignore defensively.
        ignore eMonActionStart;
    }
}
