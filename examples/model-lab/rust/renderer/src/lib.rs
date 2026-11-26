//! One renderer per WASM instance. The host holds browser objects; this crate
//! chooses the projection, orbit, shaders and indexed draw command.
#![cfg(any(target_arch = "wasm32", test))]

const MAX_INDICES: usize = 6_000_000; // forge:mesh ABI-v1 limit
const HALF_FOV: f32 = 20.0 * std::f32::consts::PI / 180.0;

#[derive(Default)]
struct IndexScratch {
    words: Vec<u32>,
}

impl IndexScratch {
    // The browser writes the validated triangle indices into the first third
    // before expand(). Reserve once so expanding never invalidates that view.
    fn prepare(&mut self, count: usize) -> Option<*mut u32> {
        if !(3..=MAX_INDICES).contains(&count) || !count.is_multiple_of(3) {
            return None;
        }
        let total = count * 3;
        self.words.try_reserve(total.saturating_sub(self.words.len())).ok()?;
        self.words.resize(total, 0);
        Some(self.words.as_mut_ptr())
    }

    fn expand(&mut self) -> Option<&[u32]> {
        let count = self.words.len() / 3;
        if count == 0 {
            return None;
        }
        let (triangles, edges) = self.words.split_at_mut(count);
        for (triangle, lines) in triangles.chunks_exact(3).zip(edges.chunks_exact_mut(6)) {
            let (a, b, c) = (triangle[0], triangle[1], triangle[2]);
            lines.copy_from_slice(&[a, b, b, c, c, a]);
        }
        Some(edges)
    }
}

struct Camera {
    aspect: f32,
    azimuth: f32,
    elevation: f32,
    zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self { aspect: 1.0, azimuth: 0.68, elevation: 0.44, zoom: 1.0 }
    }
}

impl Camera {
    fn reset(&mut self) {
        *self = Self { aspect: self.aspect, ..Self::default() };
    }

    fn orbit(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() && dy.is_finite() {
            self.azimuth = (self.azimuth + dx).rem_euclid(std::f32::consts::TAU);
            // Input uses the same polar-angle convention as OrbitControls.
            self.elevation = (self.elevation - dy).clamp(-1.52, 1.52);
        }
    }

