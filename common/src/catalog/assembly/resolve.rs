use super::*;
use std::collections::{BTreeMap, BTreeSet};

type Occupancy<'a> = BTreeMap<(Option<u16>, &'a str), Vec<(u16, u16)>>;

impl Catalog {
    pub(in crate::catalog) fn resolve_assembly(
        &self,
        weapon: &WeaponDefinition,
        attachments: &[Attachment],
        field: &str,
    ) -> Result<(ResolvedAssembly, u32), CatalogError> {
        let fail = |message: String| error(&self.file, format!("{field}.components"), message);
        if attachments.is_empty() || attachments.len() > MAX_COMPONENTS {
            return Err(fail(format!("expected 1..={MAX_COMPONENTS} components")));
        }
        let mut nodes = BTreeMap::new();
        for (i, attachment) in attachments.iter().enumerate() {
            if attachment.node == 0 || nodes.insert(attachment.node, attachment).is_some() {
                return Err(fail(format!("duplicate/reserved node {}", attachment.node)));
            }
            if !self.components.contains_key(&attachment.definition) {
                return Err(error(
                    &self.file,
                    format!("{field}.components[{i}]"),
                    "unknown component ID",
                ));
            }
            if attachment.socket.is_empty() || attachment.socket.len() > 64 {
                return Err(fail(format!(
                    "node {} has invalid socket name",
                    attachment.node
                )));
            }
        }
        // Walk bounded parent chains before composing anything. This rejects
        // orphan/cyclic/disconnected subgraphs independently of input ordering.
        let mut depths = BTreeMap::new();
        for node in nodes.values() {
            let mut visited = BTreeSet::new();
            let mut cursor = Some(node.node);
            while let Some(id) = cursor {
                if !visited.insert(id) {
                    return Err(fail(format!("cycle at node {id}")));
                }
                if visited.len() > MAX_DEPTH {
                    return Err(fail(format!(
                        "node {} exceeds depth {MAX_DEPTH}",
                        node.node
                    )));
                }
                cursor = nodes
                    .get(&id)
                    .ok_or_else(|| fail(format!("missing parent node {id}")))?
                    .parent;
            }
            depths.insert(node.node, visited.len());
        }
        let mut ordered: Vec<_> = nodes.values().copied().collect();
        ordered.sort_by_key(|node| (depths[&node.node], node.node));
        let root = &weapon.assembly;
        let mut poses = BTreeMap::new();
        let mut occupied = Occupancy::new();
        let installed: BTreeSet<_> = nodes.values().map(|n| n.definition).collect();
        let mut capacity = None;
        for node in &ordered {
            let component = &self.components[&node.definition];
            let part = &component.assembly;
            let (geometry, parent_pose) = if let Some(parent) = node.parent {
                (
                    &self.components[&nodes[&parent].definition]
                        .assembly
                        .geometry,
                    poses[&parent],
                )
            } else {
                (&root.geometry, Transform::IDENTITY)
            };
            let socket = geometry
                .sockets
                .iter()
                .find(|s| s.key == node.socket)
                .ok_or_else(|| {
                    fail(format!(
                        "node {}: missing socket {}",
                        node.node, node.socket
                    ))
                })?;
            if socket.interface != part.interface || !socket.accepts.contains(&component.kind) {
                return Err(error(
                    &self.file,
                    format!(
                        "{field}.components[{}]",
                        attachments
                            .iter()
                            .position(|n| n.node == node.node)
                            .unwrap()
                    ),
                    format!(
                        "node {}: incompatible interface/kind at {}",
                        node.node, node.socket
                    ),
                ));
            }
            if part.platform.as_ref().is_some_and(|s| *s != root.platform)
                || part.caliber.as_ref().is_some_and(|s| *s != root.caliber)
            {
                return Err(fail(format!(
                    "node {}: incompatible platform/caliber",
                    node.node
                )));
            }
            let end = u32::from(node.position) + u32::from(part.occupied_positions);
            if end > u32::from(socket.positions) {
                return Err(fail(format!(
                    "node {} exceeds socket/rail length",
                    node.node
                )));
            }
            let intervals = occupied.entry((node.parent, &node.socket)).or_default();
            if intervals
                .iter()
                .any(|&(start, stop)| node.position < stop && start < end as u16)
            {
                return Err(fail(format!(
                    "node {}: occupied socket/rail overlap",
                    node.node
                )));
            }
            intervals.push((node.position, end as u16));
            if let Some(id) = part.requires.iter().find(|id| !installed.contains(id)) {
                return Err(fail(format!(
                    "node {} requires component {}",
                    node.node, id.0
                )));
            }
            if let Some(id) = part.excludes.iter().find(|id| installed.contains(id)) {
                return Err(fail(format!(
                    "node {} excludes component {}",
                    node.node, id.0
                )));
            }
            if component.kind == ComponentKind::Magazine {
                if component.ammo != weapon.ammo {
                    return Err(error(
                        &self.file,
                        format!(
                            "{field}.components[{}]",
                            attachments
                                .iter()
                                .position(|n| n.node == node.node)
                                .unwrap()
                        ),
                        "magazine ammo is incompatible with weapon",
                    ));
                }
                if capacity.replace(component.capacity).is_some() {
                    return Err(fail("multiple magazines installed".into()));
                }
            }
            let pose = parent_pose
                * socket.pose.transform()
                * Transform::from_translation(Vec3::Z * socket.pitch_m * f32::from(node.position));
            poses.insert(node.node, pose);
        }
        for (parent, geometry) in
            std::iter::once((None, &root.geometry)).chain(nodes.values().map(|n| {
                (
                    Some(n.node),
                    &self.components[&n.definition].assembly.geometry,
                )
            }))
        {
            for socket in &geometry.sockets {
                if socket.required && !occupied.contains_key(&(parent, socket.key.as_str())) {
                    return Err(fail(format!(
                        "required socket {} on {parent:?} is empty",
                        socket.key
                    )));
                }
            }
        }
        let capacity = capacity.ok_or_else(|| fail("missing magazine".into()))?;

        // Canonical node order, f64 accumulators: permutation of serialized
        // entries cannot change reductions or rounding. Root anchors are fallback;
        // descendants refine their ancestors (grip on handguard, can on mount).
        // Unrelated branches cannot compete for the same semantic anchor.
        let mut mass = f64::from(root.mass_kg);
        let mut moment = root.geometry.center_of_mass.map(|v| f64::from(v) * mass);
        let mut factors = [1.0_f64; 3];
        let mut anchors: BTreeMap<_, _> = root
            .geometry
            .anchors
            .iter()
            .map(|a| (a.kind, a.pose.transform()))
            .collect();
        let mut provided = BTreeMap::new();
        // Parent chains were already bounded and validated above.
        let is_ancestor = |ancestor, mut child| {
            while let Some(parent) = nodes[&child].parent {
                if parent == ancestor {
                    return true;
                }
                child = parent;
            }
            false
        };
        let mut solids: Vec<(u16, Bounds)> = root.geometry.solids.iter().map(|b| (0, *b)).collect();
        let mut clearance: Vec<(u16, Bounds)> =
            root.geometry.clearance.iter().map(|b| (0, *b)).collect();
        let mut parts = Vec::new();
        for node in nodes.values() {
            let component = &self.components[&node.definition];
            let geometry = &component.assembly.geometry;
            let pose = poses[&node.node];
            let m = f64::from(component.mass_kg);
            mass += m;
            let com = pose
                .transform_point(Vec3::from(geometry.center_of_mass))
                .to_array();
            for axis in 0..3 {
                moment[axis] += f64::from(com[axis]) * m;
            }
            let modifiers = component.assembly.modifiers;
            for (value, factor) in
                factors
                    .iter_mut()
                    .zip([modifiers.recoil, modifiers.velocity, modifiers.signature])
            {
                *value *= f64::from(factor);
            }
            for anchor in &geometry.anchors {
                if let Some(&previous) = provided.get(&anchor.kind) {
                    if is_ancestor(node.node, previous) {
                        continue;
                    }
                    if !is_ancestor(previous, node.node) {
                        return Err(fail(format!("ambiguous {:?} anchor", anchor.kind)));
                    }
                }
                provided.insert(anchor.kind, node.node);
                anchors.insert(anchor.kind, pose * anchor.pose.transform());
            }
            solids.extend(
                geometry
                    .solids
                    .iter()
                    .map(|b| (node.node, b.transformed(pose))),
            );
            clearance.extend(
                geometry
                    .clearance
                    .iter()
                    .map(|b| (node.node, b.transformed(pose))),
            );
            parts.push(ResolvedPart {
                node: node.node,
                definition: node.definition,
                kind: component.kind,
                pose,
            });
        }
        if mass > 50.0
            || factors
                .iter()
                .any(|v| !v.is_finite() || !(0.05..=8.0).contains(v))
        {
            return Err(fail("resolved mass/modifier exceeds limits".into()));
        }
        // Once per accepted revision, with bounded part/volume counts. AABB of
        // rotated local boxes deliberately errs on the conservative side.
        for (i, &(owner, bounds)) in solids.iter().enumerate() {
            if let Some((other, _)) = solids[i + 1..]
                .iter()
                .find(|(other, b)| *other != owner && bounds.overlaps(*b))
            {
                return Err(fail(format!(
                    "solid conflict between nodes {owner} and {other}"
                )));
            }
            if let Some((other, _)) = clearance
                .iter()
                .find(|(other, b)| *other != owner && bounds.overlaps(*b))
            {
                return Err(fail(format!(
                    "clearance conflict between nodes {owner} and {other}"
                )));
            }
        }
        let center_of_mass = moment.map(|v| (v / mass) as f32);
        let balance = moment.iter().map(|v| v * v).sum::<f64>().sqrt();
        if center_of_mass
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 10.0)
            || balance > 500.0
        {
            return Err(fail(
                "resolved center of mass/balance exceeds limits".into(),
            ));
        }
        let ads_from_root = if root.ads_at_sight {
            if root.ads_from_root != RigidPose::default() {
                return Err(fail("ADS eye binding also has an explicit pose".into()));
            }
            let eye = anchors
                .get(&AnchorKind::Sight)
                .ok_or_else(|| fail("ADS eye binding requires Sight anchor".into()))?;
            Transform::from_matrix(eye.to_matrix().inverse())
        } else {
            root.ads_from_root.transform()
        };
        let pivots = if let Some(name) = &root.geometry.authored_rig {
            authored_rig(name)
                .ok_or_else(|| fail(format!("unknown authored rig {name}")))?
                .pivots
                .iter()
                .map(|(name, pose)| (name.clone(), pose.transform()))
                .collect()
        } else {
            BTreeMap::new()
        };
        Ok((
            ResolvedAssembly {
                parts,
                mass_kg: mass as f32,
                center_of_mass,
                balance_kg_m: balance as f32,
                recoil_multiplier: factors[0] as f32,
                velocity_multiplier: factors[1] as f32,
                signature_multiplier: factors[2] as f32,
                anchors,
                obstruction: solids.into_iter().map(|(_, b)| b).collect(),
                hip_from_root: root.hip_from_root.transform(),
                ads_from_root,
                pivots,
            },
            capacity,
        ))
    }
}
