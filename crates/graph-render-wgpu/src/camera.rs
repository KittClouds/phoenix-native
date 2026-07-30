use crate::buffers::CameraUniform;
use glam::{Mat4, Vec3};
use graph_model::NodeVisual;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraSnapshot {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub aspect: f32,
}

pub struct Camera {
    orbit_center: Vec3,
    target: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    fov_y: f32,
    aspect: f32,
    z_near: f32,
    z_far: f32,
    viewport_width: f32,
    viewport_height: f32,
}

impl Camera {
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        let viewport_width = width.max(1.0);
        let viewport_height = height.max(1.0);
        Self {
            orbit_center: Vec3::ZERO,
            target: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.2,
            distance: 100.0,
            fov_y: std::f32::consts::FRAC_PI_4,
            aspect: viewport_width / viewport_height,
            z_near: 0.1,
            z_far: 100_000.0,
            viewport_width,
            viewport_height,
        }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.viewport_width = width.max(1.0);
        self.viewport_height = height.max(1.0);
        self.aspect = self.viewport_width / self.viewport_height;
    }

    #[must_use]
    pub fn snapshot(&self) -> CameraSnapshot {
        CameraSnapshot {
            target: self.target.to_array(),
            yaw: self.yaw,
            pitch: self.pitch,
            distance: self.distance,
            fov_y: self.fov_y,
            aspect: self.aspect,
        }
    }

    #[must_use]
    pub fn viewport_size(&self) -> (u32, u32) {
        (self.viewport_width as u32, self.viewport_height as u32)
    }

    #[must_use]
    pub fn eye_position(&self) -> Vec3 {
        let cos_pitch = self.pitch.cos();
        self.target
            + Vec3::new(
                self.distance * cos_pitch * self.yaw.sin(),
                self.distance * self.pitch.sin(),
                self.distance * cos_pitch * self.yaw.cos(),
            )
    }

    #[must_use]
    pub fn view_basis(&self) -> (Vec3, Vec3) {
        let (right, up, _) = self.frame_basis();
        (right, up)
    }

    #[must_use]
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye_position(), self.target, Vec3::Y)
    }

    #[must_use]
    pub fn view_projection_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, self.aspect, self.z_near, self.z_far) * self.view_matrix()
    }

    #[must_use]
    pub fn uniform(&self) -> CameraUniform {
        let eye = self.eye_position();
        let (right, up) = self.view_basis();
        CameraUniform {
            view_proj: self.view_projection_matrix().to_cols_array_2d(),
            eye_position: [eye.x, eye.y, eye.z, 1.0],
            view_right: [right.x, right.y, right.z, 0.0],
            view_up: [up.x, up.y, up.z, 0.0],
            viewport_size: [self.viewport_width, self.viewport_height],
            edge_opacity: 0.18,
            _padding: 0.0,
        }
    }

    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        // V3 rotates the scene root around its semantic origin. The native
        // camera is the inverse representation of that transform, so carry only
        // the screen-space framing offset through the new camera frame. The
        // authoritative orbit center must never be replaced by a page's AABB.
        let (old_right, old_up, old_backward) = self.frame_basis();
        let offset = self.target - self.orbit_center;
        let local_offset = Vec3::new(
            offset.dot(old_right),
            offset.dot(old_up),
            offset.dot(old_backward),
        );

        self.yaw += delta_x * 0.006;
        self.pitch = (self.pitch + delta_y * 0.006).clamp(-1.35, 1.35);

        let (right, up, backward) = self.frame_basis();
        self.target = self.orbit_center
            + right * local_offset.x
            + up * local_offset.y
            + backward * local_offset.z;
    }

    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        // Match the V3 root-translation contract. Pan is independent from the
        // scene rotation and remains a framing offset around the fixed pivot.
        let world_per_pixel = self.distance / 900.0;
        self.target.x -= delta_x * world_per_pixel;
        self.target.y += delta_y * world_per_pixel;
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (-delta * 0.12).exp()).clamp(0.05, 100_000.0);
    }

    pub fn zoom_at(&mut self, delta: f32, screen_x: f32, screen_y: f32) {
        let previous_distance = self.distance;
        let next_distance = (previous_distance * (-delta * 0.12).exp()).clamp(0.05, 100_000.0);
        if next_distance == previous_distance {
            return;
        }

        // Angular only retargets on zoom-in. Zoom-out keeps the current framing,
        // which avoids target drift during repeated wheel reversals.
        if next_distance < previous_distance {
            if let Some(anchor) = self.target_plane_anchor(screen_x, screen_y) {
                let ratio = next_distance / previous_distance;
                self.target = anchor + (self.target - anchor) * ratio;
            }
        }
        self.distance = next_distance;
    }

    pub fn reset(&mut self) {
        self.orbit_center = Vec3::ZERO;
        self.target = Vec3::ZERO;
        self.yaw = 0.0;
        self.pitch = 0.2;
        self.distance = 100.0;
    }

    pub fn orient(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch.clamp(-1.35, 1.35);
    }

    pub fn focus(&mut self, position: [f32; 3]) {
        self.orbit_center = Vec3::from_array(position);
        self.target = self.orbit_center;
        self.distance = self.distance.clamp(8.0, 180.0);
    }

    pub fn fit_graph<'a, I>(&mut self, nodes: I)
    where
        I: IntoIterator<Item = &'a NodeVisual>,
        I::IntoIter: Clone,
    {
        self.fit_graph_around(nodes, Vec3::ZERO);
    }

    pub fn fit_graph_around_bounds<'a, I>(&mut self, nodes: I)
    where
        I: IntoIterator<Item = &'a NodeVisual>,
        I::IntoIter: Clone,
    {
        let nodes = nodes.into_iter();
        if nodes.clone().next().is_none() {
            self.reset();
            return;
        }

        let (minimum, maximum) = nodes.clone().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(minimum, maximum), node| {
                let position = Vec3::from_array(node.position);
                (minimum.min(position), maximum.max(position))
            },
        );
        self.fit_graph_around(nodes, (minimum + maximum) * 0.5);
    }

    fn fit_graph_around<'a, I>(&mut self, nodes: I, center: Vec3)
    where
        I: IntoIterator<Item = &'a NodeVisual>,
        I::IntoIter: Clone,
    {
        let nodes = nodes.into_iter();
        if nodes.clone().next().is_none() {
            self.reset();
            return;
        }

        self.orbit_center = center;
        self.target = center;
        let radius = nodes
            .map(|node| Vec3::from_array(node.position).distance(center) + node.radius.max(0.0))
            .fold(1.0_f32, f32::max);
        let vertical_half = self.fov_y * 0.5;
        let horizontal_half = (vertical_half.tan() * self.aspect).atan();
        let limiting_half_angle = vertical_half.min(horizontal_half);
        self.distance = (radius / limiting_half_angle.sin() * 1.1).clamp(0.05, 100_000.0);
    }

    #[must_use]
    pub fn viewport_to_ray(&self, screen_x: f32, screen_y: f32) -> (Vec3, Vec3) {
        let ndc_x = 2.0 * screen_x / self.viewport_width - 1.0;
        let ndc_y = 1.0 - 2.0 * screen_y / self.viewport_height;
        let inverse = self.view_projection_matrix().inverse();
        let near = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
        let far = inverse.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));
        (near, (far - near).normalize_or_zero())
    }

    #[must_use]
    pub fn project_to_viewport(&self, position: [f32; 3]) -> Option<(f32, f32, f32)> {
        let clip = self.view_projection_matrix() * Vec3::from_array(position).extend(1.0);
        if !clip.is_finite() || clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        if !ndc.is_finite()
            || !(-1.0..=1.0).contains(&ndc.x)
            || !(-1.0..=1.0).contains(&ndc.y)
            || !(0.0..=1.0).contains(&ndc.z)
        {
            return None;
        }
        Some((
            (ndc.x + 1.0) * 0.5 * self.viewport_width,
            (1.0 - ndc.y) * 0.5 * self.viewport_height,
            ndc.z,
        ))
    }

    fn frame_basis(&self) -> (Vec3, Vec3, Vec3) {
        let backward = (self.eye_position() - self.target)
            .try_normalize()
            .unwrap_or(Vec3::Z);
        let forward = -backward;
        let right = forward.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let up = right.cross(forward).normalize_or_zero();
        (right, up, backward)
    }

    fn target_plane_anchor(&self, screen_x: f32, screen_y: f32) -> Option<Vec3> {
        let (ray_origin, ray_direction) = self.viewport_to_ray(screen_x, screen_y);
        let plane_normal = (self.target - self.eye_position()).normalize_or_zero();
        let denominator = ray_direction.dot(plane_normal);
        if denominator.abs() <= 1.0e-6 {
            return None;
        }
        let distance = (self.target - ray_origin).dot(plane_normal) / denominator;
        (distance >= 0.0).then(|| ray_origin + ray_direction * distance)
    }
}

