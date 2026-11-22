//! Render public loot without reading or reconstructing private inventory.
use std::collections::BTreeMap;

use bevy::prelude::*;
use common::{
    catalog::{
        ItemInstanceId,
        item_manifest::{S1_SEEDS, item_visuals},
    },
    replication::inventory::GroundItem,
};

use super::voxel::VoxelModel;
use crate::sync::inventory::State;

pub(super) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, reconcile.in_set(crate::ClientSystems::Present));
    }
}

type Key = (u64, ItemInstanceId);

fn reconcile(
    mut commands: Commands,
    state: Res<State>,
    mut entities: Local<BTreeMap<Key, (Entity, GroundItem)>>,
) {
    if !state.is_changed() {
        return;
    }
    let (items, session) = state.ground();
    let epoch = session.map(|session| session.raid_epoch);
    entities.retain(|&(raid, id), (entity, previous)| {
        if epoch == Some(raid)
            && let Some(item) = items
                .iter()
                .find(|item| item.id == id && item.seed == previous.seed)
        {
            if item.root != previous.root {
                commands
                    .entity(*entity)
                    .insert(Transform::from_translation(item.root.into()));
                previous.root = item.root;
            }
            return true;
        }
        commands.entity(*entity).try_despawn();
        false
    });
    if let Some(raid) = epoch {
        for item in items {
            entities.entry((raid, item.id)).or_insert_with(|| {
                let seed = S1_SEEDS[usize::from(item.seed)];
                let entity = commands
                    .spawn((
                        VoxelModel {
                            asset: item_visuals()[seed].asset_key.clone(),
                            view_model: false,
                        },
                        Transform::from_translation(item.root.into()),
                        Visibility::Inherited,
                    ))
                    .id();
                (entity, item.clone())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{
        catalog::item_inventory::SessionKey,
        replication::inventory::{InventoryUpdate, UpdateBody},
    };

    #[test]
    fn ground_models_follow_valid_current_raid_list() {
        let mut app = App::new();
        app.init_resource::<State>().add_systems(Update, reconcile);
        let key = SessionKey {
            raid_epoch: 1,
            session_epoch: 1,
        };
        let next = SessionKey {
            raid_epoch: 2,
            session_epoch: 2,
        };
        let item = GroundItem {
            id: ItemInstanceId(1),
            seed: 3,
            root: [1.0, 0.1, 2.0],
        };
        let apply = |app: &mut App, session, body| {
            app.world_mut()
                .resource_mut::<State>()
                .apply(InventoryUpdate { session, body });
            app.update();
        };
        let sample = |app: &mut App| {
            let world = app.world_mut();
            world
                .query::<(Entity, &VoxelModel, &Transform)>()
                .iter(world)
                .map(|(entity, model, pose)| (entity, model.asset.clone(), pose.translation))
                .collect::<Vec<_>>()
        };
        let ground = || UpdateBody::Ground(vec![item.clone()]);
        apply(&mut app, key, ground());
        assert!(sample(&mut app).is_empty());
        apply(&mut app, key, UpdateBody::Begin);
        apply(&mut app, key, ground());
        let first = sample(&mut app);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].1, item_visuals()[S1_SEEDS[3]].asset_key);
        assert_eq!(first[0].2, Vec3::from_array(item.root));
        apply(&mut app, key, UpdateBody::GroundFocus(Some(item.id)));
        assert_eq!(
            app.world().resource::<State>().focused_ground(1),
            Some(&item)
        );
        assert!(app.world().resource::<State>().focused_ground(2).is_none());
        apply(&mut app, key, ground());
        for invalid in [
            vec![item.clone(), item.clone()],
            vec![GroundItem {
                seed: u8::MAX,
                ..item.clone()
            }],
            vec![GroundItem {
                root: [f32::NAN; 3],
                ..item.clone()
            }],
            vec![GroundItem {
                id: ItemInstanceId(0),
                ..item.clone()
            }],
        ] {
            apply(&mut app, key, UpdateBody::Ground(invalid));
        }
        assert_eq!(sample(&mut app), first);
        let moved = GroundItem {
            root: [1.0, 0.05, 2.0],
            ..item.clone()
        };
        apply(&mut app, key, UpdateBody::Ground(vec![moved.clone()]));
        let updated = sample(&mut app);
        assert_eq!(updated[0].0, first[0].0);
        assert_eq!(updated[0].2, Vec3::from_array(moved.root));
        apply(&mut app, key, UpdateBody::Ground(vec![]));
        assert!(sample(&mut app).is_empty());
        assert!(app.world().resource::<State>().focused_ground(1).is_none());
        apply(&mut app, next, UpdateBody::Begin);
        apply(&mut app, key, ground());
        assert!(sample(&mut app).is_empty());
        apply(&mut app, next, ground());
        assert_ne!(sample(&mut app)[0].0, first[0].0);
        *app.world_mut().resource_mut::<State>() = State::default();
        app.update();
        assert!(sample(&mut app).is_empty());
    }
}
