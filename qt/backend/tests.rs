//! Focused maintenance contracts; external effects are covered by the private Qt fixture.
#[test]
fn safe_report_is_independent_of_workspace_state() {
    let report = super::maintenance::report();
    assert!(report.contains("separate source build"));
    assert!(report.contains(env!("CARGO_PKG_VERSION")));
    assert!(report.contains("MIT OR Apache-2.0"));
    assert!(!report.contains("unix://"));
    assert!(!report.contains("ssh://"));
    assert!(!report.contains("/home/"));
}