#[cfg(test)]
mod tests {
    use super::Camera;
    use glam::Vec3;
    use graph_model::{NodeId, NodeVisual};

    fn node(id: u64, position: [f32; 3]) -> NodeVisual {
        NodeVisual {
            id: NodeId(id),
            position,
            radius: 2.0,
            color: [1.0; 4],
            kind: 0,
            flags: 0,
        }
    }

    #[test]
    fn center_ray_matches_view_direction() {
        let camera = Camera::new(800.0, 600.0);
        let (_, ray) = camera.viewport_to_ray(400.0, 300.0);
        let expected = (camera.target - camera.eye_position()).normalize();
        assert!((ray.dot(expected) - 1.0).abs() < 0.0001);
    }

    #[test]
    fn camera_target_projects_to_viewport_center() {
        let camera = Camera::new(800.0, 600.0);
        let (x, y, depth) = camera
            .project_to_viewport(camera.target.to_array())
            .unwrap();
        assert!((x - 400.0).abs() < 0.001);
        assert!((y - 300.0).abs() < 0.001);
        assert!((0.0..=1.0).contains(&depth));
    }

    #[test]
    fn fit_graph_centers_bounds_and_accounts_for_aspect() {
        let mut camera = Camera::new(1600.0, 500.0);
        let nodes = [node(1, [-50.0, -10.0, 0.0]), node(2, [50.0, 10.0, 0.0])];
        camera.fit_graph(&nodes);
        assert!((camera.target - Vec3::ZERO).length() < 0.0001);
        assert!(camera.distance > 50.0);
    }

