use super::*;
use crate::config::{Config, ProviderConfig, ProviderKind};

fn config() -> Config {
    let mut config = Config::default();
    config.providers.insert(
        "deepseek".into(),
        ProviderConfig {
            model: Some("deepseek-reasoner".into()),
            ..ProviderConfig::defaults_for(ProviderKind::OpenAi)
        },
    );
    config
}

#[test]
fn lists_every_catalog_model_per_provider() {
    let picker = ModelPicker::from_config(&config());
    assert!(
        picker
            .all
            .iter()
            .filter(|i| i.provider == "anthropic")
            .count()
            >= 3
    );
    assert!(picker.all.iter().any(|i| i.provider == "custom"));
}

#[test]
fn configured_model_is_first() {
    let picker = ModelPicker::from_config(&config());
    let first = picker
        .all
        .iter()
        .find(|i| i.provider == "deepseek")
        .unwrap();
    assert_eq!(first.model, "deepseek-reasoner");
}

#[test]
fn query_filters_by_provider_or_model() {
    let mut picker = ModelPicker::from_config(&config());
    picker.query = "sonnet".into();
    assert!(picker.visible().iter().all(|i| i.model.contains("sonnet")));
    picker.query = "deepseek/".into();
    assert!(picker.visible().iter().all(|i| i.provider == "deepseek"));
}

#[test]
fn live_models_are_prepended_and_catalog_entries_kept() {
    let mut picker = ModelPicker::from_config(&config());
    picker.merge_models(
        "anthropic",
        &["claude-live".into(), "claude-sonnet-4-5".into()],
    );
    let anthropic: Vec<&ModelItem> = picker
        .all
        .iter()
        .filter(|i| i.provider == "anthropic")
        .collect();
    assert_eq!(anthropic[0].model, "claude-live");
    assert!(anthropic.iter().any(|i| i.model == "claude-opus-4-1"));
    assert_eq!(
        anthropic
            .iter()
            .filter(|i| i.model == "claude-sonnet-4-5")
            .count(),
        1
    );
}

#[test]
fn selection_wraps() {
    let mut picker = ModelPicker::from_config(&config());
    let len = picker.visible().len();
    picker.selected = len - 1;
    picker.move_selection(1);
    assert_eq!(picker.selected, 0);
    picker.move_selection(-1);
    assert_eq!(picker.selected, len - 1);
}
