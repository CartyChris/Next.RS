//! Dependency-free procedural mesh generation. The browser ABI is versioned below.
//! Coordinates are right-handed, Y-up; scale is expressed in metres.

use std::f32::consts::{PI, TAU};

pub const MIN_DETAIL: u32 = 8;
pub const MAX_DETAIL: u32 = 128;

#[derive(Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum MeshError {
    Kind = 1,
    Detail = 2,
    Scale = 3,
    Variation = 4,
    Allocation = 5,
}

#[derive(Debug, Default, PartialEq)]
pub struct Mesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub uvs: Vec<f32>,
    pub indices: Vec<u32>,
}

impl Mesh {
    fn with_capacity(vertices: usize, indices: usize) -> Result<Self, MeshError> {
        let mut mesh = Self::default();
        mesh.positions.try_reserve_exact(vertices * 3).map_err(|_| MeshError::Allocation)?;
        mesh.normals.try_reserve_exact(vertices * 3).map_err(|_| MeshError::Allocation)?;
        mesh.uvs.try_reserve_exact(vertices * 2).map_err(|_| MeshError::Allocation)?;
        mesh.indices.try_reserve_exact(indices).map_err(|_| MeshError::Allocation)?;
        Ok(mesh)
    }

    fn vertex(&mut self, position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) {
        self.positions.extend(position);
        self.normals.extend(normal);
        self.uvs.extend(uv);
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }
}

fn normalize(p: [f32; 3]) -> [f32; 3] {
    let length = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    [p[0] / length, p[1] / length, p[2] / length]
}

fn phase(seed: u32) -> f32 {
    let mut n = seed.wrapping_add(0x9e37_79b9);
    n = (n ^ (n >> 16)).wrapping_mul(0x85eb_ca6b);
    n = (n ^ (n >> 13)).wrapping_mul(0xc2b2_ae35);
    n ^= n >> 16;
    (n as f64 / u32::MAX as f64 * std::f64::consts::TAU) as f32
}

fn rock_radius(p: [f32; 3], seed_phase: f32, variation: f32) -> f32 {
    let broad =
        (p[0] * 3.1 + seed_phase).sin() * (p[1] * 2.7 - seed_phase * 0.7).cos() * (p[2] * 3.3 + seed_phase * 1.3).sin();
    let fine = (p[0] * 7.0 + p[1] * 5.0 - p[2] * 6.0 + seed_phase * 2.0).sin();
    1.0 + variation * (broad * 0.25 + fine * 0.045)
}

fn rock_normal(position: [f32; 3], seed_phase: f32, variation: f32) -> [f32; 3] {
    let field = |p: [f32; 3]| {
        let length = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        length - rock_radius(normalize(p), seed_phase, variation)
    };
    let mut gradient = [0.0; 3];
    for axis in 0..3 {
        let mut lo = position;
        let mut hi = position;
        lo[axis] -= 0.001;
        hi[axis] += 0.001;
        gradient[axis] = field(hi) - field(lo);
    }
    normalize(gradient)
}

fn sphere(detail: u32, seed: u32, scale: f32, variation: f32, rock: bool) -> Result<Mesh, MeshError> {
    let rows = detail / 2;
    let stride = detail + 1;
    let mut mesh = Mesh::with_capacity(((rows + 1) * stride) as usize, ((rows - 1) * detail * 6) as usize)?;
    let seed_phase = phase(seed);
    for row in 0..=rows {
        let v = row as f32 / rows as f32;
        let latitude = v * PI;
        for col in 0..=detail {
            let u = col as f32 / detail as f32;
            // Exact seam/pole coordinates prevent cracks from sin(TAU) rounding.
            let longitude = (col % detail) as f32 / detail as f32 * TAU;
            let direction = if row == 0 {
                [0.0, 1.0, 0.0]
            } else if row == rows {
                [0.0, -1.0, 0.0]
            } else {
                [latitude.sin() * longitude.cos(), latitude.cos(), latitude.sin() * longitude.sin()]
            };
            let radius = if rock { rock_radius(direction, seed_phase, variation) } else { 1.0 };
            let local = direction.map(|x| x * radius);
            let normal = if rock { rock_normal(local, seed_phase, variation) } else { direction };
            mesh.vertex(local.map(|x| x * scale), normal, [u, 1.0 - v]);
        }
    }
    for row in 0..rows {
        for col in 0..detail {
            let a = row * stride + col;
            let b = a + 1;
            let c = a + stride;
            let d = c + 1;
            if row != 0 {
                mesh.triangle(a, b, c);
            }
            if row != rows - 1 {
                mesh.triangle(b, d, c);
            }
        }
    }
    Ok(mesh)
}

