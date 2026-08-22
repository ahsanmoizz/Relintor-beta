use relintor_investigator::{
    AnswerChoice, ClaimClassification, ConflictResolutionState, IntakeError, IntakeService,
    InvestigationResult, Investigator, InvestigatorConfig, NfrClassification, ProjectDraft,
    Provenance, QuestionDisposition, UserAnswer,
};
use std::fs;
use std::path::PathBuf;

fn provenance(label: &str) -> Provenance {
    Provenance {
        source_id: format!("answer-source-{label}"),
        locator: format!("user://answer/{label}"),
        content_hash: format!("answer-hash-{label}"),
        note: "Independent acceptance fixture.".into(),
    }
}

fn answer(question_id: &str, option_id: &str, label: &str) -> UserAnswer {
    UserAnswer {
        id: format!("answer-{label}"),
        question_id: question_id.into(),
        choice: AnswerChoice::Selected(option_id.into()),
        explanation: format!("Selected {label} during independent acceptance."),
        confirmed: true,
        provenance: provenance(label),
        created_at: 1,
    }
}

fn source(
    service: &IntakeService,
    name: &str,
    text: &str,
) -> relintor_investigator::SourceDocument {
    service.intake_text(name, text).expect("fixture source")
}

fn initial(idea: &str) -> InvestigationResult {
    Investigator::default()
        .investigate(ProjectDraft::from_idea(idea).unwrap(), vec![], &[])
        .unwrap()
}

#[test]
fn typed_idea_explicit_fact_retains_provenance_without_documents() {
    let output = initial("A local-first desktop project");
    let claim = output
        .blueprint
        .intent
        .product_purpose
        .as_ref()
        .expect("product purpose");

    assert_eq!(claim.classification, ClaimClassification::ExplicitFact);
    assert!(
        !claim.provenance.is_empty(),
        "the typed idea is evidence and must remain in explicit-fact provenance"
    );
}

#[test]
fn selected_answer_changes_structured_blueprint_semantics() {
    let output = initial("An authenticated application where users login");
    let question = output
        .blueprint
        .questions
        .iter()
        .find(|question| {
            question.disposition == QuestionDisposition::Asked
                && question.question.contains("identity")
        })
        .expect("identity question");

    let option = question.options.first().expect("identity option");
    let revision = Investigator::default()
        .apply_answer(&output, answer(&question.id, &option.id, &option.label))
        .expect("answer revision");

    assert!(
        revision.blueprint.architecture_decisions.iter().any(|decision| {
            decision.decision_question == question.question
                && decision.selected_option.as_deref() == Some(option.label.as_str())
        }),
        "a material selected answer must become structured blueprint/ADR state, not only flip question disposition"
    );
}

#[test]
fn sequential_answer_revisions_preserve_prior_answers() {
    let investigator = Investigator::default();
    let initial =
        initial("An authenticated AI application where users login and an LLM assists them");
    let asked: Vec<_> = initial
        .blueprint
        .questions
        .iter()
        .filter(|question| question.disposition == QuestionDisposition::Asked)
        .cloned()
        .collect();

    assert!(
        asked.len() >= 2,
        "fixture must expose multiple material questions"
    );

    let first_option = asked[0].options.first().expect("first option");
    let first_answer = answer(&asked[0].id, &first_option.id, "first");
    let first_revision = investigator
        .apply_answer(&initial, first_answer.clone())
        .expect("first revision");

    let state_after_first = InvestigationResult {
        project: initial.project.clone(),
        investigation: initial.investigation.clone(),
        inputs: initial.inputs.clone(),
        sources: initial.sources.clone(),
        answers: vec![first_answer],
        blueprint: first_revision.blueprint.clone(),
    };

    let second_option = asked[1].options.first().expect("second option");
    let second_revision = investigator
        .apply_answer(
            &state_after_first,
            answer(&asked[1].id, &second_option.id, "second"),
        )
        .expect("second revision");

    let first_question = second_revision
        .blueprint
        .questions
        .iter()
        .find(|question| question.id == asked[0].id)
        .expect("first question retained");

    assert_ne!(
        first_question.disposition,
        QuestionDisposition::Asked,
        "answering a second question must not erase the first answer"
    );
}

