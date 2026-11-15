use super::*;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin))
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_plugins(Plugin);
    app
}

fn spawn(app: &mut App, view_model: bool) -> Entity {
    app.world_mut()
        .spawn((
            VoxelModel {
                asset: "pipeline_fixture".into(),
                view_model,
            },
            Transform::default(),
            Visibility::Inherited,
        ))
        .id()
}

fn descendants(world: &World, entity: Entity) -> Vec<Entity> {
    let mut result = Vec::new();
    if let Some(children) = world.get::<Children>(entity) {
        for child in children.iter() {
            result.push(child);
            result.extend(descendants(world, child));
        }
    }
    result
}

#[test]
fn voxel_instances_share_surface_handles_across_actors_and_perspectives() {
    let mut app = app();
    let roots = [
        spawn(&mut app, false),
        spawn(&mut app, false),
        spawn(&mut app, true),
    ];
    app.update();
    let mut sets = Vec::new();
    for (index, root) in roots.into_iter().enumerate() {
        let entities = descendants(app.world(), root);
        assert_eq!(
            entities.len(),
            13,
            "container +3 pivots +7 surfaces +2 sockets"
        );
        let surfaces: Vec<_> = entities
            .iter()
            .filter_map(|entity| {
                let mesh = app.world().get::<Mesh3d>(*entity)?;
                let material = app
                    .world()
                    .get::<MeshMaterial3d<StandardMaterial>>(*entity)
                    .unwrap();
                let layers = app.world().get::<RenderLayers>(*entity).unwrap();
                assert!(layers.intersects(&RenderLayers::layer(if index == 2 {
                    VIEW_MODEL_RENDER_LAYER
                } else {
                    0
                })));
                assert_eq!(
                    app.world().get::<NotShadowCaster>(*entity).is_some(),
                    index == 2
                );
                Some((mesh.0.id(), material.0.id()))
            })
            .collect();
        assert_eq!(surfaces.len(), 7);
        sets.push(surfaces);
    }
    assert_eq!(sets[0], sets[1]);
    assert_eq!(
        sets[0], sets[2],
        "view and world use the same handles, not baked copies"
    );
    let count = app.world().entities().len();
    for _ in 0..60 {
        app.update();
    }
    assert_eq!(app.world().entities().len(), count);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 7);
    assert_eq!(app.world().resource::<Assets<StandardMaterial>>().len(), 4);
    assert_eq!(app.world().resource::<VoxelCache>().assets.len(), 1);
    // Even a Changed notification containing identical data is idempotent.
    let content = app.world().get::<VoxelInstance>(roots[0]).unwrap().content;
    app.world_mut()
        .get_mut::<VoxelModel>(roots[0])
        .unwrap()
        .asset = "pipeline_fixture".into();
    app.update();
    assert_eq!(
        app.world().get::<VoxelInstance>(roots[0]).unwrap().content,
        content
    );
}

#[test]
fn voxel_mechanisms_and_sockets_follow_pivots_without_rebuilding_meshes() {
    let mut app = app();
    let root = spawn(&mut app, false);
    app.update();
    let find = |app: &App, name: &str| {
        descendants(app.world(), root)
            .into_iter()
            .find(|entity| {
                app.world()
                    .get::<Name>(*entity)
                    .is_some_and(|n| n.as_str() == name)
            })
            .unwrap()
    };
    let magazine = find(&app, "magazine");
    let bolt = find(&app, "bolt");
    let receiver = find(&app, "receiver");
    let grasp = find(&app, "magazine.grasp");
    let control = find(&app, "control.bolt");
    assert!(app.world().get::<MovableVoxelPart>(magazine).is_some());
    assert!(app.world().get::<MovableVoxelPart>(bolt).is_some());
    assert!(app.world().get::<MovableVoxelPart>(receiver).is_none());
    let before = app
        .world()
        .get::<GlobalTransform>(grasp)
        .unwrap()
        .translation();
    assert!(before.distance(Vec3::new(0.0153, -0.0654, -0.0494)) < 1e-7);
    let bolt_before = *app.world().get::<GlobalTransform>(control).unwrap();
    let magazine_pose = *app.world().get::<Transform>(magazine).unwrap();
    let root_pose = Transform::from_xyz(2.0, 3.0, 4.0).with_rotation(Quat::from_rotation_y(0.7));
    *app.world_mut().get_mut::<Transform>(root).unwrap() = root_pose;
    app.world_mut()
        .get_mut::<Transform>(magazine)
        .unwrap()
        .translation
        .y -= 0.1;
    app.update();
    let expected = root_pose.transform_point(
        magazine_pose.transform_point(Vec3::new(0.0153, -0.0401, 0.0007)) - Vec3::Y * 0.1,
    );
    let actual = app
        .world()
        .get::<GlobalTransform>(grasp)
        .unwrap()
        .translation();
    assert!(actual.distance(expected) < 1e-6);
    assert!(
        app.world()
            .get::<GlobalTransform>(control)
            .unwrap()
            .translation()
            .distance(root_pose.transform_point(bolt_before.translation()))
            < 1e-6
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 7);
}

