use std::{fs, path::PathBuf};

use serde_json::Value;

#[test]
fn g431_fdcan_message_ram_sections_fit_without_overlap() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fdcan-message-ram.json");
    let document: Value = serde_json::from_slice(&fs::read(path).expect("budget must exist"))
        .expect("budget must be valid JSON");
    let limit = document["g431_allocation_words"].as_u64().expect("limit");
    assert_eq!(limit, 256);

    let mut expected_offset = 0;
    for section in document["sections"].as_array().expect("sections") {
        let offset = section["offset_words"].as_u64().expect("offset");
        let count = section["elements"].as_u64().expect("elements");
        let words = section["words_per_element"].as_u64().expect("element size");
        assert_eq!(offset, expected_offset, "sections must be contiguous");
        expected_offset += count * words;
    }
    assert_eq!(document["allocated_words"].as_u64(), Some(expected_offset));
    assert!(expected_offset <= limit);
}