fn torus(detail: u32, scale: f32, variation: f32) -> Result<Mesh, MeshError> {
    let rows = detail / 2;
    let stride = detail + 1;
    let mut mesh = Mesh::with_capacity(((rows + 1) * stride) as usize, (rows * detail * 6) as usize)?;
    let major = 1.25;
    let minor = 0.2 + variation * 0.4;
    for row in 0..=rows {
        let v = row as f32 / rows as f32;
        let tube = (row % rows) as f32 / rows as f32 * TAU;
        for col in 0..=detail {
            let u = col as f32 / detail as f32;
            let ring = (col % detail) as f32 / detail as f32 * TAU;
            let normal = [tube.cos() * ring.cos(), tube.sin(), tube.cos() * ring.sin()];
            let radius = major + minor * tube.cos();
            mesh.vertex(
                [radius * ring.cos() * scale, minor * tube.sin() * scale, radius * ring.sin() * scale],
                normal,
                [u, v],
            );
        }
    }
    for row in 0..rows {
        for col in 0..detail {
            let a = row * stride + col;
            mesh.triangle(a, a + stride, a + 1);
            mesh.triangle(a + 1, a + stride, a + stride + 1);
        }
    }
    Ok(mesh)
}

fn terrain_height(x: f32, z: f32, seed_phase: f32, variation: f32) -> f32 {
    let broad = (x * 1.4 + seed_phase).sin() * (z * 1.1 - seed_phase).cos();
    let detail = (x * 3.8 + z * 2.9 + seed_phase * 0.3).sin() * 0.25;
    let ridge = (x * 7.1 - z * 5.2 + seed_phase * 1.7).cos() * 0.08;
    variation * 0.65 * (broad + detail + ridge)
}

fn terrain(detail: u32, seed: u32, scale: f32, variation: f32) -> Result<Mesh, MeshError> {
    let stride = detail + 1;
    let mut mesh = Mesh::with_capacity((stride * stride) as usize, (detail * detail * 6) as usize)?;
    let seed_phase = phase(seed);
    let height = |x, z| terrain_height(x, z, seed_phase, variation);
    for row in 0..=detail {
        let v = row as f32 / detail as f32;
        let z = (v - 0.5) * 4.0;
        for col in 0..=detail {
            let u = col as f32 / detail as f32;
            let x = (u - 0.5) * 4.0;
            let dx = (height(x + 0.001, z) - height(x - 0.001, z)) / 0.002;
            let dz = (height(x, z + 0.001) - height(x, z - 0.001)) / 0.002;
            mesh.vertex([x * scale, height(x, z) * scale, z * scale], normalize([-dx, 1.0, -dz]), [u, v]);
        }
    }
    for row in 0..detail {
        for col in 0..detail {
            let a = row * stride + col;
            mesh.triangle(a, a + stride, a + 1);
            mesh.triangle(a + 1, a + stride, a + stride + 1);
        }
    }
    Ok(mesh)
}

/// Kind: 0 sphere, 1 torus, 2 seeded rock, 3 seeded terrain.
/// Bounds are checked before calculating capacities or allocating any buffers.
pub fn generate(kind: u32, detail: u32, seed: u32, scale: f32, variation: f32) -> Result<Mesh, MeshError> {
    if kind > 3 {
        return Err(MeshError::Kind);
    }
    if !(MIN_DETAIL..=MAX_DETAIL).contains(&detail) {
        return Err(MeshError::Detail);
    }
    if !scale.is_finite() || !(0.01..=100.0).contains(&scale) {
        return Err(MeshError::Scale);
    }
    if !variation.is_finite() || !(0.0..=1.0).contains(&variation) {
        return Err(MeshError::Variation);
    }
    match kind {
        0 => sphere(detail, seed, scale, variation, false),
        1 => torus(detail, scale, variation),
        2 => sphere(detail, seed, scale, variation, true),
        3 => terrain(detail, seed, scale, variation),
        _ => unreachable!(),
    }
}

#[cfg(target_arch = "wasm32")]
mod abi {
    use super::{Mesh, generate};
    use std::cell::RefCell;

    thread_local! { static CURRENT: RefCell<Option<Mesh>> = const { RefCell::new(None) }; }

    // SAFETY: these unique, versioned C ABI symbols belong to this standalone module.
    #[unsafe(no_mangle)]
    pub extern "C" fn mesh_abi_version() -> u32 {
        1
    }

    // SAFETY: scalar arguments and status return are the documented mesh ABI.
    #[unsafe(no_mangle)]
    pub extern "C" fn mesh_generate(kind: u32, detail: u32, seed: u32, scale: f32, variation: f32) -> u32 {
        match generate(kind, detail, seed, scale, variation) {
            Ok(mesh) => {
                CURRENT.with_borrow_mut(|current| *current = Some(mesh));
                0
            }
            Err(error) => error as u32,
        }
    }

