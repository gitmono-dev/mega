use super::*;

fn vectors() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/snapshot/namespace-v1.json"
    ))
    .unwrap()
}
fn binding() -> NamespaceBinding {
    serde_json::from_value(vectors()["bindings"][0]["binding"].clone()).unwrap()
}
fn view() -> NamespaceView {
    serde_json::from_value(vectors()["views"][0]["view"].clone()).unwrap()
}

#[test]
fn snapshot_namespace_vectors_match_independent_dotnet_and_json_roundtrip() {
    for vector in vectors()["bindings"].as_array().unwrap() {
        let value: NamespaceBinding = serde_json::from_value(vector["binding"].clone()).unwrap();
        let bytes = hex::decode(vector["canonical_hex"].as_str().unwrap()).unwrap();
        assert_eq!(value.canonical_bytes(), bytes);
        assert_eq!(value.id().as_str(), vector["digest"].as_str().unwrap());
        assert_eq!(
            NamespaceBinding::from_canonical_bytes(&bytes).unwrap(),
            value
        );
        assert_eq!(serde_json::to_value(value).unwrap(), vector["binding"]);
    }
    for vector in vectors()["views"].as_array().unwrap() {
        let value: NamespaceView = serde_json::from_value(vector["view"].clone()).unwrap();
        let bytes = hex::decode(vector["canonical_hex"].as_str().unwrap()).unwrap();
        assert_eq!(value.canonical_bytes(), bytes);
        assert_eq!(value.id().as_str(), vector["digest"].as_str().unwrap());
        assert_eq!(NamespaceView::from_canonical_bytes(&bytes).unwrap(), value);
        assert_eq!(serde_json::to_value(value).unwrap(), vector["view"]);
    }
}

#[test]
fn snapshot_namespace_provenance_routing_policy_and_instance_affect_identity() {
    let base = view();
    let other_instance = NamespaceView::new(
        InstanceId::new("99999999-9999-4999-8999-999999999999").unwrap(),
        base.native().clone(),
        base.bindings_root().clone(),
        None,
        base.materialization_policy(),
    )
    .unwrap();
    assert_ne!(base.id(), other_instance.id());
    let mut native = base.native().clone();
    native.commit_oid = ObjectId::new("f".repeat(40)).unwrap();
    let other_commit = NamespaceView::new(
        base.instance_id().clone(),
        native,
        base.bindings_root().clone(),
        None,
        base.materialization_policy(),
    )
    .unwrap();
    assert_eq!(
        base.native().root_tree_oid,
        other_commit.native().root_tree_oid
    );
    assert_ne!(base.id(), other_commit.id());
    let other_bindings = NamespaceView::new(
        base.instance_id().clone(),
        base.native().clone(),
        hash_bytes(b"different routing"),
        None,
        base.materialization_policy(),
    )
    .unwrap();
    assert_ne!(base.id(), other_bindings.id());
    let base_binding = binding();
    let other_policy = NamespaceBinding::new(
        base_binding.mount_path().clone(),
        base_binding.source_snapshot().clone(),
        base_binding.source_subpath().clone(),
        BindingPolicy::ImmutableRelease,
    )
    .unwrap();
    assert_ne!(base_binding.id(), other_policy.id());
    let moved = NamespaceBinding::new(
        RepoPath::new("/other").unwrap(),
        base_binding.source_snapshot().clone(),
        base_binding.source_subpath().clone(),
        base_binding.policy(),
    )
    .unwrap();
    assert_ne!(base_binding.id(), moved.id());
    let subpath = NamespaceBinding::new(
        base_binding.mount_path().clone(),
        base_binding.source_snapshot().clone(),
        RelativePath::new("other").unwrap(),
        base_binding.policy(),
    )
    .unwrap();
    assert_ne!(base_binding.id(), subpath.id());
}

