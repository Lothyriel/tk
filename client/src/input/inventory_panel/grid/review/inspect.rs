use bevy::ecs::system::SystemParam;

use crate::render::{
    ViewModelCamera,
    item_inspection::Preview,
    voxel::{VoxelInstance, VoxelModel},
};
use common::catalog::item_manifest::item_visuals;

use super::*;

type Models<'w, 's> = Query<
    'w,
    's,
    (
        &'static VoxelModel,
        &'static Transform,
        &'static ChildOf,
        &'static InheritedVisibility,
        Has<VoxelInstance>,
    ),
>;

type Roots<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Preview,
        &'static Transform,
        &'static ChildOf,
    ),
>;

#[derive(SystemParam)]
pub(super) struct Previews<'w, 's> {
    roots: Roots<'w, 's>,
    models: Models<'w, 's>,
    camera: Query<'w, 's, &'static Camera, With<ViewModelCamera>>,
}

type Target<'a> = (
    u32,
    SessionKey,
    VersionedCompartment,
    ItemInstanceId,
    &'a ProjectedItem,
);

#[derive(Default)]
pub(super) struct Route {
    step: usize,
    release: Option<(KeyCode, Key)>,
    captured: bool,
    baseline: Option<Transform>,
    orbit: Option<Transform>,
}

const FRAMES: [&str; 4] = ["open", "orbit", "zoom", "reset"];

impl Route {
    pub(super) fn awaiting_activation(&self) -> bool {
        self.step == 0
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn drive(
        &mut self,
        commands: &mut Commands,
        output: &Output,
        keyboard: &mut MessageWriter<KeyboardInput>,
        window: Entity,
        panel: &Panel,
        (raid, key, version, item, confirmed): Target,
        previews: &Previews,
    ) -> bool {
        let emit = |(key_code, logical_key), state| KeyboardInput {
            key_code,
            logical_key,
            state,
            window,
            text: None,
            repeat: false,
        };
        if let Some(control) = self.release.take() {
            keyboard.write(emit(control, ButtonState::Released));
            if self.step == 5 {
                assert!(
                    panel.consumed_escape && panel.inspection.is_none() && panel.context.is_none()
                );
            }
            return false;
        }
        if self.step == 5 {
            assert!(panel.open && panel.inspection.is_none());
            if !previews.roots.is_empty() {
                return false;
            }
            info!("GRID INSPECT closed via Escape; resuming keyboard route");
            return true;
        }
        if self.step == 0 {
            assert!(panel.context.is_some_and(|context| context.item == item
                && context.session == key
                && context.version == version
                && CONTEXT_ACTIONS[context.focus].0 == ContextAction::Inspect));
        } else if !self.captured {
            let Some(inspection) = panel.inspection else {
                return false;
            };
            assert_eq!(inspection.item, item);
            assert!(panel.context.is_none());
            let visual = &item_visuals()[confirmed.definition.as_str()];
            let Ok((root, preview, root_pose, camera)) = previews.roots.single() else {
                return false;
            };
            let model_entity = preview.model_entity();
            let Ok((model, local_pose, parent, visible, assembled)) =
                previews.models.get(model_entity)
            else {
                return false;
            };
            if !assembled
                || !visible.get()
                || !previews
                    .camera
                    .get(camera.0)
                    .is_ok_and(|camera| camera.is_active)
            {
                return false;
            }
            let pose = if model_entity == root {
                *local_pose
            } else {
                assert_eq!(
                    parent.0, root,
                    "GRID INSPECT model must belong to its holder"
                );
                root_pose.mul_transform(*local_pose)
            };
            assert_eq!((preview.raid, preview.item), (raid, item));
            assert!(model.view_model && model.asset == visual.asset_key);
            let expected = [(0, 0), (1, 0), (1, 1), (0, 0)][self.step - 1];
            assert_eq!((inspection.yaw_steps, inspection.zoom_steps), expected);
            assert_eq!(inspection.pitch_steps, 0);
            let base = *self.baseline.get_or_insert(pose);
            let center = Vec3::from_array(visual.collider_center());
            check_pose(self.step, base, self.orbit, pose, center);
            if self.step == 2 {
                self.orbit = Some(pose);
            }
            let frame = FRAMES[self.step - 1];
            info!(
                "GRID INSPECT {frame} item={} revision={} asset={}",
                item.0, version.revision, model.asset
            );
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(
                    output.directory.join(format!("inspect-{frame}.png")),
                ));
            self.captured = true;
            return false;
        }
        let control = match self.step {
            0 => (KeyCode::Enter, Key::Enter),
            1 => (KeyCode::ArrowRight, Key::ArrowRight),
            2 => (KeyCode::Equal, Key::Character("=".into())),
            3 => (KeyCode::KeyR, Key::Character("r".into())),
            4 => (KeyCode::Escape, Key::Escape),
            _ => unreachable!(),
        };
        keyboard.write(emit(control.clone(), ButtonState::Pressed));
        self.release = Some(control);
        self.step += 1;
        self.captured = false;
        false
    }
}

fn check_pose(
    step: usize,
    base: Transform,
    orbit: Option<Transform>,
    pose: Transform,
    center: Vec3,
) {
    assert_eq!(pose.scale, Vec3::ONE);
    let reference = if step == 3 {
        orbit.expect("observed orbit before zoom")
    } else {
        base
    };
    let displacement = pose.transform_point(center) - reference.transform_point(center);
    let angle = reference.rotation.angle_between(pose.rotation);
    // The supporting arm can limit depth as the model turns. Compare zoom
    // to the preceding orbit, and reset to the original framing.
    assert!(
        displacement.xy().length() < 0.0001,
        "GRID INSPECT center moved in the image plane"
    );
    match step {
        1 | 4 => assert!(
            angle.abs() < 0.001 && displacement.z.abs() < 0.0001,
            "GRID INSPECT did not match the opening pose"
        ),
        2 => {
            let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_6) * base.rotation;
            assert!(
                expected.angle_between(pose.rotation).abs() < 0.001,
                "GRID INSPECT did not orbit right by one step"
            );
        }
        3 => assert!(
            angle.abs() < 0.001 && displacement.z > 0.0001,
            "GRID INSPECT zoom did not bring the orbited model closer"
        ),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_grid_inspection_compares_zoom_to_the_orbited_depth() {
        let center = Vec3::new(0.01, 0.02, 0.03);
        let pose = |z, rotation: Quat| {
            Transform::from_translation(Vec3::new(0.0, -0.025, z) - rotation * center)
                .with_rotation(rotation)
        };
        let tilt = Quat::from_rotation_x(0.25);
        let yaw = Quat::from_rotation_y(std::f32::consts::FRAC_PI_6) * tilt;
        let base = pose(-0.30, tilt);
        let orbit = pose(-0.35, yaw);
        check_pose(2, base, None, orbit, center);
        check_pose(3, base, Some(orbit), pose(-0.32, yaw), center);
        check_pose(4, base, Some(orbit), base, center);
    }
}
