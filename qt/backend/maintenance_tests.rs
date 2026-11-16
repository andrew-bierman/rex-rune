use super::*;

#[test]
fn stalled_child_is_reaped_and_maintenance_recovers() {
    let work = tempfile::tempdir().unwrap();
    let pid_path = work.path().join("child.pid");
    let code = format!(
        "import os,pathlib; pathlib.Path({}).write_text(str(os.getpid())); os.execv('/usr/bin/sleep',['sleep','600'])",
        serde_json::to_string(pid_path.to_str().unwrap()).unwrap()
    );
    let started = Instant::now();
    let result = rex_with_timeout(
        std::ffi::OsStr::new("/usr/bin/python3"),
        &["-c".into(), code],
        Duration::from_millis(250),
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(
        result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
    let pid = std::fs::read_to_string(&pid_path).unwrap();
    assert!(!Path::new("/proc").join(pid).exists());

    let mut maintenance = Maintenance::default();
    let (tx, rx) = mpsc::channel();
    maintenance.rx = Some(rx);
    assert!(
        tx.send(Reply {
            kind: "apply",
            result,
            change: None,
            generation: 0,
        })
        .is_ok()
    );
    assert_eq!(maintenance.snapshot()["busy"], true);
    assert!(maintenance.poll());
    assert_eq!(maintenance.snapshot()["busy"], false);
    assert!(
        maintenance.snapshot()["error"]
            .as_str()
            .unwrap()
            .contains("partially applied")
    );

    let result = rex_with_timeout(
        std::ffi::OsStr::new("/usr/bin/python3"),
        &["-c".into(), "print('{\"harnesses\":[]}')".into()],
        Duration::from_secs(2),
    );
    assert_eq!(result.unwrap(), json!({"harnesses":[]}));
}

#[test]
fn cancelled_reply_clears_busy_without_restoring_preview() {
    let mut maintenance = Maintenance::default();
    let (tx, rx) = mpsc::channel();
    maintenance.rx = Some(rx);
    maintenance
        .request(&json!({"action":"cancel"}), Path::new("unused"))
        .unwrap();
    assert!(
        tx.send(Reply {
            kind: "preview",
            result: Ok(json!({"steps":[]})),
            change: Some(Change {
                agent: "claude".into(),
                remove: false,
                naming: true,
                force: false
            }),
            generation: 0,
        })
        .is_ok()
    );
    assert!(maintenance.poll());
    assert_eq!(maintenance.snapshot()["busy"], false);
    assert!(maintenance.preview.is_none());
    assert_eq!(maintenance.snapshot()["preview"], Value::Null);
}
