//! Catalog invariants: the parameter surface Cursor renders must stay unique and
//! well-formed for every thinking/non-thinking model.

use super::*;

/// A duplicate effort tier would show the same choice twice in Cursor's picker, so
/// every variant of a model must carry a distinct display name.
#[test]
fn model_variant_display_names_are_unique() {
    let contexts = context_options(Some(200_000));
    for thinking in [false, true] {
        let tooltip = TooltipData {
            markdown_content: None,
        };
        let variants = model_variants("m", "Model", &tooltip, &contexts, thinking);
        assert!(!variants.is_empty(), "no variants for thinking={thinking}");
        let mut names = std::collections::BTreeSet::new();
        for variant in &variants {
            assert!(
                names.insert(variant.display_name.clone()),
                "duplicate variant name {} for thinking={thinking}",
                variant.display_name
            );
        }
    }
}

#[test]
fn effort_parameter_exposes_the_published_tier_slugs() {
    let parameters = model_parameters(&context_options(None), true);
    let reasoning = parameters
        .iter()
        .find(|parameter| parameter.id == "reasoning")
        .expect("thinking models expose a reasoning parameter");
    let values: Vec<String> = reasoning
        .parameter_type
        .as_ref()
        .and_then(|parameter_type| parameter_type.enum_parameter.as_ref())
        .map(|enum_parameter| {
            enum_parameter
                .values
                .iter()
                .map(|value| value.value.clone())
                .collect()
        })
        .expect("reasoning parameter is an enum");
    for (slug, _) in EFFORTS {
        assert!(
            values.iter().any(|value| value == slug),
            "published effort slug {slug} is missing from the parameter"
        );
    }
}

#[test]
fn non_thinking_models_omit_the_reasoning_parameter() {
    let parameters = model_parameters(&context_options(None), false);
    assert!(
        !parameters
            .iter()
            .any(|parameter| parameter.id == "reasoning"),
        "non-thinking models must not offer an effort control"
    );
}
