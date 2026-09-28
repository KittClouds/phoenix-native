use super::*;

#[test]
fn query_tokenizer_preserves_only_the_bracketed_focal_vote() {
    let values = query_tokens("How to [pull] data before pulling more");
    assert_eq!(values.iter().filter(|(_, focal)| *focal).count(), 1);
    assert_eq!(values.iter().find(|(_, focal)| *focal).unwrap().0, "pull");
}

#[test]
fn transport_feature_firewall_excludes_candidate_terms() {
    let context = "before [pull] data after";
    let features = features::context_features(
        &p1o1::Occurrence {
            field: "text".to_owned(),
            excerpt: context.to_owned(),
        },
        "pluck",
        "pull",
    );
    assert!(
        !features
            .tokens
            .iter()
            .any(|token| token == "pull" || token == "pluck")
    );
}

#[test]
fn e1_relation_support_requires_positive_and_negative_evidence() {
    assert!(relation_support_qualified(4, 4));
    assert!(!relation_support_qualified(3, 4));
    assert!(!relation_support_qualified(4, 0));
}

#[test]
fn observed_allow_on_different_is_counted_as_false_authorization() {
    let mut decisions = ObservedDecisions::default();
    decisions.add(DecisionLabel::Different, DecisionLabel::Same);
    assert_eq!(decisions.different.allow, 1);
    assert_eq!(decisions.different.refuse, 0);
    assert_eq!(decisions.same.allow, 0);
}
