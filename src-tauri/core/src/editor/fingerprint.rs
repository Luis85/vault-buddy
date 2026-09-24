//! Canonical JSON fingerprinting and rendered-product lineage
//! (`DATA-MODEL.md` § Persistence and source identity; R5). A tutorial
//! project's `edit_fingerprint` is a content hash of its render-affecting
//! graph, used to tell an already-rendered product apart from one whose
//! source has since changed (A16). `assets_referenced` is the shared
//! reachability computation a later package collector needs to bundle
//! every asset a HISTORICAL product snapshot still depends on, even after
//! the live project has stopped using it (A17).

use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{Asset, Map, Product, Project, RenderRange};

/// Serializes `p` to a compact JSON string with every object's keys
/// sorted, recursively, at every nesting level -- so the result depends
/// only on the project's *content*, never on struct field-declaration
/// order or on which collection type happened to hand us the entries.
/// Numbers serialize exactly as `serde_json::Number` prints them (R3's
/// int-vs-float literal fidelity; this function does not renormalize a
/// numeric literal).
///
/// The recursive sort deliberately bounces every object's entries through
/// a plain `HashMap` before sorting, rather than trusting that whatever
/// map handed them to us already iterates in key order. `serde_json::Map`
/// happens to be a `BTreeMap` (already sorted) in this workspace today
/// -- `preserve_order` is not enabled for `vault_buddy_core`'s `serde_json`
/// anywhere in the dependency graph (`cargo tree -e features -i
/// serde_json` on this crate shows only `default`/`std`) -- so an
/// implementation that skipped the explicit sort and just walked
/// `Map::iter()` would already *look* correct today, right up until some
/// other crate's `serde_json` requirement got unified with this one's and
/// `Map` silently became an insertion-order `IndexMap`. Routing through a
/// `HashMap` (whose default hasher is randomly seeded per instance) means
/// the `sort_by` below is load-bearing *now*, not just in that
/// hypothetical future: drop it and `fingerprint_ignores_key_order` fails
/// today, because two independently-built `HashMap`s over the same keys
/// essentially never iterate in the same order. A canonicalizer whose own
/// test cannot fail without an upstream feature flip was not actually
/// testing the sort.
pub fn canonical_json(p: &Project) -> String {
    let value = serde_json::to_value(p).expect("a Project always serializes to JSON");
    let mut out = String::new();
    write_canonical(&value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            out.push_str(&serde_json::to_string(s).expect("a string always serializes"));
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            // See `canonical_json`'s doc: the HashMap bounce is what makes
            // the `sort_by` below observably necessary rather than a
            // silent no-op over an already-sorted BTreeMap.
            let mut entries: Vec<(&str, &Value)> = map
                .iter()
                .map(|(k, v)| (k.as_str(), v))
                .collect::<HashMap<_, _>>()
                .into_iter()
                .collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (k, v)) in entries.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).expect("a key always serializes"));
                out.push(':');
                write_canonical(v, out);
            }
            out.push('}');
        }
    }
}

/// `"sha256:" + hex(sha256(canonical_json(p)))` -- 71 characters, inside
/// the interchange schema's `edit_fingerprint` `maxLength: 80`.
pub fn edit_fingerprint(p: &Project) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(p).as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        write!(hex, "{byte:02x}").expect("writing to a String is infallible");
    }
    format!("sha256:{hex}")
}

/// Builds an immutable rendered product at `revision`, embedding an
/// independent snapshot of `project` (`DATA-MODEL.md` § Persistence and
/// source identity: "A product's missing encoded binary does not erase
/// its lineage") and this project's current `edit_fingerprint` (A16: a
/// later restore compares this against the live project's fingerprint to
/// tell whether the source has since changed). The snapshot is a full
/// clone taken now -- a later edit to `project` (undo included, F-13) can
/// never reach back into it.
#[allow(
    clippy::too_many_arguments,
    reason = "signature is the task brief's contract verbatim (P01 task 5); a parameter struct would just move the same eight fields, not reduce them"
)]
pub fn new_product(
    project: &Project,
    revision: u64,
    product_id: &str,
    name: &str,
    filename: &str,
    duration_ms: u64,
    range: Option<RenderRange>,
    created_at: &str,
) -> Product {
    Product {
        id: product_id.to_string(),
        project_id: project.id.clone(),
        name: name.to_string(),
        filename: filename.to_string(),
        mime: "video/mp4".to_string(),
        revision,
        duration_ms,
        created_at: created_at.to_string(),
        edit_fingerprint: edit_fingerprint(project),
        snapshot: Some(Box::new(project.clone())),
        render_range: range,
        extra: Map::new(),
    }
}

