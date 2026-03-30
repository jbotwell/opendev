use std::collections::HashMap;

use chrono::Utc;
use opendev_models::file_change::{FileChange, FileChangeType};
use opendev_models::session::Session;
use opendev_models::transition::ValidateTransition;

use super::*;

// ---------------------------------------------------------------------------
// Serialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_session_event_serialization_roundtrip() {
    let events: Vec<SessionEvent> = vec![
        SessionEvent::SessionCreated {
            id: "abc123".into(),
            working_directory: Some("/tmp".into()),
            channel: "cli".into(),
            title: Some("Test session".into()),
            parent_id: None,
            metadata: HashMap::new(),
        },
        SessionEvent::MessageAdded {
            role: "user".into(),
            content: "hello".into(),
            timestamp: Utc::now(),
            tool_calls: vec![],
            tokens: Some(42),
            thinking_trace: None,
            reasoning_content: None,
        },
        SessionEvent::MessageEdited {
            seq: 0,
            content: "updated".into(),
        },
        SessionEvent::TitleChanged {
            title: "New title".into(),
        },
        SessionEvent::SessionArchived {
            time_archived: Utc::now(),
        },
        SessionEvent::SessionUnarchived,
        SessionEvent::FileChangeRecorded {
            file_change: FileChange::new(FileChangeType::Created, "foo.rs".into()),
        },
        SessionEvent::MetadataUpdated {
            key: "key".into(),
            value: serde_json::json!("value"),
        },
        SessionEvent::SessionForked {
            source_session_id: "src-session".into(),
            fork_point: Some(3),
        },
    ];

    for event in &events {
        let json = serde_json::to_string(event).expect("serialize");
        let roundtripped: SessionEvent = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(event.event_type(), roundtripped.event_type());
    }
}

// ---------------------------------------------------------------------------
// EventEnvelope::new
// ---------------------------------------------------------------------------

#[test]
fn test_event_envelope_new() {
    let event = SessionEvent::TitleChanged {
        title: "Hello".into(),
    };
    let envelope = EventEnvelope::new("session-1", 5, &event);

    assert_eq!(envelope.aggregate_id, "session-1");
    assert_eq!(envelope.seq, 5);
    assert_eq!(envelope.event_type, "TitleChanged");
    assert!(!envelope.id.is_empty());

    // data should round-trip back to the same event type
    let deserialized: SessionEvent =
        serde_json::from_value(envelope.data).expect("deserialize data");
    assert_eq!(deserialized.event_type(), "TitleChanged");
}

// ---------------------------------------------------------------------------
// event_type() names
// ---------------------------------------------------------------------------

#[test]
fn test_event_type_names() {
    let cases: Vec<(SessionEvent, &str)> = vec![
        (
            SessionEvent::SessionCreated {
                id: String::new(),
                working_directory: None,
                channel: String::new(),
                title: None,
                parent_id: None,
                metadata: HashMap::new(),
            },
            "SessionCreated",
        ),
        (
            SessionEvent::MessageAdded {
                role: String::new(),
                content: String::new(),
                timestamp: Utc::now(),
                tool_calls: vec![],
                tokens: None,
                thinking_trace: None,
                reasoning_content: None,
            },
            "MessageAdded",
        ),
        (
            SessionEvent::MessageEdited {
                seq: 0,
                content: String::new(),
            },
            "MessageEdited",
        ),
        (
            SessionEvent::TitleChanged {
                title: String::new(),
            },
            "TitleChanged",
        ),
        (
            SessionEvent::SessionArchived {
                time_archived: Utc::now(),
            },
            "SessionArchived",
        ),
        (SessionEvent::SessionUnarchived, "SessionUnarchived"),
        (
            SessionEvent::FileChangeRecorded {
                file_change: FileChange::new(FileChangeType::Modified, "x".into()),
            },
            "FileChangeRecorded",
        ),
        (
            SessionEvent::MetadataUpdated {
                key: String::new(),
                value: serde_json::Value::Null,
            },
            "MetadataUpdated",
        ),
        (
            SessionEvent::SessionForked {
                source_session_id: String::new(),
                fork_point: None,
            },
            "SessionForked",
        ),
    ];

    for (event, expected_name) in cases {
        assert_eq!(event.event_type(), expected_name);
    }
}

