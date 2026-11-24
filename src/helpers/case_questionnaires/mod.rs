//! Volunteer-only case questionnaires and the rules that connect them.
//!
//! These definitions are intentionally code-backed for the first iteration.
//! They can later be replaced by administrator-managed templates without
//! changing how answers are stored on a case.
//!
//! Every answer is an ordinary [`CaseProperty`]: one property per question, keyed
//! by the question's `key` and grouped under the questionnaire's `section`. A
//! repeat group flattens the same way, one property per field per entry, with the
//! entry number in the key ("Child 2 date of birth"), so nothing here needs its
//! own table or a blob column.

use std::collections::BTreeMap;

use crate::helpers::visibility::Visibility;
use crate::server_fns::case_properties::CaseProperty;

mod general;
mod specialized;

pub use general::GENERAL_QUESTIONNAIRE;
pub use specialized::{
    EMPLOYMENT_QUESTIONNAIRE, FAMILY_QUESTIONNAIRE, IMMIGRATION_QUESTIONNAIRE,
    LEGAL_CONFLICT_QUESTIONNAIRE, MENTAL_HEALTH_QUESTIONNAIRE, RESOURCE_QUESTIONNAIRE,
    SUPERVISED_VISITATION_QUESTIONNAIRE,
};

pub const COMPLETED_KEY: &str = "Questionnaire completed";
pub const COMPLETED_VALUE: &str = "Yes";

/// How one question is answered. The variants map to form controls; every one of
/// them still stores a single string value per question.
#[derive(Clone, Copy, Debug)]
pub enum QuestionnaireInput {
    /// One choice from a fixed list.
    Select(&'static [&'static str]),
    /// One choice from a fixed list, or free text when "Other" is picked.
    SelectOther(&'static [&'static str]),
    /// Any number of choices, stored comma-separated.
    MultiSelect(&'static [&'static str]),
    Text,
    TextArea,
    Date,
    /// A whole number, e.g. household size. Drives repeat-group counts.
    Number,
    Currency,
    Email,
    Phone,
    /// Typeahead against the organization directory, falling back to free text
    /// for a volunteer without the information-management grant.
    OrganizationLookup,
    /// Typeahead against the contact directory, same fallback.
    ContactLookup,
}

/// When a question is shown. Conditions read answers already given *in the same
/// questionnaire*, which is what every trigger in the intake spec does.
#[derive(Clone, Copy, Debug)]
pub enum ShowWhen {
    /// No condition: the question is always shown.
    Always,
    /// Shown when `key`'s answer is one of `values`.
    AnswerIs {
        key: &'static str,
        values: &'static [&'static str],
    },
    /// Shown when `key`'s multi-select answer contains any of `values`.
    AnswerIncludes {
        key: &'static str,
        values: &'static [&'static str],
    },
    /// Shown when `key`'s numeric answer is greater than `than`.
    CountAbove { key: &'static str, than: i64 },
    /// Shown when any of the nested conditions hold.
    Any(&'static [ShowWhen]),
}

impl ShowWhen {
    /// Whether this condition holds for the answers given so far.
    pub fn holds(&self, answers: &BTreeMap<String, String>) -> bool {
        let answer = |key: &str| {
            answers
                .get(key)
                .map(|value| value.trim())
                .unwrap_or_default()
                .to_string()
        };
        match self {
            ShowWhen::Always => true,
            ShowWhen::AnswerIs { key, values } => {
                let given = answer(key);
                values.iter().any(|value| given == *value)
            }
            ShowWhen::AnswerIncludes { key, values } => {
                let given = answer(key);
                given
                    .split(american_separator())
                    .map(str::trim)
                    .any(|part| values.iter().any(|value| part == *value))
            }
            ShowWhen::CountAbove { key, than } => {
                answer(key).parse::<i64>().map(|n| n > *than).unwrap_or(false)
            }
            ShowWhen::Any(conditions) => {
                conditions.iter().any(|condition| condition.holds(answers))
            }
        }
    }
}

/// Multi-select answers are stored as one string; this is the separator.
pub const fn american_separator() -> char {
    ','
}

#[derive(Clone, Copy, Debug)]
pub struct QuestionnaireQuestion {
    pub key: &'static str,
    pub label: &'static str,
    pub input: QuestionnaireInput,
    /// Whether an answer is needed *when the question is shown*. A question that
    /// is hidden is never required, which is how the spec's "Yes, but only after
    /// a specific answer" rows behave.
    pub required: bool,
    pub show_when: ShowWhen,
    /// Optional clarifying text shown under the label.
    pub help: Option<&'static str>,
}

impl QuestionnaireQuestion {
    /// A required, always-shown question — the common case.
    pub const fn new(
        key: &'static str,
        label: &'static str,
        input: QuestionnaireInput,
    ) -> Self {
        Self {
            key,
            label,
            input,
            required: true,
            show_when: ShowWhen::Always,
            help: None,
        }
    }

    pub const fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub const fn when(mut self, show_when: ShowWhen) -> Self {
        self.show_when = show_when;
        self
    }

    pub const fn help(mut self, help: &'static str) -> Self {
        self.help = Some(help);
        self
    }
}

/// A set of fields captured once per entry, e.g. one block per child. The number
/// of entries comes from `count_key`'s numeric answer, capped by `max`.
#[derive(Clone, Copy, Debug)]
pub struct RepeatGroup {
    /// Question key whose number drives how many entries are shown.
    pub count_key: &'static str,
    /// Singular noun used to label each entry, e.g. "Child".
    pub noun: &'static str,
    pub fields: &'static [QuestionnaireQuestion],
    pub max: usize,
}

impl RepeatGroup {
    /// How many entries to render for the answers given so far.
    pub fn entries(&self, answers: &BTreeMap<String, String>) -> usize {
        answers
            .get(self.count_key)
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0)
            .min(self.max)
    }

    /// The stored property key for one field of one entry, e.g.
    /// "Child 2 date of birth". One-based to match how people count.
    pub fn field_key(&self, index: usize, field: &QuestionnaireQuestion) -> String {
        format!("{} {} {}", self.noun, index + 1, field.key)
    }

    /// The label shown for one field of one entry.
    pub fn field_label(&self, index: usize, field: &QuestionnaireQuestion) -> String {
        format!("{} {} \u{2014} {}", self.noun, index + 1, field.label)
    }
}

