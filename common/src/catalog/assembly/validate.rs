use super::*;
use std::collections::BTreeSet;

fn invalid(file: &str, field: &str, message: &str) -> Result<(), CatalogError> {
    Err(error(file, field, message))
}

fn name(file: &str, field: &str, value: &str) -> Result<(), CatalogError> {
    if value.is_empty() || value.len() > 64 || !value.is_ascii() {
        return invalid(
            file,
            field,
            "expected nonempty ASCII name of at most 64 bytes",
        );
    }
    Ok(())
}

fn pose(file: &str, field: &str, value: RigidPose) -> Result<(), CatalogError> {
    let rotation = Quat::from_array(value.rotation);
    if value
        .translation
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10.0)
        || !rotation.is_finite()
        || (rotation.length_squared() - 1.0).abs() > 0.0001
    {
        return invalid(
            file,
            field,
            "expected translation within 10m and unit quaternion",
        );
    }
    Ok(())
}

fn interface(file: &str, field: &str, value: &Interface) -> Result<(), CatalogError> {
    name(file, field, &value.standard)?;
    if value.size == 0 {
        return invalid(file, field, "interface size code must be nonzero");
    }
    Ok(())
}

fn geometry(file: &str, field: &str, value: &Geometry) -> Result<(), CatalogError> {
    if value.sockets.len() > MAX_SOCKETS
        || value.solids.len() > MAX_VOLUMES
        || value.clearance.len() > MAX_VOLUMES
        || value.anchors.len() > 10
    {
        return invalid(file, field, "geometry exceeds socket/volume/anchor limits");
    }
    if value
        .center_of_mass
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10.0)
    {
        return invalid(file, field, "invalid center of mass");
    }
    let mut names = BTreeSet::new();
    for (i, socket) in value.sockets.iter().enumerate() {
        let field = format!("{field}.sockets[{i}]");
        name(file, &field, &socket.key)?;
        interface(file, &field, &socket.interface)?;
        pose(file, &field, socket.pose)?;
        if !names.insert(&socket.key)
            || socket.accepts.is_empty()
            || socket.accepts.len() > 12
            || socket.positions == 0
            || socket.positions > 256
            || !socket.pitch_m.is_finite()
            || (socket.interface.kind == InterfaceKind::Rail
                && !(0.001..=0.1).contains(&socket.pitch_m))
            || (socket.interface.kind != InterfaceKind::Rail
                && (socket.positions != 1 || socket.pitch_m != 0.0))
        {
            return invalid(file, &field, "invalid or duplicate socket/rail definition");
        }
    }
    for (i, bounds) in value.solids.iter().chain(&value.clearance).enumerate() {
        if (0..3).any(|axis| {
            !bounds.min[axis].is_finite()
                || !bounds.max[axis].is_finite()
                || bounds.min[axis] >= bounds.max[axis]
                || bounds.min[axis].abs() > 10.0
                || bounds.max[axis].abs() > 10.0
        }) {
            return invalid(file, &format!("{field}.volumes[{i}]"), "invalid bounds");
        }
    }
    let mut anchors = BTreeSet::new();
    for (i, anchor) in value.anchors.iter().enumerate() {
        let field = format!("{field}.anchors[{i}]");
        pose(file, &field, anchor.pose)?;
        if !anchors.insert(anchor.kind) {
            return invalid(file, &field, "duplicate anchor kind");
        }
    }
    Ok(())
}

pub(in crate::catalog) fn document(
    file: &str,
    document: &CatalogDocument,
) -> Result<(), CatalogError> {
    for (i, weapon) in document.weapons.iter().enumerate() {
        let field = format!("weapons[{i}].assembly");
        let root = &weapon.assembly;
        name(file, &field, &root.platform)?;
        name(file, &field, &root.caliber)?;
        if !root.mass_kg.is_finite() || !(0.01..=30.0).contains(&root.mass_kg) {
            return invalid(file, &field, "root mass outside 0.01..=30 kg");
        }
        geometry(file, &field, &root.geometry)?;
        pose(file, &format!("{field}.hip_from_root"), root.hip_from_root)?;
        pose(file, &format!("{field}.ads_from_root"), root.ads_from_root)?;
    }
    for (i, component) in document.components.iter().enumerate() {
        let field = format!("components[{i}].assembly");
        let part = &component.assembly;
        interface(file, &field, &part.interface)?;
        for value in [&part.platform, &part.caliber].into_iter().flatten() {
            name(file, &field, value)?;
        }
        if component.mass_kg > 30.0
            || part.occupied_positions == 0
            || part.occupied_positions > 256
            || (part.interface.kind != InterfaceKind::Rail && part.occupied_positions != 1)
        {
            return invalid(file, &field, "invalid component mass/occupied positions");
        }
        geometry(file, &field, &part.geometry)?;
        for value in [
            part.modifiers.recoil,
            part.modifiers.velocity,
            part.modifiers.signature,
        ] {
            if !value.is_finite() || !(0.1..=4.0).contains(&value) {
                return invalid(file, &field, "modifier outside 0.1..=4");
            }
        }
        for (label, refs) in [("requires", &part.requires), ("excludes", &part.excludes)] {
            let mut seen = BTreeSet::new();
            if refs.len() > MAX_COMPONENTS {
                return invalid(file, &field, "too many component constraints");
            }
            for id in refs {
                if *id == component.id
                    || !seen.insert(id)
                    || !document.components.iter().any(|c| c.id == *id)
                {
                    return invalid(
                        file,
                        &format!("{field}.{label}"),
                        "self, duplicate or unknown component constraint",
                    );
                }
            }
        }
        if part.requires.iter().any(|id| part.excludes.contains(id)) {
            return invalid(file, &field, "required component is also excluded");
        }
    }
    // Dependency cycles are content errors, even in parts not equipped by defaults.
    fn visit(
        id: DefinitionId,
        document: &CatalogDocument,
        stack: &mut BTreeSet<DefinitionId>,
        done: &mut BTreeSet<DefinitionId>,
    ) -> bool {
        if done.contains(&id) {
            return true;
        }
        if stack.len() >= MAX_COMPONENTS || !stack.insert(id) {
            return false;
        }
        let part = document.components.iter().find(|c| c.id == id).unwrap();
        if !part
            .assembly
            .requires
            .iter()
            .all(|id| visit(*id, document, stack, done))
        {
            return false;
        }
        stack.remove(&id);
        done.insert(id);
        true
    }
    let mut done = BTreeSet::new();
    for part in &document.components {
        if !visit(part.id, document, &mut BTreeSet::new(), &mut done) {
            return invalid(
                file,
                "components.requires",
                "dependency cycle or chain exceeding component limit",
            );
        }
    }
    Ok(())
}