    #[test]
    fn fit_graph_uses_the_scene_radius_not_the_empty_box_corners() {
        let mut camera = Camera::new(800.0, 800.0);
        let nodes = [
            node(1, [-36.0, 0.0, 0.0]),
            node(2, [36.0, 0.0, 0.0]),
            node(3, [0.0, -36.0, 0.0]),
            node(4, [0.0, 36.0, 0.0]),
            node(5, [0.0, 0.0, -36.0]),
            node(6, [0.0, 0.0, 36.0]),
        ];
        camera.fit_graph(&nodes);
        let exact_radius = 38.0_f32;
        let expected = exact_radius / (camera.snapshot().fov_y * 0.5).sin() * 1.1;
        assert!((camera.snapshot().distance - expected).abs() < 0.001);
        assert!(camera.snapshot().distance < 112.0);
    }

    #[test]
    fn subset_fit_centers_the_visible_cluster_without_changing_scene_fit() {
        let nodes = [node(1, [1.0, 10.0, -24.0]), node(2, [5.0, 18.0, -16.0])];
        let mut subset = Camera::new(800.0, 800.0);
        subset.fit_graph_around_bounds(&nodes);
        assert!((subset.target - Vec3::new(3.0, 14.0, -20.0)).length() < 0.0001);

        let mut scene = Camera::new(800.0, 800.0);
        scene.fit_graph(&nodes);
        assert!((scene.target - Vec3::ZERO).length() < 0.0001);
        assert!(subset.distance < scene.distance);
    }

    #[test]
    fn resize_updates_projection_aspect() {
        let mut camera = Camera::new(800.0, 600.0);
        camera.resize(1920.0, 1080.0);
        assert!((camera.snapshot().aspect - 16.0 / 9.0).abs() < 0.0001);
    }