    fn zoom(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(0.4, 4.0);
        }
    }

    // Column-major projection * view, matching WebGL uniformMatrix4fv. The
    // vertex shader recentres/scales each mesh into a unit bounding sphere.
    fn view_projection(&self) -> [f32; 16] {
        let (s, c) = self.azimuth.sin_cos();
        let (h, v) = self.elevation.sin_cos();
        let (right, up, back) = ([c, 0.0, -s], [-s * h, v, -c * h], [s * v, h, c * v]);
        let fy = 1.0 / HALF_FOV.tan();
        let fx = fy / self.aspect;
        let limiting_fov = (HALF_FOV.tan() * self.aspect.min(1.0)).atan();
        let distance = self.zoom * 1.08 / limiting_fov.sin();
        let near = (distance - 1.1).max(0.01);
        let far = distance + 2.0;
        let z_scale = -(far + near) / (far - near);
        let z_offset = -(2.0 * far * near) / (far - near);
        let mut m = [0.0; 16];
        for axis in 0..3 {
            m[axis * 4..axis * 4 + 4].copy_from_slice(&[
                right[axis] * fx,
                up[axis] * fy,
                back[axis] * z_scale,
                -back[axis],
            ]);
        }
        m[14] = z_offset - distance * z_scale;
        m[15] = distance;
        m
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;
    use std::cell::RefCell;

    // Each call is synchronous. The browser may view each pointer only during
    // its import invocation; retaining a typed view across memory.grow is invalid.
    #[link(wasm_import_module = "webgl2")]
    unsafe extern "C" {
        fn program(vertex: *const u8, vertex_bytes: usize, fragment: *const u8, fragment_bytes: usize);
        fn present(
            mode: u32,
            count: u32,
            view_projection: *const f32,
            center_inv_radius: *const f32,
            sine: f32,
            cosine: f32,
            color: *const f32,
            show_normals: u32,
        );
    }

    #[derive(Default)]
    struct Scene {
        camera: Camera,
        center_inv_radius: [f32; 4],
        color: [f32; 3],
        count: u32,
    }
    thread_local! {
        static SCENE: RefCell<Scene> = RefCell::new(Scene::default());
        static INDICES: RefCell<IndexScratch> = RefCell::new(IndexScratch::default());
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_abi_version() -> u32 {
        1
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_init() {
        let vertex = include_str!("model.vert");
        let fragment = include_str!("model.frag");
        // SAFETY: both strings are embedded in this module's linear memory.
        unsafe { program(vertex.as_ptr(), vertex.len(), fragment.as_ptr(), fragment.len()) }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_indices(count: usize) -> *mut u32 {
        INDICES.with_borrow_mut(|s| s.prepare(count).unwrap_or(std::ptr::null_mut()))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_edges() -> *const u32 {
        INDICES.with_borrow_mut(|s| s.expand().map_or(std::ptr::null(), |edges| edges.as_ptr()))
    }

    // Supply axis-aligned bounds in f64 to avoid overflow during midpoint
    // and radius calculations for f32 geometry.
    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_mesh(
        count: u32,
        x0: f64,
        y0: f64,
        z0: f64,
        x1: f64,
        y1: f64,
        z1: f64,
        red: f32,
        green: f32,
        blue: f32,
    ) -> u32 {
        let dx = x1 - x0;
        let dy = y1 - y0;
        let dz = z1 - z0;
        let inv_radius = (2.0 / dx.hypot(dy).hypot(dz)) as f32;
        if !inv_radius.is_finite()
            || inv_radius <= 0.0
            || count as usize > MAX_INDICES
            || count < 3
            || !count.is_multiple_of(3)
            || ![dx, dy, dz].iter().all(|v| v.is_finite() && *v >= 0.0)
            || ![red, green, blue].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        {
            return 1;
        }
        SCENE.with_borrow_mut(|s| {
            s.center_inv_radius =
                [((x0 + x1) * 0.5) as f32, ((y0 + y1) * 0.5) as f32, ((z0 + z1) * 0.5) as f32, inv_radius];
            s.color = [red, green, blue];
            s.count = count;
            s.camera.reset();
        });
        0
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_resize(width: u32, height: u32) {
        if width > 0 && height > 0 {
            SCENE.with_borrow_mut(|s| s.camera.aspect = width as f32 / height as f32);
        }
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_orbit(x: f32, y: f32) {
        SCENE.with_borrow_mut(|s| s.camera.orbit(x, y));
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_zoom(factor: f32) {
        SCENE.with_borrow_mut(|s| s.camera.zoom(factor));
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_reset() {
        SCENE.with_borrow_mut(|s| s.camera.reset());
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn renderer_draw(rotation: f32, wire: u32, normals: u32) {
        let (view_projection, center, color, count) =
            SCENE.with_borrow(|s| (s.camera.view_projection(), s.center_inv_radius, s.color, s.count));
        let (sine, cosine) = rotation.sin_cos();
        let (mode, count) = if wire != 0 { (0x0001, count * 2) } else { (0x0004, count) };
        // SAFETY: frame values live on the WASM stack until present returns.
        // No Rust borrow is held across an import that could throw.
        unsafe {
            present(mode, count, view_projection.as_ptr(), center.as_ptr(), sine, cosine, color.as_ptr(), normals)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projected(matrix: &[f32; 16], point: [f32; 3]) -> [f32; 3] {
        let axis = |i| matrix[i] * point[0] + matrix[4 + i] * point[1] + matrix[8 + i] * point[2] + matrix[12 + i];
        [axis(0) / axis(3), axis(1) / axis(3), axis(2) / axis(3)]
    }

    #[test]
    fn reset_fits_normalized_model_in_portrait_and_landscape() {
        let mut camera = Camera::default();
        for aspect in [0.33, 1.0, 1.9] {
            camera.aspect = aspect;
            let matrix = camera.view_projection();
            let center = projected(&matrix, [0.; 3]);
            assert!(center[0].abs() < 1e-5 && center[1].abs() < 1e-5);
            assert!((-1.0..1.0).contains(&center[2]));
            for point in [[0., 1., 0.], [0., -1., 0.], [1., 0., 0.], [-1., 0., 0.], [0., 0., 1.], [0., 0., -1.]] {
                assert!(projected(&matrix, point).iter().all(|v| v.abs() < 1.0));
            }
        }
    }

    #[test]
    fn orbit_zoom_and_reset_preserve_finite_camera() {
        let mut camera = Camera::default();
        let initial = camera.view_projection();
        let elevation = camera.elevation;
        camera.orbit(0.5, -1000.0);
        assert!(camera.elevation > elevation);
        camera.zoom(1e-30);
        assert_ne!(camera.view_projection(), initial);
        assert!(camera.view_projection().iter().all(|v| v.is_finite()));
        camera.orbit(f32::NAN, f32::INFINITY);
        camera.zoom(-1.);
        camera.zoom(f32::NAN);
        assert!(camera.view_projection().iter().all(|v| v.is_finite()));
        camera.reset();
        assert_eq!(camera.view_projection(), initial);
    }

    #[test]
    fn wire_indices_only_connect_vertices_of_each_triangle() {
        let mut scratch = IndexScratch::default();
        assert!(scratch.prepare(6).is_some());
        scratch.words[..6].copy_from_slice(&[0, 1, 2, 8, 10, 7]);
        assert_eq!(scratch.expand().unwrap(), &[0, 1, 1, 2, 2, 0, 8, 10, 10, 7, 7, 8]);
        assert!(scratch.prepare(4).is_none());
        assert!(scratch.prepare(MAX_INDICES + 3).is_none());
    }
}