#[test]
fn voxel_replacement_and_removal_clean_owned_entities_and_reuse_cached_buffers() {
    let mut app = app();
    let invalid = app
        .world_mut()
        .spawn((
            VoxelModel {
                asset: "not-in-registry".into(),
                view_model: false,
            },
            Transform::default(),
            Visibility::Inherited,
        ))
        .id();
    app.update();
    assert!(app.world().get::<VoxelLoadError>(invalid).is_some());
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    app.world_mut().entity_mut(invalid).remove::<VoxelModel>();
    app.update();
    assert!(app.world().get::<VoxelLoadError>(invalid).is_none());
    let root = spawn(&mut app, false);
    app.update();
    let initial = descendants(app.world(), root);
    app.world_mut().get_mut::<VoxelModel>(root).unwrap().asset = "missing".into();
    app.update();
    assert!(
        app.world()
            .get::<VoxelLoadError>(root)
            .unwrap()
            .0
            .contains("missing")
    );
    assert_eq!(
        descendants(app.world(), root),
        initial,
        "invalid replacement remains atomic"
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 7);
    *app.world_mut().get_mut::<VoxelModel>(root).unwrap() = VoxelModel {
        asset: "pipeline_fixture".into(),
        view_model: true,
    };
    app.update();
    assert!(app.world().get::<VoxelLoadError>(root).is_none());
    for entity in &initial {
        assert!(app.world().get_entity(*entity).is_err());
    }
    assert_eq!(descendants(app.world(), root).len(), 13);
    app.world_mut().entity_mut(root).remove::<VoxelModel>();
    app.update();
    assert!(descendants(app.world(), root).is_empty());
    assert!(app.world().get::<VoxelInstance>(root).is_none());
    app.world_mut().entity_mut(root).insert(VoxelModel {
        asset: "pipeline_fixture".into(),
        view_model: false,
    });
    app.update();
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 7);
    let children = descendants(app.world(), root);
    app.world_mut().entity_mut(root).despawn();
    app.update();
    for entity in children {
        assert!(app.world().get_entity(entity).is_err());
    }
}

#[test]
fn voxel_material_properties_and_generated_buffers_match_the_bake() {
    let mut app = app();
    spawn(&mut app, false);
    app.update();
    let materials = app.world().resource::<Assets<StandardMaterial>>();
    let mut pbr: Vec<_> = materials
        .iter()
        .map(|(_, m)| (m.metallic, m.perceptual_roughness))
        .collect();
    pbr.sort_by(|a, b| a.1.total_cmp(&b.1));
    assert_eq!(pbr, [(0.9, 0.32), (0.0, 0.65), (0.0, 0.8), (0.0, 0.95)]);
    let meshes = app.world().resource::<Assets<Mesh>>();
    assert_eq!(
        meshes
            .iter()
            .map(|(_, m)| m.count_vertices())
            .sum::<usize>(),
        168
    );
    assert_eq!(
        meshes
            .iter()
            .map(|(_, m)| m.indices().unwrap().len() / 3)
            .sum::<usize>(),
        84
    );
}