/// The one file name a product may have under its project's `products\`
/// directory (Task 46's controller ruling): exactly `<productId>.mp4`. The
/// ledger records it in `Product::filename`, and every reader (the media
/// resolver, a package import) refuses any other value -- so a hand-edited
/// or foreign record can never point a lookup at some other file.
pub fn product_file_name(product_id: &str) -> String {
    format!("{product_id}.mp4")
}

/// Does `product` carry a valid id AND exactly `product_file_name(id)`?
pub fn has_canonical_file_name(product: &Product) -> bool {
    super::is_valid_id(&product.id) && product.filename == product_file_name(&product.id)
}

/// The union of every asset id the CURRENT project graph depends on and
/// every asset id each `products` entry's own frozen snapshot depended on
/// (A17: "package collector includes snapshot-only assets" -- an asset a
/// user has since deleted from the live project must still ship with a
/// package built around an old rendered product that used it). Within
/// each graph (the live one, or a snapshot), an asset counts as
/// referenced when some clip's `asset_id` names it directly, OR when it
/// is reachable by following `linked_asset` from an asset that does
/// (detached audio, `DATA-MODEL.md` § Entities) -- never merely by
/// appearing in that project's `assets` list unused.
pub fn assets_referenced(project: &Project, products: &[Product]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_referenced(project, &mut out);
    for product in products {
        if let Some(snapshot) = &product.snapshot {
            collect_referenced(snapshot, &mut out);
        }
    }
    out
}