/// One display grouping within a questionnaire. Purely presentational: every
/// question in every block still stores into the questionnaire's one section.
#[derive(Clone, Copy, Debug)]
pub struct QuestionnaireBlock {
    pub title: &'static str,
    pub description: &'static str,
    pub questions: &'static [QuestionnaireQuestion],
    /// Repeat groups rendered after this block's questions.
    pub repeats: &'static [RepeatGroup],
    pub show_when: ShowWhen,
}

impl QuestionnaireBlock {
    pub const fn new(
        title: &'static str,
        description: &'static str,
        questions: &'static [QuestionnaireQuestion],
    ) -> Self {
        Self {
            title,
            description,
            questions,
            repeats: &[],
            show_when: ShowWhen::Always,
        }
    }

    pub const fn repeating(mut self, repeats: &'static [RepeatGroup]) -> Self {
        self.repeats = repeats;
        self
    }

    pub const fn when(mut self, show_when: ShowWhen) -> Self {
        self.show_when = show_when;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CaseQuestionnaire {
    pub slug: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub section: &'static str,
    pub blocks: &'static [QuestionnaireBlock],
}

impl CaseQuestionnaire {
    /// Every question in every block, ignoring conditions. Used where the shape
    /// of the form matters but the answers do not.
    pub fn all_questions(&self) -> impl Iterator<Item = &'static QuestionnaireQuestion> {
        self.blocks
            .iter()
            .flat_map(|block| block.questions.iter())
    }

    /// The questions currently shown, given the answers so far.
    pub fn visible_questions(
        &self,
        answers: &BTreeMap<String, String>,
    ) -> Vec<&'static QuestionnaireQuestion> {
        self.blocks
            .iter()
            .filter(|block| block.show_when.holds(answers))
            .flat_map(|block| {
                block
                    .questions
                    .iter()
                    .filter(|question| question.show_when.holds(answers))
            })
            .collect()
    }

