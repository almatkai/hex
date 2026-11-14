use super::*;
use crate::meeting::{MeetingSource, TranscriptEntry};

fn meeting() -> Meeting {
    let library = super::super::query::Library::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/meetings"),
    );
    let meeting = library.meeting("1790000000000-1001").unwrap();
    drop(library);
    std::sync::Arc::try_unwrap(meeting).ok().unwrap()
}

#[test]
fn meeting_ask_gates_precede_account_access_and_meeting_read() {
    let mut settings = AppSettings::default();
    assert!(require(&settings, || panic!("account read with Labs off")).is_err());
    settings.labs.meeting_ask = true;
    assert!(require(&settings, || panic!("account read with Meetings off")).is_err());
    settings.meetings.enabled = true;
    assert_eq!(
        require(&settings, || false).unwrap_err().to_string(),
        "Sign in to Antigravity"
    );
    assert!(require(&settings, || true).is_ok());
    settings.labs.meeting_ask = false;
    assert!(
        answer(
            &settings,
            || panic!("meeting read with Labs off"),
            "Question",
            &AtomicBool::new(false)
        )
        .is_err()
    );
}

#[test]
fn meeting_ask_honors_managed_labs_and_remote_processing_locks() {
    use crate::policy::{Lock, Policy};
    for lock in [Lock::Labs, Lock::OpenCodeProcessing] {
        assert_eq!(
            Policy::default()
                .with(lock)
                .labs_lock(crate::labs::Feature::MeetingAsk),
            Some(lock)
        );
    }
}

#[test]
fn short_meeting_ask_preserves_the_transcript_notes_and_question_language() {
    let meeting = meeting();
    for question in [
        "What was decided?",
        "Что решили?",
        "Қандай шешім қабылданды?",
    ] {
        let (prompt, excerpted) = prompt(&meeting, question).unwrap();
        let json: serde_json::Value = serde_json::from_str(&prompt).unwrap();
        assert_eq!(json["question"], question);
        assert!(json["transcript"].as_str().unwrap().contains("[00:"));
        assert!(!excerpted);
        if let Some(notes) = &meeting.manifest.notes {
            assert!(json["notes"].as_str().unwrap().contains(&notes.summary));
        }
    }
    assert!(INSTRUCTIONS.contains("only from"));
    assert!(INSTRUCTIONS.contains("language of the question"));
    assert!(INSTRUCTIONS.contains("do not contain"));
    assert!(INSTRUCTIONS.contains("[mm:ss]"));
}

#[test]
fn long_meeting_ask_selects_relevant_chunks_in_time_order_with_a_hard_budget() {
    let mut meeting = meeting();
    meeting.entries = (0..500)
        .map(|i| TranscriptEntry {
            source: MeetingSource::System,
            start_ms: i * 10_000,
            end_ms: i * 10_000 + 1_000,
            text: if i == 387 {
                "The cobalt launch deadline is Friday.".into()
            } else {
                "Routine unrelated status update. ".repeat(8)
            },
        })
        .collect();
    let (text, excerpted) = excerpt(&meeting, "cobalt deadline", 1_000);
    assert!(excerpted);
    assert!(text.len() <= 1_000);
    assert!(
        text.contains("[64:30] Them: The cobalt launch deadline is Friday."),
        "{text}"
    );
    let positions = text
        .lines()
        .filter_map(|line| line.split(']').next())
        .collect::<Vec<_>>();
    assert!(positions.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn meeting_ask_bounds_unicode_pathological_words_notes_and_questions() {
    let mut meeting = meeting();
    meeting.entries = vec![TranscriptEntry {
        source: MeetingSource::Microphone,
        start_ms: 0,
        end_ms: 1_000,
        text: "Қ".repeat(20_000),
    }];
    meeting.manifest.notes = Some(crate::meeting::MeetingNotes {
        summary: "Я".repeat(20_000),
        ..Default::default()
    });
    for budget in [0, 1, 20, 1_000, EXCERPT_BYTES] {
        let (text, excerpted) = excerpt(&meeting, "Қ", budget);
        assert!(excerpted);
        assert!(text.len() <= budget);
    }
    let (text, excerpted) = prompt(&meeting, "Қандай шешім?").unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(excerpted);
    assert!(json["notes"].as_str().unwrap().len() <= NOTES_BYTES);
    assert!(json["transcript"].as_str().unwrap().len() <= EXCERPT_BYTES);
    assert!(prompt(&meeting, "  ").is_err());
    assert!(prompt(&meeting, &"Я".repeat(QUESTION_BYTES)).is_err());
}

#[test]
fn meeting_ask_without_lexical_hits_samples_the_meeting_instead_of_only_the_opening() {
    let mut meeting = meeting();
    meeting.entries = (0..500)
        .map(|i| TranscriptEntry {
            source: MeetingSource::System,
            start_ms: i * 10_000,
            end_ms: i * 10_000 + 1_000,
            text: format!("Routine update {i}. ").repeat(8),
        })
        .collect();
    let (text, excerpted) = excerpt(&meeting, "қорытынды", 4_000);
    assert!(excerpted);
    assert!(text.len() <= 4_000);
    assert!(text.contains("update 0."));
    assert!(text.contains("update 499."));
}

#[test]
fn meeting_ask_cancellation_and_deadline_are_public_errors() {
    let future = Instant::now() + Duration::from_secs(1);
    assert_eq!(
        check(future, &AtomicBool::new(true))
            .unwrap_err()
            .to_string(),
        "Antigravity request cancelled"
    );
    assert_eq!(
        check(Instant::now(), &AtomicBool::new(false))
            .unwrap_err()
            .to_string(),
        "Antigravity request timed out"
    );
}
