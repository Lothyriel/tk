use super::super::WorldModelCamera;
use super::*;

#[derive(Component)]
pub(super) struct SurfaceLod {
    pub near: Handle<Mesh>,
    pub far: Handle<Mesh>,
    pub bounds: Bounds,
    pub deviation: f32,
}

/// Engineering thresholds: far geometry is subpixel and the whole model is
/// small on screen. Wider return thresholds prevent toggling under jitter.
fn choose_far(was_far: bool, distance: f32, pixels: f32, error: f32) -> bool {
    if !distance.is_finite() || !pixels.is_finite() || !error.is_finite() {
        return false;
    }
    if was_far {
        distance > 4.0 && pixels < 100.0 && error < 0.75
    } else {
        distance >= 5.0 && pixels <= 80.0 && error <= 0.5
    }
}

pub(super) fn select(
    debug: Option<Res<super::debug::DebugView>>,
    cameras: Query<(&Camera, &Projection, &GlobalTransform), With<WorldModelCamera>>,
    mut models: Query<(&GlobalTransform, &mut VoxelInstance)>,
    mut surfaces: Query<(&GlobalTransform, &SurfaceLod, &mut Mesh3d)>,
) {
    for (root, mut instance) in &mut models {
        let mut far = !instance.source.view_model;
        let mut observed = false;
        for (camera, projection, camera_transform) in &cameras {
            if !far || !camera.is_active {
                continue;
            }
            observed = true;
            let (Projection::Perspective(perspective), Some(size)) =
                (projection, camera.physical_viewport_size())
            else {
                far = false;
                break;
            };
            let lens = 2.0 * (perspective.fov * 0.5).tan();
            let focal = Vec2::new(
                size.x as f32 / (lens * perspective.aspect_ratio),
                size.y as f32 / lens,
            );
            if size.x == 0
                || size.y == 0
                || !focal.is_finite()
                || focal.min_element() <= 0.0
                || !perspective.near.is_finite()
                || perspective.near <= 0.0
                || !(0.0..std::f32::consts::PI).contains(&perspective.fov)
            {
                far = false;
                break;
            }
            let world_to_view = camera_transform.affine().inverse();
            let mut low = Vec2::splat(f32::INFINITY);
            let mut high = Vec2::splat(f32::NEG_INFINITY);
            let mut error = 0.0_f32;
            for &entity in &instance.surfaces {
                let Ok((global, lod, _)) = surfaces.get(entity) else {
                    far = false;
                    break;
                };
                let mut depth = f32::INFINITY;
                let mut slope = 0.0_f32;
                for bits in 0..8 {
                    let local = Vec3::from_array(std::array::from_fn(|d| {
                        if bits & (1 << d) == 0 {
                            lod.bounds.min[d]
                        } else {
                            lod.bounds.max[d]
                        }
                    }));
                    let point = world_to_view.transform_point3(global.transform_point(local));
                    if -point.z <= perspective.near || !point.is_finite() {
                        far = false;
                        break;
                    }
                    depth = depth.min(-point.z);
                    let projected = point.truncate() / -point.z;
                    slope = slope.max(projected.length());
                    low = low.min(projected);
                    high = high.max(projected);
                }
                if !far {
                    break;
                }
                // sqrt(||M||1 * ||M||infinity) bounds scale even under shear
                // from nonuniformly scaled parents and rotated rigid pivots.
                let matrix = (world_to_view * global.affine()).matrix3;
                let columns = [
                    matrix.x_axis.abs(),
                    matrix.y_axis.abs(),
                    matrix.z_axis.abs(),
                ];
                let norm1 = columns
                    .iter()
                    .map(|v| v.element_sum())
                    .fold(0.0_f32, f32::max);
                let norm_inf = (columns[0] + columns[1] + columns[2]).max_element();
                // Projection also changes with depth, especially off-axis.
                // Bound ||delta(x/z,y/z)|| by e*(1+||xy/z||)/(z-e).
                let deviation = lod.deviation * (norm1 * norm_inf).sqrt();
                if !deviation.is_finite() || depth <= deviation {
                    far = false;
                    break;
                }
                error = error
                    .max(deviation * focal.max_element() * (1.0 + slope) / (depth - deviation));
            }
            let pixels = ((high - low) * focal).max_element();
            let distance = root.translation().distance(camera_transform.translation());
            far &= choose_far(instance.far, distance, pixels, error);
        }
        far &= observed; // no valid world camera: retain detailed geometry
        // Explicit inspection only; ordinary selection and view-model quality
        // retain their normal projection thresholds.
        if let Some(forced) = debug.as_ref().and_then(|debug| debug.lod) {
            far = forced;
        }
        if far != instance.far {
            for &entity in &instance.surfaces {
                if let Ok((_, lod, mut mesh)) = surfaces.get_mut(entity) {
                    let handle = if far { &lod.far } else { &lod.near };
                    if mesh.0 != *handle {
                        mesh.0 = handle.clone();
                    }
                }
            }
            instance.far = far;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voxel_lod_thresholds_hold_a_band_and_reject_invalid_projection() {
        assert!(!choose_far(false, 4.9, 50.0, 0.3));
        assert!(choose_far(false, 5.1, 50.0, 0.3));
        assert!(choose_far(true, 4.5, 90.0, 0.6));
        assert!(!choose_far(false, 4.5, 90.0, 0.6));
        assert!(!choose_far(true, 3.9, 50.0, 0.3));
        assert!(!choose_far(true, 6.0, 101.0, 0.3));
        assert!(!choose_far(true, 6.0, 50.0, 0.76));
        assert!(!choose_far(false, f32::NAN, 50.0, 0.3));
    }
}