fn collect_referenced(project: &Project, out: &mut BTreeSet<String>) {
    let by_id: HashMap<&str, &Asset> = project.assets.iter().map(|a| (a.id.as_str(), a)).collect();

    let mut seen: BTreeSet<String> = project.clips.iter().map(|c| c.asset_id.clone()).collect();
    let mut frontier: Vec<String> = seen.iter().cloned().collect();

    // Cycle-safe by construction (`seen.insert` only re-queues a truly new
    // id) even though `validate_project` already rejects a cyclic
    // `linked_asset` graph -- this is a standalone pure function that a
    // package collector may run over data nothing here re-validates.
    while let Some(id) = frontier.pop() {
        if let Some(asset) = by_id.get(id.as_str()) {
            if let Some(linked) = &asset.linked_asset {
                if seen.insert(linked.clone()) {
                    frontier.push(linked.clone());
                }
            }
        }
    }

    out.extend(seen);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::test_support;
    use crate::editor::{AssetKind, Clip, FadeCurve};

    fn asset_with_link(id: &str, linked_asset: Option<&str>) -> Asset {
        Asset {
            id: id.to_string(),
            kind: AssetKind::Video,
            name: id.to_string(),
            duration_ms: 10_000,
            width: None,
            height: None,
            size: None,
            builtin: None,
            media_type: None,
            linked_asset: linked_asset.map(str::to_string),
            original_name: None,
            extra: Map::new(),
        }
    }

    fn clip_on_asset(id: &str, asset_id: &str) -> Clip {
        Clip {
            id: id.to_string(),
            asset_id: asset_id.to_string(),
            track_id: "t1".to_string(),
            name: id.to_string(),
            start_ms: 0,
            in_ms: 0,
            out_ms: 1_000,
            fade_in_ms: 0,
            fade_out_ms: 0,
            fade_curve: FadeCurve::Linear,
            opacity: serde_json::Number::from(1),
            volume: serde_json::Number::from(1),
            muted: false,
            x: serde_json::Number::from(0),
            y: serde_json::Number::from(0),
            w: serde_json::Number::from(1),
            h: serde_json::Number::from(1),
            speed: None,
            rotation: None,
            frame_shape: None,
            fit: None,
            mirror: None,
            flip_y: None,
            preserve_pitch: None,
            group_id: None,
            crop_zoom: None,
            crop_x: None,
            crop_y: None,
            adjustments: None,
            card: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn fingerprint_ignores_key_order() {
        // Two JSON texts describing the exact same document, with every
        // object's keys -- top-level, nested (`canvas`/`destination`), and
        // inside the `#[serde(flatten)]`ed extra map -- written in a
        // different order. Without the explicit sort in `canonical_json`,
        // these would have no reason to fingerprint the same.
        let schema = super::super::PROJECT_SCHEMA;
        let ordered = format!(
            r#"{{"schema":"{schema}","id":"p1","title":"t","canvas":{{"width":1280,"height":720,"fps":30}},"master_gain":1.0,"assets":[],"tracks":[],"clips":[],"effects":[],"markers":[],"transitions":[],"destination":{{"vault":"","folder":"","dated":false}},"alpha":1,"mid":2,"zeta":3}}"#
        );
        let shuffled = format!(
            r#"{{"zeta":3,"destination":{{"dated":false,"vault":"","folder":""}},"mid":2,"transitions":[],"markers":[],"effects":[],"clips":[],"tracks":[],"assets":[],"master_gain":1.0,"canvas":{{"fps":30,"height":720,"width":1280}},"title":"t","id":"p1","schema":"{schema}","alpha":1}}"#
        );

        let project_a: Project = serde_json::from_str(&ordered).expect("ordered JSON parses");
        let project_b: Project = serde_json::from_str(&shuffled).expect("shuffled JSON parses");

        assert_eq!(
            edit_fingerprint(&project_a),
            edit_fingerprint(&project_b),
            "key order must never affect the fingerprint"
        );
    }

    #[test]
    fn fingerprint_changes_when_a_clip_moves() {
        let mut project = test_support::minimal_project();
        project.assets.push(asset_with_link("a1", None));
        project.clips.push(clip_on_asset("c1", "a1"));

        let before = edit_fingerprint(&project);
        project.clips[0].start_ms += 1;
        let after = edit_fingerprint(&project);

        assert_ne!(before, after, "moving a clip must change the fingerprint");
    }

    #[test]
    fn fingerprint_has_the_schema_length() {
        let project = test_support::minimal_project();
        let fingerprint = edit_fingerprint(&project);

        assert!(
            fingerprint.starts_with("sha256:"),
            "fingerprint: {fingerprint}"
        );
        assert_eq!(fingerprint.len(), 71, "fingerprint: {fingerprint}");
        assert!(
            fingerprint.len() <= 80,
            "must fit the schema's edit_fingerprint maxLength: {fingerprint}"
        );
    }

    #[test]
    fn product_snapshot_is_independent_of_later_edits() {
        let mut project = test_support::minimal_project();
        project.title = "Before".to_string();

        let product = new_product(
            &project,
            1,
            "prod-1",
            "Name",
            "file.mp4",
            5_000,
            None,
            "2026-09-21T00:00:00Z",
        );

        project.title = "After".to_string();

        let snapshot = product
            .snapshot
            .as_ref()
            .expect("new_product always embeds a snapshot");
        assert_eq!(snapshot.title, "Before");
        assert_ne!(
            snapshot.title, project.title,
            "the snapshot must not see a later edit to the live project"
        );
        assert_eq!(product.mime, "video/mp4");
        assert_eq!(product.project_id, "project");
    }

    #[test]
    fn assets_referenced_includes_snapshot_only_assets() {
        let mut project = test_support::minimal_project();
        project.assets.push(asset_with_link("a-old", None));
        project.clips.push(clip_on_asset("c-old", "a-old"));

        let product = new_product(
            &project,
            1,
            "prod-1",
            "Name",
            "file.mp4",
            5_000,
            None,
            "2026-09-21T00:00:00Z",
        );

        // The live project moves on: the old asset and its clip are gone.
        project.assets.clear();
        project.clips.clear();

        let referenced = assets_referenced(&project, std::slice::from_ref(&product));
        assert!(
            referenced.contains("a-old"),
            "an asset used only by a product's frozen snapshot must still be referenced: {referenced:?}"
        );
    }

    #[test]
    fn assets_referenced_follows_linked_assets() {
        let mut project = test_support::minimal_project();
        project
            .assets
            .push(asset_with_link("video-1", Some("audio-1")));
        project.assets.push(asset_with_link("audio-1", None));
        project.clips.push(clip_on_asset("c1", "video-1"));

        let referenced = assets_referenced(&project, &[]);

        assert!(referenced.contains("video-1"));
        assert!(
            referenced.contains("audio-1"),
            "linked_asset must be followed even though no clip references it directly: {referenced:?}"
        );
    }
    // Task 46: a product's file is `products\<productId>.mp4` and nothing
    // else -- a record naming another file, or carrying an id that could
    // not be a file name at all, is not canonical.
    #[test]
    fn only_product_id_dot_mp4_is_a_canonical_product_file_name() {
        let project = test_support::minimal_project();
        let good = new_product(
            &project,
            2,
            "prod-a1",
            "Demo",
            "prod-a1.mp4",
            1_000,
            None,
            "t",
        );
        assert_eq!(product_file_name("prod-a1"), "prod-a1.mp4");
        assert!(has_canonical_file_name(&good));
        for (id, filename) in [
            ("prod-a1", "prod-b2.mp4"),
            ("prod-a1", "prod-a1.MP4"),
            ("prod-a1", "../prod-a1.mp4"),
            ("prod a1", "prod a1.mp4"),
        ] {
            let mut bad = good.clone();
            bad.id = id.to_string();
            bad.filename = filename.to_string();
            assert!(!has_canonical_file_name(&bad), "{id} / {filename}");
        }
    }
}
