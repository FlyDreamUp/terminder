use terminder_session_core::model::*;
#[test]
fn rust_and_typescript_share_the_same_wire_fixture() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/fixtures/protocol.json")).unwrap();
    let binding: Binding = serde_json::from_value(fixture["binding"].clone()).unwrap();
    binding.validate().unwrap();
    let state: SessionEvent = serde_json::from_value(fixture["state"].clone()).unwrap();
    let end: SessionEvent = serde_json::from_value(fixture["outputEnd"].clone()).unwrap();
    assert_eq!(serde_json::to_value(state).unwrap(), fixture["state"]);
    assert_eq!(serde_json::to_value(end).unwrap(), fixture["outputEnd"]);
    let total: usize = fixture["splitUtf8"]
        .as_array()
        .unwrap()
        .iter()
        .map(|chunk| chunk.as_array().unwrap().len())
        .sum();
    assert_eq!(total.to_string(), fixture["ack"]["bytes"]);
}
