use super::*;
use pretty_assertions::assert_eq;

#[test]
fn numeric_ids_round_trip_as_stable_strings_and_accept_legacy_numbers() {
    let id = ToolGroupId::new(42);

    assert_eq!(serde_json::to_string(&id).expect("serialize id"), r#""TG000042""#);
    assert_eq!(
        serde_json::from_str::<ToolGroupId>(r#""TG000042""#).expect("read string id"),
        id
    );
    assert_eq!(
        serde_json::from_str::<ToolGroupId>("42").expect("read legacy numeric id"),
        id
    );
}