#[test]
fn invalid_option_id_fails_closed() {
    let output = initial("An authenticated application where users login");
    let question = output
        .blueprint
        .questions
        .iter()
        .find(|question| question.disposition == QuestionDisposition::Asked)
        .expect("asked question");

    let bad = answer(&question.id, "option-does-not-exist", "invalid");

    let result = Investigator::default().investigate(output.project, output.sources, &[bad]);

    assert!(
        result.is_err(),
        "an arbitrary option ID must not be accepted as a confirmed answer"
    );
}

#[test]
fn idea_and_document_conflict_is_detected() {
    let service = IntakeService::new(InvestigatorConfig::default());
    let project = ProjectDraft::from_idea("A local-only project").unwrap();
    let cloud = source(
        &service,
        "deployment.md",
        "The product requires cloud synchronization across devices.",
    );

    let output = Investigator::default()
        .investigate(project, vec![cloud], &[])
        .unwrap();

    assert!(
        output.blueprint.conflicts.iter().any(|conflict| {
            conflict.resolution_state == ConflictResolutionState::Unresolved
                && conflict
                    .affected_blueprint_areas
                    .iter()
                    .any(|area| area == "deployment")
        }),
        "typed idea and attached documents are both sources and must be reconciled"
    );
}

#[test]
fn diverse_corpus_is_executed_not_only_counted() {
    let service = IntakeService::new(InvestigatorConfig::default());

    for case in relintor_investigator::diverse_corpus() {
        let sources = case
            .documents
            .iter()
            .enumerate()
            .map(|(index, document)| {
                source(&service, &format!("{}-{index}.md", case.name), document)
            })
            .collect::<Vec<_>>();

        let output = Investigator::default()
            .investigate(ProjectDraft::from_idea(&case.idea).unwrap(), sources, &[])
            .unwrap();

        let actual = output
            .blueprint
            .intent
            .product_type
            .as_ref()
            .expect("product type")
            .value
            .clone();

        assert_eq!(
            actual, case.expected_type,
            "corpus case '{}' produced an unexpected product type",
            case.name
        );

        if case.name == "contradictory" {
            assert!(
                !output.blueprint.conflicts.is_empty(),
                "contradictory corpus case must exercise the conflict gate"
            );
        }
    }
}

#[test]
fn pure_backend_does_not_receive_ui_accessibility_as_required_project_nfr() {
    let output = initial("A headless API backend service with endpoints");

    assert!(
        !output
            .blueprint
            .non_functional_requirements
            .iter()
            .any(|nfr| {
                nfr.domain == "accessibility"
                    && nfr.classification != NfrClassification::NotApplicable
            }),
        "a headless API must not automatically inherit a UI accessibility requirement"
    );
}

#[test]
fn multi_document_intake_is_exercised_and_duplicate_hashes_fail() {
    let root = temp_path("multi-root");
    fs::create_dir_all(&root).unwrap();
    let a = root.join("a.md");
    let b = root.join("b.md");
    fs::write(&a, "alpha").unwrap();
    fs::write(&b, "beta").unwrap();

    let service = IntakeService::new(InvestigatorConfig {
        allowed_root: Some(root.clone()),
        ..Default::default()
    });

    let documents = service
        .intake_files(&[a.clone(), b.clone()])
        .expect("multi intake");

    assert_eq!(documents.len(), 2);
    assert_ne!(documents[0].content_hash, documents[1].content_hash);

    fs::write(&b, "alpha").unwrap();
    let duplicate = service.intake_files(&[a.clone(), b.clone()]);
    assert!(matches!(duplicate, Err(IntakeError::DuplicateDocument(_))));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn allowed_root_rejects_outside_source() {
    let root = temp_path("allowed-root");
    let outside_root = temp_path("outside-root");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside_root).unwrap();
    let outside = outside_root.join("outside.md");
    fs::write(&outside, "outside").unwrap();

    let service = IntakeService::new(InvestigatorConfig {
        allowed_root: Some(root.clone()),
        ..Default::default()
    });

    assert!(matches!(
        service.intake_file(&outside),
        Err(IntakeError::PathOutsideAllowedRoot(_))
    ));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(outside_root);
}

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "relintor-independent-p4-{}-{label}",
        std::process::id()
    ))
}
