//! Mesh-free rig data generated from the same source as the client's baked parts.
//! Resolve authoring references while loading the catalogue; simulation only sees
//! ordinary validated geometry, with no filesystem access or renderer dependency.
use super::*;

pub const LOGICAL_RIGS_SOURCE: &str = include_str!("../../../../assets/baked/logical_rigs.ron");

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoredRig {
    pub pivots: BTreeMap<String, RigidPose>,
    pub sockets: BTreeMap<String, RigidPose>,
}

pub fn authored_rig(asset: &str) -> Option<&'static AuthoredRig> {
    static RIGS: OnceLock<BTreeMap<String, AuthoredRig>> = OnceLock::new();
    RIGS.get_or_init(|| ron::from_str(LOGICAL_RIGS_SOURCE).expect("valid generated logical rigs"))
        .get(asset)
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct GeometrySource {
    center_of_mass: [f32; 3],
    sockets: Vec<Socket>,
    solids: Vec<Bounds>,
    clearance: Vec<Bounds>,
    anchors: Vec<Anchor>,
    authored: Option<Bindings>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Bindings {
    asset: String,
    anchors: Vec<(AnchorKind, String)>,
    sockets: Vec<(String, String)>,
}

impl TryFrom<GeometrySource> for Geometry {
    type Error = String;

    fn try_from(source: GeometrySource) -> Result<Self, Self::Error> {
        let mut geometry = Self {
            authored_rig: None,
            center_of_mass: source.center_of_mass,
            sockets: source.sockets,
            solids: source.solids,
            clearance: source.clearance,
            anchors: source.anchors,
        };
        if let Some(bindings) = source.authored {
            geometry.authored_rig = Some(bindings.asset.clone());
            let rig = authored_rig(&bindings.asset)
                .ok_or_else(|| format!("unknown authored rig {}", bindings.asset))?;
            let socket_pose = |name: &str| {
                rig.sockets
                    .get(name)
                    .copied()
                    .ok_or_else(|| format!("authored rig {} has no socket {name}", bindings.asset))
            };
            for (kind, name) in bindings.anchors {
                if geometry.anchors.iter().any(|a| a.kind == kind) {
                    return Err(format!("duplicate authored {kind:?} anchor"));
                }
                geometry.anchors.push(Anchor {
                    kind,
                    pose: socket_pose(&name)?,
                });
            }
            let mut seen = std::collections::BTreeSet::new();
            for (key, name) in bindings.sockets {
                if !seen.insert(key.clone()) {
                    return Err(format!("duplicate authored socket binding {key}"));
                }
                let socket = geometry
                    .sockets
                    .iter_mut()
                    .find(|s| s.key == key)
                    .ok_or_else(|| format!("authored binding has no assembly socket {key}"))?;
                if socket.pose != RigidPose::default() {
                    return Err(format!("authored socket {key} also has an explicit pose"));
                }
                socket.pose = socket_pose(&name)?;
            }
        }
        Ok(geometry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"(
        sockets: [(key: "magazine", interface: (kind: Magwell, standard: "stanag", size: 556),
            accepts: [Magazine], positions: 1, pitch_m: 0.0, required: true)],
        authored: Some((asset: "m4a1_refined",
            anchors: [(Bore, "bore.muzzle"), (FrontIron, "sight.front"),
                (RearIron, "sight.rear"), (TriggerGrip, "grip.trigger")],
            sockets: [("magazine", "magazine.seat")])),
    )"#;

    #[test]
    fn authored_geometry_preserves_dimensions_axes_and_catalogue_socket_contract() {
        let geometry: Geometry = ron::from_str(SOURCE).unwrap();
        let anchor = |kind| geometry.anchors.iter().find(|a| a.kind == kind).unwrap();
        assert!((anchor(AnchorKind::Bore).pose.translation[2] + 14.5 * 0.0254).abs() < 1e-6);
        for kind in [AnchorKind::FrontIron, AnchorKind::RearIron] {
            let pose = anchor(kind).pose.transform();
            assert!(pose.translation.x.abs() < 0.00025);
            assert!((pose.translation.y - 0.066).abs() < 0.00025);
            assert!((pose.rotation * Vec3::NEG_Z - Vec3::NEG_Z).length() < 1e-6);
        }
        // Authored 25-degree grip tilt survives hierarchical export.
        let grip = anchor(AnchorKind::TriggerGrip).pose.transform();
        assert!((grip.rotation * Vec3::Y - Vec3::new(0.0, 0.9063078, -0.4226183)).length() < 1e-5);
        let socket = &geometry.sockets[0];
        assert_eq!(socket.key, "magazine");
        assert_eq!(socket.accepts, [ComponentKind::Magazine]);
        assert_eq!(socket.interface.kind, InterfaceKind::Magwell);
        assert_eq!(socket.pose.translation, [0.0, -0.065, 0.029]);
    }

    #[test]
    fn authored_geometry_rejects_missing_ambiguous_or_duplicated_offsets() {
        for (source, expected) in [
            (SOURCE.replace("m4a1_refined", "missing"), "unknown authored rig"),
            (SOURCE.replace("bore.muzzle", "missing"), "has no socket missing"),
            (SOURCE.replace("(FrontIron,", "(Bore,"), "duplicate authored Bore"),
            (SOURCE.replace("(\"magazine\",", "(\"missing\","), "no assembly socket missing"),
            (
                SOURCE.replace("required: true", "required: true, pose: (translation: (0.1, 0.0, 0.0), rotation: (0.0, 0.0, 0.0, 1.0))"),
                "also has an explicit pose",
            ),
        ] {
            let error = ron::from_str::<Geometry>(&source).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }
}