// ---------------------------------------------------------------------------
// ValidateTransition tests
// ---------------------------------------------------------------------------

fn make_session(archived: bool, message_count: usize) -> Session {
    let mut session = Session::new();
    if archived {
        session.archive();
    }
    for _ in 0..message_count {
        session.messages.push(opendev_models::message::ChatMessage {
            role: opendev_models::message::Role::User,
            content: "msg".into(),
            timestamp: Utc::now(),
            metadata: HashMap::new(),
            tool_calls: vec![],
            tokens: None,
            thinking_trace: None,
            reasoning_content: None,
            token_usage: None,
            provenance: None,
        });
    }
    session
}

#[test]
fn test_validate_archive_already_archived() {
    let session = make_session(true, 0);
    let event = SessionEvent::SessionArchived {
        time_archived: Utc::now(),
    };
    let err = session.validate_transition(&event).unwrap_err();
    assert!(err.to_string().contains("already archived"));
}

#[test]
fn test_validate_unarchive_not_archived() {
    let session = make_session(false, 0);
    let event = SessionEvent::SessionUnarchived;
    let err = session.validate_transition(&event).unwrap_err();
    assert!(err.to_string().contains("not archived"));
}

#[test]
fn test_validate_add_message_when_archived() {
    let session = make_session(true, 0);
    let event = SessionEvent::MessageAdded {
        role: "user".into(),
        content: "hello".into(),
        timestamp: Utc::now(),
        tool_calls: vec![],
        tokens: None,
        thinking_trace: None,
        reasoning_content: None,
    };
    let err = session.validate_transition(&event).unwrap_err();
    assert!(err.to_string().contains("archived session"));
}

#[test]
fn test_validate_title_change_empty() {
    let session = make_session(false, 0);
    let event = SessionEvent::TitleChanged {
        title: "   ".into(),
    };
    let err = session.validate_transition(&event).unwrap_err();
    assert!(err.to_string().contains("empty"));
}

#[test]
fn test_validate_fork_point_out_of_range() {
    let session = make_session(false, 3);
    let event = SessionEvent::SessionForked {
        source_session_id: "src".into(),
        fork_point: Some(10),
    };
    let err = session.validate_transition(&event).unwrap_err();
    assert!(err.to_string().contains("out of range"));
}

#[test]
fn test_validate_valid_transitions() {
    let session = make_session(false, 5);

    let event = SessionEvent::MessageAdded {
        role: "user".into(),
        content: "hello".into(),
        timestamp: Utc::now(),
        tool_calls: vec![],
        tokens: None,
        thinking_trace: None,
        reasoning_content: None,
    };
    assert!(session.validate_transition(&event).is_ok());

    let event = SessionEvent::TitleChanged {
        title: "Good title".into(),
    };
    assert!(session.validate_transition(&event).is_ok());

    let event = SessionEvent::SessionArchived {
        time_archived: Utc::now(),
    };
    assert!(session.validate_transition(&event).is_ok());

    let event = SessionEvent::SessionForked {
        source_session_id: "src".into(),
        fork_point: Some(5),
    };
    assert!(session.validate_transition(&event).is_ok());

    let event = SessionEvent::SessionForked {
        source_session_id: "src".into(),
        fork_point: None,
    };
    assert!(session.validate_transition(&event).is_ok());

    let archived_session = make_session(true, 0);
    let event = SessionEvent::SessionUnarchived;
    assert!(archived_session.validate_transition(&event).is_ok());

    let event = SessionEvent::SessionCreated {
        id: "new".into(),
        working_directory: None,
        channel: "cli".into(),
        title: None,
        parent_id: None,
        metadata: HashMap::new(),
    };
    assert!(session.validate_transition(&event).is_ok());
}
