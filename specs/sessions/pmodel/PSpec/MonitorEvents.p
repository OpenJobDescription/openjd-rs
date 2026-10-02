/*****************************************************************************
 * MonitorEvents.p — events announced by Session for the spec monitors.
 * These carry no behavior; they let the monitors observe the abstract state.
 *****************************************************************************/

event eMonStateChanged : (fromState: tSessionState, toState: tSessionState);
event eMonActionStart  : (actionId: tActionId, kind: tActionKind);
event eMonActionEnd    : (actionId: tActionId, st: tActionState);
event eMonEnter        : tEnvId;   // env pushed
event eMonExit         : tEnvId;   // env popped
event eMonCleanup      : int;      // stack depth at cleanup()
event eMonHelperBadToken;          // helper rejected a bad/missing token
// A cancel was issued for the current action. viaWatch = travelled the watch/
// pipe channel (cancel_action / malformed directive); false = token-only
// (external SessionConfig.cancel_token cascade).
event eMonCancelIssued : (actionId: tActionId, viaWatch: bool);
