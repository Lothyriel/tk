//! One shared, lazily decoded page for inventory UI thumbnails.
use std::collections::BTreeMap;

use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    prelude::*,
};
use serde::Deserialize;

const METADATA: &str = include_str!("../../../assets/item-icons/atlas.json");
const PNG: &[u8] = include_bytes!("../../../assets/item-icons/atlas.png");

/// Set on a UI node to select the thumbnail by canonical item ID.
#[derive(Component)]
#[require(ImageNode)]
pub(crate) struct ItemIcon(pub String);

#[derive(Deserialize)]
struct Atlas {
    width: u32,
    height: u32,
    items: BTreeMap<String, Tile>,
}

#[derive(Deserialize)]
struct Tile {
    rect: [u32; 4],
}

#[derive(Resource, Default)]
struct Page {
    loaded: Option<(Atlas, Handle<Image>)>,
}

impl Page {
    fn load(&mut self, images: &mut Assets<Image>) -> &(Atlas, Handle<Image>) {
        self.loaded.get_or_insert_with(|| {
            let atlas: Atlas = serde_json::from_str(METADATA).expect("valid item icon metadata");
            let image = Image::from_buffer(
                PNG,
                ImageType::Extension("png"),
                CompressedImageFormats::NONE,
                true,
                ImageSampler::nearest(),
                RenderAssetUsages::default(),
            )
            .expect("valid item icon page");
            assert_eq!(image.size(), UVec2::new(atlas.width, atlas.height));
            for (id, tile) in &atlas.items {
                let [x, y, w, h] = tile.rect;
                assert!(
                    w > 0
                        && h > 0
                        && x.checked_add(w).is_some_and(|right| right <= atlas.width)
                        && y.checked_add(h)
                            .is_some_and(|bottom| bottom <= atlas.height),
                    "item icon outside atlas: {id}"
                );
            }
            (atlas, images.add(image))
        })
    }
}

fn bind(
    mut page: ResMut<Page>,
    mut images: ResMut<Assets<Image>>,
    mut icons: Query<(&ItemIcon, &mut ImageNode), Changed<ItemIcon>>,
) {
    if icons.is_empty() {
        return;
    }
    let (atlas, texture) = page.load(&mut images);
    for (icon, mut node) in &mut icons {
        node.texture_atlas = None;
        if let Some(tile) = atlas.items.get(&icon.0) {
            let [x, y, w, h] = tile.rect;
            node.image = texture.clone();
            node.rect = Some(Rect::new(
                x as f32,
                y as f32,
                (x + w) as f32,
                (y + h) as f32,
            ));
        } else {
            node.image = ImageNode::default().image;
            node.rect = None;
        }
    }
}

pub(super) struct Plugin;

impl bevy::prelude::Plugin for Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Page>().add_systems(Update, bind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::catalog::item_manifest::S1_SEEDS;

    #[test]
    fn inventory_icon_page_is_lazy_and_shared_across_ui_nodes() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>().add_plugins(Plugin);
        app.update();
        assert!(app.world().resource::<Assets<Image>>().is_empty());
        let entities = S1_SEEDS.map(|id| app.world_mut().spawn(ItemIcon(id.into())).id());
        app.update();
        let (atlas, texture) = app.world().resource::<Page>().loaded.as_ref().unwrap();
        let mut rects = Vec::new();
        for (entity, id) in entities.into_iter().zip(S1_SEEDS) {
            let node = app.world().get::<ImageNode>(entity).unwrap();
            assert_eq!(&node.image, texture);
            let rect = node.rect.unwrap();
            let [x, y, w, h] = atlas.items[id].rect;
            assert_eq!(rect.min, Vec2::new(x as f32, y as f32));
            assert_eq!(rect.size(), Vec2::new(w as f32, h as f32));
            assert!(!rects.contains(&rect));
            rects.push(rect);
        }
        let texture = texture.clone();
        for entity in entities {
            app.world_mut().despawn(entity);
        }
        let reopened = app.world_mut().spawn(ItemIcon("med.bandage".into())).id();
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(reopened).unwrap().image,
            texture
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    }
}