    /// The repeat-group fields currently shown, as `(stored key, field)` pairs.
    pub fn visible_repeat_fields(
        &self,
        answers: &BTreeMap<String, String>,
    ) -> Vec<(String, &'static QuestionnaireQuestion)> {
        let mut fields = Vec::new();
        for block in self.blocks.iter().filter(|b| b.show_when.holds(answers)) {
            for repeat in block.repeats {
                for index in 0..repeat.entries(answers) {
                    for field in repeat.fields {
                        if field.show_when.holds(answers) {
                            fields.push((repeat.field_key(index, field), field));
                        }
                    }
                }
            }
        }
        fields
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RequiredQuestionnaire {
    pub questionnaire: &'static CaseQuestionnaire,
    pub reason: &'static str,
}

/// Every questionnaire, general first.
pub const QUESTIONNAIRES: &[&CaseQuestionnaire] = &[
    &GENERAL_QUESTIONNAIRE,
    &LEGAL_CONFLICT_QUESTIONNAIRE,
    &FAMILY_QUESTIONNAIRE,
    &IMMIGRATION_QUESTIONNAIRE,
    &MENTAL_HEALTH_QUESTIONNAIRE,
    &EMPLOYMENT_QUESTIONNAIRE,
    &SUPERVISED_VISITATION_QUESTIONNAIRE,
    &RESOURCE_QUESTIONNAIRE,
];

pub fn questionnaire(slug: &str) -> Option<&'static CaseQuestionnaire> {
    QUESTIONNAIRES
        .iter()
        .copied()
        .find(|questionnaire| questionnaire.slug == slug)
}

pub fn is_questionnaire_section(section: &str) -> bool {
    QUESTIONNAIRES
        .iter()
        .any(|questionnaire| questionnaire.section == section)
}

pub fn answers_for(
    properties: &[CaseProperty],
    questionnaire: &CaseQuestionnaire,
) -> BTreeMap<String, String> {
    properties
        .iter()
        .filter(|property| {
            property.visibility == Visibility::VolunteerOnly
                && property.section == questionnaire.section
                && property.key != COMPLETED_KEY
        })
        .map(|property| (property.key.clone(), property.value.clone()))
        .collect()
}

pub fn is_complete(properties: &[CaseProperty], questionnaire: &CaseQuestionnaire) -> bool {
    properties.iter().any(|property| {
        property.visibility == Visibility::VolunteerOnly
            && property.section == questionnaire.section
            && property.key == COMPLETED_KEY
            && property.value == COMPLETED_VALUE
    })
}

pub fn required_followups(properties: &[CaseProperty]) -> Vec<RequiredQuestionnaire> {
    if !is_complete(properties, &GENERAL_QUESTIONNAIRE) {
        return Vec::new();
    }

    let answers = answers_for(properties, &GENERAL_QUESTIONNAIRE);
    followups_for_answers(&answers)
}

/// The specialized intakes required by the routing section of the general
/// intake. Mirrors "General Intake Section 10 Specialized Intake Routing".
pub fn followups_for_answers(answers: &BTreeMap<String, String>) -> Vec<RequiredQuestionnaire> {
    let answer_is = |key: &str, expected: &[&str]| {
        answers
            .get(key)
            .map(|answer| answer.trim())
            .is_some_and(|answer| expected.iter().any(|value| answer == *value))
    };
    let answer_includes = |key: &str, expected: &[&str]| {
        answers.get(key).is_some_and(|answer| {
            answer
                .split(american_separator())
                .map(str::trim)
                .any(|part| expected.iter().any(|value| part == *value))
        })
    };

    let family = answer_is("Family or matrimonial legal help needed", &["Yes"]);
    let immigration = answer_is("Immigration help needed", &["Yes"]);

    let mut required = Vec::new();

    // The conflict check gates the legal work, so it comes first.
    if family || immigration {
        required.push(RequiredQuestionnaire {
            questionnaire: &LEGAL_CONFLICT_QUESTIONNAIRE,
            reason: "Required before legal services begin, because legal help was requested.",
        });
    }
    if family {
        required.push(RequiredQuestionnaire {
            questionnaire: &FAMILY_QUESTIONNAIRE,
            reason: "Required because family or matrimonial legal help was requested.",
        });
    }
    if immigration {
        required.push(RequiredQuestionnaire {
            questionnaire: &IMMIGRATION_QUESTIONNAIRE,
            reason: "Required because immigration help was requested.",
        });
    }
    if answer_is("Mental health support wanted", &["Yes"]) {
        required.push(RequiredQuestionnaire {
            questionnaire: &MENTAL_HEALTH_QUESTIONNAIRE,
            reason: "Required because counseling support was requested or functioning is affected.",
        });
    }
    if answer_is("Employment help wanted", &["Yes"]) {
        required.push(RequiredQuestionnaire {
            questionnaire: &EMPLOYMENT_QUESTIONNAIRE,
            reason: "Required because employment, career, or training help was requested.",
        });
    }
    if answer_is("Supervised visitation needed", &["Yes"]) {
        required.push(RequiredQuestionnaire {
            questionnaire: &SUPERVISED_VISITATION_QUESTIONNAIRE,
            reason: "Required because supervised visitation or safe exchange is involved.",
        });
    }
    // Resource navigation is also required for anyone outside the service area,
    // which is where the out-of-state referral questions lead.
    let wants_resources = answer_includes(
        "Resource or stabilization help needed",
        &[
            "Housing",
            "Food",
            "Benefits",
            "Childcare",
            "Transportation",
            "Healthcare",
            "Technology",
            "Documents",
            "Other",
        ],
    );
    let out_of_state = answer_is("Is the client located in New York State", &["No"]);
    if wants_resources || out_of_state {
        required.push(RequiredQuestionnaire {
            questionnaire: &RESOURCE_QUESTIONNAIRE,
            reason: if out_of_state && !wants_resources {
                "Required because the client is outside New York State."
            } else {
                "Required because stabilization or resource help was requested."
            },
        });
    }
    required
}

/// Questionnaires that hold stored answers but are no longer required by the
/// current general intake — e.g. "needs housing" was answered Yes, the housing
/// form was filled in, and the answer was later changed to No.
///
/// Returned so the page can still show that work rather than letting it vanish
/// from every screen while remaining in the database.
pub fn answered_but_not_required(properties: &[CaseProperty]) -> Vec<&'static CaseQuestionnaire> {
    let required = required_followups(properties);
    QUESTIONNAIRES
        .iter()
        .copied()
        .filter(|questionnaire| questionnaire.slug != GENERAL_QUESTIONNAIRE.slug)
        .filter(|questionnaire| is_complete(properties, questionnaire))
        .filter(|questionnaire| {
            !required
                .iter()
                .any(|entry| entry.questionnaire.slug == questionnaire.slug)
        })
        .collect()
}