    #[test]
    fn explicit_orientation_clamps_pitch_without_moving_the_fit() {
        let mut camera = Camera::new(800.0, 600.0);
        camera.fit_graph(&[node(1, [-10.0, 0.0, 0.0]), node(2, [10.0, 0.0, 0.0])]);
        let distance = camera.snapshot().distance;
        camera.orient(0.72, std::f32::consts::PI);
        assert_eq!(camera.snapshot().yaw, 0.72);
        assert!(camera.snapshot().pitch < std::f32::consts::FRAC_PI_2);
        assert_eq!(camera.snapshot().distance, distance);
    }

    #[test]
    fn asymmetric_page_rotates_about_the_shared_projection_origin() {
        let mut camera = Camera::new(1200.0, 800.0);
        camera.fit_graph(&[
            node(1, [4.0, 2.0, 0.0]),
            node(2, [38.0, 9.0, -3.0]),
            node(3, [12.0, -16.0, 7.0]),
        ]);
        camera.pan(70.0, -25.0);
        let before = camera.project_to_viewport([0.0, 0.0, 0.0]).unwrap();

        camera.orbit(90.0, 45.0);

        let after = camera.project_to_viewport([0.0, 0.0, 0.0]).unwrap();
        assert!((before.0 - after.0).abs() < 0.001);
        assert!((before.1 - after.1).abs() < 0.001);
        assert_eq!(camera.orbit_center, Vec3::ZERO);
    }

    #[test]
    fn orbit_preserves_a_panned_scene_offset_in_the_camera_frame() {
        let mut camera = Camera::new(1200.0, 800.0);
        camera.pan(80.0, -30.0);
        let (old_right, old_up, old_backward) = camera.frame_basis();
        let old_offset = camera.target - camera.orbit_center;
        let old_local = Vec3::new(
            old_offset.dot(old_right),
            old_offset.dot(old_up),
            old_offset.dot(old_backward),
        );

        camera.orbit(90.0, 45.0);

        let (right, up, backward) = camera.frame_basis();
        let offset = camera.target - camera.orbit_center;
        let local = Vec3::new(offset.dot(right), offset.dot(up), offset.dot(backward));
        assert!((local - old_local).length() < 0.0001);
        assert_ne!(camera.target, old_offset);
    }

    #[test]
    fn vertical_drag_uses_the_angular_tilt_direction_and_sensitivity() {
        let mut camera = Camera::new(800.0, 600.0);
        camera.orbit(0.0, 25.0);
        assert!((camera.snapshot().pitch - 0.35).abs() < 0.0001);
    }

    #[test]
    fn pan_matches_angular_world_xy_offsets_after_tilt() {
        let mut camera = Camera::new(800.0, 600.0);
        camera.orbit(60.0, 35.0);
        let before = camera.target;
        camera.pan(10.0, -5.0);
        let scale = camera.distance / 900.0;
        assert!((camera.target.x - (before.x - 10.0 * scale)).abs() < 0.0001);
        assert!((camera.target.y - (before.y - 5.0 * scale)).abs() < 0.0001);
        assert_eq!(camera.target.z, before.z);
    }

    #[test]
    fn zoom_in_tracks_the_pointer_on_the_camera_target_plane() {
        let mut camera = Camera::new(1000.0, 800.0);
        let anchor = camera.target_plane_anchor(750.0, 400.0).unwrap();
        let old_target = camera.target;
        let old_distance = camera.distance;

        camera.zoom_at(1.0, 750.0, 400.0);

        let ratio = camera.distance / old_distance;
        let expected = anchor + (old_target - anchor) * ratio;
        assert!((camera.target - expected).length() < 0.0001);
        assert!(camera.target.x > old_target.x);
    }

    #[test]
    fn zoom_out_keeps_the_current_target() {
        let mut camera = Camera::new(1000.0, 800.0);
        camera.pan(20.0, -10.0);
        let target = camera.target;
        camera.zoom_at(-1.0, 900.0, 100.0);
        assert_eq!(camera.target, target);
    }
}