    // SAFETY: frees only Rust-owned buffers; the host never supplies pointers.
    #[unsafe(no_mangle)]
    pub extern "C" fn mesh_clear() {
        CURRENT.with_borrow_mut(|current| *current = None);
    }

    macro_rules! getter {
        ($name:ident, $body:expr) => {
            // SAFETY: exported getters return scalars, valid until generation/clear.
            #[unsafe(no_mangle)]
            pub extern "C" fn $name() -> u32 {
                CURRENT.with_borrow(|current| current.as_ref().map_or(0, $body))
            }
        };
    }
    getter!(mesh_vertex_count, |m: &Mesh| (m.positions.len() / 3) as u32);
    getter!(mesh_index_count, |m: &Mesh| m.indices.len() as u32);
    getter!(mesh_positions_ptr, |m: &Mesh| m.positions.as_ptr() as u32);
    getter!(mesh_normals_ptr, |m: &Mesh| m.normals.as_ptr() as u32);
    getter!(mesh_uvs_ptr, |m: &Mesh| m.uvs.as_ptr() as u32);
    getter!(mesh_indices_ptr, |m: &Mesh| m.indices.as_ptr() as u32);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_valid(mesh: &Mesh) {
        let count = mesh.positions.len() / 3;
        assert!(count > 0 && count <= 16_641);
        assert_eq!(mesh.positions.len(), count * 3);
        assert_eq!(mesh.normals.len(), count * 3);
        assert_eq!(mesh.uvs.len(), count * 2);
        assert_eq!(mesh.indices.len() % 3, 0);
        assert!(!mesh.indices.is_empty());
        assert!(mesh.positions.iter().chain(&mesh.normals).chain(&mesh.uvs).all(|n| n.is_finite()));
        assert!(mesh.indices.iter().all(|i| (*i as usize) < count));
        for normal in mesh.normals.chunks_exact(3) {
            let length = normal.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((length - 1.0).abs() < 0.0002, "normal length {length}");
        }
        for triangle in mesh.indices.chunks_exact(3) {
            let a = &mesh.positions[triangle[0] as usize * 3..][..3];
            let b = &mesh.positions[triangle[1] as usize * 3..][..3];
            let c = &mesh.positions[triangle[2] as usize * 3..][..3];
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let face = [ab[1] * ac[2] - ab[2] * ac[1], ab[2] * ac[0] - ab[0] * ac[2], ab[0] * ac[1] - ab[1] * ac[0]];
            let mut average = [0.0; 3];
            for index in triangle {
                for (axis, sum) in average.iter_mut().enumerate() {
                    *sum += mesh.normals[*index as usize * 3 + axis];
                }
            }
            let dot = face.iter().zip(average).map(|(f, n)| f * n).sum::<f32>();
            assert!(dot > 0.0, "degenerate or inward triangle {triangle:?}: {dot}");
        }
    }

    #[test]
    fn all_shapes_have_finite_attributes_unit_normals_and_outward_winding() {
        for kind in 0..=3 {
            for detail in [8, 9, 32, 128] {
                for variation in [0.0, 0.5, 1.0] {
                    let mesh = generate(kind, detail, 17, 1.0, variation).unwrap();
                    assert_valid(&mesh);
                }
            }
        }
    }

    #[test]
    fn seeded_meshes_are_reproducible_and_seed_sensitive() {
        for kind in [2, 3] {
            let first = generate(kind, 32, 17, 1.0, 0.8).unwrap();
            assert_eq!(first, generate(kind, 32, 17, 1.0, 0.8).unwrap());
            assert_ne!(first.positions, generate(kind, 32, 18, 1.0, 0.8).unwrap().positions);
        }
    }

    #[test]
    fn invalid_parameters_are_rejected_before_allocation() {
        assert_eq!(generate(4, 32, 0, 1.0, 0.5), Err(MeshError::Kind));
        for detail in [0, 7, 129, u32::MAX] {
            assert_eq!(generate(0, detail, 0, 1.0, 0.5), Err(MeshError::Detail));
        }
        for scale in [0.0, -1.0, 101.0, f32::NAN, f32::INFINITY] {
            assert_eq!(generate(0, 32, 0, scale, 0.5), Err(MeshError::Scale));
        }
        for variation in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            assert_eq!(generate(0, 32, 0, 1.0, variation), Err(MeshError::Variation));
        }
    }

    #[test]
    fn scale_changes_positions_without_changing_topology_or_normals() {
        for kind in 0..=3 {
            let small = generate(kind, 32, 17, 0.5, 0.5).unwrap();
            let large = generate(kind, 32, 17, 1.0, 0.5).unwrap();
            assert_eq!(small.indices, large.indices);
            assert_eq!(small.normals, large.normals);
            assert_eq!(small.uvs, large.uvs);
            for (a, b) in small.positions.iter().zip(large.positions) {
                assert_eq!(*a * 2.0, b);
            }
        }
    }
}
