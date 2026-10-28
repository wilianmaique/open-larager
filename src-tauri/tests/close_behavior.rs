use larager::{
    engine::Engine,
    model::{CloseBehavior, Settings},
    storage::Storage,
};

#[test]
fn existing_settings_ask_before_closing_and_reject_unknown_behaviors() {
    let mut settings = serde_json::to_value(Settings::default()).unwrap();
    settings.as_object_mut().unwrap().remove("closeBehavior");
    let original = settings.clone();
    assert_eq!(
        serde_json::from_value::<Settings>(settings.clone())
            .unwrap()
            .close_behavior,
        CloseBehavior::Ask
    );
    settings["closeBehavior"] = "run-a-command".into();
    assert!(serde_json::from_value::<Settings>(settings).is_err());
    let temp = tempfile::tempdir().unwrap();
    let storage = Storage::open(temp.path().join("environment")).unwrap();
    std::fs::write(
        storage.root.join("settings.json"),
        serde_json::to_vec(&original).unwrap(),
    )
    .unwrap();
    assert_eq!(storage.load().unwrap().close_behavior, CloseBehavior::Ask);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &std::fs::read(storage.root.join("settings.json")).unwrap()
        )
        .unwrap(),
        original
    );
}

#[tokio::test]
async fn close_behavior_persists_without_overwriting_other_preferences_or_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("environment");
    let engine = Engine::open(root.clone()).unwrap();
    engine
        .save_preferences("pt-BR".into(), "dark".into())
        .await
        .unwrap();
    let data = root.join("projects/keep.txt");
    std::fs::write(&data, "user project").unwrap();
    for behavior in [CloseBehavior::Tray, CloseBehavior::Exit, CloseBehavior::Ask] {
        engine.save_close_behavior(behavior).await.unwrap();
        let stored = engine.storage.load().unwrap();
        assert_eq!(stored.close_behavior, behavior);
        assert_eq!(stored.language, "pt-BR");
        assert_eq!(stored.theme, "dark");
        assert_eq!(stored.schema_version, 1);
    }
    engine
        .save_close_behavior(CloseBehavior::Tray)
        .await
        .unwrap();
    drop(engine);
    let reopened = Engine::open(root).unwrap();
    assert_eq!(
        reopened.settings.read().await.close_behavior,
        CloseBehavior::Tray
    );
    assert_eq!(std::fs::read_to_string(data).unwrap(), "user project");
}