#[test]
fn snapshot_namespace_json_cannot_bypass_schema_scope_or_unknown_field_checks() {
    let raw = vectors()["views"][0]["view"].clone();
    for (field, value) in [
        ("schema_version", serde_json::json!(2)),
        (
            "instance_id",
            serde_json::json!("00000000-0000-0000-0000-000000000000"),
        ),
        (
            "materialization_policy",
            serde_json::json!("hydrate_everything"),
        ),
        ("lease", serde_json::json!("not identity")),
        ("publication_seq", serde_json::json!(1)),
    ] {
        let mut changed = raw.clone();
        changed[field] = value;
        assert!(
            serde_json::from_value::<NamespaceView>(changed).is_err(),
            "{field}"
        );
    }
    let mut scoped = raw;
    scoped["native"]["scope_path"] = serde_json::json!("/child");
    assert!(serde_json::from_value::<NamespaceView>(scoped).is_err());
    let raw = vectors()["bindings"][0]["binding"].clone();
    for (field, value) in [
        ("mount_path", serde_json::json!("/deps//bad")),
        ("source_subpath", serde_json::json!("../escape")),
        ("policy", serde_json::json!("guess_from_path")),
        ("ref_name", serde_json::json!("refs/heads/main")),
    ] {
        let mut changed = raw.clone();
        changed[field] = value;
        assert!(
            serde_json::from_value::<NamespaceBinding>(changed).is_err(),
            "{field}"
        );
    }
}

#[test]
fn snapshot_namespace_codec_rejects_truncation_oversize_unknown_tags_and_domains() {
    let binding = binding().canonical_bytes();
    let view = view().canonical_bytes();
    for end in 0..binding.len() {
        assert!(NamespaceBinding::from_canonical_bytes(&binding[..end]).is_err());
    }
    for end in 0..view.len() {
        assert!(NamespaceView::from_canonical_bytes(&view[..end]).is_err());
    }
    assert!(NamespaceBinding::from_canonical_bytes(&view).is_err());
    assert!(NamespaceView::from_canonical_bytes(&binding).is_err());
    for original in [&binding, &view] {
        let mut extra = original.clone();
        extra.push(0);
        assert!(NamespaceBinding::from_canonical_bytes(&extra).is_err());
        assert!(NamespaceView::from_canonical_bytes(&extra).is_err());
    }
    let mut bad = binding.clone();
    *bad.last_mut().unwrap() = 0;
    assert!(NamespaceBinding::from_canonical_bytes(&bad).is_err());
    let mut bad = view.clone();
    *bad.last_mut().unwrap() = 99;
    assert!(NamespaceView::from_canonical_bytes(&bad).is_err());
    let mut bad = view.clone();
    bad[view.len() - 2] = 3;
    assert!(NamespaceView::from_canonical_bytes(&bad).is_err());
    let mut bad = view;
    bad[VIEW_DOMAIN.len() + 1] = 2;
    assert!(NamespaceView::from_canonical_bytes(&bad).is_err());
    let mut bad = binding;
    bad[BINDING_DOMAIN.len()..BINDING_DOMAIN.len() + 4].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(NamespaceBinding::from_canonical_bytes(&bad).is_err());
    let huge = vec![0; MAX_MANIFEST_BYTES + 1];
    assert!(NamespaceView::from_canonical_bytes(&huge).is_err());
    assert!(NamespaceBinding::from_canonical_bytes(&huge).is_err());
}

#[test]
fn snapshot_namespace_maximum_paths_fit_bounded_manifests_and_proofs() {
    let path = format!("/{}", vec!["x".repeat(255); 16].join("/"));
    assert_eq!(path.len(), MAX_PATH_BYTES);
    let mut native = view().native().clone();
    native.scope_path = RepoPath::new(&path).unwrap();
    let maximal = NamespaceBinding::new(
        RepoPath::new(&path).unwrap(),
        native.clone(),
        RelativePath::new("").unwrap(),
        BindingPolicy::Mutable,
    )
    .unwrap();
    assert!(maximal.canonical_bytes().len() < MAX_MANIFEST_BYTES);
    assert_eq!(
        NamespaceBinding::from_canonical_bytes(&maximal.canonical_bytes()).unwrap(),
        maximal
    );
    assert!(
        NamespaceBinding::new(
            RepoPath::new("/deps").unwrap(),
            native,
            RelativePath::new("x").unwrap(),
            BindingPolicy::Mutable,
        )
        .is_err()
    );
    let mut raw = serde_json::to_value(maximal).unwrap();
    raw["source_subpath"] = serde_json::json!("x");
    assert!(serde_json::from_value::<NamespaceBinding>(raw).is_err());
}
