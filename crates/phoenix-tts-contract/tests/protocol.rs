use phoenix_tts_contract::*;
fn binding() -> Binding {
    Binding {
        request: 1,
        epoch: 1,
        plan: [1; 32],
        segment: 0,
        audio_key: [2; 32],
    }
}
fn e(sequence: u64, event: Event) -> Envelope {
    Envelope {
        binding: binding(),
        sequence,
        event,
    }
}
fn start() -> Event {
    Event::Started {
        provider: [3; 32],
        format: AudioFormat::PCM24,
    }
}
#[test]
fn only_normal_contiguous_complete_audio_can_seal() {
    for reason in [
        FinishReason::Normal,
        FinishReason::TokenLimit,
        FinishReason::TransportEof,
    ] {
        let mut v = StreamValidator::new(binding(), [3; 32], 10).unwrap();
        v.accept(e(0, start())).unwrap();
        v.accept(e(
            1,
            Event::AudioChunk {
                first_frame: 0,
                frames: 10,
            },
        ))
        .unwrap();
        let result = v.accept(e(2, Event::Completed { frames: 10, reason }));
        assert_eq!(result.is_ok(), reason == FinishReason::Normal);
        assert!(v
            .accept(e(
                3,
                Event::AudioChunk {
                    first_frame: 10,
                    frames: 1
                }
            ))
            .is_err());
    }
}
#[test]
fn protocol_faults_poison_streams() {
    for faulty in [
        e(2, start()),
        e(
            0,
            Event::AudioChunk {
                first_frame: 0,
                frames: 1,
            },
        ),
        Envelope {
            binding: Binding {
                epoch: 2,
                ..binding()
            },
            sequence: 0,
            event: start(),
        },
    ] {
        let mut v = StreamValidator::new(binding(), [3; 32], 10).unwrap();
        assert!(v.accept(faulty).is_err());
        assert!(v.accept(e(0, start())).is_err());
    }
    let mut v = StreamValidator::new(binding(), [3; 32], 10).unwrap();
    v.accept(e(0, start())).unwrap();
    assert!(v
        .accept(e(
            1,
            Event::AudioChunk {
                first_frame: 1,
                frames: 1
            }
        ))
        .is_err());
}

#[test]
fn late_quiescence_cannot_release_a_new_request() {
    let mut c = Cancellation::default();
    c.start(1).unwrap();
    c.quiescent(1).unwrap();
    c.start(2).unwrap();
    assert!(c.quiescent(1).is_err());
    assert_eq!(c.state(), WorkerState::Running);
    c.cancel(10).unwrap();
    c.quiescent(2).unwrap();
    assert!(c.may_retry());
    assert!(c.start(2).is_err());
}
#[test]
fn cancel_escalation_requires_confirmed_exit_and_partial_speech_forbids_retry() {
    let mut c = Cancellation::default();
    let epoch = c.start(1).unwrap();
    c.mark_presented(1, epoch);
    assert_eq!(c.cancel(100).unwrap(), 2);
    assert!(c.start(2).is_err());
    assert!(!c.tick(2099));
    assert!(c.tick(2100));
    assert!(c.replacement_ready().is_err());
    c.exit_confirmed().unwrap();
    c.replacement_ready().unwrap();
    assert!(!c.may_retry());
}