/// Validate the answers to the questions that are actually *shown*. A hidden
/// question is never required, which is how the spec's conditionally-mandatory
/// rows behave, and a stale answer to a now-hidden question is simply ignored.
pub fn validate_answers(
    questionnaire: &CaseQuestionnaire,
    answers: &BTreeMap<String, String>,
) -> Result<(), String> {
    let check = |key: &str, question: &QuestionnaireQuestion| -> Result<(), String> {
        let answer = answers
            .get(key)
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        if question.required && answer.is_empty() {
            return Err(format!("Please answer \"{}\".", question.label));
        }
        if answer.is_empty() {
            return Ok(());
        }
        match question.input {
            QuestionnaireInput::Select(options) => {
                if !options.contains(&answer) {
                    return Err(format!("Choose a valid answer for \"{}\".", question.label));
                }
            }
            // "Other" allows free text, so only the listed options are checked
            // when the answer happens to match one of them.
            QuestionnaireInput::SelectOther(_) => {}
            QuestionnaireInput::MultiSelect(options) => {
                for part in answer.split(american_separator()).map(str::trim) {
                    if !part.is_empty() && !options.contains(&part) {
                        return Err(format!(
                            "\"{part}\" is not a valid choice for \"{}\".",
                            question.label
                        ));
                    }
                }
            }
            QuestionnaireInput::Number => {
                if answer.parse::<i64>().is_err() {
                    return Err(format!("\"{}\" needs a whole number.", question.label));
                }
            }
            QuestionnaireInput::Currency => {
                let cleaned = answer.replace(['$', ','], "");
                if cleaned.parse::<f64>().is_err() {
                    return Err(format!("\"{}\" needs an amount.", question.label));
                }
            }
            QuestionnaireInput::Email => {
                if !answer.contains('@') {
                    return Err(format!("\"{}\" needs a valid email.", question.label));
                }
            }
            _ => {}
        }
        Ok(())
    };

    for question in questionnaire.visible_questions(answers) {
        check(question.key, question)?;
    }
    for (key, field) in questionnaire.visible_repeat_fields(answers) {
        check(&key, field)?;
    }
    Ok(())
}

/// The properties to store for a completed questionnaire: one per visible
/// question, one per visible repeat-group field, plus the completion marker.
///
/// Only visible questions are stored, so answers to questions the client was
/// never asked do not linger when a branch is changed.
pub fn answer_properties(
    questionnaire: &CaseQuestionnaire,
    answers: &BTreeMap<String, String>,
) -> Vec<CaseProperty> {
    let property = |key: String, value: String| CaseProperty {
        key,
        value,
        section: questionnaire.section.to_string(),
        visibility: Visibility::VolunteerOnly,
    };
    let value_of = |key: &str| {
        answers
            .get(key)
            .map(|answer| answer.trim().to_string())
            .unwrap_or_default()
    };

    let mut properties = questionnaire
        .visible_questions(answers)
        .into_iter()
        .map(|question| property(question.key.to_string(), value_of(question.key)))
        .collect::<Vec<_>>();
    properties.extend(
        questionnaire
            .visible_repeat_fields(answers)
            .into_iter()
            .map(|(key, _)| {
                let value = value_of(&key);
                property(key, value)
            }),
    );
    properties.push(property(COMPLETED_KEY.to_string(), COMPLETED_VALUE.to_string()));
    properties
}

pub fn draft_properties(
    questionnaire: &CaseQuestionnaire,
    answers: &BTreeMap<String, String>,
) -> Vec<CaseProperty> {
    let mut properties = answer_properties(questionnaire, answers);
    properties.retain(|property| property.key != COMPLETED_KEY && !property.value.is_empty());
    properties
}
