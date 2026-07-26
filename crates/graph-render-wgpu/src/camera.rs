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
        let forward = (self.target - self.eye_position()).normalize_or_zero();
        let right = forward.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let up = right.cross(forward).normalize_or_zero();
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
            _padding: [0.0; 2],
        }
    }

    pub fn orbit(&mut self, delta_x: f32, delta_y: f32) {
        self.yaw += delta_x * 0.005;
        self.pitch -= delta_y * 0.005;
        let limit = std::f32::consts::FRAC_PI_2 - 0.01;
        self.pitch = self.pitch.clamp(-limit, limit);
    }

    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        let (right, up) = self.view_basis();
        let world_per_pixel = 2.0 * self.distance * (self.fov_y * 0.5).tan() / self.viewport_height;
        self.target -= right * delta_x * world_per_pixel;
        self.target += up * delta_y * world_per_pixel;
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (-delta * 0.12).exp()).clamp(0.05, 100_000.0);
    }

    pub fn reset(&mut self) {
        self.target = Vec3::ZERO;
        self.yaw = 0.0;
        self.pitch = 0.2;
        self.distance = 100.0;
    }

    pub fn fit_graph<'a>(&mut self, nodes: impl IntoIterator<Item = &'a NodeVisual>) {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut found = false;
        for node in nodes {
            found = true;
            let position = Vec3::from_array(node.position);
            let radius = Vec3::splat(node.radius);
            min = min.min(position - radius);
            max = max.max(position + radius);
        }
        if !found {
            self.reset();
            return;
        }

        self.target = (min + max) * 0.5;
        let radius = ((max - min) * 0.5).length().max(1.0);
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
    fn fit_graph_centers_bounds_and_accounts_for_aspect() {
        let mut camera = Camera::new(1600.0, 500.0);
        let nodes = [node(1, [-50.0, -10.0, 0.0]), node(2, [50.0, 10.0, 0.0])];
        camera.fit_graph(&nodes);
        assert!((camera.target - Vec3::ZERO).length() < 0.0001);
        assert!(camera.distance > 50.0);
    }

    #[test]
    fn resize_updates_projection_aspect() {
        let mut camera = Camera::new(800.0, 600.0);
        camera.resize(1920.0, 1080.0);
        assert!((camera.snapshot().aspect - 16.0 / 9.0).abs() < 0.0001);
    }
}
