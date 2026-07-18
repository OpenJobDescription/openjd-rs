// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// Copyright by contributors to this project.
// SPDX-License-Identifier: (Apache-2.0 OR MIT)

//! Wire-format tests for `job::CancelationMode`'s hand-written serde impls
//! (see `specs/model/job-types.md`), including backward compatibility with
//! documents written by the previous derived impl, which serialized an
//! unset period as an explicit `"notifyPeriodInSeconds": null`.

use openjd_model::job::CancelationMode;

#[test]
fn explicit_null_notify_period_reads_as_unset() {
    // The pre-DeferredMode derived impl always wrote
    // `"notifyPeriodInSeconds": null` for an unset period; documents from
    // released versions must read back as None instead of failing.
    let mode: CancelationMode =
        serde_json::from_str(r#"{"mode": "NOTIFY_THEN_TERMINATE", "notifyPeriodInSeconds": null}"#)
            .expect("explicit null must deserialize");
    assert_eq!(
        mode,
        CancelationMode::NotifyThenTerminate {
            notify_period_in_seconds: None
        }
    );
}

#[test]
fn unset_notify_period_serializes_without_field_and_round_trips() {
    let mode = CancelationMode::NotifyThenTerminate {
        notify_period_in_seconds: None,
    };
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#"{"mode":"NOTIFY_THEN_TERMINATE"}"#);
    let back: CancelationMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, mode);
}

#[test]
fn terminate_round_trips() {
    let mode = CancelationMode::Terminate;
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#"{"mode":"TERMINATE"}"#);
    let back: CancelationMode = serde_json::from_str(&json).unwrap();
    assert_eq!(back, mode);
}

#[test]
fn deferred_mode_round_trips() {
    let json = r#"{"mode":"{{WrappedAction.Cancelation.Mode}}","notifyPeriodInSeconds":"{{WrappedAction.Cancelation.NotifyPeriodInSeconds}}"}"#;
    let mode: CancelationMode = serde_json::from_str(json).unwrap();
    match &mode {
        CancelationMode::DeferredMode {
            mode,
            notify_period_in_seconds,
        } => {
            assert_eq!(mode.raw(), "{{WrappedAction.Cancelation.Mode}}");
            assert_eq!(
                notify_period_in_seconds.as_ref().unwrap().raw(),
                "{{WrappedAction.Cancelation.NotifyPeriodInSeconds}}"
            );
        }
        other => panic!("expected DeferredMode, got {other:?}"),
    }
    assert_eq!(serde_json::to_string(&mode).unwrap(), json);
}

#[test]
fn deferred_mode_with_explicit_null_notify_period_reads_as_unset() {
    let mode: CancelationMode = serde_json::from_str(
        r#"{"mode": "{{WrappedAction.Cancelation.Mode}}", "notifyPeriodInSeconds": null}"#,
    )
    .expect("explicit null must deserialize");
    match mode {
        CancelationMode::DeferredMode {
            notify_period_in_seconds: None,
            ..
        } => {}
        other => panic!("expected DeferredMode with unset period, got {other:?}"),
    }
}

#[test]
fn non_string_mode_is_rejected() {
    let err = serde_json::from_str::<CancelationMode>(r#"{"mode": 42}"#).unwrap_err();
    assert!(
        err.to_string().contains("`mode` must be a string"),
        "got: {err}"
    );
}
