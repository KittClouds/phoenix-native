#[allow(dead_code)]
mod support;
use phoenix_reader_session::*;
use support::*;
fn profile() -> VoiceProfile {
    VoiceProfile {
        id: [31; 32],
        revision: 1,
        name: "Narrator".into(),
        description: "Clear narrator".into(),
        reference: None,
        default_delivery: "Calm".into(),
        seed: 42,
    }
}
#[test]
fn profile_history_is_immutable_and_missing_voice_is_not_fallback() {
    let root = tempfile::tempdir().unwrap();
    let library = VoiceLibrary::open(root.path()).unwrap();
    let mut p = profile();
    let old = library.save(&p).unwrap();
    p.name = "Renamed".into();
    p.revision += 1;
    let new = library.save(&p).unwrap();
    assert_ne!(old.fingerprint, new.fingerprint);
    assert_eq!(library.load(old).unwrap().name, "Narrator");
    let missing = VoiceChoice {
        fingerprint: [42; 32],
        ..old
    };
    assert!(library.load(missing).is_err());
    library.select([2; 32], old).unwrap();
    assert_eq!(library.selected([2; 32]).unwrap(), Some(old));
    assert_eq!(library.selected([3; 32]).unwrap(), None);
    assert_eq!(library.list().unwrap().len(), 2);
    assert!(library.select([2; 32], missing).is_err());
    assert!(VoiceLibrary::open(root.path()).is_err());
}
#[test]
fn casting_is_revision_bound_and_never_guesses_across_speaker_boundaries() {
    let source = "Hello. Goodbye.";
    let plan = plan(&lease(source, 1));
    let choice = VoiceChoice::of(&profile()).unwrap();
    let mut cast = CastProfile {
        revision: 1,
        document: plan.spec().document,
        narrator: choice,
        members: vec![CastMember {
            character: [44; 32],
            name: "Kai".into(),
            voice: choice,
            delivery: "Firm".into(),
        }],
        assignments: vec![SpeakerAssignment {
            source: ByteRange { start: 7, end: 15 },
            character: [44; 32],
        }],
    };
    cast.validate(source, &plan).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let library = VoiceLibrary::open(directory.path()).unwrap();
    library.save(&profile()).unwrap();
    library.save_cast(&cast, source, &plan).unwrap();
    assert_eq!(
        library
            .load_cast(source, &plan)
            .unwrap()
            .unwrap()
            .fingerprint()
            .unwrap(),
        cast.fingerprint().unwrap()
    );
    assert_eq!(cast.resolve(ByteRange { start: 0, end: 6 }).unwrap().1, "");
    assert_eq!(
        cast.resolve(ByteRange { start: 7, end: 15 }).unwrap().1,
        "Firm"
    );
    assert!(cast.resolve(ByteRange { start: 0, end: 15 }).is_err());
    let override_voice = VoiceChoice {
        id: [72; 32],
        ..choice
    };
    assert_eq!(
        cast.resolve_with_narrator(ByteRange { start: 0, end: 6 }, override_voice)
            .unwrap()
            .0,
        override_voice
    );
    assert_eq!(
        cast.resolve_with_narrator(ByteRange { start: 7, end: 15 }, override_voice)
            .unwrap()
            .0,
        choice
    );
    cast.document.revision += 1;
    assert!(cast.validate(source, &plan).is_err());
    cast.document = plan.spec().document;
    cast.assignments[0].character = [88; 32];
    assert!(cast.validate(source, &plan).is_err());
}
#[test]
fn cache_identity_changes_only_for_the_changed_speaker() {
    let lease = lease("Hello. Goodbye.", 1);
    let plan = plan_markdown([1; 32], &lease, PlannerConfig::default())
        .unwrap()
        .plan;
    let a = identity();
    let mut b = a.clone();
    b.voice = [43; 32];
    let voices = UtteranceVoices::new(&plan, vec![a.clone(), b.clone()], vec![0, 1]).unwrap();
    b.voice = [44; 32];
    let changed = UtteranceVoices::new(&plan, vec![a, b], vec![0, 1]).unwrap();
    assert_ne!(voices.fingerprint(), changed.fingerprint());
    let key = |v: &UtteranceVoices, s| {
        v.identity(s)
            .unwrap()
            .audio_key(
                plan.segment(s)
                    .unwrap()
                    .spoken
                    .slice(&plan.spec().spoken)
                    .unwrap(),
            )
            .unwrap()
    };
    assert_eq!(key(&voices, 0), key(&changed, 0));
    assert_ne!(key(&voices, 1), key(&changed, 1));
    assert!(UtteranceVoices::new(&plan, vec![identity()], vec![0, 1]).is_err());
}
